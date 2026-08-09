/// Zero-allocation NDA writer that writes triples directly into a fixed-size byte buffer.
pub struct ZeroAllocNdaWriter<'a> {
    pub buffer: &'a mut [u8],
    pub cursor: usize,
}

impl<'a> ZeroAllocNdaWriter<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, cursor: 0 }
    }

    /// Write a triple with zero heap allocations directly into fixed-size byte buffer.
    pub fn write_triple(
        &mut self,
        subject: &[u8],
        predicate_id: u16,
        object: &[u8],
    ) -> Result<usize, &'static str> {
        let req_len = 2 + subject.len() + 2 + 2 + object.len();
        if self.cursor + req_len > self.buffer.len() {
            return Err("BufferOverflow: Zero-alloc NDA buffer full");
        }

        // Subject len & bytes
        let subj_len = subject.len() as u16;
        self.buffer[self.cursor..self.cursor + 2].copy_from_slice(&subj_len.to_le_bytes());
        self.cursor += 2;
        self.buffer[self.cursor..self.cursor + subject.len()].copy_from_slice(subject);
        self.cursor += subject.len();

        // Predicate ID
        self.buffer[self.cursor..self.cursor + 2].copy_from_slice(&predicate_id.to_le_bytes());
        self.cursor += 2;

        // Object len & bytes
        let obj_len = object.len() as u16;
        self.buffer[self.cursor..self.cursor + 2].copy_from_slice(&obj_len.to_le_bytes());
        self.cursor += 2;
        self.buffer[self.cursor..self.cursor + object.len()].copy_from_slice(object);
        self.cursor += object.len();

        Ok(self.cursor)
    }

    /// Write a string triple (convenience wrapper).
    pub fn write_str_triple(
        &mut self,
        subject: &str,
        predicate_id: u16,
        object: &str,
    ) -> Result<usize, &'static str> {
        self.write_triple(subject.as_bytes(), predicate_id, object.as_bytes())
    }

    /// Get the number of bytes written so far.
    pub fn bytes_written(&self) -> usize {
        self.cursor
    }

    /// Get remaining capacity.
    pub fn remaining(&self) -> usize {
        self.buffer.len() - self.cursor
    }

    /// Reset the cursor to the beginning (does not zero the buffer).
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Get a slice of the written data.
    pub fn written_slice(&self) -> &[u8] {
        &self.buffer[..self.cursor]
    }
}

/// Zero-allocation NDA reader that reads triples from a byte buffer.
pub struct ZeroAllocNdaReader<'a> {
    pub buffer: &'a [u8],
    pub cursor: usize,
}

impl<'a> ZeroAllocNdaReader<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self { buffer, cursor: 0 }
    }

    /// Read the next triple, returning (subject, predicate_id, object) as byte slices.
    pub fn read_triple(&mut self) -> Result<(&'a [u8], u16, &'a [u8]), &'static str> {
        if self.cursor + 2 > self.buffer.len() {
            return Err("BufferUnderflow: not enough data for subject length");
        }
        let subj_len =
            u16::from_le_bytes([self.buffer[self.cursor], self.buffer[self.cursor + 1]]) as usize;
        self.cursor += 2;

        if self.cursor + subj_len > self.buffer.len() {
            return Err("BufferUnderflow: not enough data for subject");
        }
        let subject = &self.buffer[self.cursor..self.cursor + subj_len];
        self.cursor += subj_len;

        if self.cursor + 2 > self.buffer.len() {
            return Err("BufferUnderflow: not enough data for predicate");
        }
        let predicate_id =
            u16::from_le_bytes([self.buffer[self.cursor], self.buffer[self.cursor + 1]]);
        self.cursor += 2;

        if self.cursor + 2 > self.buffer.len() {
            return Err("BufferUnderflow: not enough data for object length");
        }
        let obj_len =
            u16::from_le_bytes([self.buffer[self.cursor], self.buffer[self.cursor + 1]]) as usize;
        self.cursor += 2;

        if self.cursor + obj_len > self.buffer.len() {
            return Err("BufferUnderflow: not enough data for object");
        }
        let object = &self.buffer[self.cursor..self.cursor + obj_len];
        self.cursor += obj_len;

        Ok((subject, predicate_id, object))
    }

    /// Check if there are more triples to read.
    pub fn has_more(&self) -> bool {
        self.cursor < self.buffer.len()
    }

    /// Reset the reader to the beginning.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_single_triple() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        let result = writer.write_triple(b"subject", 42, b"object");
        assert!(result.is_ok());
        assert!(writer.bytes_written() > 0);
    }

    #[test]
    fn test_write_multiple_triples() {
        let mut buf = [0u8; 1024];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"a", 1, b"b").unwrap();
        writer.write_triple(b"c", 2, b"d").unwrap();
        writer.write_triple(b"e", 3, b"f").unwrap();
        assert!(writer.bytes_written() > 0);
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buf = [0u8; 4]; // too small
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        let result = writer.write_triple(b"subject", 1, b"object");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_str_triple() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_str_triple("hello", 10, "world").unwrap();
        assert!(writer.bytes_written() > 0);
    }

    #[test]
    fn test_remaining() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        let before = writer.remaining();
        writer.write_triple(b"a", 1, b"b").unwrap();
        assert!(writer.remaining() < before);
    }

    #[test]
    fn test_reset() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"a", 1, b"b").unwrap();
        writer.reset();
        assert_eq!(writer.bytes_written(), 0);
    }

    #[test]
    fn test_written_slice() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"a", 1, b"b").unwrap();
        let slice = writer.written_slice();
        assert!(!slice.is_empty());
    }

    #[test]
    fn test_read_write_roundtrip() {
        let mut buf = [0u8; 1024];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"hello", 42, b"world").unwrap();
        writer.write_triple(b"foo", 99, b"bar").unwrap();
        let written = writer.bytes_written();

        let mut reader = ZeroAllocNdaReader::new(&buf[..written]);

        let (s1, p1, o1) = reader.read_triple().unwrap();
        assert_eq!(s1, b"hello");
        assert_eq!(p1, 42);
        assert_eq!(o1, b"world");

        let (s2, p2, o2) = reader.read_triple().unwrap();
        assert_eq!(s2, b"foo");
        assert_eq!(p2, 99);
        assert_eq!(o2, b"bar");

        assert!(!reader.has_more());
    }

    #[test]
    fn test_reader_empty() {
        let buf = [];
        let mut reader = ZeroAllocNdaReader::new(&buf);
        assert!(!reader.has_more());
        assert!(reader.read_triple().is_err());
    }

    #[test]
    fn test_reader_reset() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"a", 1, b"b").unwrap();
        let written = writer.bytes_written();

        let mut reader = ZeroAllocNdaReader::new(&buf[..written]);
        reader.read_triple().unwrap();
        assert!(!reader.has_more());
        reader.reset();
        assert!(reader.has_more());
    }

    #[test]
    fn exact_byte_count_single_triple() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        // subject="a"(1), predicate(2), object="b"(1) = 2+1+2+2+1 = 8 bytes
        writer.write_triple(b"a", 1, b"b").unwrap();
        assert_eq!(writer.bytes_written(), 8);
    }

    #[test]
    fn exact_size_buffer_fits_one() {
        let mut buf = [0u8; 8]; // exactly fits one "a",1,"b" triple
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        assert!(writer.write_triple(b"a", 1, b"b").is_ok());
        assert!(
            writer.write_triple(b"c", 2, b"d").is_err(),
            "Second triple should overflow"
        );
    }

    #[test]
    fn empty_subject_and_object() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"", 0, b"").unwrap();
        let written = writer.bytes_written();
        let mut reader = ZeroAllocNdaReader::new(&buf[..written]);
        let (s, p, o) = reader.read_triple().unwrap();
        assert_eq!(s, b"");
        assert_eq!(p, 0);
        assert_eq!(o, b"");
    }

    #[test]
    fn reader_has_more_after_partial_read() {
        let mut buf = [0u8; 1024];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"a", 1, b"b").unwrap();
        writer.write_triple(b"c", 2, b"d").unwrap();
        let written = writer.bytes_written();

        let mut reader = ZeroAllocNdaReader::new(&buf[..written]);
        assert!(reader.has_more());
        reader.read_triple().unwrap();
        assert!(
            reader.has_more(),
            "Should still have more after first triple"
        );
        reader.read_triple().unwrap();
        assert!(!reader.has_more(), "Should have no more after all triples");
    }

    #[test]
    fn remaining_decreases_with_writes() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        let r0 = writer.remaining();
        writer.write_triple(b"a", 1, b"b").unwrap();
        let r1 = writer.remaining();
        writer.write_triple(b"cc", 2, b"dd").unwrap();
        let r2 = writer.remaining();
        assert!(r0 > r1 && r1 > r2);
    }

    #[test]
    fn reset_allows_rewrite() {
        let mut buf = [0u8; 8]; // tiny buffer
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"a", 1, b"b").unwrap();
        assert!(writer.write_triple(b"x", 2, b"y").is_err());
        writer.reset();
        assert!(
            writer.write_triple(b"x", 2, b"y").is_ok(),
            "After reset, buffer is available again"
        );
    }

    #[test]
    fn large_predicate_id() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"s", u16::MAX, b"o").unwrap();
        let written = writer.bytes_written();
        let mut reader = ZeroAllocNdaReader::new(&buf[..written]);
        let (_, p, _) = reader.read_triple().unwrap();
        assert_eq!(p, u16::MAX);
    }

    #[test]
    fn reader_truncated_subject() {
        // Buffer says subject is 100 bytes but only has 2
        let buf = [100u8, 0, b'a', b'b'];
        let mut reader = ZeroAllocNdaReader::new(&buf);
        assert!(reader.read_triple().is_err());
    }

    #[test]
    fn reader_truncated_object() {
        let mut buf = [0u8; 256];
        let mut writer = ZeroAllocNdaWriter::new(&mut buf);
        writer.write_triple(b"s", 1, b"object").unwrap();
        // Truncate the buffer mid-object
        let truncated = writer.bytes_written() - 2;
        let mut reader = ZeroAllocNdaReader::new(&buf[..truncated]);
        assert!(reader.read_triple().is_err());
    }
}
