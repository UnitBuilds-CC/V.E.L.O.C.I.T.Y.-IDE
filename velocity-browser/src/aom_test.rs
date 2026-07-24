use super::aom::{AomExtractor, SpatialNode};
use crate::nda::NdaTriple;

#[test]
fn extracts_role_triple_for_each_node() {
    let nodes = vec![
        SpatialNode { id: "n1".into(), role: "button".into(), name: "Submit".into() },
        SpatialNode { id: "n2".into(), role: "link".into(), name: "".into() },
    ];
    
    let triples = AomExtractor::extract_triples(&nodes);
    
    assert_eq!(triples.len(), 3);
    assert!(triples.contains(&NdaTriple::new("n1", 10, "button")));
    assert!(triples.contains(&NdaTriple::new("n2", 10, "link")));
}

#[test]
fn adds_name_triple_only_when_non_empty() {
    let nodes = vec![
        SpatialNode { id: "n3".into(), role: "heading".into(), name: "Welcome".into() },
        SpatialNode { id: "n4".into(), role: "text".into(), name: "".into() },
    ];
    
    let triples = AomExtractor::extract_triples(&nodes);
    
    assert_eq!(triples.len(), 3);
    assert!(triples.contains(&NdaTriple::new("n3", 11, "Welcome")));
    assert!(!triples.iter().any(|t| t.subject == "n4" && t.predicate == 11));
}

#[test]
fn maintains_node_id_association() {
    let node = SpatialNode { id: "unique_id123".into(), role: "form".into(), name: "Login".into() };
    
    let triples = AomExtractor::extract_triples(&[node]);
    
    assert_eq!(triples[0].subject, "unique_id123");
    assert_eq!(triples[1].subject, "unique_id123");
}