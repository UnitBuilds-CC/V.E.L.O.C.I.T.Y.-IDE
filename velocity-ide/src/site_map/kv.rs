use crate::nda_int::NdaVec;
use anyhow::Result;
use serde::Serialize;

/// Binary layout of a `.kv` file (little-endian):
///
///   [0..2]   len:        u16   — vector length (number of elements)
///   [2]      log2_scale: i8    — shared scale for K and V
///   [3]      reserved:   u8
///   [4..]    k_sign:     ceil(len/8) bytes
///   [..]     k_extra:    ceil(len/8) bytes
///   [..]     v_sign:     ceil(len/8) bytes
///   [..]     v_extra:    ceil(len/8) bytes
///
/// Total: 4 + 4 × ceil(len/8) bytes.  For hidden=896: 4 + 448 = 452 bytes.
pub struct KvRecord {
    pub k: NdaVec,
    pub v: NdaVec,
}

impl KvRecord {
    pub fn serialise(&self) -> Vec<u8> {
        let len = self.k.len as u16;
        let mut buf = Vec::with_capacity(4 + 4 * self.k.len.div_ceil(8));
        buf.extend_from_slice(&len.to_le_bytes());
        buf.push(self.k.log2_scale as u8);
        buf.push(0u8); // reserved
        buf.extend_from_slice(&self.k.sign);
        buf.extend_from_slice(&self.k.extra);
        buf.extend_from_slice(&self.v.sign);
        buf.extend_from_slice(&self.v.extra);
        buf
    }

    pub fn deserialise(data: &[u8]) -> Result<Self> {
        anyhow::ensure!(data.len() >= 4, "KV record too short");
        let len = u16::from_le_bytes([data[0], data[1]]) as usize;
        let log2_scale = data[2] as i8;
        let bitmap_bytes = len.div_ceil(8);
        anyhow::ensure!(
            data.len() >= 4 + 4 * bitmap_bytes,
            "KV record truncated (len={len}, expected {} bytes)",
            4 + 4 * bitmap_bytes
        );
        let base = 4;
        let k = NdaVec {
            len,
            log2_scale,
            sign: data[base..base + bitmap_bytes].to_vec().into(),
            extra: data[base + bitmap_bytes..base + 2 * bitmap_bytes]
                .to_vec()
                .into(),
        };
        let v = NdaVec {
            len,
            log2_scale,
            sign: data[base + 2 * bitmap_bytes..base + 3 * bitmap_bytes]
                .to_vec()
                .into(),
            extra: data[base + 3 * bitmap_bytes..base + 4 * bitmap_bytes]
                .to_vec()
                .into(),
        };
        Ok(KvRecord { k, v })
    }

    /// Return a diagnostic snapshot of this KV record.
    pub fn info(&self) -> KvRecordInfo {
        let bitmap_bytes = self.k.len.div_ceil(8);
        let total_bytes = 4 + 4 * bitmap_bytes;
        KvRecordInfo {
            vector_len: self.k.len,
            log2_scale: self.k.log2_scale,
            bitmap_bytes,
            total_serialized_bytes: total_bytes,
            k_sign_bytes: bitmap_bytes,
            v_sign_bytes: bitmap_bytes,
        }
    }

    /// Validate that the KV record is internally consistent.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.k.len == 0 {
            issues.push("K vector has zero length".to_string());
        }
        if self.v.len == 0 {
            issues.push("V vector has zero length".to_string());
        }
        if self.k.len != self.v.len {
            issues.push(format!(
                "K/V length mismatch: K={}, V={}",
                self.k.len, self.v.len
            ));
        }
        if self.k.log2_scale != self.v.log2_scale {
            issues.push(format!(
                "K/V scale mismatch: K={}, V={}",
                self.k.log2_scale, self.v.log2_scale
            ));
        }
        let expected_sign_bytes = self.k.len.div_ceil(8);
        if self.k.sign.len() != expected_sign_bytes {
            issues.push(format!(
                "K sign bytes {} != expected {} for len={}",
                self.k.sign.len(),
                expected_sign_bytes,
                self.k.len
            ));
        }
        issues
    }
}

/// Diagnostic info for a KV record.
#[derive(Debug, Clone, Serialize)]
pub struct KvRecordInfo {
    pub vector_len: usize,
    pub log2_scale: i8,
    pub bitmap_bytes: usize,
    pub total_serialized_bytes: usize,
    pub k_sign_bytes: usize,
    pub v_sign_bytes: usize,
}

/// Validate raw bytes before deserialization.
pub fn validate_kv_bytes(data: &[u8]) -> Vec<String> {
    let mut issues = Vec::new();
    if data.len() < 4 {
        issues.push(format!(
            "KV record too short: {} bytes (minimum 4)",
            data.len()
        ));
        return issues;
    }
    let len = u16::from_le_bytes([data[0], data[1]]) as usize;
    if len == 0 {
        issues.push("KV record has zero-length vectors".to_string());
    }
    let bitmap_bytes = len.div_ceil(8);
    let expected = 4 + 4 * bitmap_bytes;
    if data.len() < expected {
        issues.push(format!(
            "KV record truncated: {} bytes, expected {} for len={}",
            data.len(),
            expected,
            len
        ));
    }
    issues
}

/// Batch serialize multiple KV records.
pub fn batch_serialize(records: &[KvRecord]) -> Vec<Vec<u8>> {
    records.iter().map(|r| r.serialise()).collect()
}

/// Batch validate multiple KV records.
pub fn batch_validate(records: &[KvRecord]) -> Vec<Vec<String>> {
    records.iter().map(|r| r.validate()).collect()
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_kv(len: usize) -> KvRecord {
        // Use same scale for K and V by constructing NdaVec directly
        let bitmap_bytes = len.div_ceil(8);
        let k = NdaVec {
            len,
            log2_scale: 0,
            sign: vec![0xAA; bitmap_bytes].into(),
            extra: vec![0x55; bitmap_bytes].into(),
        };
        let v = NdaVec {
            len,
            log2_scale: 0,
            sign: vec![0x33; bitmap_bytes].into(),
            extra: vec![0xCC; bitmap_bytes].into(),
        };
        KvRecord { k, v }
    }

    #[test]
    fn kv_roundtrip() {
        let kv = make_test_kv(16);
        let bytes = kv.serialise();
        let kv2 = KvRecord::deserialise(&bytes).unwrap();
        assert_eq!(kv.k.len, kv2.k.len);
        assert_eq!(kv.k.log2_scale, kv2.k.log2_scale);
        assert_eq!(kv.k.sign, kv2.k.sign);
        assert_eq!(kv.k.extra, kv2.k.extra);
        assert_eq!(kv.v.sign, kv2.v.sign);
    }

    #[test]
    fn kv_info() {
        let kv = make_test_kv(64);
        let info = kv.info();
        assert_eq!(info.vector_len, 64);
        assert_eq!(info.bitmap_bytes, 8);
        assert_eq!(info.total_serialized_bytes, 36); // 4 + 4*8
    }

    #[test]
    fn kv_validate_clean() {
        let kv = make_test_kv(16);
        let issues = kv.validate();
        assert!(issues.is_empty());
    }

    #[test]
    fn kv_validate_mismatched() {
        let k = NdaVec {
            len: 16,
            log2_scale: 0,
            sign: vec![0; 2].into(),
            extra: vec![0; 2].into(),
        };
        let v = NdaVec {
            len: 8,
            log2_scale: 0,
            sign: vec![0; 1].into(),
            extra: vec![0; 1].into(),
        };
        let kv = KvRecord { k, v };
        let issues = kv.validate();
        assert!(issues.iter().any(|i| i.contains("length mismatch")));
    }

    #[test]
    fn validate_kv_bytes_too_short() {
        let issues = validate_kv_bytes(&[0, 0]);
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn validate_kv_bytes_truncated() {
        // len=64 needs 4+4*8=36 bytes, but we only provide 10
        let mut data = vec![0u8; 10];
        data[0] = 64; // len=64
        data[1] = 0;
        let issues = validate_kv_bytes(&data);
        assert!(issues.iter().any(|i| i.contains("truncated")));
    }

    #[test]
    fn validate_kv_bytes_clean() {
        let kv = make_test_kv(16);
        let bytes = kv.serialise();
        let issues = validate_kv_bytes(&bytes);
        assert!(issues.is_empty());
    }

    #[test]
    fn kv_deserialise_too_short() {
        let result = KvRecord::deserialise(&[0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn batch_serialize_test() {
        let records = vec![make_test_kv(8), make_test_kv(16)];
        let batch = batch_serialize(&records);
        assert_eq!(batch.len(), 2);
        assert!(!batch[0].is_empty());
        assert!(!batch[1].is_empty());
    }

    #[test]
    fn batch_validate_test() {
        let records = vec![make_test_kv(8), make_test_kv(16)];
        let results = batch_validate(&records);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_empty());
        assert!(results[1].is_empty());
    }

    #[test]
    fn kv_record_info_serializable() {
        let kv = make_test_kv(32);
        let info = kv.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("vector_len"));
        assert!(json.contains("total_serialized_bytes"));
    }
}
