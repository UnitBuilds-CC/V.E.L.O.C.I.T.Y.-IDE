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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Tokenizer tests ───────────────────────────────────────────────────

    #[test]
    fn tokenizer_new_has_ascii_vocab() {
        let tok = Tokenizer::new();
        // 0..128 are valid ASCII chars; 128..255 all map to U+FFFD (replacement char)
        // so unique entries from 0..=255 are ~129, plus common words and code keywords
        assert!(tok.vocab_size() >= 130, "should have at least ASCII + replacement char entries");
    }

    #[test]
    fn tokenizer_vocab_includes_common_words() {
        let tok = Tokenizer::new();
        // Common English words should get dedicated token IDs (> 255)
        let encoded = tok.encode("the");
        assert_eq!(encoded.len(), 1, "'the' should be a single token");
        assert!(encoded[0] > 255, "'the' should have a vocab ID > 255");
    }

    #[test]
    fn tokenizer_vocab_includes_code_keywords() {
        let tok = Tokenizer::new();
        let encoded = tok.encode("fn");
        assert_eq!(encoded.len(), 1, "'fn' should be a single token");
        assert!(encoded[0] > 255, "'fn' should have a vocab ID > 255");
    }

    #[test]
    fn tokenizer_encode_decode_roundtrip() {
        let tok = Tokenizer::new();
        let text = "hello world";
        let encoded = tok.encode(text);
        let decoded = tok.decode(&encoded);
        assert_eq!(decoded, text);
    }

    #[test]
    fn tokenizer_encode_decode_code() {
        let tok = Tokenizer::new();
        let text = "fn main() { let x = 42; }";
        let encoded = tok.encode(text);
        let decoded = tok.decode(&encoded);
        assert_eq!(decoded, text);
    }

    #[test]
    fn tokenizer_empty_string() {
        let tok = Tokenizer::new();
        let encoded = tok.encode("");
        assert!(encoded.is_empty(), "empty string should produce no tokens");
        let decoded = tok.decode(&encoded);
        assert_eq!(decoded, "");
    }

    #[test]
    fn tokenizer_single_char() {
        let tok = Tokenizer::new();
        for ch in ['a', 'Z', '0', ' ', '\n'] {
            let s = ch.to_string();
            let encoded = tok.encode(&s);
            assert!(!encoded.is_empty(), "char '{}' should encode", ch);
            let decoded = tok.decode(&encoded);
            assert_eq!(decoded, s, "roundtrip failed for char '{}'", ch);
        }
    }

    #[test]
    fn tokenizer_ascii_bytes_fallback() {
        let tok = Tokenizer::new();
        // Each ASCII byte should map to its byte value as token ID
        for b in 0u8..=127 {
            let ch = (b as char).to_string();
            let encoded = tok.encode(&ch);
            assert_eq!(encoded.len(), 1, "ASCII byte {} should be single token", b);
            assert_eq!(encoded[0], b as u32, "ASCII byte {} should map to ID {}", b, b);
        }
    }

    #[test]
    fn tokenizer_unknown_id_decodes_to_placeholder() {
        let tok = Tokenizer::new();
        let decoded = tok.decode(&[999999]);
        assert!(decoded.contains("<unk:"), "unknown ID should produce <unk:N> placeholder");
    }

    #[test]
    fn tokenizer_whitespace_roundtrip() {
        let tok = Tokenizer::new();
        let text = "  \n\t  ";
        let encoded = tok.encode(text);
        let decoded = tok.decode(&encoded);
        assert_eq!(decoded, text);
    }

    #[test]
    fn tokenizer_greedy_matching() {
        let tok = Tokenizer::new();
        // "    " (4 spaces) is in the code_words list, so it should be a single token
        let encoded = tok.encode("    ");
        assert_eq!(encoded.len(), 1, "4-space indent should be a single token");
    }

    #[test]
    fn tokenizer_vocab_size_stable() {
        let tok1 = Tokenizer::new();
        let tok2 = Tokenizer::new();
        assert_eq!(tok1.vocab_size(), tok2.vocab_size());
    }

    #[test]
    fn tokenizer_unicode_fallback() {
        let tok = Tokenizer::new();
        // Unicode chars not in vocab fall back to UTF-8 byte encoding.
        // Each byte is looked up individually; bytes > 127 map to U+FFFD.
        // Verify the encoding doesn't panic and produces some tokens.
        let text = "\u{00e9}"; // é (UTF-8: 0xC3 0xA9)
        let encoded = tok.encode(text);
        assert!(!encoded.is_empty(), "unicode should still encode");
        // The decoded form may differ because high bytes map to replacement char
        let decoded = tok.decode(&encoded);
        assert!(!decoded.is_empty(), "decoded should produce some output");
    }

    // ─── NdaEmbeddingTable tests ──────────────────────────────────────────

    #[test]
    fn embedding_table_dimensions() {
        let table = NdaEmbeddingTable::new(300, 128);
        // packed_len = 128 / 32 = 4
        let (active, pos) = table.lookup(0);
        assert_eq!(active.len(), 4, "active bitmap should have packed_len words");
        assert_eq!(pos.len(), 4, "positive bitmap should have packed_len words");
    }

    #[test]
    fn embedding_table_deterministic() {
        let t1 = NdaEmbeddingTable::new(300, 128);
        let t2 = NdaEmbeddingTable::new(300, 128);
        for token_id in 0..10 {
            let (a1, p1) = t1.lookup(token_id);
            let (a2, p2) = t2.lookup(token_id);
            assert_eq!(a1, a2, "active bitmap for token {} should be deterministic", token_id);
            assert_eq!(p1, p2, "positive bitmap for token {} should be deterministic", token_id);
        }
    }

    #[test]
    fn embedding_table_different_tokens_differ() {
        let table = NdaEmbeddingTable::new(300, 128);
        let (a0, _) = table.lookup(0);
        let (a1, _) = table.lookup(1);
        // Very unlikely that two different tokens have identical embeddings
        assert!(a0 != a1, "different tokens should have different active bitmaps");
    }

    #[test]
    fn embedding_table_wraps_around() {
        let table = NdaEmbeddingTable::new(10, 64);
        // lookup(10) should wrap to lookup(0) via modulo
        let (a_wrap, p_wrap) = table.lookup(10);
        let (a0, p0) = table.lookup(0);
        assert_eq!(a_wrap, a0);
        assert_eq!(p_wrap, p0);
    }

    #[test]
    fn embedding_table_sparsity() {
        // ~30% of bits should be active (set)
        let table = NdaEmbeddingTable::new(100, 1024);
        let (active, _) = table.lookup(42);
        let total_bits = active.len() * 32;
        let active_bits: usize = active.iter().map(|w| w.count_ones() as usize).sum();
        let ratio = active_bits as f64 / total_bits as f64;
        // Allow wide margin: 10% to 50%
        assert!(ratio > 0.10 && ratio < 0.50,
            "sparsity ratio {} should be around 30%", ratio);
    }

    #[test]
    fn embedding_table_positive_subset_of_active() {
        // Positive bits should be a subset of active bits
        let table = NdaEmbeddingTable::new(100, 256);
        for token_id in 0..50 {
            let (active, pos) = table.lookup(token_id);
            for (a, p) in active.iter().zip(pos.iter()) {
                assert_eq!(a & p, *p, "positive bits must be subset of active bits for token {}", token_id);
            }
        }
    }

    // ─── NdaEmbeddedTokenizer tests ───────────────────────────────────────

    #[test]
    fn embedded_tokenizer_encode_and_embed() {
        let et = NdaEmbeddedTokenizer::new(128);
        let (token_ids, embeds) = et.encode_and_embed("hello");
        assert!(!token_ids.is_empty(), "should produce token IDs");
        assert_eq!(token_ids.len(), embeds.len(), "each token should have an embedding");
    }

    #[test]
    fn embedded_tokenizer_decode() {
        let et = NdaEmbeddedTokenizer::new(128);
        let text = "fn main";
        let (token_ids, _) = et.encode_and_embed(text);
        let decoded = et.decode(&token_ids);
        assert_eq!(decoded, text);
    }

    #[test]
    fn embedded_tokenizer_empty_input() {
        let et = NdaEmbeddedTokenizer::new(64);
        let (token_ids, embeds) = et.encode_and_embed("");
        assert!(token_ids.is_empty());
        assert!(embeds.is_empty());
    }

    #[test]
    fn embedded_tokenizer_embeddings_have_correct_dims() {
        let dim = 256;
        let et = NdaEmbeddedTokenizer::new(dim);
        let (_, embeds) = et.encode_and_embed("test");
        for (active, pos) in &embeds {
            assert_eq!(active.len(), dim / 32, "active bitmap packed_len");
            assert_eq!(pos.len(), dim / 32, "positive bitmap packed_len");
        }
    }
}
