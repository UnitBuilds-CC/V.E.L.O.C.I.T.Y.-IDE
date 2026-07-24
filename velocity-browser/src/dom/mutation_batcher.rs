use crate::dom::MutationRecord;

pub struct MutationBatcher {
    pub pending_mutations: Vec<MutationRecord>,
}

impl Default for MutationBatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl MutationBatcher {
    pub fn new() -> Self {
        Self { pending_mutations: Vec::new() }
    }

    pub fn push_mutation(&mut self, record: MutationRecord) {
        self.pending_mutations.push(record);
    }

    pub fn flush_batch(&mut self) -> Vec<MutationRecord> {
        std::mem::take(&mut self.pending_mutations)
    }
}
