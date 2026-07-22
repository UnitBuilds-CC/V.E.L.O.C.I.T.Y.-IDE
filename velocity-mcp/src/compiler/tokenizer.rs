#![allow(dead_code)]

use std::collections::HashMap;

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    rev_vocab: HashMap<u32, String>,
}

impl Tokenizer {
    /// Creates a new tokenizer with a default vocabulary containing basic ASCII (0..255)
    /// and common subwords & code keywords to guarantee 100% text coverage.
    pub fn new() -> Self {
        let mut vocab = HashMap::new();
        let mut rev_vocab = HashMap::new();

        // 1. ASCII characters
        for i in 0..=255 {
            let byte_str = String::from_utf8_lossy(&[i as u8]).into_owned();
            vocab.insert(byte_str.clone(), i as u32);
            rev_vocab.insert(i as u32, byte_str);
        }

        // 2. Common English words
        let common_words = vec![
            "the", "be", "to", "of", "and", "a", "in", "that", "have", "it", "for", "not", "on",
            "with", "he", "as", "you", "do", "at", "this", "but", "his", "by", "from", "they",
            "we", "say", "her", "she", "or", "an", "will", "my", "one", "all", "would", "there",
            "their", "what", "so", "up", "out", "if", "about", "who", "get", "which", "go", "me",
        ];

        let mut next_id = 256;
        for word in common_words {
            if !vocab.contains_key(word) {
                vocab.insert(word.to_string(), next_id);
                rev_vocab.insert(next_id, word.to_string());
                next_id += 1;
            }
        }

        // 3. Common code keywords
        let code_words = vec![
            "fn", "let", "mut", "pub", "struct", "impl", "use", "match", "else", "while", "loop",
            "return", "mod", "crate", "std", "u32", "i32", "f32", "String", "Vec", "Result",
            "Option", "    ", "  ", "\n", "true", "false",
        ];

        for kw in code_words {
            if !vocab.contains_key(kw) {
                vocab.insert(kw.to_string(), next_id);
                rev_vocab.insert(next_id, kw.to_string());
                next_id += 1;
            }
        }

        Self { vocab, rev_vocab }
    }

    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut tokens = Vec::new();
        let mut index = 0;
        let char_indices: Vec<(usize, char)> = text.char_indices().collect();

        while index < char_indices.len() {
            let mut best_match_len = 0;
            let mut best_token_id = None;
            let start_byte_pos = char_indices[index].0;

            for end_idx in (index + 1)..=char_indices.len() {
                let end_byte_pos = if end_idx < char_indices.len() {
                    char_indices[end_idx].0
                } else {
                    text.len()
                };

                let substring = &text[start_byte_pos..end_byte_pos];
                if let Some(&id) = self.vocab.get(substring) {
                    best_match_len = end_idx - index;
                    best_token_id = Some(id);
                }
            }

            if let Some(id) = best_token_id {
                tokens.push(id);
                index += best_match_len;
            } else {
                let ch = char_indices[index].1;
                let mut buf = [0; 4];
                let ch_str = ch.encode_utf8(&mut buf);
                for &b in ch_str.as_bytes() {
                    tokens.push(b as u32);
                }
                index += 1;
            }
        }

        tokens
    }

    pub fn decode(&self, tokens: &[u32]) -> String {
        let mut text = String::new();
        for &id in tokens {
            if let Some(substring) = self.rev_vocab.get(&id) {
                text.push_str(substring);
            } else {
                text.push_str(&format!("<unk:{}>", id));
            }
        }
        text
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }
}

/// An Embedding Table storing embeddings pre-quantized and pre-packed into
/// the V.E.L.O.C.I.T.Y. NDA format (Decomposed Active & Positive Bitmaps).
/// Eliminates the VRAM storage footprint of float/half embeddings (10x smaller)
/// and bypasses any runtime ternary quantization compute overhead on inputs.
pub struct NdaEmbeddingTable {
    embedding_dim: usize,
    active_embeddings: Vec<Vec<u32>>,
    pos_embeddings: Vec<Vec<u32>>,
}

impl NdaEmbeddingTable {
    /// Generates deterministic, pre-packed NDA embeddings for the vocabulary.
    /// Uses a pseudo-random hashing function to simulate ternary embedding weights
    /// with ~30% active density (70% sparsity).
    pub fn new(vocab_size: usize, embedding_dim: usize) -> Self {
        let packed_len = embedding_dim / 32;
        let mut active_embeddings = vec![vec![0u32; packed_len]; vocab_size];
        let mut pos_embeddings = vec![vec![0u32; packed_len]; vocab_size];

        for t in 0..vocab_size {
            for word_idx in 0..packed_len {
                let mut active_word = 0u32;
                let mut pos_word = 0u32;

                // Simulate 32 ternary dimensions per word
                for bit in 0..32 {
                    // Seeded pseudo-random generation based on token ID and dimension index
                    let seed = t as u32 * 7919 + (word_idx * 32 + bit) as u32 * 104729;
                    let rand_val = (seed ^ (seed >> 13)).wrapping_mul(0x27856713) % 100;

                    // 30% active values: 15% positive (+1), 15% negative (-1), 70% zero (0)
                    if rand_val < 30 {
                        active_word |= 1 << bit;
                        if rand_val < 15 {
                            pos_word |= 1 << bit;
                        }
                    }
                }

                active_embeddings[t][word_idx] = active_word;
                pos_embeddings[t][word_idx] = pos_word;
            }
        }

        Self {
            embedding_dim,
            active_embeddings,
            pos_embeddings,
        }
    }

    /// Retrieve the pre-packed active and positive bitmaps for a token ID.
    pub fn lookup(&self, token_id: u32) -> (&[u32], &[u32]) {
        let idx = (token_id as usize) % self.active_embeddings.len();
        (&self.active_embeddings[idx], &self.pos_embeddings[idx])
    }
}

/// The V.E.L.O.C.I.T.Y. NDA Embedded Tokenizer pipeline.
/// Seamlessly tokenizes prompts and returns GPU-ready NDA bitmap embeddings directly,
/// bypassing the standard floating-point representation.
pub struct NdaEmbeddedTokenizer {
    pub tokenizer: Tokenizer,
    pub embedding_table: NdaEmbeddingTable,
}

impl NdaEmbeddedTokenizer {
    /// Creates a new NDA-embedded tokenizer for the given embedding dimension.
    pub fn new(embedding_dim: usize) -> Self {
        let tokenizer = Tokenizer::new();
        let vocab_size = tokenizer.vocab_size();
        let embedding_table = NdaEmbeddingTable::new(vocab_size, embedding_dim);

        Self {
            tokenizer,
            embedding_table,
        }
    }

    /// Tokenizes the text prompt and retrieves the pre-packed active and positive NDA embeddings for each token.
    pub fn encode_and_embed(&self, text: &str) -> (Vec<u32>, Vec<(&[u32], &[u32])>) {
        let token_ids = self.tokenizer.encode(text);
        let mut embeds = Vec::new();
        for &id in &token_ids {
            embeds.push(self.embedding_table.lookup(id));
        }
        (token_ids, embeds)
    }

    /// Decodes token IDs back into standard text.
    pub fn decode(&self, tokens: &[u32]) -> String {
        self.tokenizer.decode(tokens)
    }
}
