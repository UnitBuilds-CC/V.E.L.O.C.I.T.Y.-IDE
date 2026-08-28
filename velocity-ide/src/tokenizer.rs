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

    // ── Advanced Encoding ────────────────────────────────────────────────────

    /// Compute token frequency distribution for a text.
    /// Returns (token_id, count) pairs sorted by count descending.
    pub fn token_frequency(&self, text: &str, add_bos: bool) -> Vec<(u32, usize)> {
        let ids = self.encode(text, add_bos);
        let mut freq: HashMap<u32, usize> = HashMap::new();
        for id in ids {
            *freq.entry(id).or_insert(0) += 1;
        }
        let mut result: Vec<(u32, usize)> = freq.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        result
    }

    /// Encode text with structured diagnostics.
    pub fn encode_reported(&self, text: &str, add_bos: bool) -> EncodeReport {
        let start = std::time::Instant::now();
        let ids = self.encode(text, add_bos);
        let elapsed = start.elapsed();

        let unique_count = {
            let mut seen = std::collections::HashSet::new();
            for id in &ids {
                seen.insert(*id);
            }
            seen.len()
        };

        let decoded = self.decode(&ids);
        let roundtrip_ok = decoded == text;

        EncodeReport {
            token_count: ids.len(),
            unique_token_count: unique_count,
            input_bytes: text.len(),
            output_bytes: decoded.len(),
            bytes_per_token: if ids.is_empty() { 0.0 } else { text.len() as f64 / ids.len() as f64 },
            elapsed_us: elapsed.as_micros() as u64,
            roundtrip_ok,
            has_bos: add_bos && ids.first().is_some_and(|&id| id == self.bos_id),
        }
    }

    /// Run-length encode token IDs for compact storage.
    /// Returns (token_id, run_length) pairs.
    pub fn encode_rle(&self, text: &str, add_bos: bool) -> Vec<(u32, u32)> {
        let ids = self.encode(text, add_bos);
        let mut rle: Vec<(u32, u32)> = Vec::new();
        for id in ids {
            if let Some(last) = rle.last_mut() {
                if last.0 == id {
                    last.1 += 1;
                    continue;
                }
            }
            rle.push((id, 1));
        }
        rle
    }

    /// Decode run-length encoded token IDs back to a string.
    pub fn decode_rle(&self, rle: &[(u32, u32)]) -> String {
        let mut ids = Vec::new();
        for &(id, count) in rle {
            for _ in 0..count {
                ids.push(id);
            }
        }
        self.decode(&ids)
    }

    /// Split text into word-level tokens, preserving whitespace.
    /// Returns (word, start_byte, end_byte) tuples.
    pub fn split_into_words<'a>(&self, text: &'a str) -> Vec<(&'a str, usize, usize)> {
        let mut words = Vec::new();
        let mut start = None;
        for (i, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if let Some(s) = start.take() {
                    words.push((&text[s..i], s, i));
                }
            } else if start.is_none() {
                start = Some(i);
            }
        }
        if let Some(s) = start {
            words.push((&text[s..], s, text.len()));
        }
        words
    }

    /// Compute vocabulary utilization for a given text.
    /// Returns the fraction of unique tokens that appear in the vocabulary.
    pub fn vocabulary_utilization(&self, text: &str) -> VocabularyUtilization {
        let ids = self.encode(text, false);
        let total = ids.len();
        let unique: std::collections::HashSet<u32> = ids.into_iter().collect();
        let unique_count = unique.len();
        let in_vocab = unique.iter().filter(|&&id| (id as usize) < self.id_to_token.len()).count();
        let coverage = if unique_count == 0 { 1.0 } else { in_vocab as f64 / unique_count as f64 };

        VocabularyUtilization {
            total_tokens: total,
            unique_tokens: unique_count,
            in_vocabulary: in_vocab,
            coverage: (coverage * 1000.0).round() / 1000.0,
            vocab_size: self.vocab_size(),
        }
    }
}

/// Structured encoding diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct EncodeReport {
    pub token_count: usize,
    pub unique_token_count: usize,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub bytes_per_token: f64,
    pub elapsed_us: u64,
    pub roundtrip_ok: bool,
    pub has_bos: bool,
}

/// Vocabulary utilization report.
#[derive(Debug, Clone, Serialize)]
pub struct VocabularyUtilization {
    pub total_tokens: usize,
    pub unique_tokens: usize,
    pub in_vocabulary: usize,
    pub coverage: f64,
    pub vocab_size: usize,
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
        for (_id, start, end) in &result {
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

    // ── Block 79: Advanced encoding tests ──────────────────────────────

    #[test]
    fn token_frequency_basic() {
        let tok = make_test_tokenizer();
        let freq = tok.token_frequency("hi", false);
        assert!(!freq.is_empty());
        // Each char in "hi" is unique, so each should appear once
        for &(_, count) in &freq {
            assert!(count >= 1);
        }
    }

    #[test]
    fn token_frequency_with_repeats() {
        let tok = make_test_tokenizer();
        let freq = tok.token_frequency("hhh", false);
        // 'h' appears 3 times
        assert!(!freq.is_empty());
        let total: usize = freq.iter().map(|(_, c)| *c).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn encode_reported_basic() {
        let tok = make_test_tokenizer();
        let report = tok.encode_reported("hello", false);
        assert_eq!(report.token_count, 5);
        assert!(report.bytes_per_token > 0.0);
        assert!(report.roundtrip_ok);
        assert!(!report.has_bos);
    }

    #[test]
    fn encode_reported_with_bos() {
        let tok = make_test_tokenizer();
        let report = tok.encode_reported("hi", true);
        assert!(report.has_bos);
        assert_eq!(report.token_count, 3); // bos + h + i
    }

    #[test]
    fn encode_reported_serializes() {
        let report = EncodeReport {
            token_count: 10,
            unique_token_count: 8,
            input_bytes: 20,
            output_bytes: 18,
            bytes_per_token: 2.0,
            elapsed_us: 100,
            roundtrip_ok: true,
            has_bos: false,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"token_count\":10"));
        assert!(json.contains("\"roundtrip_ok\":true"));
    }

    #[test]
    fn encode_rle_basic() {
        let tok = make_test_tokenizer();
        let rle = tok.encode_rle("hi", false);
        assert!(!rle.is_empty());
        // Each char is different, so each run is length 1
        for &(_, count) in &rle {
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn decode_rle_roundtrip() {
        let tok = make_test_tokenizer();
        let text = "hello";
        let rle = tok.encode_rle(text, false);
        let decoded = tok.decode_rle(&rle);
        assert_eq!(decoded, text);
    }

    #[test]
    fn decode_rle_empty() {
        let tok = make_test_tokenizer();
        let decoded = tok.decode_rle(&[]);
        assert_eq!(decoded, "");
    }

    #[test]
    fn split_into_words_basic() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("hello world foo");
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].0, "hello");
        assert_eq!(words[1].0, "world");
        assert_eq!(words[2].0, "foo");
    }

    #[test]
    fn split_into_words_empty() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("");
        assert!(words.is_empty());
    }

    #[test]
    fn split_into_words_multiple_spaces() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("a  b   c");
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].0, "a");
        assert_eq!(words[1].0, "b");
        assert_eq!(words[2].0, "c");
    }

    #[test]
    fn split_into_words_offsets() {
        let tok = make_test_tokenizer();
        let text = "hello world";
        let words = tok.split_into_words(text);
        for (word, start, end) in &words {
            assert_eq!(&text[*start..*end], *word);
        }
    }

    #[test]
    fn vocabulary_utilization_basic() {
        let tok = make_test_tokenizer();
        let util = tok.vocabulary_utilization("hello");
        assert_eq!(util.total_tokens, 5);
        assert!(util.unique_tokens > 0);
        assert!(util.coverage <= 1.0);
        assert!(util.coverage > 0.0);
    }

    #[test]
    fn vocabulary_utilization_empty() {
        let tok = make_test_tokenizer();
        let util = tok.vocabulary_utilization("");
        assert_eq!(util.total_tokens, 0);
        assert_eq!(util.coverage, 1.0); // empty text = full coverage
    }

    #[test]
    fn vocabulary_utilization_serializes() {
        let util = VocabularyUtilization {
            total_tokens: 100,
            unique_tokens: 50,
            in_vocabulary: 48,
            coverage: 0.96,
            vocab_size: 151936,
        };
        let json = serde_json::to_string(&util).unwrap();
        assert!(json.contains("\"unique_tokens\":50"));
        assert!(json.contains("\"coverage\":0.96"));
    }

    // ── Block 98: byte↔unicode mapping tests ─────────────────────────────

    #[test]
    fn byte_to_unicode_printable_range_identity() {
        // Bytes 33..=126 map to themselves (ASCII printable)
        for b in 33u8..=126 {
            assert_eq!(byte_to_unicode(b) as u32, b as u32);
        }
    }

    #[test]
    fn byte_to_unicode_control_bytes_map_above_256() {
        // Control bytes (0..32) and whitespace map to Unicode 256+
        for b in 0u8..32 {
            let c = byte_to_unicode(b);
            assert!((c as u32) >= 256, "byte {} mapped to {}", b, c as u32);
        }
        // Space (32) is also remapped
        assert!((byte_to_unicode(32) as u32) >= 256);
    }

    #[test]
    fn byte_to_unicode_all_bytes_produce_unique_chars() {
        let mut seen = std::collections::HashSet::new();
        for b in 0u8..=255 {
            let c = byte_to_unicode(b);
            assert!(seen.insert(c), "byte {} produced duplicate char {}", b, c as u32);
        }
        assert_eq!(seen.len(), 256);
    }

    #[test]
    fn unicode_to_byte_full_roundtrip() {
        // Every byte 0..=255 roundtrips through byte_to_unicode → unicode_to_byte
        for b in 0u8..=255 {
            let c = byte_to_unicode(b);
            let recovered = unicode_to_byte(c);
            assert_eq!(recovered, b, "roundtrip failed for byte {}", b);
        }
    }

    // ── Block 98: encode/decode edge cases ───────────────────────────────

    #[test]
    fn encode_empty_string_no_bos() {
        let tok = make_test_tokenizer();
        let ids = tok.encode("", false);
        assert!(ids.is_empty());
    }

    #[test]
    fn encode_empty_string_with_bos() {
        let tok = make_test_tokenizer();
        let ids = tok.encode("", true);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], tok.bos_id);
    }

    #[test]
    fn encode_bos_prepends_exactly_one_token() {
        let tok = make_test_tokenizer();
        let ids_no_bos = tok.encode("hi", false);
        let ids_bos = tok.encode("hi", true);
        assert_eq!(ids_bos.len(), ids_no_bos.len() + 1);
        assert_eq!(ids_bos[0], tok.bos_id);
        assert_eq!(&ids_bos[1..], &ids_no_bos[..]);
    }

    #[test]
    fn decode_token_tiktoken_strips_special_tokens() {
        let tok = make_test_tokenizer();
        // <|endoftext|> in tiktoken mode → empty string
        let endoftext_id = tok.vocab.get("<|endoftext|>").copied().unwrap();
        assert_eq!(tok.decode_token(endoftext_id), "");
    }

    #[test]
    fn decode_empty_slice() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.decode(&[]), "");
    }

    #[test]
    fn encode_batch_empty_list() {
        let tok = make_test_tokenizer();
        let results = tok.encode_batch(&[], false);
        assert!(results.is_empty());
    }

    #[test]
    fn encode_batch_single_empty_string() {
        let tok = make_test_tokenizer();
        let results = tok.encode_batch(&[""], false);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_empty());
    }

    #[test]
    fn count_tokens_empty_string() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.count_tokens(""), 0);
    }

    #[test]
    fn total_tokens_empty_batch() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.total_tokens(&[]), 0);
    }

    // ── Block 98: budget boundary ────────────────────────────────────────

    #[test]
    fn exceeds_budget_exact_boundary() {
        let tok = make_test_tokenizer();
        // "hi" → 2 tokens; budget of exactly 2 → NOT exceeded
        assert!(!tok.exceeds_budget("hi", 2));
        // budget of 1 → exceeded
        assert!(tok.exceeds_budget("hi", 1));
        // budget of 0 → exceeded for non-empty
        assert!(tok.exceeds_budget("hi", 0));
        // empty text with budget 0 → not exceeded
        assert!(!tok.exceeds_budget("", 0));
    }

    // ── Block 98: chunk_text advanced ────────────────────────────────────

    #[test]
    fn chunk_text_multiple_chunks() {
        let tok = make_test_tokenizer();
        // Each char = 1 token in tiktoken mode. "abcdefgh" = 8 tokens.
        // With max_tokens=3, we need multiple chunks.
        let text = "abc def ghi jkl";
        let chunks = tok.chunk_text(text, 3);
        assert!(chunks.len() >= 2, "expected multiple chunks, got {}", chunks.len());
    }

    #[test]
    fn chunk_text_all_content_preserved() {
        let tok = make_test_tokenizer();
        let text = "hello world foo bar baz";
        let chunks = tok.chunk_text(text, 4);
        // Rejoin chunks — should approximately reconstruct original words
        let rejoined: String = chunks.join(" ");
        // Every original word should appear in the rejoined string
        for word in text.split_whitespace() {
            assert!(
                rejoined.contains(word),
                "word {:?} missing from rejoined chunks: {:?}",
                word,
                chunks
            );
        }
    }

    #[test]
    fn chunk_text_splits_long_text() {
        let tok = make_test_tokenizer();
        // Use text with whitespace so chunker can split on word boundaries
        let text = "ab cd ef gh ij kl mn op";
        let max = 4;
        let chunks = tok.chunk_text(text, max);
        // Should produce multiple chunks for long text
        assert!(chunks.len() >= 2, "expected multiple chunks, got {}", chunks.len());
        // Each chunk should be strictly shorter than the original
        for chunk in &chunks {
            assert!(chunk.len() < text.len(), "chunk should be shorter than original");
            assert!(!chunk.is_empty(), "no empty chunks");
        }
    }

    // ── Block 98: estimate_text_length ───────────────────────────────────

    #[test]
    fn estimate_text_length_scales_linearly() {
        let tok = make_test_tokenizer();
        let est10 = tok.estimate_text_length_for_tokens(10);
        let est20 = tok.estimate_text_length_for_tokens(20);
        // 20 tokens should be approximately 2× 10 tokens (allow rounding)
        let ratio = est20 as f64 / est10 as f64;
        assert!(
            (ratio - 2.0).abs() < 0.15,
            "expected ratio ~2.0, got {}",
            ratio
        );
    }

    #[test]
    fn estimate_text_length_zero_tokens() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.estimate_text_length_for_tokens(0), 0);
    }

    // ── Block 98: validation edge cases ──────────────────────────────────

    #[test]
    fn tokenizer_validate_bad_eos() {
        let tok = Tokenizer {
            vocab: HashMap::from([("a".to_string(), 0)]),
            id_to_token: vec![TokenRef::Owned("a".into())],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 999, // beyond vocabulary
            is_tiktoken: false,
            file_bytes: vec![],
        };
        let errors = tok.validate();
        assert!(errors.iter().any(|e| e.contains("eos_id")));
    }

    #[test]
    fn tokenizer_validate_vocab_id_beyond_id_to_token() {
        let tok = Tokenizer {
            vocab: HashMap::from([
                ("a".to_string(), 0),
                ("b".to_string(), 99), // id 99 but id_to_token only has 1 entry
            ]),
            id_to_token: vec![TokenRef::Owned("a".into())],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 0,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        let errors = tok.validate();
        assert!(
            errors.iter().any(|e| e.contains("beyond id_to_token") || e.contains("99")),
            "expected error about id beyond id_to_token, got: {:?}",
            errors
        );
    }

    // ── Block 98: encode_reported edge cases ─────────────────────────────

    #[test]
    fn encode_reported_empty_text() {
        let tok = make_test_tokenizer();
        let report = tok.encode_reported("", false);
        assert_eq!(report.token_count, 0);
        assert_eq!(report.bytes_per_token, 0.0);
        assert_eq!(report.input_bytes, 0);
    }

    #[test]
    fn encode_reported_unique_count_less_than_total() {
        let tok = make_test_tokenizer();
        // "hhh" → 3 tokens but only 1 unique
        let report = tok.encode_reported("hhh", false);
        assert_eq!(report.token_count, 3);
        assert_eq!(report.unique_token_count, 1);
    }

    // ── Block 98: RLE advanced ───────────────────────────────────────────

    #[test]
    fn encode_rle_repeated_tokens_compress() {
        let tok = make_test_tokenizer();
        // "hhh" → 3 identical tokens → single RLE run
        let rle = tok.encode_rle("hhh", false);
        assert_eq!(rle.len(), 1);
        assert_eq!(rle[0].1, 3); // run length = 3
    }

    #[test]
    fn decode_rle_single_long_run() {
        let tok = make_test_tokenizer();
        // Single run of 5 copies of 'h' token
        let h_id = tok.encode("h", false)[0];
        let decoded = tok.decode_rle(&[(h_id, 5)]);
        assert_eq!(decoded, "hhhhh");
    }

    // ── Block 98: vocabulary utilization ─────────────────────────────────

    #[test]
    fn vocabulary_utilization_all_fields_accurate() {
        let tok = make_test_tokenizer();
        let util = tok.vocabulary_utilization("hi");
        assert_eq!(util.total_tokens, 2);
        assert_eq!(util.unique_tokens, 2); // 'h' and 'i' are different
        assert_eq!(util.in_vocabulary, 2); // both in vocab
        assert_eq!(util.coverage, 1.0);
        assert_eq!(util.vocab_size, tok.vocab_size());
    }

    #[test]
    fn vocabulary_utilization_serializes_all_fields() {
        let tok = make_test_tokenizer();
        let util = tok.vocabulary_utilization("hi");
        let json = serde_json::to_string(&util).unwrap();
        assert!(json.contains("\"total_tokens\""));
        assert!(json.contains("\"unique_tokens\""));
        assert!(json.contains("\"in_vocabulary\""));
        assert!(json.contains("\"coverage\""));
        assert!(json.contains("\"vocab_size\""));
    }

    // ── Block 98: TokenizerInfo comprehensive ────────────────────────────

    #[test]
    fn tokenizer_info_all_fields_accurate() {
        let tok = make_test_tokenizer();
        let info = tok.info();
        assert_eq!(info.vocab_size, tok.vocab_size());
        assert_eq!(info.merge_count, tok.merge_count());
        assert_eq!(info.bos_id, tok.bos_id);
        assert_eq!(info.eos_id, tok.eos_id);
        assert_eq!(info.is_tiktoken, tok.is_tiktoken());
        assert_eq!(info.has_file_bytes, false);
        assert!(info.special_token_count >= 3); // <s>, </s>, <|endoftext|>
    }

    // ── Block 98: decode_token with Slice variant (NDAT format) ─────────

    #[test]
    fn decode_token_slice_variant_ndat_format() {
        // Build a minimal NDAT-format tokenizer with file_bytes
        // Layout: header(24) + vocab_table(vocab_size*8) + merges_table(merges_count*20) + string_data
        let vocab_size = 2u32;
        let merges_count = 0u32;
        let _string_data_start = 24 + vocab_size as usize * 8 + merges_count as usize * 20;
        let token_strings = ["hello", "world"];

        // Build string data block and offset table
        let mut string_data = Vec::new();
        let mut offset_table = Vec::new();
        for s in &token_strings {
            let offset = string_data.len() as u32;
            let len = s.len() as u32;
            offset_table.extend_from_slice(&offset.to_le_bytes());
            offset_table.extend_from_slice(&len.to_le_bytes());
            string_data.extend_from_slice(s.as_bytes());
        }

        // Build header
        let mut header = Vec::new();
        header.extend_from_slice(b"NDAT"); // magic
        header.extend_from_slice(&1u16.to_le_bytes()); // version
        header.extend_from_slice(&vocab_size.to_le_bytes()); // vocab_size
        header.extend_from_slice(&merges_count.to_le_bytes()); // merges_count
        header.extend_from_slice(&0u32.to_le_bytes()); // bos_id
        header.extend_from_slice(&1u32.to_le_bytes()); // eos_id
        header.push(0); // is_tiktoken = false
        header.push(0); // padding byte

        let mut file_bytes = header;
        file_bytes.extend_from_slice(&offset_table);
        file_bytes.extend_from_slice(&string_data);

        let mut vocab = HashMap::new();
        vocab.insert("hello".to_string(), 0u32);
        vocab.insert("world".to_string(), 1u32);

        let tok = Tokenizer {
            vocab,
            id_to_token: vec![
                TokenRef::Slice { offset: 0, len: 5 },
                TokenRef::Slice { offset: 5, len: 5 },
            ],
            merges: HashMap::new(),
            bos_id: 0,
            eos_id: 1,
            is_tiktoken: false,
            file_bytes,
        };

        assert_eq!(tok.decode_token(0), "hello");
        assert_eq!(tok.decode_token(1), "world");
    }

    // ── Block 98: split_into_words edge cases ────────────────────────────

    #[test]
    fn split_into_words_leading_spaces() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("  hello");
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].0, "hello");
    }

    #[test]
    fn split_into_words_trailing_spaces() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("hello  ");
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].0, "hello");
    }

    #[test]
    fn split_into_words_only_spaces() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("   ");
        assert!(words.is_empty());
    }

    // ── Block 98: token_frequency ordering ───────────────────────────────

    #[test]
    fn token_frequency_sorted_descending() {
        let tok = make_test_tokenizer();
        let freq = tok.token_frequency("hello", false);
        // Verify sorted descending by count
        for window in freq.windows(2) {
            assert!(
                window[0].1 >= window[1].1,
                "not sorted descending: {:?} before {:?}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn token_frequency_empty_text() {
        let tok = make_test_tokenizer();
        let freq = tok.token_frequency("", false);
        assert!(freq.is_empty());
    }

    // ── Block 98: encode_with_offsets edge cases ─────────────────────────

    #[test]
    fn encode_with_offsets_empty_text() {
        let tok = make_test_tokenizer();
        let result = tok.encode_with_offsets("", false);
        assert!(result.is_empty());
    }

    #[test]
    fn encode_with_offsets_offsets_monotonic() {
        let tok = make_test_tokenizer();
        let result = tok.encode_with_offsets("hello world", false);
        for window in result.windows(2) {
            assert!(window[1].1 >= window[0].1, "start offsets not monotonic");
            assert!(window[1].2 >= window[0].2, "end offsets not monotonic");
        }
    }

    // ── Block 183: JSON key counts ────────────────────────────────────────

    #[test]
    fn encode_report_json_has_exactly_8_keys() {
        let report = EncodeReport {
            token_count: 5, unique_token_count: 3, input_bytes: 10,
            output_bytes: 8, bytes_per_token: 2.0, elapsed_us: 50,
            roundtrip_ok: true, has_bos: false,
        };
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&report).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 8);
    }

    #[test]
    fn vocab_util_json_has_exactly_5_keys() {
        let util = VocabularyUtilization {
            total_tokens: 10, unique_tokens: 5, in_vocabulary: 5,
            coverage: 1.0, vocab_size: 100,
        };
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&util).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 5);
    }

    #[test]
    fn tokenizer_info_json_has_exactly_7_keys() {
        let info = TokenizerInfo {
            vocab_size: 100, merge_count: 50, bos_id: 1, eos_id: 2,
            is_tiktoken: true, special_token_count: 3, has_file_bytes: false,
        };
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&info).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 7);
    }

    // ── Block 183: JSON roundtrip via Value ───────────────────────────────

    #[test]
    fn encode_report_json_roundtrip_via_value() {
        let report = EncodeReport {
            token_count: 42, unique_token_count: 20, input_bytes: 100,
            output_bytes: 80, bytes_per_token: 2.38, elapsed_us: 500,
            roundtrip_ok: false, has_bos: true,
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["token_count"], 42);
        assert_eq!(val["unique_token_count"], 20);
        assert_eq!(val["roundtrip_ok"], false);
        assert_eq!(val["has_bos"], true);
        assert!((val["bytes_per_token"].as_f64().unwrap() - 2.38).abs() < 0.01);
    }

    #[test]
    fn vocab_util_json_roundtrip_via_value() {
        let util = VocabularyUtilization {
            total_tokens: 200, unique_tokens: 150, in_vocabulary: 148,
            coverage: 0.987, vocab_size: 50000,
        };
        let json = serde_json::to_string(&util).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["total_tokens"], 200);
        assert_eq!(val["in_vocabulary"], 148);
        assert!((val["coverage"].as_f64().unwrap() - 0.987).abs() < 0.001);
    }

    // ── Block 183: Clone independence ─────────────────────────────────────

    #[test]
    fn encode_report_clone_independent() {
        let mut report = EncodeReport {
            token_count: 10, unique_token_count: 5, input_bytes: 20,
            output_bytes: 18, bytes_per_token: 2.0, elapsed_us: 100,
            roundtrip_ok: true, has_bos: false,
        };
        let cloned = report.clone();
        report.token_count = 999;
        assert_eq!(cloned.token_count, 10);
        assert_eq!(report.token_count, 999);
    }

    #[test]
    fn vocab_util_clone_independent() {
        let mut util = VocabularyUtilization {
            total_tokens: 50, unique_tokens: 30, in_vocabulary: 28,
            coverage: 0.93, vocab_size: 1000,
        };
        let cloned = util.clone();
        util.coverage = 0.0;
        assert!((cloned.coverage - 0.93).abs() < 0.001);
        assert!((util.coverage - 0.0).abs() < 0.001);
    }

    #[test]
    fn tokenizer_info_clone_independent() {
        let mut info = TokenizerInfo {
            vocab_size: 100, merge_count: 50, bos_id: 1, eos_id: 2,
            is_tiktoken: true, special_token_count: 3, has_file_bytes: false,
        };
        let cloned = info.clone();
        info.vocab_size = 0;
        assert_eq!(cloned.vocab_size, 100);
        assert_eq!(info.vocab_size, 0);
    }

    // ── Block 183: Debug format ───────────────────────────────────────────

    #[test]
    fn encode_report_debug_has_all_fields() {
        let report = EncodeReport {
            token_count: 5, unique_token_count: 3, input_bytes: 10,
            output_bytes: 8, bytes_per_token: 2.0, elapsed_us: 50,
            roundtrip_ok: true, has_bos: false,
        };
        let debug = format!("{:?}", report);
        assert!(debug.contains("token_count"));
        assert!(debug.contains("unique_token_count"));
        assert!(debug.contains("bytes_per_token"));
        assert!(debug.contains("roundtrip_ok"));
        assert!(debug.contains("has_bos"));
    }

    #[test]
    fn vocab_util_debug_has_all_fields() {
        let util = VocabularyUtilization {
            total_tokens: 10, unique_tokens: 5, in_vocabulary: 5,
            coverage: 1.0, vocab_size: 100,
        };
        let debug = format!("{:?}", util);
        assert!(debug.contains("total_tokens"));
        assert!(debug.contains("unique_tokens"));
        assert!(debug.contains("in_vocabulary"));
        assert!(debug.contains("coverage"));
        assert!(debug.contains("vocab_size"));
    }

    // ── Block 183: EncodeReport formula verification ──────────────────────

    #[test]
    fn encode_reported_bytes_per_token_formula() {
        let tok = make_test_tokenizer();
        let text = "hello"; // 5 tokens in tiktoken mode
        let report = tok.encode_reported(text, false);
        let expected_bpt = text.len() as f64 / report.token_count as f64;
        assert!((report.bytes_per_token - expected_bpt).abs() < 1e-9);
    }

    #[test]
    fn encode_reported_input_bytes_matches_text_len() {
        let tok = make_test_tokenizer();
        let text = "test input";
        let report = tok.encode_reported(text, false);
        assert_eq!(report.input_bytes, text.len());
    }

    // ── Block 183: RLE advanced patterns ──────────────────────────────────

    #[test]
    fn encode_rle_mixed_runs() {
        let tok = make_test_tokenizer();
        // "hhi" → h,h,i → two runs: (h,2),(i,1)
        let rle = tok.encode_rle("hhi", false);
        assert_eq!(rle.len(), 2);
        assert_eq!(rle[0].1, 2); // h repeated twice
        assert_eq!(rle[1].1, 1); // i once
    }

    #[test]
    fn decode_rle_multiple_runs() {
        let tok = make_test_tokenizer();
        let h_id = tok.encode("h", false)[0];
        let i_id = tok.encode("i", false)[0];
        let decoded = tok.decode_rle(&[(h_id, 3), (i_id, 2)]);
        assert_eq!(decoded, "hhhii");
    }

    // ── Block 183: Consistency checks ─────────────────────────────────────

    #[test]
    fn encode_batch_consistent_with_encode() {
        let tok = make_test_tokenizer();
        let texts = ["hello", "world", "hi"];
        let batch = tok.encode_batch(&texts, false);
        for (i, text) in texts.iter().enumerate() {
            assert_eq!(batch[i], tok.encode(text, false));
        }
    }

    #[test]
    fn count_tokens_batch_consistent_with_count() {
        let tok = make_test_tokenizer();
        let texts = ["hello", "world", "hi"];
        let batch_counts = tok.count_tokens_batch(&texts);
        for (i, text) in texts.iter().enumerate() {
            assert_eq!(batch_counts[i], tok.count_tokens(text));
        }
    }

    #[test]
    fn total_tokens_equals_sum_of_individual_counts() {
        let tok = make_test_tokenizer();
        let texts = ["hello", "world", "hi", "abc"];
        let total = tok.total_tokens(&texts);
        let manual: usize = texts.iter().map(|t| tok.count_tokens(t)).sum();
        assert_eq!(total, manual);
    }

    // ── Block 183: Vocabulary utilization formula ─────────────────────────

    #[test]
    fn vocabulary_utilization_coverage_formula() {
        let tok = make_test_tokenizer();
        let util = tok.vocabulary_utilization("hi");
        // coverage = in_vocabulary / unique_tokens
        let expected = util.in_vocabulary as f64 / util.unique_tokens as f64;
        let expected_rounded = (expected * 1000.0).round() / 1000.0;
        assert!((util.coverage - expected_rounded).abs() < 1e-6);
    }

    #[test]
    fn vocabulary_utilization_repeated_chars() {
        let tok = make_test_tokenizer();
        // "hhh" → 3 tokens, 1 unique
        let util = tok.vocabulary_utilization("hhh");
        assert_eq!(util.total_tokens, 3);
        assert_eq!(util.unique_tokens, 1);
        assert_eq!(util.in_vocabulary, 1);
        assert_eq!(util.coverage, 1.0);
    }

    // ── Block 183: encode_with_offsets consistency ────────────────────────

    #[test]
    fn encode_with_offsets_length_matches_encode() {
        let tok = make_test_tokenizer();
        let text = "hello world";
        let ids = tok.encode(text, false);
        let with_offsets = tok.encode_with_offsets(text, false);
        assert_eq!(ids.len(), with_offsets.len());
        for (i, &(id, _, _)) in with_offsets.iter().enumerate() {
            assert_eq!(id, ids[i]);
        }
    }

    #[test]
    fn decode_with_ids_consistent_with_decode_token() {
        let tok = make_test_tokenizer();
        let ids = tok.encode("test", false);
        let pairs = tok.decode_with_ids(&ids);
        for &(id, ref s) in &pairs {
            assert_eq!(*s, tok.decode_token(id));
        }
    }

    // ── Block 183: byte_to_unicode boundary values ────────────────────────

    #[test]
    fn byte_to_unicode_161_to_172_identity() {
        // Bytes 161..=172 map to themselves (Latin-1 supplement range)
        for b in 161u8..=172 {
            assert_eq!(byte_to_unicode(b) as u32, b as u32, "byte {} should be identity", b);
        }
    }

    #[test]
    fn byte_to_unicode_173_maps_above_256() {
        // Byte 173 (soft hyphen) is NOT in the identity ranges
        let c = byte_to_unicode(173);
        assert!((c as u32) >= 256);
    }

    #[test]
    fn byte_to_unicode_174_to_255_identity() {
        // Bytes 174..=255 map to themselves
        for b in 174u8..=255 {
            assert_eq!(byte_to_unicode(b) as u32, b as u32, "byte {} should be identity", b);
        }
    }

    // ── Block 183: special_tokens filter criteria ─────────────────────────

    #[test]
    fn special_tokens_includes_pipe_tokens() {
        let tok = make_test_tokenizer();
        let specials = tok.special_tokens();
        // <|endoftext|> matches the |> filter
        let names: Vec<&str> = specials.iter().map(|(_, n)| *n).collect();
        assert!(names.contains(&"<|endoftext|>"));
    }

    #[test]
    fn special_tokens_excludes_regular_tokens() {
        let tok = make_test_tokenizer();
        let specials = tok.special_tokens();
        let names: Vec<&str> = specials.iter().map(|(_, n)| *n).collect();
        // Regular characters like 'a' or 'h' should not appear
        assert!(!names.contains(&"h"));
        assert!(!names.contains(&"a"));
    }

    // ── Block 183: validate multiple errors ───────────────────────────────

    #[test]
    fn validate_multiple_errors_at_once() {
        let tok = Tokenizer {
            vocab: HashMap::new(),
            id_to_token: vec![],
            merges: HashMap::new(),
            bos_id: 999,
            eos_id: 888,
            is_tiktoken: false,
            file_bytes: vec![],
        };
        let errors = tok.validate();
        // Should have: empty vocab, bad bos_id, bad eos_id
        assert!(errors.len() >= 3, "expected >= 3 errors, got {}: {:?}", errors.len(), errors);
        assert!(errors.iter().any(|e| e.contains("vocabulary is empty")));
        assert!(errors.iter().any(|e| e.contains("bos_id")));
        assert!(errors.iter().any(|e| e.contains("eos_id")));
    }

    // ── Block 183: chunk_text exact fit ───────────────────────────────────

    #[test]
    fn chunk_text_exact_fit_returns_single() {
        let tok = make_test_tokenizer();
        // "hi" = 2 tokens; budget of exactly 2 → single chunk
        let chunks = tok.chunk_text("hi", 2);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hi");
    }

    // ── Block 183: split_into_words single word ───────────────────────────

    #[test]
    fn split_into_words_single_word() {
        let tok = make_test_tokenizer();
        let words = tok.split_into_words("hello");
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].0, "hello");
        assert_eq!(words[0].1, 0);
        assert_eq!(words[0].2, 5);
    }

    // ── Block 183: token_frequency with BOS ───────────────────────────────

    #[test]
    fn token_frequency_with_bos_includes_bos_token() {
        let tok = make_test_tokenizer();
        let freq = tok.token_frequency("hi", true);
        // BOS token should appear with count 1
        let bos_entry = freq.iter().find(|(id, _)| *id == tok.bos_id);
        assert!(bos_entry.is_some(), "BOS token should appear in frequency");
        assert_eq!(bos_entry.unwrap().1, 1);
    }

    // ── Block 183: estimate_text_length proportionality ───────────────────

    #[test]
    fn estimate_text_length_zero_tokens_returns_zero() {
        let tok = make_test_tokenizer();
        assert_eq!(tok.estimate_text_length_for_tokens(0), 0);
    }

    #[test]
    fn estimate_text_length_positive_for_positive_tokens() {
        let tok = make_test_tokenizer();
        assert!(tok.estimate_text_length_for_tokens(1) > 0);
        assert!(tok.estimate_text_length_for_tokens(100) > 0);
    }

    // ── Block 183: JSON compact format ────────────────────────────────────

    #[test]
    fn encode_report_compact_json() {
        let report = EncodeReport {
            token_count: 1, unique_token_count: 1, input_bytes: 1,
            output_bytes: 1, bytes_per_token: 1.0, elapsed_us: 0,
            roundtrip_ok: true, has_bos: false,
        };
        let json = serde_json::to_string(&report).unwrap();
        // Compact JSON should not contain unnecessary whitespace
        assert!(!json.contains("\n"));
    }

    #[test]
    fn vocab_util_compact_json() {
        let util = VocabularyUtilization {
            total_tokens: 1, unique_tokens: 1, in_vocabulary: 1,
            coverage: 1.0, vocab_size: 1,
        };
        let json = serde_json::to_string(&util).unwrap();
        assert!(!json.contains("\n"));
    }

    // ── Block 183: decode_raw_token_sp additional ─────────────────────────

    #[test]
    fn decode_sp_byte_token_hex_lowercase() {
        // <0x61> → 'a'
        assert_eq!(decode_raw_token_sp("<0x61>"), "a");
    }

    #[test]
    fn decode_sp_multiple_space_markers() {
        // Multiple ▁ markers each become spaces
        assert_eq!(decode_raw_token_sp("\u{2581}\u{2581}hello"), "  hello");
    }
}
