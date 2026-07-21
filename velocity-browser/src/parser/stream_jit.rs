#[derive(Debug, Clone)]
pub struct StreamJitToken {
    pub token_kind: String,
    pub raw_bytes: Vec<u8>,
}

pub struct StreamJitTokenizer;

impl StreamJitTokenizer {
    pub fn tokenize_stream_chunk(chunk_bytes: &[u8]) -> Vec<StreamJitToken> {
        vec![StreamJitToken {
            token_kind: "stream_html_node".to_string(),
            raw_bytes: chunk_bytes.to_vec(),
        }]
    }
}
