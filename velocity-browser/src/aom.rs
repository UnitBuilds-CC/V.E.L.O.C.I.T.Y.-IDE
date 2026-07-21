use crate::nda::NdaTriple;

pub struct SpatialNode {
    pub id: String,
    pub role: String,
    pub name: String,
}

pub struct AomExtractor;

impl AomExtractor {
    pub fn extract_triples(nodes: &[SpatialNode]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(nodes.len() * 2);
        for node in nodes {
            triples.push(NdaTriple::new(&node.id, 10, &node.role));
            if !node.name.is_empty() {
                triples.push(NdaTriple::new(&node.id, 11, &node.name));
            }
        }
        triples
    }
}
