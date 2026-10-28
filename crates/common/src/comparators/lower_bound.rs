use std::{
    borrow::Borrow,
    cmp::Ordering,
    ops::Bound,
};

use crate::{
    bounds::LowerBound,
    comparators::AsComparator,
};

pub trait LowerBoundKey<T: ?Sized> {
    fn key(&self) -> LowerBound<&T>;
}

impl<Q: ?Sized, T: Borrow<Q>> LowerBoundKey<Q> for LowerBound<T> {
    fn key(&self) -> LowerBound<&Q> {
        match self.0 {
            Bound::Included(ref t) => LowerBound(Bound::Included(t.borrow())),
            Bound::Excluded(ref t) => LowerBound(Bound::Excluded(t.borrow())),
            Bound::Unbounded => LowerBound(Bound::Unbounded),
        }
    }
}

impl<'a, Q: ?Sized, T: Borrow<Q> + 'a> Borrow<dyn LowerBoundKey<Q> + 'a> for LowerBound<T> {
    fn borrow(&self) -> &(dyn LowerBoundKey<Q> + 'a) {
        self
    }
}

impl<T: Ord + ?Sized> Ord for dyn LowerBoundKey<T> + '_ {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl<T: Ord + ?Sized> PartialOrd for dyn LowerBoundKey<T> + '_ {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Eq + ?Sized> Eq for dyn LowerBoundKey<T> + '_ {}

impl<T: PartialEq + ?Sized> PartialEq for dyn LowerBoundKey<T> + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.key().eq(&other.key())
    }
}

impl<'a, T: ?Sized> AsComparator for LowerBound<&'a T> {
    type Comparator = dyn LowerBoundKey<T> + 'a;

    fn as_comparator(&self) -> &Self::Comparator {
        self
    }
}
