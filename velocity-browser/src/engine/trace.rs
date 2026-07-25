use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct ConsoleTraceRecord {
    pub level: String, // "info", "warn", "error", "debug", "trace"
    pub message: String,
    pub timestamp_ms: u64,
    pub source_url: Option<String>,
    pub source_line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct DomMutationTraceRecord {
    pub target_id: String,
    pub mutation_type: String, // "attribute_changed", "child_added", "child_removed", "text_updated"
    pub detail: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PerformanceTraceRecord {
    pub name: String,
    pub entry_type: String, // "navigation", "resource", "mark", "measure"
    pub start_ms: u64,
    pub duration_ms: u64,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NetworkTraceRecord {
    pub request_id: String,
    pub method: String,
    pub url: String,
    pub status_code: u16,
    pub response_size_bytes: usize,
    pub duration_ms: u64,
    pub timestamp_ms: u64,
}

pub struct TraceCollector {
    pub console_traces: Vec<ConsoleTraceRecord>,
    pub mutation_traces: Vec<DomMutationTraceRecord>,
    pub performance_traces: Vec<PerformanceTraceRecord>,
    pub network_traces: Vec<NetworkTraceRecord>,
    max_console: usize,
    max_mutations: usize,
    max_perf: usize,
    max_network: usize,
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
            performance_traces: Vec::new(),
            network_traces: Vec::new(),
            max_console: 1000,
            max_mutations: 500,
            max_perf: 500,
            max_network: 1000,
        }
    }

    /// Set maximum buffer sizes for each trace type.
    pub fn set_limits(&mut self, console: usize, mutations: usize, perf: usize, network: usize) {
        self.max_console = console;
        self.max_mutations = mutations;
        self.max_perf = perf;
        self.max_network = network;
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn record_console(&mut self, level: &str, message: &str) {
        self.console_traces.push(ConsoleTraceRecord {
            level: level.to_string(),
            message: message.to_string(),
            timestamp_ms: Self::now_ms(),
            source_url: None,
            source_line: None,
        });
        if self.console_traces.len() > self.max_console {
            self.console_traces.remove(0);
        }
    }

    /// Record a console message with source location.
    pub fn record_console_with_source(&mut self, level: &str, message: &str, source: &str, line: u32) {
        self.console_traces.push(ConsoleTraceRecord {
            level: level.to_string(),
            message: message.to_string(),
            timestamp_ms: Self::now_ms(),
            source_url: Some(source.to_string()),
            source_line: Some(line),
        });
        if self.console_traces.len() > self.max_console {
            self.console_traces.remove(0);
        }
    }

    pub fn record_mutation(&mut self, target_id: &str, mutation_type: &str, detail: &str) {
        self.mutation_traces.push(DomMutationTraceRecord {
            target_id: target_id.to_string(),
            mutation_type: mutation_type.to_string(),
            detail: detail.to_string(),
            timestamp_ms: Self::now_ms(),
        });
        if self.mutation_traces.len() > self.max_mutations {
            self.mutation_traces.remove(0);
        }
    }

    /// Record a performance mark or measurement.
    pub fn record_performance(&mut self, name: &str, entry_type: &str, start_ms: u64, duration_ms: u64) {
        self.performance_traces.push(PerformanceTraceRecord {
            name: name.to_string(),
            entry_type: entry_type.to_string(),
            start_ms,
            duration_ms,
            metadata: None,
        });
        if self.performance_traces.len() > self.max_perf {
            self.performance_traces.remove(0);
        }
    }

    /// Record a performance mark with metadata.
    pub fn record_performance_with_meta(&mut self, name: &str, entry_type: &str, start_ms: u64, duration_ms: u64, meta: &str) {
        self.performance_traces.push(PerformanceTraceRecord {
            name: name.to_string(),
            entry_type: entry_type.to_string(),
            start_ms,
            duration_ms,
            metadata: Some(meta.to_string()),
        });
        if self.performance_traces.len() > self.max_perf {
            self.performance_traces.remove(0);
        }
    }

    /// Record a network request trace.
    pub fn record_network(&mut self, request_id: &str, method: &str, url: &str, status: u16, size: usize, duration_ms: u64) {
        self.network_traces.push(NetworkTraceRecord {
            request_id: request_id.to_string(),
            method: method.to_string(),
            url: url.to_string(),
            status_code: status,
            response_size_bytes: size,
            duration_ms,
            timestamp_ms: Self::now_ms(),
        });
        if self.network_traces.len() > self.max_network {
            self.network_traces.remove(0);
        }
    }

    /// Filter console traces by level.
    pub fn console_by_level(&self, level: &str) -> Vec<&ConsoleTraceRecord> {
        self.console_traces.iter().filter(|t| t.level == level).collect()
    }

    /// Get all errors.
    pub fn errors(&self) -> Vec<&ConsoleTraceRecord> {
        self.console_by_level("error")
    }

    /// Get all warnings.
    pub fn warnings(&self) -> Vec<&ConsoleTraceRecord> {
        self.console_by_level("warn")
    }

    /// Get mutations for a specific target.
    pub fn mutations_for(&self, target_id: &str) -> Vec<&DomMutationTraceRecord> {
        self.mutation_traces.iter().filter(|t| t.target_id == target_id).collect()
    }

    /// Get performance entries by type.
    pub fn perf_by_type(&self, entry_type: &str) -> Vec<&PerformanceTraceRecord> {
        self.performance_traces.iter().filter(|t| t.entry_type == entry_type).collect()
    }

    /// Get failed network requests (4xx/5xx).
    pub fn failed_requests(&self) -> Vec<&NetworkTraceRecord> {
        self.network_traces.iter().filter(|t| t.status_code >= 400).collect()
    }

    /// Total bytes transferred across all network traces.
    pub fn total_bytes_transferred(&self) -> usize {
        self.network_traces.iter().map(|t| t.response_size_bytes).sum()
    }

    /// Clear all traces.
    pub fn clear_all(&mut self) {
        self.console_traces.clear();
        self.mutation_traces.clear();
        self.performance_traces.clear();
        self.network_traces.clear();
    }

    /// Clear only console traces.
    pub fn clear_console(&mut self) {
        self.console_traces.clear();
    }

    /// Total trace count across all categories.
    pub fn total_count(&self) -> usize {
        self.console_traces.len() + self.mutation_traces.len()
            + self.performance_traces.len() + self.network_traces.len()
    }

    pub fn export_traces_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for c in &self.console_traces {
            triples.push(NdaTriple::new(&c.level, 120, &c.message));
        }
        for m in &self.mutation_traces {
            triples.push(NdaTriple::new(&m.target_id, 121, &format!("{}:{}", m.mutation_type, m.detail)));
        }
        for p in &self.performance_traces {
            triples.push(NdaTriple::new(&p.name, 122, &format!("{}:{}ms:{}", p.entry_type, p.duration_ms, p.start_ms)));
        }
        for n in &self.network_traces {
            triples.push(NdaTriple::new(&n.request_id, 123, &format!("{} {} {} {}B {}ms", n.method, n.status_code, n.url, n.response_size_bytes, n.duration_ms)));
        }
        triples
    }
}
