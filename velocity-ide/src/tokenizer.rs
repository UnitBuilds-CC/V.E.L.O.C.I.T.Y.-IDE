// src/tokenizer.rs — V.E.L.O.C.I.T.Y.-IDE
//
// BPE tokenizer supporting both fast zero-copy binary `.nda` format (NDAT)
// and legacy HuggingFace `tokenizer.json` directly.
//
// No C library dependencies — pure Rust.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

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
        let version = u16::from_le_bytes(file_bytes[4..6].try_into().expect("slice is exactly 2 bytes"));
        if version != 1 {
            anyhow::bail!("Unsupported NDAT version: {version}");
        }
        let vocab_size = u32::from_le_bytes(file_bytes[6..10].try_into().expect("slice is exactly 4 bytes")) as usize;
        let merges_count = u32::from_le_bytes(file_bytes[10..14].try_into().expect("slice is exactly 4 bytes")) as usize;
        let bos_id = u32::from_le_bytes(file_bytes[14..18].try_into().expect("slice is exactly 4 bytes"));
        let eos_id = u32::from_le_bytes(file_bytes[18..22].try_into().expect("slice is exactly 4 bytes"));
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
            char::from_u32(256 + n).expect("256+n is always a valid Unicode scalar value for n in 0..=33")
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
}
