use anyhow::Result;
use crate::nda_int::NdaVec;

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
}
