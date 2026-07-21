use velocity_browser::dom::SlabDomTree;
use velocity_browser::session::BrowserSession;

#[test]
fn test_unmanaged_slab_dom_tree_allocations() {
    let mut tree = SlabDomTree::new(10);
    let n1 = tree.arena.allocate_node("div");
    let n2 = tree.arena.allocate_node("span");

    tree.arena.set_attribute(n1, "id", "main");
    tree.arena.set_attribute(n2, "class", "highlight");

    assert_eq!(tree.arena.slots.len(), 13); // html root (1) + 10 prealloc + 2 alloc
    assert_eq!(tree.arena.attributes[n1 as usize].get("id").unwrap(), "main");
}

#[test]
fn test_slab_tree_session_nda_export() {
    let session = BrowserSession::new("sess_slab".to_string());
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 210)); // Slab slot predicate
}
