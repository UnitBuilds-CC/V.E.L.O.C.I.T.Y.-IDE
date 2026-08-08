use crate::nda::NdaTriple;

#[derive(Debug)]
pub struct NdaEncoder {
    pub triples: Vec<NdaTriple>,
}

impl Default for NdaEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl NdaEncoder {
    pub fn new() -> Self {
        Self { triples: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            triples: Vec::with_capacity(capacity),
        }
    }

    pub fn encode_fact(&mut self, subject: &str, predicate_id: u16, object: &str) {
        self.triples.push(NdaTriple::new(subject, predicate_id, object));
    }

    pub fn encode_fact_raw(&mut self, subject_hash: u64, predicate_id: u16, object_hash: u64) {
        self.triples.push(NdaTriple {
            subject_hash,
            predicate_id,
            object_hash,
        });
    }

    /// Convert all encoded triples into a single contiguous binary byte vector (18 bytes per triple)
    pub fn to_binary_stream(&self) -> Vec<u8> {
        let mut stream = Vec::with_capacity(self.triples.len() * 18);
        for triple in &self.triples {
            stream.extend_from_slice(&triple.to_bytes());
        }
        stream
    }

    /// Parse a contiguous binary byte stream back into an NdaEncoder
    pub fn from_binary_stream(stream: &[u8]) -> Result<Self, &'static str> {
        if !stream.len().is_multiple_of(18) {
            return Err("Binary stream length must be a multiple of 18 bytes");
        }

        let count = stream.len() / 18;
        let mut triples = Vec::with_capacity(count);

        for i in 0..count {
            let offset = i * 18;
            let triple = NdaTriple::from_bytes(&stream[offset..offset + 18])?;
            triples.push(triple);
        }

        Ok(Self { triples })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_encoder_is_empty() {
        let enc = NdaEncoder::new();
        assert!(enc.triples.is_empty());
    }

    #[test]
    fn default_equals_new() {
        let enc = NdaEncoder::default();
        assert!(enc.triples.is_empty());
    }

    #[test]
    fn with_capacity_preallocates() {
        let enc = NdaEncoder::with_capacity(100);
        assert!(enc.triples.capacity() >= 100);
        assert!(enc.triples.is_empty());
    }

    #[test]
    fn encode_fact_appends_triple() {
        let mut enc = NdaEncoder::new();
        enc.encode_fact("subject", 42, "object");
        assert_eq!(enc.triples.len(), 1);
        assert_eq!(enc.triples[0].predicate_id, 42);
    }

    #[test]
    fn encode_fact_raw_appends_triple() {
        let mut enc = NdaEncoder::new();
        enc.encode_fact_raw(123, 7, 456);
        assert_eq!(enc.triples.len(), 1);
        assert_eq!(enc.triples[0].subject_hash, 123);
        assert_eq!(enc.triples[0].predicate_id, 7);
        assert_eq!(enc.triples[0].object_hash, 456);
    }

    #[test]
    fn binary_stream_roundtrip() {
        let mut enc = NdaEncoder::new();
        enc.encode_fact("hello", 1, "world");
        enc.encode_fact("foo", 2, "bar");
        let bytes = enc.to_binary_stream();
        assert_eq!(bytes.len(), 36); // 2 triples * 18 bytes
        let dec = NdaEncoder::from_binary_stream(&bytes).unwrap();
        assert_eq!(dec.triples.len(), 2);
    }

    #[test]
    fn binary_stream_empty() {
        let enc = NdaEncoder::new();
        let bytes = enc.to_binary_stream();
        assert!(bytes.is_empty());
        let dec = NdaEncoder::from_binary_stream(&bytes).unwrap();
        assert!(dec.triples.is_empty());
    }

    #[test]
    fn from_binary_stream_rejects_invalid_length() {
        let result = NdaEncoder::from_binary_stream(&[0u8; 10]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("multiple of 18"));
    }

    #[test]
    fn multiple_facts_accumulate() {
        let mut enc = NdaEncoder::new();
        for i in 0..5 {
            enc.encode_fact("s", i, "o");
        }
        assert_eq!(enc.triples.len(), 5);
    }
}
