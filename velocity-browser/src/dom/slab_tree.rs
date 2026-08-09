use std::collections::HashMap;

pub const SLAB_NODE_DIRTY: u32 = 1 << 0;
pub const SLAB_NODE_VISIBLE: u32 = 1 << 1;
pub const SLAB_NODE_FOCUSED: u32 = 1 << 2;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawSlabNode {
    pub slot_id: u32,
    pub parent_slot: u32,
    pub first_child_slot: u32,
    pub next_sibling_slot: u32,
    pub flags: u32,
    pub tag_hash: u64,
}

pub struct UnmanagedSlabArena {
    pub slots: Vec<RawSlabNode>,
    pub attributes: Vec<HashMap<String, String>>,
    pub text_content: Vec<String>,
    pub free_head: Option<u32>,
    pub allocated_count: usize,
}

impl UnmanagedSlabArena {
    pub fn with_capacity(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        let mut attributes = Vec::with_capacity(capacity);
        let mut text_content = Vec::with_capacity(capacity);

        for i in 0..capacity {
            slots.push(RawSlabNode {
                slot_id: i as u32,
                parent_slot: u32::MAX,
                first_child_slot: u32::MAX,
                next_sibling_slot: u32::MAX,
                flags: SLAB_NODE_VISIBLE,
                tag_hash: 0,
            });
            attributes.push(HashMap::new());
            text_content.push(String::new());
        }

        Self {
            slots,
            attributes,
            text_content,
            free_head: None,
            allocated_count: 0,
        }
    }

    pub fn allocate_node(&mut self, tag: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        tag.hash(&mut hasher);
        let hash = hasher.finish();

        let slot_id = self.slots.len() as u32;
        self.slots.push(RawSlabNode {
            slot_id,
            parent_slot: u32::MAX,
            first_child_slot: u32::MAX,
            next_sibling_slot: u32::MAX,
            flags: SLAB_NODE_VISIBLE | SLAB_NODE_DIRTY,
            tag_hash: hash,
        });
        self.attributes.push(HashMap::new());
        self.text_content.push(String::new());
        self.allocated_count += 1;
        slot_id
    }

    pub fn set_attribute(&mut self, slot_id: u32, key: &str, val: &str) {
        if (slot_id as usize) < self.slots.len() {
            self.attributes[slot_id as usize].insert(key.to_string(), val.to_string());
            self.slots[slot_id as usize].flags |= SLAB_NODE_DIRTY;
        }
    }

    pub fn mark_clean(&mut self, slot_id: u32) {
        if (slot_id as usize) < self.slots.len() {
            self.slots[slot_id as usize].flags &= !SLAB_NODE_DIRTY;
        }
    }
}

pub struct SlabDomTree {
    pub arena: UnmanagedSlabArena,
    pub root_slot: u32,
}

impl SlabDomTree {
    pub fn new(capacity: usize) -> Self {
        let mut arena = UnmanagedSlabArena::with_capacity(capacity);
        let root_slot = arena.allocate_node("html");
        Self { arena, root_slot }
    }

    /// Append a child node under a parent. Returns the new child's slot id.
    pub fn append_child(&mut self, parent_slot: u32, tag: &str) -> u32 {
        let child_slot = self.arena.allocate_node(tag);
        let child_idx = child_slot as usize;
        let parent_idx = parent_slot as usize;

        if child_idx < self.arena.slots.len() && parent_idx < self.arena.slots.len() {
            self.arena.slots[child_idx].parent_slot = parent_slot;

            // Link as last child
            let first = self.arena.slots[parent_idx].first_child_slot;
            if first == u32::MAX {
                self.arena.slots[parent_idx].first_child_slot = child_slot;
            } else {
                // Walk sibling chain to end
                let mut sibling = first;
                loop {
                    let next = self.arena.slots[sibling as usize].next_sibling_slot;
                    if next == u32::MAX {
                        self.arena.slots[sibling as usize].next_sibling_slot = child_slot;
                        break;
                    }
                    sibling = next;
                }
            }
            self.arena.slots[parent_idx].flags |= SLAB_NODE_DIRTY;
        }
        child_slot
    }

    /// Set text content for a node.
    pub fn set_text_content(&mut self, slot_id: u32, text: &str) {
        if (slot_id as usize) < self.arena.slots.len() {
            self.arena.text_content[slot_id as usize] = text.to_string();
            self.arena.slots[slot_id as usize].flags |= SLAB_NODE_DIRTY;
        }
    }

    /// Get the tag hash for a node.
    pub fn tag_hash(&self, slot_id: u32) -> u64 {
        if (slot_id as usize) < self.arena.slots.len() {
            self.arena.slots[slot_id as usize].tag_hash
        } else {
            0
        }
    }

    /// Get text content for a node.
    pub fn text_content(&self, slot_id: u32) -> &str {
        if (slot_id as usize) < self.arena.slots.len() {
            &self.arena.text_content[slot_id as usize]
        } else {
            ""
        }
    }

    /// Get an attribute value for a node.
    pub fn get_attribute(&self, slot_id: u32, key: &str) -> Option<&str> {
        if (slot_id as usize) < self.arena.attributes.len() {
            self.arena.attributes[slot_id as usize]
                .get(key)
                .map(|s| s.as_str())
        } else {
            None
        }
    }

    /// Parent slot id, or u32::MAX if root/orphan.
    pub fn parent(&self, slot_id: u32) -> u32 {
        if (slot_id as usize) < self.arena.slots.len() {
            self.arena.slots[slot_id as usize].parent_slot
        } else {
            u32::MAX
        }
    }

    /// First child slot id, or u32::MAX if leaf.
    pub fn first_child(&self, slot_id: u32) -> u32 {
        if (slot_id as usize) < self.arena.slots.len() {
            self.arena.slots[slot_id as usize].first_child_slot
        } else {
            u32::MAX
        }
    }

    /// Next sibling slot id, or u32::MAX if last.
    pub fn next_sibling(&self, slot_id: u32) -> u32 {
        if (slot_id as usize) < self.arena.slots.len() {
            self.arena.slots[slot_id as usize].next_sibling_slot
        } else {
            u32::MAX
        }
    }

    /// Depth-first traversal returning all slot ids in DFS order.
    pub fn dfs(&self) -> Vec<u32> {
        let mut result = Vec::new();
        let mut stack = vec![self.root_slot];
        while let Some(slot) = stack.pop() {
            result.push(slot);
            // Push children in reverse so leftmost is processed first
            let mut children = Vec::new();
            let mut child = self.first_child(slot);
            while child != u32::MAX {
                children.push(child);
                child = self.next_sibling(child);
            }
            for &c in children.iter().rev() {
                stack.push(c);
            }
        }
        result
    }

    /// Breadth-first traversal returning all slot ids in BFS order.
    pub fn bfs(&self) -> Vec<u32> {
        let mut result = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(self.root_slot);
        while let Some(slot) = queue.pop_front() {
            result.push(slot);
            let mut child = self.first_child(slot);
            while child != u32::MAX {
                queue.push_back(child);
                child = self.next_sibling(child);
            }
        }
        result
    }

    /// Find all nodes matching a tag hash.
    pub fn query_by_tag_hash(&self, tag_hash: u64) -> Vec<u32> {
        self.dfs()
            .into_iter()
            .filter(|&s| self.tag_hash(s) == tag_hash)
            .collect()
    }

    /// Collect all text content from the tree (DFS order).
    pub fn collect_all_text(&self) -> Vec<String> {
        self.dfs()
            .into_iter()
            .filter(|&s| !self.text_content(s).is_empty())
            .map(|s| self.text_content(s).to_string())
            .collect()
    }

    /// Total number of allocated nodes.
    pub fn node_count(&self) -> usize {
        self.arena.allocated_count
    }

    /// Check if a slot is valid (within bounds and not freed).
    pub fn is_valid_slot(&self, slot_id: u32) -> bool {
        (slot_id as usize) < self.arena.slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tree_has_root() {
        let tree = SlabDomTree::new(16);
        assert!(tree.is_valid_slot(tree.root_slot));
        assert_eq!(tree.node_count(), 1);
    }

    #[test]
    fn test_append_child_and_traverse() {
        let mut tree = SlabDomTree::new(16);
        let body = tree.append_child(tree.root_slot, "body");
        let div1 = tree.append_child(body, "div");
        let div2 = tree.append_child(body, "div");
        let _p = tree.append_child(div1, "p");

        // DFS from root: root, body, div1, p, div2
        let dfs = tree.dfs();
        assert_eq!(dfs.len(), 5);
        assert_eq!(dfs[0], tree.root_slot);
        assert_eq!(dfs[1], body);
        assert_eq!(dfs[4], div2);

        // BFS: root, body, div1, div2, p
        let bfs = tree.bfs();
        assert_eq!(bfs.len(), 5);
        assert_eq!(bfs[0], tree.root_slot);
        assert_eq!(bfs[1], body);
    }

    #[test]
    fn test_parent_child_navigation() {
        let mut tree = SlabDomTree::new(16);
        let child = tree.append_child(tree.root_slot, "head");
        assert_eq!(tree.parent(child), tree.root_slot);
        assert_eq!(tree.first_child(tree.root_slot), child);
        assert_eq!(tree.next_sibling(child), u32::MAX); // only child
    }

    #[test]
    fn test_text_content() {
        let mut tree = SlabDomTree::new(16);
        let p = tree.append_child(tree.root_slot, "p");
        tree.set_text_content(p, "Hello world");
        assert_eq!(tree.text_content(p), "Hello world");
        let texts = tree.collect_all_text();
        assert_eq!(texts, vec!["Hello world".to_string()]);
    }

    #[test]
    fn test_attributes() {
        let mut tree = SlabDomTree::new(16);
        let div = tree.append_child(tree.root_slot, "div");
        tree.arena.set_attribute(div, "class", "container");
        assert_eq!(tree.get_attribute(div, "class"), Some("container"));
        assert_eq!(tree.get_attribute(div, "id"), None);
    }

    #[test]
    fn test_query_by_tag() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut tree = SlabDomTree::new(16);
        tree.append_child(tree.root_slot, "div");
        tree.append_child(tree.root_slot, "span");
        tree.append_child(tree.root_slot, "div");

        let mut hasher = DefaultHasher::new();
        "div".hash(&mut hasher);
        let div_hash = hasher.finish();

        let matches = tree.query_by_tag_hash(div_hash);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_invalid_slot_returns_defaults() {
        let tree = SlabDomTree::new(4);
        assert_eq!(tree.parent(999), u32::MAX);
        assert_eq!(tree.first_child(999), u32::MAX);
        assert_eq!(tree.next_sibling(999), u32::MAX);
        assert_eq!(tree.tag_hash(999), 0);
        assert_eq!(tree.text_content(999), "");
        assert_eq!(tree.get_attribute(999, "class"), None);
    }

    #[test]
    fn test_is_valid_slot_bounds() {
        let tree = SlabDomTree::new(4);
        assert!(tree.is_valid_slot(tree.root_slot));
        assert!(!tree.is_valid_slot(9999));
    }

    #[test]
    fn test_node_count_increments() {
        let mut tree = SlabDomTree::new(4);
        assert_eq!(tree.node_count(), 1); // root
        tree.append_child(tree.root_slot, "div");
        assert_eq!(tree.node_count(), 2);
        tree.append_child(tree.root_slot, "span");
        assert_eq!(tree.node_count(), 3);
    }

    #[test]
    fn test_sibling_chain() {
        let mut tree = SlabDomTree::new(16);
        let a = tree.append_child(tree.root_slot, "a");
        let b = tree.append_child(tree.root_slot, "b");
        let c = tree.append_child(tree.root_slot, "c");
        // Walk sibling chain from first child
        assert_eq!(tree.first_child(tree.root_slot), a);
        assert_eq!(tree.next_sibling(a), b);
        assert_eq!(tree.next_sibling(b), c);
        assert_eq!(tree.next_sibling(c), u32::MAX); // last sibling
    }

    #[test]
    fn test_dfs_order_is_preorder() {
        let mut tree = SlabDomTree::new(16);
        let body = tree.append_child(tree.root_slot, "body");
        let div = tree.append_child(body, "div");
        let _p = tree.append_child(div, "p");
        let _span = tree.append_child(body, "span");
        let dfs = tree.dfs();
        // Preorder: root, body, div, p, span
        assert_eq!(dfs[0], tree.root_slot);
        assert_eq!(dfs[1], body);
        assert_eq!(dfs[2], div);
        assert_eq!(dfs[3], _p);
        assert_eq!(dfs[4], _span);
    }

    #[test]
    fn test_bfs_order_is_level_order() {
        let mut tree = SlabDomTree::new(16);
        let body = tree.append_child(tree.root_slot, "body");
        let head = tree.append_child(tree.root_slot, "head");
        let div = tree.append_child(body, "div");
        let _span = tree.append_child(body, "span");
        let bfs = tree.bfs();
        // Level order: root, body, head, div, span
        assert_eq!(bfs[0], tree.root_slot);
        assert_eq!(bfs[1], body);
        assert_eq!(bfs[2], head);
        assert_eq!(bfs[3], div);
    }

    #[test]
    fn test_mark_clean_clears_dirty_flag() {
        let mut tree = SlabDomTree::new(16);
        let div = tree.append_child(tree.root_slot, "div");
        // Newly allocated nodes have DIRTY flag
        assert!(tree.arena.slots[div as usize].flags & SLAB_NODE_DIRTY != 0);
        tree.arena.mark_clean(div);
        assert_eq!(tree.arena.slots[div as usize].flags & SLAB_NODE_DIRTY, 0);
    }

    #[test]
    fn test_set_attribute_marks_dirty() {
        let mut tree = SlabDomTree::new(16);
        let div = tree.append_child(tree.root_slot, "div");
        tree.arena.mark_clean(div);
        assert_eq!(tree.arena.slots[div as usize].flags & SLAB_NODE_DIRTY, 0);
        tree.arena.set_attribute(div, "class", "container");
        assert!(tree.arena.slots[div as usize].flags & SLAB_NODE_DIRTY != 0);
    }

    #[test]
    fn test_collect_all_text_dfs_order() {
        let mut tree = SlabDomTree::new(16);
        let p1 = tree.append_child(tree.root_slot, "p");
        let p2 = tree.append_child(tree.root_slot, "p");
        tree.set_text_content(p2, "second");
        tree.set_text_content(p1, "first");
        let texts = tree.collect_all_text();
        assert_eq!(texts, vec!["first".to_string(), "second".to_string()]);
    }
}
