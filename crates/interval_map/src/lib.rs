#![feature(impl_trait_in_assoc_type)]
#![allow(clippy::manual_flatten)]
#![allow(clippy::collapsible_else_if)]

use std::{
    collections::HashMap,
    num::NonZeroU32,
    ops::{
        Index,
        IndexMut,
    },
};

use common::{
    interval::{
        End,
        Interval,
        StartIncluded,
    },
    types::SubscriberId,
};
use fastrand::Rng;
use slab::Slab;

/// A data structure storing a set of (possibly overlapping) [Interval]s, that
/// can efficiently query which intervals overlap a given point.
///
/// This is implemented as a treap ordered by `interval.start`, and with an
/// annotation on each subtree recording the maximum `interval.end` in that
/// subtree.
pub struct IntervalMap {
    nodes: Slab<Node>,
    root: Option<NodeKey>,
    subscribers: HashMap<SubscriberId, Option<NodeKey>>,
    rng: Rng,
}

// TODO: the node layout could be optimized
struct Node {
    // These fields are "immutable"
    weight: u32,        // treap property: a node's weight is minimal within its subtree
    key: StartIncluded, // BST key, also the lower bound of the interval
    upper_bound: End,
    subscriber: SubscriberId,

    // These form the binary tree structure
    parent: Option<NodeKey>,
    child: [Option<NodeKey>; 2],

    // Points to the `Node` with the greatest `upper_bound` in the subtree
    // rooted at this node
    max_upper_bound: NodeKey,

    // This forms a linked list of nodes with the same `subscriber`
    next: Option<NodeKey>,
}
impl Node {
    /// Panics if `from` is not a child of `self`
    fn replace_child(&mut self, from: NodeKey, to: Option<NodeKey>) {
        if self.child[0] == Some(from) {
            self.child[0] = to;
        } else {
            assert_eq!(self.child[1], Some(from));
            self.child[1] = to;
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct NodeKey(NonZeroU32);

impl NodeKey {
    fn new(index: usize) -> Result<Self, TooFull> {
        if let Some(inner) = u32::try_from(index + 1).ok().and_then(NonZeroU32::new) {
            Ok(Self(inner))
        } else {
            Err(TooFull)
        }
    }

    fn key(&self) -> usize {
        self.0.get() as usize - 1
    }
}

impl Index<NodeKey> for Slab<Node> {
    type Output = Node;

    fn index(&self, index: NodeKey) -> &Self::Output {
        &self[index.key()]
    }
}

impl IndexMut<NodeKey> for Slab<Node> {
    fn index_mut(&mut self, index: NodeKey) -> &mut Self::Output {
        &mut self[index.0.get() as usize - 1]
    }
}

#[derive(Debug)]
pub struct TooFull;

impl IntervalMap {
    #[inline]
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            subscribers: HashMap::new(),
            rng: Rng::new(),
        }
    }

    /// Returns the number of subscribers (_not_ intervals) registered in the
    /// map
    #[inline]
    pub fn subscriber_len(&self) -> usize {
        self.subscribers.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.subscribers.is_empty()
    }

    /// Adds the given intervals under the provided `subscriber`. Panics if
    /// `subscriber` is already registered.
    pub fn insert(
        &mut self,
        subscriber: SubscriberId,
        intervals: impl IntoIterator<Item = Interval>,
    ) -> Result<(), TooFull> {
        assert!(
            self.subscribers.insert(subscriber, None).is_none(),
            "double-inserted subscriber {subscriber}"
        );
        for interval in intervals {
            let weight = self.rng.u32(..);
            if let Err(TooFull) = self.insert_interval(subscriber, interval, weight) {
                self.remove(subscriber);
                return Err(TooFull);
            }
        }
        Ok(())
    }

    fn insert_interval(
        &mut self,
        subscriber: SubscriberId,
        interval: Interval,
        weight: u32,
    ) -> Result<(), TooFull> {
        let key = NodeKey::new(self.nodes.vacant_key())?;
        let sub = self
            .subscribers
            .get_mut(&subscriber)
            .expect("unknown subscriber");
        let node = Node {
            weight,
            key: interval.start,
            upper_bound: interval.end,
            parent: None,
            child: [None; 2],
            max_upper_bound: key,
            next: sub.replace(key),
            subscriber,
        };
        match self.root {
            None => {
                self.nodes.insert(node);
                self.root = Some(key);
            },
            Some(root) => self.insert_node(root, key, node),
        }
        Ok(())
    }

    fn insert_node(&mut self, mut parent: NodeKey, node_key: NodeKey, mut node: Node) {
        loop {
            if node.weight < self.nodes[parent].weight {
                // to maintain heap ordering, `node` replaces `parent` in the tree
                let grandparent = self.nodes[parent].parent;
                if let Some(grandparent) = grandparent {
                    self.nodes[grandparent].replace_child(parent, Some(node_key));
                    node.parent = Some(grandparent);
                } else {
                    debug_assert_eq!(self.root, Some(parent));
                    debug_assert_eq!(node.parent, None);
                    self.root = Some(node_key);
                }
                assert_eq!(self.nodes.insert(node), node_key.key());
                self.split(parent, node_key);
                break;
            } else {
                // `node` is going to become a descendant of `parent`, so update
                // its `max_upper_bound` annotation
                if node.upper_bound > self.nodes[self.nodes[parent].max_upper_bound].upper_bound {
                    self.nodes[parent].max_upper_bound = node_key;
                }
                // N.B.: if `key` is already present, we always insert the node at the
                // end of the range of equal keys
                let child = if node.key < self.nodes[parent].key {
                    0
                } else {
                    1
                };
                if let Some(child_node) = self.nodes[parent].child[child] {
                    parent = child_node;
                } else {
                    self.nodes[parent].child[child] = Some(node_key);
                    node.parent = Some(parent);
                    assert_eq!(self.nodes.insert(node), node_key.key());
                    break;
                }
            }
        }
    }

    /// Inserts the subtree rooted at `node` into `dest`; this effectively
    /// splits the subtree into one where all keys are `<= dest.key` and one
    /// `> dest.key`
    fn split(&mut self, mut node: NodeKey, dest: NodeKey) {
        debug_assert_eq!(self.nodes[dest].child[0], None);
        debug_assert_eq!(self.nodes[dest].child[1], None);
        debug_assert!(self.nodes[dest].weight < self.nodes[node].weight);
        // `l` is the rightmost node left of `dest`;
        // `r` is the leftmost node right of `dest`;
        // except if there is no such node, it points at `dest` itself.
        let (mut l, mut r) = (dest, dest);
        loop {
            // Take apart `node` and figure out which side of `dest` it belongs on.
            if self.nodes[node].key <= self.nodes[dest].key {
                debug_assert_eq!(self.nodes[l].child[1], None);
                self.nodes[node].parent = Some(l);
                self.nodes[l].child[1] = Some(node);
                l = node;
                if let Some(child) = self.nodes[node].child[1].take() {
                    node = child;
                } else {
                    break;
                }
            } else {
                debug_assert_eq!(self.nodes[r].child[0], None);
                self.nodes[node].parent = Some(r);
                self.nodes[r].child[0] = Some(node);
                r = node;
                if let Some(child) = self.nodes[node].child[0].take() {
                    node = child;
                } else {
                    break;
                }
            }
        }
        debug_assert_eq!(self.nodes[l].child[1], None);
        debug_assert_eq!(self.nodes[r].child[0], None);
        // Because we inserted into the *right* child of `l` and vice versa, the
        // children of dest are actually swapped, so unswap them.
        self.nodes[dest].child.swap(0, 1);
        // Now recalculate annotations upward on the branches that were modified.
        for mut p in [l, r] {
            while p != dest {
                self.recalculate_annotation(p);
                p = self.nodes[p]
                    .parent
                    .expect("should eventually root at `dest`");
            }
        }
        self.recalculate_annotation(dest);
    }

    /// Removes all intervals belonging to the given `subscriber` and frees that
    /// key. Panics if `subscriber` was not previously inserted.
    pub fn remove(&mut self, subscriber: SubscriberId) {
        let mut node = self
            .subscribers
            .remove(&subscriber)
            .expect("removed unknown subscriber");
        while let Some(n) = node {
            node = self.nodes[n].next.take();
            self.remove_in_place(n);
        }
    }

    /// Removes `n` from the binary tree structure and frees it from the slab
    fn remove_in_place(&mut self, n: NodeKey) {
        let new_child = self.merge(self.nodes[n].child[0], self.nodes[n].child[1]);
        let parent = self.nodes[n].parent;
        if let Some(c) = new_child {
            self.nodes[c].parent = parent;
        }
        if let Some(mut p) = parent {
            self.nodes[p].replace_child(n, new_child);
            loop {
                if self.nodes[p].max_upper_bound == n {
                    self.recalculate_annotation(p);
                }
                if let Some(gp) = self.nodes[p].parent {
                    p = gp;
                } else {
                    break;
                }
            }
        } else {
            self.root = new_child;
        }
        self.nodes.remove(n.key());
    }

    /// Merges the two subtrees into a single tree. This is order-preserving, so
    /// keys under `n` must be less than or equal to keys under `m`.
    fn merge(&mut self, n: Option<NodeKey>, m: Option<NodeKey>) -> Option<NodeKey> {
        let Some(n) = n else {
            return m;
        };
        let Some(m) = m else {
            return Some(n);
        };
        debug_assert!(self.nodes[n].key <= self.nodes[m].key);
        if self.nodes[n].weight <= self.nodes[m].weight {
            // n becomes the root
            let right = self.merge(self.nodes[n].child[1], Some(m));
            self.nodes[n].child[1] = right;
            if let Some(r) = right {
                self.nodes[r].parent = Some(n);
            }
            self.recalculate_annotation(n);
            Some(n)
        } else {
            // m becomes the root
            let left = self.merge(Some(n), self.nodes[m].child[0]);
            self.nodes[m].child[0] = left;
            if let Some(l) = left {
                self.nodes[l].parent = Some(m);
            }
            self.recalculate_annotation(m);
            Some(m)
        }
    }

    /// Recalculates `self.nodes[node].max_upper_bound`
    fn recalculate_annotation(&mut self, node: NodeKey) {
        let mut ix = node;
        for child in self.nodes[node].child {
            if let Some(c) = child {
                let m = self.nodes[c].max_upper_bound;
                if self.nodes[m].upper_bound > self.nodes[ix].upper_bound {
                    ix = m;
                }
            }
        }
        self.nodes[node].max_upper_bound = ix;
    }

    /// Calls `cb` for each interval in the map that overlaps `point`.
    ///
    /// Time complexity is on average `O((k + 1) log n)` where `k` is the number
    /// of returned intervals and `n` is the total number of intervals stored.
    pub fn query(&self, point: &[u8], mut cb: impl FnMut(SubscriberId)) {
        self.query_subtree(point, self.root, &mut cb, None);
    }

    fn query_subtree(
        &self,
        point: &[u8],
        node: Option<NodeKey>,
        cb: &mut impl FnMut(SubscriberId),
        last_upper_bound: Option<NodeKey>,
    ) {
        let Some(node) = node else {
            return;
        };
        let me = &self.nodes[node];
        let mub = me.max_upper_bound;
        if last_upper_bound == Some(mub) || self.nodes[mub].upper_bound.greater_than(point) {
            if me.key.as_ref() <= point {
                self.query_subtree(point, me.child[0], cb, Some(mub));
                if mub == node || me.upper_bound.greater_than(point) {
                    cb(me.subscriber);
                }
                self.query_subtree(point, me.child[1], cb, Some(mub));
            } else {
                self.query_subtree(point, me.child[0], cb, Some(mub));
            }
        }
    }

}

impl Default for IntervalMap {
    fn default() -> Self {
        Self::new()
    }
}
