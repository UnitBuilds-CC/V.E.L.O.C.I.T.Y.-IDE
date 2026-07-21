use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct ShadowHost {
    pub host_id: String,
    pub mode: String, // "open" or "closed"
    pub shadow_root_id: String,
}

#[derive(Debug, Clone)]
pub struct FrameTarget {
    pub frame_id: String,
    pub parent_id: Option<String>,
    pub url: String,
    pub security_origin: String,
}

pub struct ShadowFrameExtractor;

impl ShadowFrameExtractor {
    pub fn extract_shadow_hosts_nda(hosts: &[ShadowHost]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(hosts.len() * 2);
        for host in hosts {
            triples.push(NdaTriple::new(&host.host_id, 20, &host.mode));
            triples.push(NdaTriple::new(&host.host_id, 21, &host.shadow_root_id));
        }
        triples
    }

    pub fn extract_frames_nda(frames: &[FrameTarget]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(frames.len() * 2);
        for frame in frames {
            triples.push(NdaTriple::new(&frame.frame_id, 30, &frame.url));
            if let Some(parent) = &frame.parent_id {
                triples.push(NdaTriple::new(&frame.frame_id, 31, parent));
            }
        }
        triples
    }
}
