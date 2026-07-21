use crate::nda::NdaTriple;

pub struct ZeroAllocNdaWriter<'a> {
    pub buffer: &'a mut [u8],
    pub cursor: usize,
}

impl<'a> ZeroAllocNdaWriter<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, cursor: 0 }
    }

    /// Write a triple with zero heap allocations directly into fixed-size byte buffer
    pub fn write_triple(&mut self, subject: &[u8], predicate_id: u16, object: &[u8]) -> Result<usize, &'static str> {
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
}
