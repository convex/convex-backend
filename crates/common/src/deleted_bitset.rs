use std::{
    fs::File,
    io::{
        BufReader,
        BufWriter,
        Read,
        Write,
    },
    mem,
    path::{
        Path,
        PathBuf,
    },
};

use bitvec::{
    slice::BitSlice,
    vec::BitVec,
};
use byteorder::{
    LittleEndian,
    ReadBytesExt,
    WriteBytesExt,
};

/// Version 1 of the deleted bitset has the following format:
/// ```
/// [ version ] [ count ] [ num_deleted ] [ bitset block ]*
/// ```
/// - version (u8): version number for the file format
/// - count (little-endian u32): number of *bits* in the bitset
/// - num_deleted (little-endian u32): number of set bits in the bitset
/// - bitset blocks (dense array of little-endian u64s): bitset contents
pub const DELETED_BITSET_VERSION: u8 = 1;

#[derive(Clone, Default, Debug)]
pub struct DeletedBitset {
    deleted: BitVec,
    num_deleted: usize,
}

impl DeletedBitset {
    pub fn new(count: usize) -> Self {
        let mut result = DeletedBitset {
            deleted: BitVec::new(),
            num_deleted: 0,
        };
        result.resize(count);
        result
    }

    pub fn resize(&mut self, size: usize) {
        self.deleted.resize(size, false);
    }

    pub fn num_deleted(&self) -> usize {
        self.num_deleted
    }

    pub fn len(&self) -> usize {
        self.deleted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.deleted.is_empty()
    }

    pub fn is_deleted(&self, id: u32) -> bool {
        let offset = id as usize;
        offset >= self.deleted.len() || self.deleted[offset]
    }

    pub fn deleted_id_bitslice(&self) -> &BitSlice {
        &self.deleted
    }

    pub fn check_invariants(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.num_deleted == self.deleted.count_ones());
        // We shouldn't hit these codepaths, but `BitVec` can have its physical blocks
        // not be aligned with the logical bit vector.
        self.deleted.force_align();
        self.deleted.set_uninitialized(false);
        Ok(())
    }

    pub fn delete(&mut self, internal_id: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.is_deleted(internal_id),
            "Trying to delete {internal_id:?} twice"
        );
        self.deleted.set(internal_id as usize, true);
        self.num_deleted += 1;
        Ok(())
    }

    pub fn undelete(&mut self, internal_id: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            (internal_id as usize) < self.deleted.len() && self.deleted[internal_id as usize],
            "Trying to undelete {internal_id:?}, which is not deleted"
        );
        self.deleted.set(internal_id as usize, false);
        self.num_deleted -= 1;
        Ok(())
    }

    pub fn write_to_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        self.write(&mut out)?;
        out.into_inner()?.sync_all()?;
        Ok(())
    }

    /// Write out a static file with the ID tracker's (empty) deleted bitset.
    pub fn write(&mut self, mut out: impl Write) -> anyhow::Result<()> {
        self.check_invariants()?;

        let count = self.len();
        let num_deleted = self.num_deleted as u32;
        let expected_blocks = count.div_ceil(64);
        anyhow::ensure!(self.deleted.as_raw_slice().len() == expected_blocks);

        // Unfortunately, `bitset` wants `usize`s but we want to serialize `u64`s out to
        // disk. Check that we're on a 64-bit platform.
        anyhow::ensure!(mem::size_of::<usize>() * 8 == 64);

        out.write_u8(DELETED_BITSET_VERSION)?;
        out.write_u32::<LittleEndian>(count.try_into()?)?;
        out.write_u32::<LittleEndian>(num_deleted)?;
        for block in self.deleted.as_raw_slice() {
            out.write_u64::<LittleEndian>(*block as u64)?;
        }
        out.flush()?;

        Ok(())
    }

    pub fn load_from_path<P>(path: P) -> anyhow::Result<Self>
    where
        P: AsRef<Path>,
    {
        let deleted_file = File::open(path)?;
        Self::load(
            deleted_file.metadata()?.len() as usize,
            BufReader::new(deleted_file),
        )
    }

    pub fn load(file_len: usize, mut reader: impl Read) -> anyhow::Result<Self> {
        // As when writing out the index, we need to ensure we're on a 64-bit platform,
        // since `byteorder` wants `u64`s but `BitVec` wants `usize`s.
        anyhow::ensure!(mem::size_of::<usize>() * 8 == 64);

        anyhow::ensure!(reader.read_u8()? == DELETED_BITSET_VERSION);
        let num_bits = reader.read_u32::<LittleEndian>()? as usize;
        let num_deleted = reader.read_u32::<LittleEndian>()? as usize;
        anyhow::ensure!(num_deleted <= num_bits);

        // Compute the number of blocks in the bitset.
        let num_blocks = num_bits.div_ceil(64);

        // As with `Self::load_uuids`, check that the file's lengths match up.
        let mut expected_len = 0;
        expected_len += 1; // version
        expected_len += 4; // count
        expected_len += 4; // num_deleted
        expected_len += num_blocks * 8; // bitset blocks
        anyhow::ensure!(expected_len == file_len);

        let mut deleted_buf = vec![0u64; num_blocks];
        reader.read_u64_into::<LittleEndian>(&mut deleted_buf)?;

        // Unfortunately, we have to do a copy here to change the `u64`s we read via
        // `byteorder` to `usize`s. The deleted bitset is a tiny piece of the
        // index, so we'll probably not need to optimize this for a while.
        let mut deleted = BitVec::from_vec(deleted_buf.into_iter().map(|b| b as usize).collect());

        // Trim the last block to our desired length, if needed.
        deleted.resize(num_bits, false);

        // While we're reading the whole file into memory, check that `num_deleted`
        // matches the bitset.
        anyhow::ensure!(deleted.count_ones() == num_deleted);

        Ok(Self {
            deleted,
            num_deleted,
        })
    }
}
