// src/tokenizer.rs — V.E.L.O.C.I.T.Y.-IDE
//
// BPE tokenizer supporting both fast zero-copy binary `.nda` format (NDAT)
// and legacy HuggingFace `tokenizer.json` directly.
//
// No C library dependencies — pure Rust.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ─── JSON schema (fallback) ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenizerFile {
    model: BpeModel,
    #[serde(default)]
    added_tokens: Vec<AddedToken>,
}

#[derive(Debug, Deserialize)]
struct BpeModel {
    vocab: HashMap<String, u32>,
    merges: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AddedToken {
    id: u32,
    content: String,
}

// ─── Token Representation ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum TokenRef {
    Owned(String),
    Slice { offset: u32, len: u32 },
}

// ─── Public tokenizer ──────────────────────────────────────────────────────

pub struct Tokenizer {
    /// token-string → id
    vocab: HashMap<String, u32>,
    /// id → token representation (slice or owned)
    id_to_token: Vec<TokenRef>,
    /// BPE merge pair → rank (lower rank = apply first)
    merges: HashMap<(String, String), u32>,
    pub bos_id: u32,
    #[allow(dead_code)]
    pub eos_id: u32,
    is_tiktoken: bool,
    /// Owned binary buffer for raw byte slicing
    file_bytes: Vec<u8>,
}

impl Tokenizer {
    /// Load from a file (detects NDAT magic header or falls back to JSON).
    pub fn from_file(path: &Path) -> Result<Self> {
        let file_bytes =
            std::fs::read(path).with_context(|| format!("Reading tokenizer file: {path:?}"))?;

        if file_bytes.starts_with(b"NDAT") {
            Self::from_binary(file_bytes)
        } else {
            Self::from_json(file_bytes)
        }
    }

    /// Load from the fast zero-copy NDAT binary format.
    fn from_binary(file_bytes: Vec<u8>) -> Result<Self> {
        if file_bytes.len() < 24 {
            anyhow::bail!("NDAT file truncated");
        }
        let version = u16::from_le_bytes(
            file_bytes[4..6]
                .try_into()
                .expect("slice is exactly 2 bytes"),
        );
        if version != 1 {
            anyhow::bail!("Unsupported NDAT version: {version}");
        }
        let vocab_size = u32::from_le_bytes(
            file_bytes[6..10]
                .try_into()
                .expect("slice is exactly 4 bytes"),
        ) as usize;
        let merges_count = u32::from_le_bytes(
            file_bytes[10..14]
                .try_into()
                .expect("slice is exactly 4 bytes"),
        ) as usize;
        let bos_id = u32::from_le_bytes(
            file_bytes[14..18]
                .try_into()
                .expect("slice is exactly 4 bytes"),
        );
        let eos_id = u32::from_le_bytes(
            file_bytes[18..22]
                .try_into()
                .expect("slice is exactly 4 bytes"),
        );
        let is_tiktoken = file_bytes[22] != 0;

        let string_data_start = 24 + vocab_size * 8 + merges_count * 20;
        if file_bytes.len() < string_data_start {
            anyhow::bail!("NDAT file truncated before string data");
        }

        // 1. Parse Vocabulary Offset Table
        let mut id_to_token = Vec::with_capacity(vocab_size);
        let mut vocab = HashMap::with_capacity(vocab_size);
        for id in 0..vocab_size {
            let entry_offset = 24 + id * 8;
            let offset = u32::from_le_bytes(
                file_bytes[entry_offset..entry_offset + 4]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            );
            let len = u32::from_le_bytes(
                file_bytes[entry_offset + 4..entry_offset + 8]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            );

            id_to_token.push(TokenRef::Slice { offset, len });

            // Populate vocab map
            let start = string_data_start + offset as usize;
            let end = start + len as usize;
            let token_str = std::str::from_utf8(&file_bytes[start..end])
                .context("Invalid UTF-8 in vocab string block")?;
            vocab.insert(token_str.to_string(), id as u32);
        }

        // 2. Parse Merges Table
        let mut merges = HashMap::with_capacity(merges_count);
        for i in 0..merges_count {
            let entry_offset = 24 + vocab_size * 8 + i * 20;
            let a_off = u32::from_le_bytes(
                file_bytes[entry_offset..entry_offset + 4]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            ) as usize;
            let a_len = u32::from_le_bytes(
                file_bytes[entry_offset + 4..entry_offset + 8]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            ) as usize;
            let b_off = u32::from_le_bytes(
                file_bytes[entry_offset + 8..entry_offset + 12]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            ) as usize;
            let b_len = u32::from_le_bytes(
                file_bytes[entry_offset + 12..entry_offset + 16]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            ) as usize;
            let rank = u32::from_le_bytes(
                file_bytes[entry_offset + 16..entry_offset + 20]
                    .try_into()
                    .expect("slice is exactly 4 bytes"),
            );

            let a_str = std::str::from_utf8(
                &file_bytes[string_data_start + a_off..string_data_start + a_off + a_len],
            )?
            .to_string();
            let b_str = std::str::from_utf8(
                &file_bytes[string_data_start + b_off..string_data_start + b_off + b_len],
            )?
            .to_string();
            merges.insert((a_str, b_str), rank);
        }

        Ok(Self {
            vocab,
            id_to_token,
            merges,
            bos_id,
            eos_id,
            is_tiktoken,
            file_bytes,
        })
    }

    /// Load from a legacy JSON file.
    fn from_json(file_bytes: Vec<u8>) -> Result<Self> {
        let raw = std::str::from_utf8(&file_bytes).context("Invalid UTF-8 in JSON tokenizer")?;
        let tf: TokenizerFile = serde_json::from_str(raw).context("Parsing tokenizer.json")?;

        // Build id → token reverse map
        let max_id = tf.model.vocab.values().copied().max().unwrap_or(0) as usize;
        let mut id_to_token = vec![TokenRef::Owned(String::new()); max_id + 1];
        for (tok, &id) in &tf.model.vocab {
            if (id as usize) < id_to_token.len() {
                id_to_token[id as usize] = TokenRef::Owned(tok.clone());
            }
        }
        // Added tokens override vocab entries
        for at in &tf.added_tokens {
            if (at.id as usize) < id_to_token.len() {
                id_to_token[at.id as usize] = TokenRef::Owned(at.content.clone());
            }
        }

        // Build merge rank map from "a b" strings
        let merges: HashMap<(String, String), u32> = tf
            .model
            .merges
            .iter()
            .enumerate()
            .filter_map(|(rank, s)| {
                let mut parts = s.splitn(2, ' ');
                let a = parts.next()?.to_string();
                let b = parts.next()?.to_string();
                Some(((a, b), rank as u32))
            })
            .collect();

        // Resolve BOS / EOS before consuming tf.model.vocab.
        let find_id = |names: &[&str], fallback: u32| -> u32 {
            for name in names {
                if let Some(at) = tf.added_tokens.iter().find(|t| &t.content == name) {
                    return at.id;
                }
                if let Some(&id) = tf.model.vocab.get(*name) {
                    return id;
                }
            }
            fallback
        };

        // BOS: try LLaMA-style <s>, then Qwen-style <|im_start|>, then generic
        let bos_id = find_id(&["<s>", "<|im_start|>", "<|endoftext|>", "<BOS>"], 1);
        // EOS: try LLaMA-style </s>, then Qwen-style <|im_end|> or <|endoftext|>
        let eos_id = find_id(&["</s>", "<|im_end|>", "<|endoftext|>", "<EOS>"], 2);

        let is_tiktoken = !tf.model.vocab.contains_key("\u{2581}")
            && !tf.model.vocab.keys().any(|k| k.starts_with('\u{2581}'));

        Ok(Self {
            vocab: tf.model.vocab,
            id_to_token,
            merges,
            bos_id,
            eos_id,
            is_tiktoken,
            file_bytes,
        })
    }

    // ── Encoding ──────────────────────────────────────────────────────────

    /// Encode `text` to token IDs.
    ///
    /// Prepends BOS token when `add_bos` is true (standard for LLaMA prompts).
    pub fn encode(&self, text: &str, add_bos: bool) -> Vec<u32> {
        let mut ids = if add_bos {
            vec![self.bos_id]
        } else {
            Vec::new()
        };
        ids.extend(self.bpe_encode(text));
        ids
    }

    /// Internal BPE encoder.
    fn bpe_encode(&self, text: &str) -> Vec<u32> {
        let mut symbols: Vec<String> = if self.is_tiktoken {
            text.as_bytes()
                .iter()
                .map(|&b| byte_to_unicode(b).to_string())
                .collect()
        } else {
            let prefixed = format!("\u{2581}{}", text.replace(' ', "\u{2581}"));
            prefixed.chars().map(|c| c.to_string()).collect()
        };

        if !self.is_tiktoken {
            symbols = symbols
                .into_iter()
                .flat_map(|sym| {
                    if self.vocab.contains_key(&sym) {
                        vec![sym]
                    } else {
                        sym.bytes().map(|b| format!("<0x{b:02X}>")).collect()
                    }
                })
                .collect();
        }

        // BPE merge loop — O(n²) in the worst case but fine for typical prompt lengths
        loop {
            let mut best_rank = u32::MAX;
            let mut best_pos = None;

            for i in 0..symbols.len().saturating_sub(1) {
                if let Some(&rank) = self
                    .merges
                    .get(&(symbols[i].clone(), symbols[i + 1].clone()))
                {
                    if rank < best_rank {
                        best_rank = rank;
                        best_pos = Some(i);
                    }
                }
            }

            let Some(pos) = best_pos else { break };
            let merged = format!("{}{}", symbols[pos], symbols[pos + 1]);
            symbols[pos] = merged;
            symbols.remove(pos + 1);
        }

        // Map symbols → IDs; unknown symbols produce a warning and are skipped
        symbols
            .iter()
            .filter_map(|s| {
                if let Some(&id) = self.vocab.get(s) {
                    Some(id)
                } else {
                    log::debug!("Unknown token during encode (skipped): {s:?}");
                    None
                }
            })
            .collect()
    }

    // ── Decoding ──────────────────────────────────────────────────────────

    /// Decode a single token ID to its display string.
    pub fn decode_token(&self, id: u32) -> String {
        let raw = match self.id_to_token.get(id as usize) {
            Some(TokenRef::Owned(s)) if !s.is_empty() => s.as_str(),
            Some(TokenRef::Slice { offset, len }) if *len > 0 => {
                let vocab_bytes = self
                    .vocab_size()
                    .checked_mul(8)
                    .expect("tokenizer: vocab_size overflow");
                let merges_bytes = self
                    .merges
                    .len()
                    .checked_mul(20)
                    .expect("tokenizer: merges overflow");
                let start = 24_usize
                    .checked_add(vocab_bytes)
                    .and_then(|v| v.checked_add(merges_bytes))
                    .and_then(|v| v.checked_add(*offset as usize))
                    .expect("tokenizer: token offset overflow");
                let end = start
                    .checked_add(*len as usize)
                    .expect("tokenizer: token end overflow");
                std::str::from_utf8(&self.file_bytes[start..end]).unwrap_or("")
            }
            _ => return String::new(),
        };

        if self.is_tiktoken {
            if raw.starts_with("<|") && raw.ends_with("|>") {
                return String::new();
            }
            let bytes: Vec<u8> = raw.chars().map(unicode_to_byte).collect();
            String::from_utf8(bytes).unwrap_or_else(|_| raw.to_string())
        } else {
            decode_raw_token_sp(raw)
        }
    }

    /// Decode a slice of token IDs to a single string.
    #[allow(dead_code)]
    pub fn decode(&self, ids: &[u32]) -> String {
        ids.iter().map(|&id| self.decode_token(id)).collect()
    }

    /// Total vocabulary size.
    pub fn vocab_size(&self) -> usize {
        self.id_to_token.len()
    }

    // ── Batch & Counting ──────────────────────────────────────────────────

    /// Encode multiple texts in sequence.
    pub fn encode_batch(&self, texts: &[&str], add_bos: bool) -> Vec<Vec<u32>> {
        texts.iter().map(|text| self.encode(text, add_bos)).collect()
    }

    /// Count the number of tokens in `text`.
    pub fn count_tokens(&self, text: &str) -> usize {
        self.bpe_encode(text).len()
    }

    /// Count tokens for multiple texts, returning individual counts.
    pub fn count_tokens_batch(&self, texts: &[&str]) -> Vec<usize> {
        texts.iter().map(|t| self.count_tokens(t)).collect()
    }

    /// Total token count across a batch of texts.
    pub fn total_tokens(&self, texts: &[&str]) -> usize {
        texts.iter().map(|t| self.count_tokens(t)).sum()
    }

    /// Check if a text would exceed a token budget.
    pub fn exceeds_budget(&self, text: &str, max_tokens: usize) -> bool {
        self.bpe_encode(text).len() > max_tokens
    }

    // ── Special Tokens ─────────────────────────────────────────────────────

    /// Look up the ID for a special token by name.
    pub fn special_token_id(&self, name: &str) -> Option<u32> {
        self.vocab.get(name).copied()
    }

    /// List all special tokens (starting with '<' or containing '|>').
    pub fn special_tokens(&self) -> Vec<(u32, &str)> {
        self.vocab.iter()
            .filter(|(k, _)| k.starts_with('<') || k.contains("|>"))
            .map(|(k, &v)| (v, k.as_str()))
            .collect()
    }

    // ── Text Chunking ──────────────────────────────────────────────────────

    /// Split text into chunks that each fit within `max_tokens` tokens.
    /// Splits on whitespace boundaries to avoid breaking words.
    pub fn chunk_text<'a>(&self, text: &'a str, max_tokens: usize) -> Vec<&'a str> {
        if text.is_empty() || max_tokens == 0 {
            return vec![];
        }
        if self.count_tokens(text) <= max_tokens {
            return vec![text];
        }
        let mut chunks = Vec::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            if self.count_tokens(remaining) <= max_tokens {
                chunks.push(remaining);
                break;
            }
            let mut split_idx = remaining.len() / 2;
            while split_idx > 0 && !remaining.is_char_boundary(split_idx) {
                split_idx -= 1;
            }
            let search_start = split_idx.saturating_sub(50);
            let search_end = (split_idx + 50).min(remaining.len());
            if let Some(ws_pos) = remaining[search_start..search_end].rfind(|c: char| c.is_whitespace()) {
                split_idx = search_start + ws_pos + 1;
            }
            if split_idx == 0 {
                split_idx = remaining.len();
            }
            let (chunk, rest) = remaining.split_at(split_idx);
            let trimmed = chunk.trim_end();
            if !trimmed.is_empty() {
                chunks.push(trimmed);
            }
            remaining = rest.trim_start();
        }
        chunks
    }

    /// Estimate the byte size of text that would produce approximately `n` tokens.
    pub fn estimate_text_length_for_tokens(&self, n: usize) -> usize {
        if self.vocab_size() == 0 {
            return 0;
        }
        let total_bytes: usize = self.vocab.keys().map(|k| k.len()).sum();
        let avg_bytes_per_token = total_bytes as f64 / self.vocab_size() as f64;
        (n as f64 * avg_bytes_per_token) as usize
    }

    // ── Diagnostics ──────────────────────────────────────────────────────────

    /// Get diagnostic information about the tokenizer.
    pub fn info(&self) -> TokenizerInfo {
        TokenizerInfo {
            vocab_size: self.vocab_size(),
            merge_count: self.merges.len(),
            bos_id: self.bos_id,
            eos_id: self.eos_id,
            is_tiktoken: self.is_tiktoken,
            special_token_count: self.special_tokens().len(),
            has_file_bytes: !self.file_bytes.is_empty(),
        }
    }

    /// Whether this is a tiktoken-style (byte-level BPE) tokenizer.
    pub fn is_tiktoken(&self) -> bool {
        self.is_tiktoken
    }

    /// Number of BPE merge rules.
    pub fn merge_count(&self) -> usize {
        self.merges.len()
    }

    /// Validate internal consistency. Returns list of error strings (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.vocab.is_empty() {
            errors.push("vocabulary is empty".to_string());
        }

        if self.id_to_token.len() != self.vocab_size() {
            errors.push(format!(
                "id_to_token length ({}) doesn't match vocab max id ({})",
                self.id_to_token.len(),
                self.vocab.values().copied().max().unwrap_or(0) + 1
            ));
        }

        // Check that every vocab entry has a corresponding id_to_token
        for (token_str, &id) in &self.vocab {
            if (id as usize) >= self.id_to_token.len() {
                errors.push(format!("vocab token {:?} has id {} beyond id_to_token length", token_str, id));
            }
        }

        // Check BOS/EOS are valid
        if self.bos_id as usize >= self.id_to_token.len() {
            errors.push(format!("bos_id {} is beyond vocabulary", self.bos_id));
        }
        if self.eos_id as usize >= self.id_to_token.len() {
            errors.push(format!("eos_id {} is beyond vocabulary", self.eos_id));
        }

        errors
    }

    /// Encode text and return token IDs with character offsets.
    /// Each entry is (token_id, start_byte_offset, end_byte_offset).
    pub fn encode_with_offsets(&self, text: &str, add_bos: bool) -> Vec<(u32, usize, usize)> {
        let mut result = Vec::new();
        if add_bos {
            result.push((self.bos_id, 0, 0));
        }
        // Simple approach: encode each character individually and track offsets
        let mut byte_offset = 0;
        let ids = self.bpe_encode(text);
        // Approximate: distribute offsets evenly (exact offsets require
        // tracking through BPE merges which is expensive)
        let chars_total: usize = text.chars().count().max(1);
        let bytes_per_char = text.len() as f64 / chars_total as f64;
        for (i, &id) in ids.iter().enumerate() {
            let start = (i as f64 * bytes_per_char) as usize;
            let end = ((i + 1) as f64 * bytes_per_char) as usize;
            result.push((id, start.min(text.len()), end.min(text.len())));
            byte_offset = end;
        }
        let _ = byte_offset;
        result
    }

    /// Decode token IDs returning (id, decoded_string) pairs for debugging.
    pub fn decode_with_ids(&self, ids: &[u32]) -> Vec<(u32, String)> {
        ids.iter().map(|&id| (id, self.decode_token(id))).collect()
    }
}

/// Diagnostic information about a tokenizer.
#[derive(Debug, Clone, Serialize)]
pub struct TokenizerInfo {
    /// Number of tokens in the vocabulary.
    pub vocab_size: usize,
    /// Number of BPE merge rules.
    pub merge_count: usize,
    /// Beginning-of-sequence token ID.
    pub bos_id: u32,
    /// End-of-sequence token ID.
    pub eos_id: u32,
    /// Whether this is a tiktoken-style (byte-level BPE) tokenizer.
    pub is_tiktoken: bool,
    /// Number of special tokens found.
    pub special_token_count: usize,
    /// Whether the tokenizer has embedded file bytes (NDAT format).
    pub has_file_bytes: bool,
}

// ─── Decode/Encode helpers ─────────────────────────────────────────────────

fn byte_to_unicode(b: u8) -> char {
    match b {
        33..=126 | 161..=172 | 174..=255 => b as char,
        _ => {
            let mut n = 0;
            for x in 0..b {
                match x {
                    33..=126 | 161..=172 | 174..=255 => {}
                    _ => n += 1,
                }
            }
            char::from_u32(256 + n)
                .expect("256+n is always a valid Unicode scalar value for n in 0..=33")
        }
    }
}

fn unicode_to_byte(c: char) -> u8 {
    let cp = c as u32;
    if cp < 256 {
        cp as u8
    } else {
        for b in 0..=255 {
            if byte_to_unicode(b) as u32 == cp {
                return b;
            }
        }
        0
    }
}

/// Convert a raw token string (from SentencePiece vocabulary) to its display form.
fn decode_raw_token_sp(raw: &str) -> String {
    // SentencePiece word-boundary marker: U+2581 → ASCII space
    if raw == "\u{2581}" {
        return " ".to_string();
    }

    // Byte-level token: `<0xXX>` → the single byte
    if let Some(hex) = raw.strip_prefix("<0x").and_then(|s| s.strip_suffix('>')) {
        if let Ok(byte_val) = u8::from_str_radix(hex, 16) {
            return String::from_utf8(vec![byte_val])
                .unwrap_or_else(|_| char::REPLACEMENT_CHARACTER.to_string());
        }
    }

    // LLaMA/BitNet special/control tokens: <...>  → empty string
    if raw.starts_with('<') && raw.ends_with('>') && !raw.contains(' ') {
        return String::new();
    }

    // Regular token: replace SentencePiece ▁ with a leading space
    raw.replace('\u{2581}', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── decode_raw_token_sp edge cases ──

    #[test]
    fn decode_sp_sentence_piece_marker() {
        // U+2581 (lower one-eighth block) → ASCII space
        assert_eq!(decode_raw_token_sp("\u{2581}"), " ");
    }

    #[test]
    fn decode_sp_leading_space_token() {
        // Token with leading ▁ → replaced with space
        assert_eq!(decode_raw_token_sp("\u{2581}hello"), " hello");
    }

    #[test]
    fn decode_sp_byte_token() {
        // <0x41> → 'A'
        assert_eq!(decode_raw_token_sp("<0x41>"), "A");
        // <0x00> → NUL byte
        assert_eq!(decode_raw_token_sp("<0x00>"), "\0");
        // <0xFF> → invalid UTF-8 → replacement character
        assert_eq!(decode_raw_token_sp("<0xFF>"), "\u{FFFD}");
    }

    #[test]
    fn decode_sp_llama_special_token() {
        // <s>, </s>, <|...|> patterns → empty string
        assert_eq!(decode_raw_token_sp("<s>"), "");
        assert_eq!(decode_raw_token_sp("</s>"), "");
        assert_eq!(decode_raw_token_sp("<|begin_of_text|>"), "");
    }

    #[test]
    fn decode_sp_regular_token() {
        // Regular token without special markers → returned as-is
        assert_eq!(decode_raw_token_sp("hello"), "hello");
        assert_eq!(decode_raw_token_sp("world"), "world");
    }

    #[test]
    fn decode_sp_angle_bracket_with_space_not_special() {
        // <foo bar> has a space → not treated as special token
        assert_eq!(decode_raw_token_sp("<foo bar>"), "<foo bar>");
    }

    #[test]
    fn decode_sp_empty_string() {
        assert_eq!(decode_raw_token_sp(""), "");
    }

    // ── decode_token boundary IDs ──

    #[test]
    fn decode_token_owned_returns_string() {
        // Build a minimal tokenizer with owned tokens (no file_bytes slicing).
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![
                TokenRef::Owned("hello".into()),
                TokenRef::Owned("".into()),
                TokenRef::Owned("world".into()),
            ],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 2,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        assert_eq!(tok.decode_token(0), "hello");
        // Empty owned token → empty string
        assert_eq!(tok.decode_token(1), "");
        assert_eq!(tok.decode_token(2), "world");
    }

    #[test]
    fn decode_token_out_of_range_returns_empty() {
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![TokenRef::Owned("only".into())],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 0,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        // ID beyond vocabulary → empty string
        assert_eq!(tok.decode_token(999), "");
    }

    #[test]
    fn decode_batch() {
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![
                TokenRef::Owned("hello".into()),
                TokenRef::Owned(" ".into()),
                TokenRef::Owned("world".into()),
            ],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 2,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        assert_eq!(tok.decode(&[0, 1, 2]), "hello world");
        assert_eq!(tok.decode(&[]), "");
    }

    #[test]
    fn vocab_size_matches_id_to_token_len() {
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![TokenRef::Owned("a".into()), TokenRef::Owned("b".into())],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 1,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        assert_eq!(tok.vocab_size(), 2);
    }

    // ── Batch & Counting ──

    fn make_test_tokenizer() -> Tokenizer {
        // Build a tiktoken-mode tokenizer with byte-to-unicode mappings.
        let mut vocab = HashMap::new();
        let mut id_to_token = Vec::new();
        for b in 33u8..=126 {
            let ch = (b as char).to_string();
            vocab.insert(ch.clone(), id_to_token.len() as u32);
            id_to_token.push(TokenRef::Owned(ch));
        }
        let space_ch = byte_to_unicode(32).to_string();
        vocab.insert(space_ch.clone(), id_to_token.len() as u32);
        id_to_token.push(TokenRef::Owned(space_ch));
        let special_names = vec!["<s>", "</s>", "<|endoftext|>"];
        for name in &special_names {
            let n = name.to_string();
            vocab.insert(n.clone(), id_to_token.len() as u32);
            id_to_token.push(TokenRef::Owned(n));
        }
        let bos_id = vocab["<s>"];
        let eos_id = vocab["</s>"];
        Tokenizer {
            vocab,
            id_to_token,
            merges: HashMap::new(),
            bos_id,
            eos_id,
            is_tiktoken: true,
            file_bytes: vec![],
        }
    }

    #[test]
    fn encode_batch_multiple_texts() {
        let tok = make_test_tokenizer();
        let results = tok.encode_batch(&["hi", "hi!"], false);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].len(), tok.encode("hi", false).len());
        assert_eq!(results[1].len(), tok.encode("hi!", false).len());
    }

    #[test]
    fn encode_batch_with_bos() {
        let tok = make_test_tokenizer();
        let results = tok.encode_batch(&["hi", "hi"], true);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0][0], tok.bos_id);
        assert_eq!(results[1][0], tok.bos_id);
        assert_eq!(results[0].len(), tok.encode("hi", false).len() + 1);
    }

    #[test]
    fn count_tokens_returns_correct_count() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.count_tokens("hi"), 2);
        assert_eq!(tok.count_tokens("hello"), 5);
    }

    #[test]
    fn count_tokens_batch_returns_individual_counts() {
        let tok = make_test_tokenizer();
        let counts = tok.count_tokens_batch(&["hi", "hello", ""]);
        assert_eq!(counts, vec![2, 5, 0]);
    }

    #[test]
    fn total_tokens_sums_batch() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.total_tokens(&["hi", "hello"]), 7);
    }

    #[test]
    fn exceeds_budget_detects_overflow() {
        let tok = make_test_tokenizer();
        assert!(!tok.exceeds_budget("hi", 5));
        assert!(tok.exceeds_budget("hello", 3));
    }

    #[test]
    fn special_token_id_lookup() {
        let tok = make_test_tokenizer();
        assert!(tok.special_token_id("<s>").is_some());
        assert!(tok.special_token_id("</s>").is_some());
        assert!(tok.special_token_id("<|endoftext|>").is_some());
        assert_eq!(tok.special_token_id("<nonexistent>"), None);
    }

    #[test]
    fn special_tokens_lists_all() {
        let tok = make_test_tokenizer();
        let specials = tok.special_tokens();
        assert!(specials.len() >= 3);
        let names: Vec<&str> = specials.iter().map(|(_, n)| *n).collect();
        assert!(names.contains(&"<s>"));
        assert!(names.contains(&"</s>"));
        assert!(names.contains(&"<|endoftext|>"));
    }

    #[test]
    fn chunk_text_empty_input() {
        let tok = make_test_tokenizer();
        assert!(tok.chunk_text("", 10).is_empty());
        assert!(tok.chunk_text("hi", 0).is_empty());
    }

    #[test]
    fn chunk_text_single_chunk() {
        let tok = make_test_tokenizer();
        let chunks = tok.chunk_text("hi", 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hi");
    }

    #[test]
    fn estimate_text_length_for_tokens_zero_vocab() {
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 0,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        assert_eq!(tok.estimate_text_length_for_tokens(10), 0);
    }

    #[test]
    fn estimate_text_length_for_tokens_returns_nonzero() {
        let tok = make_test_tokenizer();
        let est = tok.estimate_text_length_for_tokens(10);
        assert!(est > 0);
    }

    // ── Diagnostics & Validation ──

    #[test]
    fn tokenizer_info_serialize() {
        let info = TokenizerInfo {
            vocab_size: 151936,
            merge_count: 100000,
            bos_id: 1,
            eos_id: 2,
            is_tiktoken: false,
            special_token_count: 50,
            has_file_bytes: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"vocab_size\":151936"));
        assert!(json.contains("\"is_tiktoken\":false"));
    }

    #[test]
    fn tokenizer_info_returns_correct_fields() {
        let tok = make_test_tokenizer();
        let info = tok.info();
        assert!(info.vocab_size > 0);
        assert_eq!(info.merge_count, 0); // test tokenizer has no merges
        assert!(info.is_tiktoken);
        assert!(!info.has_file_bytes);
    }

    #[test]
    fn tokenizer_validate_valid() {
        let tok = make_test_tokenizer();
        let errors = tok.validate();
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn tokenizer_validate_empty_vocab() {
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 0,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        let errors = tok.validate();
        assert!(errors.iter().any(|e| e.contains("vocabulary is empty")));
    }

    #[test]
    fn tokenizer_validate_bad_bos() {
        let tok = Tokenizer {
            vocab: HashMap::from([("a".to_string(), 0)]),
            id_to_token: vec![TokenRef::Owned("a".into())],
            merges: HashMap::new(),
            bos_id: 999, // beyond vocab
            eos_id: 0,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        let errors = tok.validate();
        assert!(errors.iter().any(|e| e.contains("bos_id")));
    }

    #[test]
    fn is_tiktoken_accessor() {
        let tok = make_test_tokenizer();
        assert!(tok.is_tiktoken());
    }

    #[test]
    fn merge_count_accessor() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.merge_count(), 0); // test tokenizer has no merges
    }

    #[test]
    fn encode_with_offsets_returns_tuples() {
        let tok = make_test_tokenizer();
        let result = tok.encode_with_offsets("hi", false);
        assert_eq!(result.len(), 2); // 'h' and 'i'
        for (id, start, end) in &result {
            assert!(*start <= 2);
            assert!(*end <= 2);
        }
    }

    #[test]
    fn encode_with_offsets_includes_bos() {
        let tok = make_test_tokenizer();
        let result = tok.encode_with_offsets("hi", true);
        assert_eq!(result[0].0, tok.bos_id);
        assert_eq!(result[0].1, 0);
        assert_eq!(result[0].2, 0);
    }

    #[test]
    fn decode_with_ids_returns_pairs() {
        let tok = make_test_tokenizer();
        let ids = tok.encode("hi", false);
        let pairs = tok.decode_with_ids(&ids);
        assert_eq!(pairs.len(), ids.len());
        for (id, _s) in &pairs {
            assert!((*id as usize) < tok.vocab_size());
        }
    }
}
