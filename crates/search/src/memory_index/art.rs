use std::{
    borrow::Borrow,
    fmt::{
        Debug,
        Formatter,
    },
    marker::PhantomData,
};

use imbl_slab::{
    Slab,
    SlabKey,
};
use itertools::Itertools;

/// Radix trees must store values associated with keys in the tree. There are
/// two simple ways to do this: store values within internal tree nodes or have
/// dedicated leaf nodes. The former allows for keys to be prefixes of each
/// other in the tree while the latter does not and also comes with one extra
/// pointer indirection for accessing leaves, at the cost of storing an extra
/// 8-byte pointer per internal node.
///
/// ART chooses to optimize for space and save on those 8-bytes per-node. We
/// choose not to do this. To use ARTs strategy here, just need to change the
/// Leaf variant.
///
/// Node4 and Node16 do not keep their children sorted, so `iter_prefix` sorts
/// each node's children as it visits them.
#[derive(Debug, Clone)]
enum ARTNode<V: Clone> {
    Leaf(NodeRef<V, 0, 0>),
    Node4(NodeRef<V, 4, 4>),
    Node16(NodeRef<V, 16, 16>),
    Node48(NodeRef<V, 256, 48>),
    Node256(NodeRef<V, 0, 256>),
}

impl<V: Clone> ARTNode<V> {
    /// Finds index of the byte if it exists as a child transition
    fn index(&self, byte: u8) -> Option<usize> {
        match self {
            Self::Leaf(_) => None,
            Self::Node4(n) => n.0.linear_search_child(byte),
            // TODO: sort and binary search. SIMD?
            Self::Node16(n) => n.0.linear_search_child(byte),
            Self::Node48(n) => n.0.keys[byte as usize].map(|i| i as usize),
            Self::Node256(n) => n.0.children[byte as usize].map(|_| byte as usize),
        }
    }

    /// Finds a pointer to the child node identified by the supplied byte, if it
    /// exists
    fn find_child(&self, byte: u8) -> Option<SlabKey> {
        let idx = self.index(byte);
        match self {
            Self::Leaf(_) => None,
            Self::Node4(n) => idx.and_then(|idx| n.0.children[idx]),
            Self::Node16(n) => idx.and_then(|idx| n.0.children[idx]),
            Self::Node48(n) => idx.and_then(|idx| n.0.children[idx]),
            Self::Node256(n) => n.0.children[byte as usize],
        }
    }

    /// Adds child to node. Node must have room left - call `grow` first if
    /// needed.
    fn add_child(&mut self, byte: u8, child_key: SlabKey) {
        match self {
            Self::Leaf(_) => panic!("cannot add child to leaf. Must grow first!"),
            Self::Node4(n) => {
                for i in 0..4 {
                    if n.0.keys[i].is_none() {
                        n.0.keys[i] = Some(byte);
                        n.0.children[i] = Some(child_key);
                        n.0.meta.num_children += 1;
                        return;
                    }
                }
            },
            Self::Node16(n) => {
                for i in 0..16 {
                    if n.0.keys[i].is_none() {
                        n.0.keys[i] = Some(byte);
                        n.0.children[i] = Some(child_key);
                        n.0.meta.num_children += 1;
                        return;
                    }
                }
            },
            Self::Node48(n) => {
                debug_assert!(n.0.keys[byte as usize].is_none());
                for i in 0..48 {
                    // Find an empty child slot
                    if n.0.children[i].is_none() {
                        n.0.keys[byte as usize] = Some(i as u8);
                        n.0.children[i] = Some(child_key);
                        n.0.meta.num_children += 1;
                        return;
                    }
                }
            },
            Self::Node256(n) => {
                n.0.children[byte as usize] = Some(child_key);
                n.0.meta.num_children += 1;
            },
        }
    }

    /// Removes child from node, if it exists.
    fn remove_child(&mut self, byte: u8) {
        let idx = self.index(byte);
        match self {
            Self::Leaf(_) => panic!("cannot remove child from leaf"),
            Self::Node4(n) => {
                if let Some(idx) = idx {
                    n.0.keys[idx] = None;
                    n.0.children[idx] = None;
                    n.0.meta.num_children -= 1;
                }
            },
            Self::Node16(n) => {
                if let Some(idx) = idx {
                    n.0.keys[idx] = None;
                    n.0.children[idx] = None;
                    n.0.meta.num_children -= 1;
                }
            },
            Self::Node48(n) => {
                if let Some(idx) = n.0.keys[byte as usize] {
                    n.0.keys[byte as usize] = None;
                    n.0.children[idx as usize] = None;
                    n.0.meta.num_children -= 1;
                }
            },
            Self::Node256(n) => {
                n.0.children[byte as usize] = None;
                n.0.meta.num_children -= 1;
            },
        }
    }

    /// Demotes an ART node to the next smallest type. This should only be
    /// called if the node has < the minimum number of children for that
    /// node type. Panics for leaves.
    fn shrink(self) -> Self {
        match self {
            Self::Leaf(_) => {
                panic!("Should never try to shrink a leaf node");
            },
            Self::Node4(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                assert_eq!(meta.num_children, 0);

                let node = NodeRef::new(meta.prefix, value);
                ARTNode::Leaf(node)
            },
            Self::Node16(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                assert_eq!(meta.num_children, 4);
                let mut children = [None; 4];
                let mut keys = [None; 4];

                let mut idx = 0;
                for i in 0..16 {
                    if n.0.keys[i].is_some() {
                        keys[idx] = n.0.keys[i];
                        children[idx] = n.0.children[i];
                        idx += 1;
                    }
                }

                let node = Node {
                    keys,
                    children,
                    value,
                    meta,
                };
                ARTNode::Node4(NodeRef::from(node))
            },
            Self::Node48(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                assert_eq!(meta.num_children, 16);
                let mut children = [None; 16];
                let mut keys = [None; 16];

                let mut idx = 0;
                for i in 0..256 {
                    if let Some(key) = n.0.keys[i] {
                        keys[idx] = Some(i as u8);
                        children[idx] = n.0.children[key as usize];
                        idx += 1;
                    }
                }
                let node = Node {
                    keys,
                    children,
                    value,
                    meta,
                };
                ARTNode::Node16(NodeRef::from(node))
            },
            Self::Node256(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                assert_eq!(meta.num_children, 48);
                let mut children = [None; 48];
                let mut keys = [None; 256];

                let mut idx = 0;
                for (i, value) in n.0.children.iter().enumerate() {
                    if let Some(value) = value {
                        keys[i] = Some(idx as u8);
                        children[idx] = Some(*value);
                        idx += 1;
                    }
                }

                let node = Node {
                    keys,
                    children,
                    value,
                    meta,
                };
                ARTNode::Node48(NodeRef::from(node))
            },
        }
    }

    /// Promotes the node to the next larger size.
    fn grow(self) -> Self {
        match self {
            Self::Leaf(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                let node = Node {
                    children: [None; 4],
                    keys: [None; 4],
                    value,
                    meta,
                };
                ARTNode::Node4(NodeRef::from(node))
            },
            Self::Node4(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                let mut children = [None; 16];
                let mut keys = [None; 16];

                keys[..4].copy_from_slice(&n.0.keys[..4]);
                children[..4].copy_from_slice(&n.0.children[..4]);

                let node = Node {
                    children,
                    keys,
                    value,
                    meta,
                };
                ARTNode::Node16(NodeRef::from(node))
            },
            Self::Node16(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                let mut children = [None; 48];
                let mut keys = [None; 256];

                for i in 0..16 {
                    keys[n.0.keys[i as usize].unwrap() as usize] = Some(i);
                    children[i as usize] = n.0.children[i as usize];
                }

                let node = Node {
                    children,
                    keys,
                    value,
                    meta,
                };
                ARTNode::Node48(NodeRef::from(node))
            },
            Self::Node48(n) => {
                let meta = n.0.meta;
                let value = n.0.value;
                let mut children = [None; 256];

                for i in 0..=255_u8 {
                    if let Some(key) = n.0.keys[i as usize] {
                        children[i as usize] = n.0.children[key as usize];
                    }
                }

                let node = Node {
                    children,
                    keys: [None; 0],
                    value,
                    meta,
                };
                ARTNode::Node256(NodeRef::from(node))
            },
            Self::Node256(_) => panic!("Cannot grow Node256"),
        }
    }

    fn get_meta(&self) -> &NodeMetadata {
        match self {
            Self::Leaf(n) => &n.0.meta,
            Self::Node4(n) => &n.0.meta,
            Self::Node16(n) => &n.0.meta,
            Self::Node48(n) => &n.0.meta,
            Self::Node256(n) => &n.0.meta,
        }
    }

    fn get_meta_mut(&mut self) -> &mut NodeMetadata {
        match self {
            Self::Leaf(n) => &mut n.0.meta,
            Self::Node4(n) => &mut n.0.meta,
            Self::Node16(n) => &mut n.0.meta,
            Self::Node48(n) => &mut n.0.meta,
            Self::Node256(n) => &mut n.0.meta,
        }
    }

    fn is_full(&self) -> bool {
        match self {
            Self::Leaf(_) => true,
            Self::Node4(n) => n.0.meta.num_children == 4,
            Self::Node16(n) => n.0.meta.num_children == 16,
            Self::Node48(n) => n.0.meta.num_children == 48,
            Self::Node256(n) => n.0.meta.num_children == 256,
        }
    }

    fn is_underfull(&self) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Node4(n) => n.0.meta.num_children == 0,
            Self::Node16(n) => n.0.meta.num_children == 4,
            Self::Node48(n) => n.0.meta.num_children == 16,
            Self::Node256(n) => n.0.meta.num_children == 48,
        }
    }

    fn get_value(&self) -> Option<&V> {
        match self {
            Self::Leaf(n) => n.0.value.as_ref(),
            Self::Node4(n) => n.0.value.as_ref(),
            Self::Node16(n) => n.0.value.as_ref(),
            Self::Node48(n) => n.0.value.as_ref(),
            Self::Node256(n) => n.0.value.as_ref(),
        }
    }

    fn get_value_mut(&mut self) -> &mut Option<V> {
        match self {
            Self::Leaf(n) => &mut n.0.value,
            Self::Node4(n) => &mut n.0.value,
            Self::Node16(n) => &mut n.0.value,
            Self::Node48(n) => &mut n.0.value,
            Self::Node256(n) => &mut n.0.value,
        }
    }

    fn replace_value(&mut self, value: V) -> Option<V> {
        self.get_value_mut().replace(value)
    }

    /// Gets the single child of this node if it has only 1 child and is
    /// not value-bearing.
    fn get_child_if_alone(&self) -> Option<(u8, SlabKey)> {
        if let ARTNode::Node4(n) = self
            && n.0.meta.num_children == 1
            && n.0.value.is_none()
        {
            // Find the index of the singular child
            let child_idx =
                n.0.children
                    .iter()
                    .find_position(|c| c.is_some())
                    .map(|pair| pair.0);

            // These unwraps are all safe assuming `meta.num_children` is maintained
            // correctly and `children` and `keys` are consistent. These unwraps
            // failing would indicate a logic error.
            let child_idx = child_idx.unwrap();
            let child_key = n.0.children[child_idx].unwrap();
            let child_byte = n.0.keys[child_idx].unwrap();

            Some((child_byte, child_key))
        } else {
            None
        }
    }

    fn iter_children(&self) -> impl Iterator<Item = (u8, SlabKey)> + '_ {
        std::iter::from_coroutine(
            #[coroutine]
            move || match self {
                Self::Leaf(_) => (),
                Self::Node4(n) => {
                    for i in 0..4 {
                        if let Some(byte) = n.0.keys[i] {
                            yield (byte, n.0.children[i].expect("child missing from Node4"))
                        }
                    }
                },
                Self::Node16(n) => {
                    for i in 0..16 {
                        if let Some(byte) = n.0.keys[i] {
                            yield (byte, n.0.children[i].expect("child missing from Node16"))
                        }
                    }
                },
                Self::Node48(n) => {
                    for i in 0..=255 {
                        if let Some(idx) = n.0.keys[i] {
                            yield (
                                i as u8,
                                n.0.children[idx as usize].expect("child missing from Node16"),
                            )
                        }
                    }
                },
                Self::Node256(n) => {
                    for i in 0..=255 {
                        if let Some(key) = n.0.children[i] {
                            yield (i as u8, key)
                        }
                    }
                },
            },
        )
    }
}

/// Counts the shared prefix size
fn max_shared_prefix(a: &[u8], b: &[u8]) -> usize {
    let (a, b) = if a.len() <= b.len() { (a, b) } else { (b, a) };

    for i in 0..a.len() {
        if a[i] != b[i] {
            return i;
        }
    }
    a.len()
}

#[derive(Clone)]
#[repr(transparent)]
struct NodeRef<V: Clone, const KEYS: usize, const CHILDREN: usize>(Box<Node<V, KEYS, CHILDREN>>);

impl<V: Clone + Debug, const KEYS: usize, const CHILDREN: usize> Debug
    for NodeRef<V, KEYS, CHILDREN>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<V: Clone, const KEYS: usize, const CHILDREN: usize> NodeRef<V, KEYS, CHILDREN> {
    fn new(prefix: Box<[u8]>, value: Option<V>) -> Self {
        Self(Box::new(Node {
            keys: [None; KEYS],
            children: [None; CHILDREN],
            value,
            meta: NodeMetadata {
                num_children: 0,
                prefix,
            },
        }))
    }
}

impl<V: Clone, const KEYS: usize, const CHILDREN: usize> From<Node<V, KEYS, CHILDREN>>
    for NodeRef<V, KEYS, CHILDREN>
{
    fn from(value: Node<V, KEYS, CHILDREN>) -> Self {
        Self(Box::new(value))
    }
}

#[derive(Clone)]
struct Node<V: Clone, const KEYS: usize, const CHILDREN: usize> {
    keys: [Option<u8>; KEYS],
    children: [Option<SlabKey>; CHILDREN],
    value: Option<V>,
    meta: NodeMetadata,
}

impl<V: Clone + Debug, const KEYS: usize, const CHILDREN: usize> Debug for Node<V, KEYS, CHILDREN> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let keys = self
            .keys
            .iter()
            .enumerate()
            .filter_map(|(i, key)| key.map(|key| (i, key)))
            .collect_vec();
        let children = self
            .children
            .iter()
            .enumerate()
            .filter_map(|(i, key)| key.map(|key| (i, key)))
            .collect_vec();
        write!(
            f,
            "value: {:?}, keys: {:?}, children: {:?}, prefix: {:?}",
            self.value, keys, children, self.meta.prefix
        )
    }
}

impl<V: Clone, const KEYS: usize, const CHILDREN: usize> Node<V, KEYS, CHILDREN> {
    /// Don't break from the loop, to allow for loop unrolling. Should only be
    /// used for small Nodes (KEYS <= 16).
    ///
    /// Ideally, we could express this with feature(generic_const_exprs)
    fn linear_search_child(&self, byte: u8) -> Option<usize> {
        let mut idx = None;
        for i in 0..KEYS {
            if self.keys[i] == Some(byte) {
                idx = Some(i);
            }
        }
        idx
    }
}

#[derive(Debug, Clone)]
struct NodeMetadata {
    num_children: u16,
    prefix: Box<[u8]>,
}

/// Copy-on-write Adaptive Radix Tree implementation: https://db.in.tum.de/~leis/papers/ART.pdf
#[derive(Debug, Clone)]
pub struct ART<K: AsRef<[u8]>, V: Clone> {
    nodes: Slab<Option<ARTNode<V>>>,
    root: Option<SlabKey>,
    _marker: PhantomData<K>,
}

impl<K: AsRef<[u8]>, V: Clone> ART<K, V> {
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            _marker: PhantomData,
        }
    }

    fn get_validated_node(&self, key: SlabKey) -> &ARTNode<V> {
        self.get_validated_maybe_node(key)
            .expect("ARTNode found but was uninit")
    }

    fn get_validated_node_mut(&mut self, key: SlabKey) -> &mut ARTNode<V> {
        self.get_validated_maybe_node_mut(key)
            .as_mut()
            .expect("ARTNode found but was uninit")
    }

    fn get_validated_maybe_node(&self, key: SlabKey) -> Option<&ARTNode<V>> {
        self.nodes
            .get(key)
            .expect("ARTNode not found; SlabKey invalid")
            .as_ref()
    }

    fn get_validated_maybe_node_mut(&mut self, key: SlabKey) -> &mut Option<ARTNode<V>> {
        self.nodes
            .get_mut(key)
            .expect("ARTNode not found; SlabKey invalid")
    }

    fn alloc_init(&mut self, value: ARTNode<V>) -> SlabKey {
        self.nodes.alloc(Some(value))
    }

    fn free_expect_init(&mut self, key: SlabKey) -> ARTNode<V> {
        self.nodes
            .free(key)
            .expect("Tried to free uninitialized ARTNode")
    }

    /// Inserts into trie, growing nodes as needed.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let key_bytes = key.as_ref();

        // Insert at root if no root exists
        let Some(mut curr_node_key) = self.root else {
            self.root =
                Some(self.alloc_init(ARTNode::Leaf(NodeRef::new(key_bytes.into(), Some(value)))));
            return None;
        };

        // Recurse.
        // `depth` is the length of the prefix in `key_bytes` that we've successfully
        // matched. Note: it is possible for `depth == key_bytes.len()` at the
        // loop start due to prefix compression.
        let mut depth = 0;
        loop {
            let curr_node = self.get_validated_node_mut(curr_node_key);
            let curr_node_prefix = curr_node.get_meta().prefix.as_ref();
            let key_slice = &key_bytes[depth..];

            let common_prefix_len = max_shared_prefix(curr_node_prefix, key_slice);

            // Prefix mismatch: construct a new parent node with the common prefix, and
            // add these two nodes as children
            if common_prefix_len < curr_node_prefix.len() {
                let new_parent_leaf_byte = curr_node_prefix[common_prefix_len];
                let new_parent_leaf_prefix = curr_node_prefix[common_prefix_len + 1..].into();

                // Case 1: key_slice is a prefix of curr_node_prefix so only 1 node needs to be
                // allocated
                if common_prefix_len == key_slice.len() {
                    // This will replace the current node
                    let new_curr = ARTNode::Node4(NodeRef::new(key_slice.into(), Some(value)));

                    // Move and alloc the current node to be a new leaf node with the new prefix
                    let mut new_leaf = std::mem::replace(curr_node, new_curr);
                    new_leaf.get_meta_mut().prefix = new_parent_leaf_prefix;
                    let new_leaf_key = self.alloc_init(new_leaf);

                    // Refetch the new current node since we consumed it to add the child
                    let new_curr = self.get_validated_node_mut(curr_node_key);
                    new_curr.add_child(new_parent_leaf_byte, new_leaf_key);
                }
                // Case 2: key_slice is not a prefix so 2 nodes must be allocated
                else {
                    let new_curr =
                        ARTNode::Node4(NodeRef::new(key_slice[..common_prefix_len].into(), None));

                    let mut new_parent_leaf = std::mem::replace(curr_node, new_curr);
                    new_parent_leaf.get_meta_mut().prefix = new_parent_leaf_prefix;
                    let new_parent_leaf_key = self.alloc_init(new_parent_leaf);

                    let new_key_leaf = ARTNode::Leaf(NodeRef::new(
                        key_slice[common_prefix_len + 1..].into(),
                        Some(value),
                    ));
                    let new_key_leaf_key = self.alloc_init(new_key_leaf);

                    let new_curr = self.get_validated_node_mut(curr_node_key);
                    new_curr.add_child(key_slice[common_prefix_len], new_key_leaf_key);
                    new_curr.add_child(new_parent_leaf_byte, new_parent_leaf_key);
                }
                return None;
            }

            // See if the key bytes are exhausted. If so, insert and we're done.
            if depth + common_prefix_len == key_bytes.len() {
                return curr_node.replace_value(value);
            }

            // Prefix matched but there's still more bytes to go.
            // We should try to descend if a transition exists
            debug_assert!(depth + common_prefix_len < key_bytes.len());
            let next = curr_node.find_child(key_bytes[depth + common_prefix_len]);
            depth += common_prefix_len + 1;

            if let Some(next) = next {
                // The next byte transition exists, descend.
                curr_node_key = next;
            } else {
                // Byte transition does not exist; grow if needed and insert the leaf
                if curr_node.is_full() {
                    let curr_node = self.get_validated_maybe_node_mut(curr_node_key);
                    let node = curr_node
                        .take()
                        .expect("Invariant broken; node must be init to grow");
                    let new_node = node.grow();
                    *curr_node = Some(new_node);
                }

                let leaf = ARTNode::Leaf(NodeRef::new(key_bytes[depth..].into(), Some(value)));
                let leaf_key = self.alloc_init(leaf);

                let curr_node = self.get_validated_node_mut(curr_node_key);
                curr_node.add_child(key_bytes[depth - 1], leaf_key);
                return None;
            }
        }
    }

    /// Seeks to the given node, calling `func` for every node encountered
    ///
    /// The transition passed to the closure will be None iff SlabKey ==
    /// self.root
    /// TODO: consider simplifying `seek` by removing the closure and returning
    /// the final seek metadata
    fn seek(&self, key_bytes: &[u8], mut func: impl FnMut(SlabKey, Option<u8>, usize)) -> bool {
        let Some(mut curr_node_key) = self.root else {
            return false;
        };

        let mut depth = 0;
        let mut last_transition = None;
        loop {
            // Call closure
            func(curr_node_key, last_transition, depth);

            let curr_node = self.get_validated_node(curr_node_key);
            let curr_node_prefix = curr_node.get_meta().prefix.as_ref();
            let key_slice = &key_bytes[depth..];

            let common_prefix_len = max_shared_prefix(curr_node_prefix, key_slice);
            if common_prefix_len != curr_node_prefix.len() {
                return false;
            }

            if depth + common_prefix_len == key_bytes.len() {
                return true;
            }

            // Prefix matched but there's still more bytes to go.
            // We should try to descend if a transition exists
            debug_assert!(depth + common_prefix_len < key_bytes.len());
            let next = curr_node.find_child(key_bytes[depth + common_prefix_len]);
            last_transition = Some(key_bytes[depth + common_prefix_len]);
            depth += common_prefix_len + 1;

            if let Some(next) = next {
                // The next byte transition exists, descend.
                curr_node_key = next;
            } else {
                return false;
            }
        }
    }

    /// Used to collapse parent into child during deletion, to avoid
    /// the case of a non-value-bearing node with only 1 child. This case is
    /// undesirable since the parent node is redundant as a strict prefix
    /// that doesn't need distinction since the node doesn't store a value.
    fn collapse_node_into_child(
        &mut self,
        parent_key: SlabKey,
        child_byte: u8,
        child_key: SlabKey,
    ) {
        // Free the child slot to move the child into the parent slot
        let mut child = self.free_expect_init(child_key);

        // Get the parent entry and `take` its value so we can replace it with the child
        // we just freed
        let parent_entry = self.get_validated_maybe_node_mut(parent_key);
        let parent_node = parent_entry
            .take()
            .expect("Invariant broken: child did not exist");

        // Build the combined prefix
        let prefix = parent_node.get_meta().prefix.clone();
        let joined_prefix = prefix
            .iter()
            .chain(std::iter::once(&child_byte))
            .chain(child.get_meta().prefix.iter())
            .cloned()
            .collect_vec();

        // Set the prefix and set the current entry to the child node we freed with new
        // prefix
        child.get_meta_mut().prefix = joined_prefix.into();
        *parent_entry = Some(child);
    }

    /// Removes key from trie, shrinking nodes as needed.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: AsRef<[u8]> + ?Sized,
    {
        // We need to keep track of the last 2 states encountered during `seek`
        let mut parent_state_and_transition = None;
        let mut curr_state = self.root?;

        let seek_succeeded = self.seek(key.as_ref(), |state, last_transition, _| {
            parent_state_and_transition = Some((curr_state, last_transition));
            curr_state = state;
        });
        if seek_succeeded {
            // We found the node corresponding to the key, let's delete its value if it
            // exists.
            let child = self.get_validated_node_mut(curr_state);
            let prev_value = child.get_value_mut().take()?;

            // If the child node is a leaf, delete it.
            if let ARTNode::Leaf(_) = child {
                self.free_expect_init(curr_state);

                if let Some((parent_state, Some(transition))) = parent_state_and_transition {
                    let parent = self.get_validated_node_mut(parent_state);
                    parent.remove_child(transition);

                    // The parent may now be underfull since we deleted a child. If so, shrink it.
                    if parent.is_underfull() {
                        let parent = self.get_validated_maybe_node_mut(parent_state);
                        let parent_node = parent
                            .take()
                            .expect("Invariant broken: parent node deoesn't exist");
                        let new_parent_node = parent_node.shrink();
                        *parent = Some(new_parent_node);
                    }
                    // If parent is not underfull, it is possibly a Node4 with an alone child,
                    // in which case we should collapse it to reduce tree height
                    else if let Some((child_byte, child_key)) = parent.get_child_if_alone() {
                        self.collapse_node_into_child(parent_state, child_byte, child_key);
                    }
                } else {
                    // We just deleted root, set this.
                    self.root = None;
                }
            }
            // Otherwise, if n has exactly one node left, we can collapse the prefix into child,
            // since this node is just storing the strict prefix of another key.
            else if let Some((child_byte, child_key)) = child.get_child_if_alone() {
                self.collapse_node_into_child(curr_state, child_byte, child_key);
            }
            // If we get here, the node we're removing a value from must have more than 1 child
            // so we shouldn't collapse or shrink it
            else {
                debug_assert!(child.get_meta().num_children > 1);
            }

            Some(prev_value)
        } else {
            None
        }
    }

    /// Get the bytes corresponding to the supplied key if exists in trie.
    ///
    /// Implementation-wise, this is a simpler version of `insert` since no
    /// need to branch nodes.
    ///
    /// We can just use `seek` and use the last element in the trail.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: AsRef<[u8]> + ?Sized,
    {
        let mut last_state = self.root?;
        let seek_succeeded = self.seek(key.as_ref(), |state, _, _| {
            last_state = state;
        });
        if seek_succeeded {
            self.get_validated_node(last_state).get_value()
        } else {
            None
        }
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: ?Sized + AsRef<[u8]>,
    {
        let mut last_state = self.root?;

        let seek_succeeded = self.seek(key.as_ref(), |state, _, _| {
            last_state = state;
        });
        if seek_succeeded {
            self.get_validated_node_mut(last_state)
                .get_value_mut()
                .as_mut()
        } else {
            None
        }
    }

    pub fn iter_values(&self) -> impl Iterator<Item = &V> + '_ {
        std::iter::from_coroutine(
            #[coroutine]
            move || {
                let Some(curr) = self.root else {
                    return;
                };

                let mut stack = vec![curr];
                while let Some(key) = stack.pop() {
                    let curr_node = self.get_validated_node(key);
                    if let Some(value) = curr_node.get_value() {
                        yield value;
                    }

                    for (_, child_key) in curr_node.iter_children() {
                        stack.push(child_key);
                    }
                }
            },
        )
    }

    pub fn len(&self) -> usize {
        self.iter_values().count()
    }

    /// Iterates over the entries whose keys start with `prefix` in
    /// lexicographic key order, yielding each value with its full key.
    pub fn iter_prefix<'a>(
        &'a self,
        prefix: &'a [u8],
    ) -> impl Iterator<Item = (&'a V, Vec<u8>)> + 'a {
        std::iter::from_coroutine(
            #[coroutine]
            move || {
                let Some(mut node_key) = self.root else {
                    return;
                };

                // Descend to the shallowest node whose key extends `prefix`. `path` holds the
                // key bytes above `node_key`.
                let mut path = Vec::with_capacity(prefix.len());
                loop {
                    let node = self.get_validated_node(node_key);
                    let node_prefix = node.get_meta().prefix.as_ref();
                    let remaining = &prefix[path.len()..];
                    let shared = max_shared_prefix(node_prefix, remaining);
                    if shared == remaining.len() {
                        break;
                    }
                    if shared < node_prefix.len() {
                        return;
                    }
                    path.extend_from_slice(node_prefix);
                    let transition = prefix[path.len()];
                    let Some(child_key) = node.find_child(transition) else {
                        return;
                    };
                    path.push(transition);
                    node_key = child_key;
                }

                // A preorder traversal that visits children in byte order yields keys in
                // lexicographic order. Each stack entry carries the length of `path` at its
                // parent so siblings can truncate back to it.
                let mut stack = vec![(node_key, None::<u8>, path.len())];
                while let Some((node_key, transition, parent_path_len)) = stack.pop() {
                    path.truncate(parent_path_len);
                    path.extend(transition);
                    let node = self.get_validated_node(node_key);
                    path.extend_from_slice(&node.get_meta().prefix);
                    if let Some(value) = node.get_value() {
                        yield (value, path.clone());
                    }

                    // Node4 and Node16 keep children in insertion order.
                    let mut children = node.iter_children().collect_vec();
                    children.sort_unstable_by_key(|(byte, _)| *byte);
                    let path_len = path.len();
                    stack.extend(
                        children
                            .into_iter()
                            .rev()
                            .map(|(byte, child_key)| (child_key, Some(byte), path_len)),
                    );
                }
            },
        )
    }
}
