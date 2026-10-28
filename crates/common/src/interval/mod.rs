/// This module provides functionality for representing sets of intervals on
/// strings of bytes.
///
/// The set of byte strings has a few useful properties:
/// 1. There is a minimum (the empty string)
/// 2. Every string `s` has a smallest string `t` such that `s < t`, its
///    successor. Note that since we don't put a bound on the length of these
///    strings, strings have successors but not predecessors.
///
/// With these properties, we can simplify our intervals greatly:
/// 1. Every interval can be formed as an inclusive lower bound and exclusive
///    upper bound.
/// 2. We don't need to represent -inf in our system, just +inf.
mod bounds;
mod interval_set;
mod key;

use std::ops::{
    Bound,
    RangeBounds,
};

use value::heap_size::HeapSize;

pub use self::{
    bounds::{
        End,
        EndRef,
        StartIncluded,
    },
    interval_set::{
        IntervalSet,
        MembershipCursor,
    },
    key::BinaryKey,
};
use crate::{
    index::IndexKeyBytes,
    query::{
        CursorPosition,
        Order,
    },
};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Interval {
    pub start: StartIncluded,
    pub end: End,
}

impl HeapSize for Interval {
    fn heap_size(&self) -> usize {
        self.start.heap_size() + self.end.heap_size()
    }
}

impl Interval {
    pub fn prefix(key: BinaryKey) -> Self {
        let end = End::after_prefix(&key);
        Self {
            start: StartIncluded(key),
            end,
        }
    }

    pub const fn empty() -> Self {
        Self {
            start: StartIncluded(BinaryKey::min()),
            end: End::Excluded(BinaryKey::min()),
        }
    }

    pub const fn all() -> Self {
        Self {
            start: StartIncluded(BinaryKey::min()),
            end: End::Unbounded,
        }
    }

    pub const fn as_ref(&self) -> IntervalRef<'_> {
        let Self { start, end } = self;
        IntervalRef {
            start: start.0.as_slice(),
            end: end.as_ref(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    pub fn is_superset(&self, other: &Self) -> bool {
        other.is_empty() || self.start <= other.start && other.end <= self.end
    }

    pub fn contains(&self, point: &[u8]) -> bool {
        self.as_ref().contains(point)
    }

    pub fn contains_cursor(&self, cursor: &CursorPosition) -> bool {
        match cursor {
            CursorPosition::After(last_key) => self.contains(last_key),
            CursorPosition::End => true,
        }
    }

    pub fn is_disjoint(&self, other: &Self) -> bool {
        self.is_empty()
            || other.is_empty()
            || other.end.is_disjoint(&self.start)
            || self.end.is_disjoint(&other.start)
    }

    pub fn is_adjacent(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.end.is_adjacent(&other.start) || other.end.is_adjacent(&self.start)
    }

    /// When reading from self in order `order`, if we've just read `last_key`,
    /// returns (interval read, interval remaining).
    /// If self=[X, Y) and order=Asc, returns [X, last_key] and (last_key, Y).
    /// If self=[X, Y) and order=Desc, returns [last_key, Y) and [X, last_key).
    /// Note last_key must be a full IndexKeyBytes, not an arbitrary BinaryKey,
    /// so we can assume there are no other IndexKeyBytes that have `index_key`
    /// as a prefix.
    pub fn split_after(&self, last_key: IndexKeyBytes, order: Order) -> (Self, Self) {
        let last_key_binary = BinaryKey::from(last_key);
        match order {
            Order::Asc => match last_key_binary.increment() {
                // It's possible to have `last_key_incr > end` if `self` is a singleton interval;
                // avoid expanding `self` in that case
                Some(last_key_incr) if self.end.greater_than(&last_key_incr) => (
                    Self {
                        start: self.start.clone(),
                        end: End::Excluded(last_key_incr.clone()),
                    },
                    Self {
                        start: StartIncluded(last_key_incr),
                        end: self.end.clone(),
                    },
                ),
                _ => (self.clone(), Interval::empty()),
            },
            Order::Desc => (
                Self {
                    start: StartIncluded(last_key_binary.clone()),
                    end: self.end.clone(),
                },
                Self {
                    start: self.start.clone(),
                    end: End::Excluded(last_key_binary),
                },
            ),
        }
    }

    pub fn split(&self, cursor: CursorPosition, order: Order) -> (Self, Self) {
        match cursor {
            CursorPosition::After(last_key) => self.split_after(last_key, order),
            CursorPosition::End => (self.clone(), Interval::empty()),
        }
    }

    /// Returns an interval that contains `value` and nothing else.
    pub fn singleton(value: BinaryKey) -> Self {
        let end = End::included(&value);
        Self {
            start: StartIncluded(value),
            end,
        }
    }

    /// If this interval contains only a single value, returns that value.
    pub fn is_singleton(&self) -> Option<&BinaryKey> {
        // check if `self.end` is exactly `self.start + [0]`
        if let End::Excluded(end) = &self.end
            && end.strip_prefix(&*self.start.0) == Some(&[0])
        {
            Some(&self.start.0)
        } else {
            None
        }
    }
}

impl RangeBounds<[u8]> for &Interval {
    fn start_bound(&self) -> Bound<&[u8]> {
        let StartIncluded(ref s) = self.start;
        Bound::Included(&s[..])
    }

    fn end_bound(&self) -> Bound<&[u8]> {
        match self.end {
            End::Excluded(ref s) => Bound::Excluded(&s[..]),
            End::Unbounded => Bound::Unbounded,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IntervalRef<'a> {
    pub start: &'a [u8],
    pub end: EndRef<'a>,
}

impl IntervalRef<'_> {
    pub fn all() -> Self {
        static ALL: Interval = Interval::all();
        ALL.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        match (self.start, self.end) {
            (_, EndRef::Unbounded) => false,
            (s, EndRef::Excluded(t)) => s >= t,
        }
    }

    pub fn contains(&self, point: &[u8]) -> bool {
        let after_start = self.start <= point;
        let before_end = match self.end {
            EndRef::Excluded(t) => point < t,
            EndRef::Unbounded => true,
        };
        after_start && before_end
    }

    pub fn to_owned(&self) -> Interval {
        Interval {
            start: StartIncluded(self.start.to_vec().into()),
            end: self.end.to_owned(),
        }
    }
}

impl RangeBounds<[u8]> for IntervalRef<'_> {
    fn start_bound(&self) -> Bound<&[u8]> {
        Bound::Included(self.start)
    }

    fn end_bound(&self) -> Bound<&[u8]> {
        match self.end {
            EndRef::Excluded(s) => Bound::Excluded(s),
            EndRef::Unbounded => Bound::Unbounded,
        }
    }
}
