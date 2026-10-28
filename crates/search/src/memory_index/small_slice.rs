#![cfg(all(target_endian = "little", target_pointer_width = "64"))]

use std::{
    borrow::Borrow,
    fmt,
    hash::Hash,
    mem,
    ops::Deref,
    ptr,
    sync::Arc,
};

const MAX_INLINE_SIZE: usize = 15;
const MAX_HEAP_SIZE: usize = 1 << 63;

/// Memory efficient representation of an `Arc<[u8]>` that stores up to 15 bytes
/// inline without a heap allocation.
///
/// We assume 64-bit pointers and a little endian architecture, yielding the
/// following layout:
/// ```text
/// 
///       [     ptr     ] [     len     ]
/// byte: 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5
///                       lo  ------>  hi
/// ```
/// Then, we can use the highest bit on `len` as a tag, turning it on when we
/// want the allocation to be inline. Note that the stdlib doesn't allow
/// allocations that exceed `isize::MAX` anyways, so we wouldn't see this bit
/// turned on for a heap allocation in practice.
///
/// When this bit is set, we use the first 15 bytes for storage and use the
/// remaining 7 bits in the final byte for storing the length.
/// ```text
/// 
///       [            data           ] [  length   ] 1
/// bit:  [                           ] 0 1 2 3 4 5 6 7
/// byte: 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 -------------
/// ```
/// Note that `u8`s are little endian too, so if we load the last byte as a
/// `u8`, we check that *its* highest bit is set (i.e. `b & 0b1000_0000 != 0`)
/// to see if the allocation is inline.
#[repr(C)]
pub struct SmallSlice {
    ptr: *const (),
    len: usize,
}

impl<'a> From<&'a [u8]> for SmallSlice {
    fn from(buf: &'a [u8]) -> Self {
        if buf.len() <= MAX_INLINE_SIZE {
            let mut inline = [0u8; 16];
            inline[..buf.len()].copy_from_slice(buf);
            inline[15] = (buf.len()) as u8 | 0b1000_0000;

            // SAFETY: mem::size_of::<SmallSlice>() == 16
            unsafe { mem::transmute::<[u8; 16], SmallSlice>(inline) }
        } else {
            // Allocate on the heap.
            let heap_allocated: Arc<[u8]> = buf.into();

            // Take ownership of the allocation into a `*const [u8]`.
            let raw_wide_ptr = Arc::into_raw(heap_allocated);

            // Unpack the wide pointer to store it ourselves.
            let (ptr, len) = raw_wide_ptr.to_raw_parts();
            assert!(len < MAX_HEAP_SIZE);
            Self { ptr, len }
        }
    }
}

// SAFETY: `Arc<[u8]>` implements both `Send` and `Sync`.
unsafe impl Send for SmallSlice {}
unsafe impl Sync for SmallSlice {}

impl Deref for SmallSlice {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        if let Some(inline_size) = self.inline_size() {
            // SAFETY: mem::size_of::<SmallSlice>() == 16 and [u8; 16] has lower alignment.
            let buf = unsafe { mem::transmute::<&Self, &[u8; 16]>(self) };
            &buf[..inline_size]
        } else {
            // SAFETY: `self.ptr` and `self.len` come from `<*const [u8]>::to_raw_parts` in
            // the constructor.
            let raw_wide_ptr: *const [u8] = ptr::from_raw_parts(self.ptr, self.len);
            unsafe { &*raw_wide_ptr }
        }
    }
}

impl Clone for SmallSlice {
    fn clone(&self) -> Self {
        if self.is_inline() {
            Self {
                ptr: self.ptr,
                len: self.len,
            }
        } else {
            let raw_wide_ptr: *const [u8] = ptr::from_raw_parts(self.ptr, self.len);
            // SAFETY: `&self` holds a strong reference, so we know we're not incrementing a
            // freed allocation.
            unsafe { Arc::increment_strong_count(raw_wide_ptr) }
            Self {
                ptr: self.ptr,
                len: self.len,
            }
        }
    }
}

impl Drop for SmallSlice {
    fn drop(&mut self) {
        if self.is_inline() {
            return;
        }
        // Rebuild the `*const [u8]` wide pointer.
        let raw_wide_ptr: *const [u8] = ptr::from_raw_parts(self.ptr, self.len);
        // Retake ownership of the `Arc` and drop it.
        unsafe { drop(Arc::from_raw(raw_wide_ptr)) };
    }
}

impl fmt::Debug for SmallSlice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self[..])
    }
}

impl PartialEq for SmallSlice {
    fn eq(&self, other: &Self) -> bool {
        self[..].eq(&other[..])
    }
}

impl Eq for SmallSlice {}

impl Ord for SmallSlice {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self[..].cmp(&other[..])
    }
}

impl PartialOrd for SmallSlice {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for SmallSlice {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self[..].hash(state)
    }
}

impl Borrow<[u8]> for SmallSlice {
    fn borrow(&self) -> &[u8] {
        &self[..]
    }
}

impl SmallSlice {
    fn is_inline(&self) -> bool {
        self.inline_size().is_some()
    }

    fn inline_size(&self) -> Option<usize> {
        if self.len & 1 << 63 != 0 {
            // SAFETY: mem::size_of::<SmallSlice>() == 16 and [u8; 16] has lower alignment.
            let inline_buf = unsafe { mem::transmute::<&SmallSlice, &[u8; 16]>(self) };
            let len = inline_buf[15] ^ 0b1000_0000;
            Some(len as usize)
        } else {
            None
        }
    }

    pub fn heap_allocations(&self) -> usize {
        // NB: Arc stores two `AtomicUsize`s for the strong and weak count before the
        // data in the heap allocation. We don't ever use weak references, so we
        // could save 8 bytes per term by finding a way to elide it.
        if self.inline_size().is_some() {
            0
        } else {
            self.len + 16
        }
    }
}
