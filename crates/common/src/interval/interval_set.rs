use std::{
    collections::BTreeMap,
    iter,
    ops::Bound,
};

use itertools::Either;
use pb::common::{
    interval::End as EndProto,
    Interval as IntervalProto,
};
use value::heap_size::{
    HeapSize,
    WithHeapSize,
};

use super::{
    bounds::{
        End,
        EndRef,
        StartIncluded,
    },
    BinaryKey,
    Interval,
    IntervalRef,
};

/// A set of `Interval`s. Intersecting and adjacent intervals are merged.
#[derive(Clone, Debug)]
pub enum IntervalSet {
    /// Map from Interval.start to Interval.end. All intervals are
    /// non-intersecting, non-adjacent, and non-empty.
    Intervals(WithHeapSize<BTreeMap<StartIncluded, End>>),
    /// In-memory optimization to avoid allocating a [`BTreeMap`] to represent
    /// `{ Start(BinaryKey::min()) => End::Unbounded }`
    All,
}

impl Default for IntervalSet {
    fn default() -> Self {
        Self::new()
    }
}

const ALL_INTERVAL_PROTO: [IntervalProto; 1] = [IntervalProto {
    start_inclusive: vec![],
    end: Some(EndProto::AfterAll(())),
}];

impl From<IntervalSet> for Vec<IntervalProto> {
    fn from(set: IntervalSet) -> Self {
        match set {
            IntervalSet::All => ALL_INTERVAL_PROTO.to_vec(),
            IntervalSet::Intervals(intervals) => intervals
                .into_iter()
                .map(|(start, end)| {
                    let start = match start {
                        StartIncluded(b) => b.into(),
                    };
                    let end = match end {
                        End::Unbounded => EndProto::AfterAll(()),
                        End::Excluded(e) => EndProto::Exclusive(e.into()),
                    };
                    IntervalProto {
                        start_inclusive: start,
                        end: Some(end),
                    }
                })
                .collect(),
        }
    }
}

impl TryFrom<Vec<IntervalProto>> for IntervalSet {
    type Error = anyhow::Error;

    fn try_from(intervals: Vec<IntervalProto>) -> anyhow::Result<Self> {
        if intervals == ALL_INTERVAL_PROTO {
            return Ok(IntervalSet::All);
        }
        let mut set: Vec<(StartIncluded, End)> = vec![];
        for interval in intervals {
            let start = StartIncluded(interval.start_inclusive.into());
            if let Some((_, last_end)) = set.last() {
                anyhow::ensure!(
                    !last_end.is_overlapping_or_adjacent(&start),
                    "IntervalProtos out of order: {last_end:?} >= {start:?}"
                );
            }
            let end = match interval.end {
                None => return Err(anyhow::anyhow!("Interval missing end")),
                Some(end) => match end {
                    EndProto::AfterAll(()) => End::Unbounded,
                    EndProto::Exclusive(end) => End::Excluded(end.into()),
                },
            };
            anyhow::ensure!(
                end.greater_than(&start.0),
                "empty IntervalProto {start:?}..{end:?}"
            );
            set.push((start, end));
        }
        Ok(IntervalSet::Intervals(
            set.into_iter().collect::<BTreeMap<_, _>>().into(),
        ))
    }
}

impl IntoIterator for IntervalSet {
    type Item = Interval;

    type IntoIter = impl Iterator<Item = Interval>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::All => Either::Left(iter::once(Interval::all())),
            Self::Intervals(intervals) => Either::Right(
                intervals
                    .into_iter()
                    .map(|(start, end)| Interval { start, end }),
            ),
        }
    }
}

impl IntervalSet {
    /// Construct an empty set.
    pub fn new() -> Self {
        Self::Intervals(WithHeapSize::default())
    }

    /// True if this `IntervalSet` contains no keys.
    pub fn is_empty(&self) -> bool {
        match self {
            // self.intervals only contains non-empty intervals, so this is sufficient.
            Self::Intervals(intervals) => intervals.is_empty(),
            Self::All => false,
        }
    }

    /// How many intervals are in this set?
    pub fn len(&self) -> usize {
        match self {
            Self::Intervals(intervals) => intervals.len(),
            Self::All => 1,
        }
    }

    /// Add the given `Interval` to the set.
    pub fn add(&mut self, interval: Interval) {
        if interval.is_empty() {
            return;
        }
        if interval == Interval::all() {
            *self = IntervalSet::All;
        }
        match self {
            IntervalSet::All => {},
            IntervalSet::Intervals(intervals) => {
                // In order to merge adjacent and overlapping intervals, we need to find all of
                // the overlapping intervals and take the min of our new interval and
                // all of the overlapping to find the start of the merged interval
                // (merged_start) and likewise for the end. Then, we remove all
                // of the overlaps and insert the merged interval. This is linear in the
                // number of overlaps, but turns out to be amoritized constant time, because you
                // can 'charge' the eviction of a interval back to the insertion that put
                // it there.
                //
                // self.intervals            --- -----    ---       -----
                // interval                           ------------------
                // merged_start                  ^
                // merged_end                                           ^
                // -> self.intervals after   --- ------------------------
                //
                // self.intervals            ---          ---       -----
                // interval                           ------------------
                // merged start                       ^
                // merged_end                                           ^
                // -> self.intervals after   ---      -------------------
                //
                // self.intervals            ---          ---   ----   --
                // interval                           ---------------
                // merged start                       ^
                // merged_end                                       ^
                // -> self.intervals after   ---      ---------------  --
                let mut cursor = intervals.upper_bound_mut(match &interval.end {
                    End::Excluded(binary_key) => Bound::Included(&binary_key[..]),
                    End::Unbounded => Bound::Unbounded,
                });
                let mut merged_interval = interval;
                // Iterate all overlapping intervals in descending order
                if let Some((_other_start, other_end)) = cursor.peek_prev()
                    && other_end.is_overlapping_or_adjacent(&merged_interval.start)
                {
                    let (other_start, other_end) = cursor.remove_prev().expect("peeked");
                    if other_end > merged_interval.end {
                        merged_interval.end = other_end;
                    }
                    if other_start <= merged_interval.start {
                        merged_interval.start = other_start;
                    } else {
                        while let Some((_other_start, other_end)) = cursor.peek_prev()
                            && other_end.is_overlapping_or_adjacent(&merged_interval.start)
                        {
                            let (other_start, other_end) = cursor.remove_prev().expect("peeked");
                            // Only the first visited interval can extend `merged_interval.end`
                            debug_assert!(other_end < merged_interval.end);
                            if other_start <= merged_interval.start {
                                merged_interval.start = other_start;
                                break;
                            }
                        }
                    }
                }
                if merged_interval == Interval::all() {
                    *self = IntervalSet::All;
                    return;
                }
                cursor
                    .insert_after(merged_interval.start, merged_interval.end)
                    .expect("invariant broken?");
            },
        };
    }

    fn interval_preceding(&self, k: &[u8]) -> Option<IntervalRef<'_>> {
        match self {
            Self::All => Some(IntervalRef::all()),
            Self::Intervals(intervals) => {
                let (start, end) = intervals
                    .range::<[u8], _>((Bound::Unbounded, Bound::Included(k)))
                    .next_back()?;
                Some(IntervalRef {
                    start: start.as_ref(),
                    end: end.as_ref(),
                })
            },
        }
    }

    /// True if any of the intervals in the `IntervalSet` contain `k`.
    pub fn contains(&self, k: &[u8]) -> bool {
        // Since self.intervals are non-overlapping, the only interval that can contain
        // k is the first preceding k.
        let Some(interval) = self.interval_preceding(k) else {
            return false;
        };
        interval.contains(k)
    }

    pub fn contains_interval(&self, target: IntervalRef<'_>) -> bool {
        self.split_interval_components(target)
            .all(|(in_set, _)| in_set)
    }

    /// Return an iterator over all the intervals within the set, borrowing
    /// their bounds.
    pub fn iter_ref(&self) -> impl Iterator<Item = IntervalRef<'_>> + '_ {
        match self {
            Self::All => Either::Left(iter::once(IntervalRef::all())),
            Self::Intervals(intervals) => Either::Right(intervals.iter().map(
                |(StartIncluded(start), end)| IntervalRef {
                    start: start.as_slice(),
                    end: end.as_ref(),
                },
            )),
        }
    }

    /// Return an iterator over all the intervals within the set.
    pub fn iter(&self) -> impl Iterator<Item = Interval> + '_ {
        self.iter_ref().map(|interval| interval.to_owned())
    }

    /// Computes the set-difference target - self.
    pub fn subtract_from_interval(&self, target: &Interval) -> Self {
        let mut difference: WithHeapSize<BTreeMap<_, _>> = WithHeapSize::default();
        for (in_set, interval) in self.split_interval_components(target.as_ref()) {
            // split_interval_components alternate between `in_set` and `!in_set`, and
            // returns intervals that are adjacent and nonempty. Therefore the intervals
            // with !in_set are not intersecting or adjacent.
            if !in_set {
                difference.insert(
                    StartIncluded(BinaryKey::from(interval.start.to_owned())),
                    interval.end.to_owned(),
                );
            }
        }
        Self::Intervals(difference)
    }

    /// Splits a target interval into components by whether they are in self.
    /// Returns (in_set, interval) where in_set indicates whether interval is in
    /// self, and the union of intervals is target.
    pub fn split_interval_components<'a>(
        &'a self,
        target: IntervalRef<'a>,
    ) -> impl Iterator<Item = (bool, IntervalRef<'a>)> + 'a {
        match self {
            Self::All => Either::Right(iter::once((true, target))),
            Self::Intervals(intervals) => {
                Either::Left(iter::from_coroutine(
                    #[coroutine]
                    move || {
                        if target.is_empty() {
                            return;
                        }
                        let target_start = target.start;
                        let interval_before = self.interval_preceding(target_start);
                        let mut component_start = match interval_before {
                            None => target_start,
                            Some(interval_before) => {
                                if target.end <= interval_before.end {
                                    yield (true, target);
                                    return;
                                }
                                let interval_before_end = match interval_before.end {
                                    EndRef::Unbounded => unreachable!(),
                                    EndRef::Excluded(interval_before_end) => interval_before_end,
                                };
                                if interval_before_end > target_start {
                                    yield (
                                        true,
                                        IntervalRef {
                                            start: target.start,
                                            end: interval_before.end,
                                        },
                                    );
                                    interval_before_end
                                } else {
                                    target_start
                                }
                            },
                        };
                        // `intersecting` is all intervals in `self` that intersect with `target`,
                        // excluding `interval_before`.
                        let intersecting = intervals.range(IntervalRef {
                            start: component_start,
                            end: target.end,
                        });
                        for (interval_start, interval_end) in intersecting {
                            yield (
                                false,
                                IntervalRef {
                                    start: component_start,
                                    end: EndRef::Excluded(interval_start.as_ref()),
                                },
                            );
                            if target.end <= interval_end.as_ref() {
                                yield (
                                    true,
                                    IntervalRef {
                                        start: interval_start.as_ref(),
                                        end: target.end,
                                    },
                                );
                                return;
                            }
                            yield (
                                true,
                                IntervalRef {
                                    start: interval_start.as_ref(),
                                    end: interval_end.as_ref(),
                                },
                            );
                            component_start = match interval_end {
                                End::Unbounded => unreachable!(),
                                End::Excluded(interval_end) => interval_end.as_ref(),
                            };
                        }
                        yield (
                            false,
                            IntervalRef {
                                start: component_start,
                                end: target.end,
                            },
                        );
                    },
                ))
            },
        }
    }

    /// Returns a cursor for testing membership of a *monotonically
    /// non-decreasing* stream of keys. Each query is O(1) amortized -- a
    /// forward merge over the set's sorted intervals -- rather than the
    /// O(log n) of [`Self::contains`]. Keys MUST be queried in ascending
    /// order; querying out of order yields incorrect results.
    pub fn membership_cursor(&self) -> MembershipCursor<impl Iterator<Item = Interval> + '_> {
        MembershipCursor::new(self.iter())
    }
}

/// Cursor for membership tests over an ascending stream of keys, returned by
/// [`IntervalSet::membership_cursor`].
pub struct MembershipCursor<I: Iterator<Item = Interval>> {
    intervals: I,
    current: Option<Interval>,
}

impl<I: Iterator<Item = Interval>> MembershipCursor<I> {
    fn new(mut intervals: I) -> Self {
        let current = intervals.next();
        Self { intervals, current }
    }

    /// Returns whether `key` is in the set. `key` must be greater than or equal
    /// to every previously queried key.
    pub fn contains(&mut self, key: &[u8]) -> bool {
        // Advance past every interval that ends at or before `key`: since keys
        // are non-decreasing, those intervals can't match this or any later
        // query.
        while self
            .current
            .as_ref()
            .is_some_and(|interval| !interval.as_ref().end.greater_than(key))
        {
            self.current = self.intervals.next();
        }
        self.current
            .as_ref()
            .is_some_and(|interval| interval.contains(key))
    }
}

impl HeapSize for IntervalSet {
    fn heap_size(&self) -> usize {
        match self {
            Self::All => 0,
            Self::Intervals(intervals) => intervals.heap_size(),
        }
    }
}
