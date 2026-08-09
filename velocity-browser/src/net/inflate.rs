//! From-scratch DEFLATE / gzip / zlib decompression (RFC 1951 / 1952 / 1950).
//!
//! No third-party crates: real web responses are frequently `gzip`- or
//! `deflate`-encoded, so an agent browser must inflate them itself. This is a
//! complete DEFLATE inflater (stored, fixed-Huffman, and dynamic-Huffman
//! blocks) with the gzip and zlib framing wrappers layered on top.

/// LSB-first bit reader over a DEFLATE byte stream.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u32, String> {
        if self.byte_pos >= self.data.len() {
            return Err("unexpected end of DEFLATE stream".to_string());
        }
        let bit = (self.data[self.byte_pos] >> self.bit_pos) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit as u32)
    }

    /// Read `n` bits, least-significant bit first (DEFLATE convention).
    fn read_bits(&mut self, n: u32) -> Result<u32, String> {
        let mut value = 0u32;
        for i in 0..n {
            value |= self.read_bit()? << i;
        }
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        if self.byte_pos >= self.data.len() {
            return Err("unexpected end of stored block".to_string());
        }
        let b = self.data[self.byte_pos];
        self.byte_pos += 1;
        Ok(b)
    }
}

/// Canonical Huffman decoder represented by symbol counts per code length.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;

        let mut offsets = [0u16; 16];
        for i in 1..16 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offsets[l as usize] as usize] = sym as u16;
                offsets[l as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, r: &mut BitReader) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= r.read_bit()? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("invalid Huffman code".to_string())
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lit = [0u8; 288];
    for (i, l) in lit.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let dist = [5u8; 30];
    (Huffman::from_lengths(&lit), Huffman::from_lengths(&dist))
}

fn dynamic_tables(r: &mut BitReader) -> Result<(Huffman, Huffman), String> {
    let hlit = r.read_bits(5)? as usize + 257;
    let hdist = r.read_bits(5)? as usize + 1;
    let hclen = r.read_bits(4)? as usize + 4;

    let mut cl_lengths = [0u8; 19];
    for i in 0..hclen {
        cl_lengths[CODE_LENGTH_ORDER[i]] = r.read_bits(3)? as u8;
    }
    let cl_huff = Huffman::from_lengths(&cl_lengths);

    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = cl_huff.decode(r)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("repeat with no previous length".to_string());
                }
                let repeat = r.read_bits(2)? as usize + 3;
                let prev = lengths[i - 1];
                for _ in 0..repeat {
                    if i >= total {
                        return Err("code length repeat overflow".to_string());
                    }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 => {
                let repeat = r.read_bits(3)? as usize + 3;
                for _ in 0..repeat {
                    if i >= total {
                        return Err("zero-run overflow".to_string());
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            18 => {
                let repeat = r.read_bits(7)? as usize + 11;
                for _ in 0..repeat {
                    if i >= total {
                        return Err("long zero-run overflow".to_string());
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => return Err("invalid code-length symbol".to_string()),
        }
    }

    let lit = Huffman::from_lengths(&lengths[..hlit]);
    let dist = Huffman::from_lengths(&lengths[hlit..]);
    Ok((lit, dist))
}

fn inflate_compressed_block(
    r: &mut BitReader,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(r)?;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let s = (sym - 257) as usize;
            if s >= LEN_BASE.len() {
                return Err("invalid length symbol".to_string());
            }
            let length = LEN_BASE[s] as usize + r.read_bits(LEN_EXTRA[s] as u32)? as usize;

            let dsym = dist.decode(r)? as usize;
            if dsym >= DIST_BASE.len() {
                return Err("invalid distance symbol".to_string());
            }
            let distance =
                DIST_BASE[dsym] as usize + r.read_bits(DIST_EXTRA[dsym] as u32)? as usize;
            if distance == 0 || distance > out.len() {
                return Err("invalid back-reference distance".to_string());
            }
            let start = out.len() - distance;
            for k in 0..length {
                let b = out[start + k];
                out.push(b);
            }
        }
    }
}

fn inflate_stored_block(r: &mut BitReader, out: &mut Vec<u8>) -> Result<(), String> {
    r.align_to_byte();
    let len = r.read_u8()? as usize | ((r.read_u8()? as usize) << 8);
    let nlen = r.read_u8()? as usize | ((r.read_u8()? as usize) << 8);
    if len != (!nlen & 0xFFFF) {
        return Err("stored block length check failed".to_string());
    }
    for _ in 0..len {
        let b = r.read_u8()?;
        out.push(b);
    }
    Ok(())
}

/// Inflate a raw DEFLATE stream (RFC 1951).
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut r = BitReader::new(data);
    let mut out = Vec::new();
    loop {
        let bfinal = r.read_bit()?;
        let btype = r.read_bits(2)?;
        match btype {
            0 => inflate_stored_block(&mut r, &mut out)?,
            1 => {
                let (lit, dist) = fixed_tables();
                inflate_compressed_block(&mut r, &lit, &dist, &mut out)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut r)?;
                inflate_compressed_block(&mut r, &lit, &dist, &mut out)?;
            }
            _ => return Err("invalid DEFLATE block type".to_string()),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

/// Decompress a gzip member (RFC 1952).
pub fn gunzip(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b {
        return Err("not a gzip stream".to_string());
    }
    if data[2] != 8 {
        return Err("unsupported gzip compression method".to_string());
    }
    let flg = data[3];
    let mut pos = 10;

    if flg & 0x04 != 0 {
        // FEXTRA
        if pos + 2 > data.len() {
            return Err("truncated gzip FEXTRA".to_string());
        }
        let xlen = data[pos] as usize | ((data[pos + 1] as usize) << 8);
        pos += 2 + xlen;
    }
    if flg & 0x08 != 0 {
        // FNAME (NUL-terminated)
        pos = skip_zstring(data, pos)?;
    }
    if flg & 0x10 != 0 {
        // FCOMMENT (NUL-terminated)
        pos = skip_zstring(data, pos)?;
    }
    if flg & 0x02 != 0 {
        // FHCRC
        pos += 2;
    }
    if pos >= data.len() {
        return Err("truncated gzip header".to_string());
    }
    // The trailing 8 bytes are CRC32 + ISIZE; inflate ignores them.
    inflate(&data[pos..])
}

fn skip_zstring(data: &[u8], mut pos: usize) -> Result<usize, String> {
    while pos < data.len() && data[pos] != 0 {
        pos += 1;
    }
    if pos >= data.len() {
        return Err("unterminated gzip string field".to_string());
    }
    Ok(pos + 1)
}

/// Decompress a zlib stream (RFC 1950).
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("zlib stream too short".to_string());
    }
    if data[0] & 0x0f != 8 {
        return Err("unsupported zlib compression method".to_string());
    }
    let flg = data[1];
    let mut pos = 2;
    if flg & 0x20 != 0 {
        // FDICT: 4-byte dictionary id (unsupported preset dictionaries)
        pos += 4;
    }
    inflate(&data[pos..])
}

/// Decompress an HTTP body according to its `Content-Encoding`.
pub fn decode_content_encoding(encoding: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    match encoding.trim().to_ascii_lowercase().as_str() {
        "" | "identity" => Ok(body.to_vec()),
        "gzip" | "x-gzip" => gunzip(body),
        // `deflate` in the wild is usually zlib-wrapped; fall back to raw.
        "deflate" => zlib_decompress(body).or_else(|_| inflate(body)),
        other => Err(format!("unsupported content-encoding: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflates_stored_block() {
        // BFINAL=1, BTYPE=00, LEN=2, NLEN=~2, then "Hi".
        let data = [0x01, 0x02, 0x00, 0xFD, 0xFF, b'H', b'i'];
        assert_eq!(inflate(&data).unwrap(), b"Hi");
    }

    // gzip("Velocity agent browser, Velocity agent browser!") produced by
    // .NET GZipStream - exercises real Huffman + back-references.
    const GZIP_VECTOR: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x0b, 0x4b, 0xcd, 0xc9, 0x4f,
        0xce, 0x2c, 0xa9, 0x54, 0x48, 0x4c, 0x4f, 0xcd, 0x2b, 0x51, 0x48, 0x2a, 0xca, 0x2f, 0x2f,
        0x4e, 0x2d, 0xd2, 0x51, 0x08, 0xc3, 0x2a, 0xae, 0x08, 0x00, 0x2c, 0x6d, 0xc8, 0x75, 0x2f,
        0x00, 0x00, 0x00,
    ];

    #[test]
    fn gunzips_real_vector() {
        let out = gunzip(GZIP_VECTOR).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Velocity agent browser, Velocity agent browser!"
        );
    }

    #[test]
    fn content_encoding_identity_is_passthrough() {
        assert_eq!(decode_content_encoding("identity", b"raw").unwrap(), b"raw");
        assert_eq!(decode_content_encoding("", b"raw").unwrap(), b"raw");
    }

    #[test]
    fn content_encoding_gzip_dispatches() {
        let out = decode_content_encoding("gzip", GZIP_VECTOR).unwrap();
        assert!(String::from_utf8(out)
            .unwrap()
            .starts_with("Velocity agent"));
    }

    #[test]
    fn rejects_non_gzip() {
        assert!(gunzip(b"not a gzip stream at all").is_err());
    }

    #[test]
    fn gunzip_too_short() {
        assert!(gunzip(b"short").is_err());
    }

    #[test]
    fn gunzip_wrong_magic() {
        let mut data = vec![0u8; 20];
        data[0] = 0xFF; // wrong magic byte
        data[1] = 0xFF;
        data[2] = 8;
        assert!(gunzip(&data).is_err());
    }

    #[test]
    fn zlib_decompress_too_short() {
        assert!(zlib_decompress(b"").is_err());
        assert!(zlib_decompress(b"\x08").is_err());
    }

    #[test]
    fn zlib_wrong_method_rejected() {
        let data = [0x07, 0x00]; // method != 8
        assert!(zlib_decompress(&data).is_err());
    }

    #[test]
    fn content_encoding_unsupported_rejected() {
        assert!(decode_content_encoding("br", b"data").is_err());
    }

    #[test]
    fn inflate_empty_stored_block() {
        // BFINAL=1, BTYPE=00, LEN=0, NLEN=0xFFFF
        let data = [0x01, 0x00, 0x00, 0xFF, 0xFF];
        assert_eq!(inflate(&data).unwrap(), b"");
    }
}
