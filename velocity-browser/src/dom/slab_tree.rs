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
}
