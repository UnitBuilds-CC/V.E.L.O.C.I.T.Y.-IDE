use crate::nda::NdaTriple;

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
