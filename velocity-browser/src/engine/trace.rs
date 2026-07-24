use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct ConsoleTraceRecord {
    pub level: String, // "info", "warn", "error"
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct DomMutationTraceRecord {
    pub target_id: String,
    pub mutation_type: String, // "attribute_changed", "child_added", "text_updated"
    pub detail: String,
}

pub struct TraceCollector {
    pub console_traces: Vec<ConsoleTraceRecord>,
    pub mutation_traces: Vec<DomMutationTraceRecord>,
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceCollector {
    pub fn new() -> Self {
        Self {
            console_traces: Vec::new(),
            mutation_traces: Vec::new(),
        }
    }

    pub fn record_console(&mut self, level: &str, message: &str) {
        self.console_traces.push(ConsoleTraceRecord {
            level: level.to_string(),
            message: message.to_string(),
            timestamp_ms: 1000,
        });
    }

    pub fn record_mutation(&mut self, target_id: &str, mutation_type: &str, detail: &str) {
        self.mutation_traces.push(DomMutationTraceRecord {
            target_id: target_id.to_string(),
            mutation_type: mutation_type.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn export_traces_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for c in &self.console_traces {
            triples.push(NdaTriple::new(&c.level, 120, &c.message));
        }
        for m in &self.mutation_traces {
            triples.push(NdaTriple::new(&m.target_id, 121, &format!("{}:{}", m.mutation_type, m.detail)));
        }
        triples
    }
}
