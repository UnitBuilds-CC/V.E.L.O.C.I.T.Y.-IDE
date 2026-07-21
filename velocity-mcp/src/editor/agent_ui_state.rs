//! Zero-allocation agentic UI state using ring buffers and message-driven architecture.
//!
//! This module manages agent thinking, approvals, and metrics using:
//! - Fixed-size ring buffers (no Vec allocation)
//! - Message-driven updates via AgentToUiMessage enum
//! - NDA serialization support for persistence
//! - Zero-copy rendering snapshots

use std::array;

/// Maximum items in ring buffers (fixed allocation)
pub const THINKING_BUFFER_SIZE: usize = 256;
pub const APPROVAL_BUFFER_SIZE: usize = 32;
pub const METRICS_HISTORY_SIZE: usize = 128;

/// Agent state enum (no allocation)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Idle = 0,
    Running = 1,
    Thinking = 2,
    Waiting = 3,
}

/// Thinking phase without allocation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkingPhase {
    Analysis = 0,
    Planning = 1,
    Execution = 2,
    Verification = 3,
}

/// Single thinking step in ring buffer (64 bytes, fixed size)
#[derive(Clone, Copy, Debug)]
pub struct ThinkingStepEntry {
    pub phase: ThinkingPhase,
    pub completed: bool,
    pub timestamp_ms: u32,
    pub text_offset: u16, // Offset into text pool
    pub text_len: u16,    // Length in text pool
}

impl Default for ThinkingStepEntry {
    fn default() -> Self {
        Self {
            phase: ThinkingPhase::Analysis,
            completed: false,
            timestamp_ms: 0,
            text_offset: 0,
            text_len: 0,
        }
    }
}

/// Zero-allocation thinking panel state with ring buffer
pub struct ThinkingPanelState {
    // Ring buffer for thinking steps
    steps: [ThinkingStepEntry; THINKING_BUFFER_SIZE],
    step_count: usize,
    step_head: usize, // For circular insertion

    // Fixed text pool for thinking content (no Vec)
    text_pool: [u8; 16384], // 16KB fixed pool
    text_used: usize,

    pub expanded: bool,
    pub auto_collapse: bool,
    pub current_phase: Option<ThinkingPhase>,
}

impl Default for ThinkingPanelState {
    fn default() -> Self {
        Self {
            steps: array::from_fn(|_| ThinkingStepEntry::default()),
            step_count: 0,
            step_head: 0,
            text_pool: [0u8; 16384],
            text_used: 0,
            expanded: true,
            auto_collapse: true,
            current_phase: None,
        }
    }
}

impl ThinkingPanelState {
    /// Add thinking token (zero-allocation copy into text pool)
    pub fn append_token(&mut self, token: &str) -> bool {
        let bytes = token.as_bytes();
        if self.text_used + bytes.len() >= self.text_pool.len() {
            return false; // Pool full, drop token (no allocation)
        }

        self.text_pool[self.text_used..self.text_used + bytes.len()].copy_from_slice(bytes);
        self.text_used += bytes.len();
        true
    }

    /// Start new thinking phase
    pub fn start_phase(&mut self, phase: ThinkingPhase) {
        if self.step_count >= THINKING_BUFFER_SIZE {
            self.step_head = (self.step_head + 1) % THINKING_BUFFER_SIZE;
        } else {
            self.step_count += 1;
        }

        let idx = self.step_head;
        self.steps[idx] = ThinkingStepEntry {
            phase,
            completed: false,
            timestamp_ms: (std::time::Instant::now().elapsed().as_millis() as u32),
            text_offset: self.text_used as u16,
            text_len: 0,
        };
        self.current_phase = Some(phase);
    }

    /// Complete current phase
    pub fn complete_phase(&mut self) {
        if let Some(idx) = self.get_current_step_idx() {
            self.steps[idx].completed = true;
            self.steps[idx].text_len =
                (self.text_used - (self.steps[idx].text_offset as usize)) as u16;
        }
    }

    /// Get current step index (handles ring buffer wrapping)
    fn get_current_step_idx(&self) -> Option<usize> {
        if self.step_count == 0 {
            return None;
        }
        Some(if self.step_count >= THINKING_BUFFER_SIZE {
            self.step_head
        } else {
            self.step_count - 1
        })
    }

    /// Get thinking text for a step (zero-copy slice)
    pub fn get_step_text(&self, idx: usize) -> &str {
        if idx >= THINKING_BUFFER_SIZE {
            return "";
        }
        let entry = self.steps[idx];
        let start = entry.text_offset as usize;
        let end = start + entry.text_len as usize;
        std::str::from_utf8(&self.text_pool[start..end]).unwrap_or("")
    }

    /// Get visible steps (zero-copy)
    pub fn visible_steps(&self) -> impl Iterator<Item = (usize, &ThinkingStepEntry)> {
        let count = self.step_count.min(THINKING_BUFFER_SIZE);
        (0..count).map(move |i| {
            let idx = if self.step_count >= THINKING_BUFFER_SIZE {
                (self.step_head + i) % THINKING_BUFFER_SIZE
            } else {
                i
            };
            (idx, &self.steps[idx])
        })
    }

    /// Get total number of thinking steps recorded
    pub fn step_count(&self) -> usize {
        self.step_count
    }
}

/// Tool approval entry (64 bytes, fixed size)
#[derive(Clone, Copy, Debug)]
pub struct ApprovalEntry {
    pub tool_id: u32,          // Unique approval ID
    pub tool_name_offset: u16, // Offset in text pool
    pub tool_name_len: u16,
    pub auto_approve: bool,
    pub timestamp_ms: u32,
}

/// Zero-allocation approval manager with ring buffer
pub struct ApprovalManagerState {
    pending: [ApprovalEntry; APPROVAL_BUFFER_SIZE],
    count: usize,
    head: usize,

    // Shared text pool
    text_pool: [u8; 8192],
    text_used: usize,

    pub auto_approve_all: bool,
}

impl Default for ApprovalManagerState {
    fn default() -> Self {
        Self {
            pending: array::from_fn(|_| ApprovalEntry {
                tool_id: 0,
                tool_name_offset: 0,
                tool_name_len: 0,
                auto_approve: false,
                timestamp_ms: 0,
            }),
            count: 0,
            head: 0,
            text_pool: [0u8; 8192],
            text_used: 0,
            auto_approve_all: false,
        }
    }
}

impl ApprovalManagerState {
    /// Add approval (returns false if full)
    pub fn add_approval(&mut self, tool_id: u32, tool_name: &str, auto_approve: bool) -> bool {
        if self.count >= APPROVAL_BUFFER_SIZE {
            return false;
        }

        let name_bytes = tool_name.as_bytes();
        if self.text_used + name_bytes.len() >= self.text_pool.len() {
            return false;
        }

        let idx = (self.head + self.count) % APPROVAL_BUFFER_SIZE;
        self.pending[idx] = ApprovalEntry {
            tool_id,
            tool_name_offset: self.text_used as u16,
            tool_name_len: name_bytes.len() as u16,
            auto_approve,
            timestamp_ms: (std::time::Instant::now().elapsed().as_millis() as u32),
        };

        self.text_pool[self.text_used..self.text_used + name_bytes.len()]
            .copy_from_slice(name_bytes);
        self.text_used += name_bytes.len();
        self.count += 1;
        true
    }

    /// Remove approval at index
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= self.count {
            return false;
        }
        // Shift ring buffer
        if self.count < APPROVAL_BUFFER_SIZE {
            let actual_idx = (self.head + idx) % APPROVAL_BUFFER_SIZE;
            for i in idx..self.count - 1 {
                let from = (self.head + i + 1) % APPROVAL_BUFFER_SIZE;
                let to = (self.head + i) % APPROVAL_BUFFER_SIZE;
                self.pending[to] = self.pending[from];
            }
        }
        self.count = self.count.saturating_sub(1);
        true
    }

    /// Get tool name for approval (zero-copy)
    pub fn get_tool_name(&self, idx: usize) -> &str {
        if idx >= self.count {
            return "";
        }
        let actual_idx = (self.head + idx) % APPROVAL_BUFFER_SIZE;
        let entry = self.pending[actual_idx];
        let start = entry.tool_name_offset as usize;
        let end = start + entry.tool_name_len as usize;
        std::str::from_utf8(&self.text_pool[start..end]).unwrap_or("")
    }

    /// Get pending approvals count
    pub fn pending_count(&self) -> usize {
        self.pending[..self.count]
            .iter()
            .filter(|e| !e.auto_approve)
            .count()
    }

    /// Get total approvals count
    pub fn total_count(&self) -> usize {
        self.count
    }
}

/// Agent metrics with fixed history
pub struct AgentMetricsState {
    pub state: AgentState,
    pub tokens_used: u32,
    pub tokens_max: u32,
    pub estimated_cost: u32, // In 0.0001 USD units
    pub estimated_cost_max: u32,
    pub tool_call_count: u32,
    pub last_tool_duration_ms: u32,
    pub thinking_enabled: bool,
}

impl Default for AgentMetricsState {
    fn default() -> Self {
        Self {
            state: AgentState::Idle,
            tokens_used: 0,
            tokens_max: 10000,
            estimated_cost: 0,
            estimated_cost_max: 5000, // $0.50
            tool_call_count: 0,
            last_tool_duration_ms: 0,
            thinking_enabled: false,
        }
    }
}

impl AgentMetricsState {
    pub fn budget_percentage(&self) -> u8 {
        if self.tokens_max == 0 {
            0
        } else {
            ((self.tokens_used as u64 * 100) / self.tokens_max as u64) as u8
        }
    }

    pub fn warning_level(&self) -> WarningLevel {
        let pct = self.budget_percentage();
        if pct >= 90 {
            WarningLevel::Critical
        } else if pct >= 75 {
            WarningLevel::Warning
        } else if pct >= 50 {
            WarningLevel::Caution
        } else {
            WarningLevel::Ok
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningLevel {
    Ok = 0,
    Caution = 1,
    Warning = 2,
    Critical = 3,
}

/// Complete agentic UI state container (all ring buffers, zero allocation)
pub struct AgentUiState {
    pub thinking: ThinkingPanelState,
    pub approvals: ApprovalManagerState,
    pub metrics: AgentMetricsState,
}

impl Default for AgentUiState {
    fn default() -> Self {
        Self {
            thinking: ThinkingPanelState::default(),
            approvals: ApprovalManagerState::default(),
            metrics: AgentMetricsState::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_buffer_no_alloc() {
        let mut state = ThinkingPanelState::default();
        state.start_phase(ThinkingPhase::Analysis);
        assert!(state.append_token("test token"));
        state.complete_phase();
        assert_eq!(state.step_count, 1);
    }

    #[test]
    fn test_approval_ring_buffer() {
        let mut state = ApprovalManagerState::default();
        assert!(state.add_approval(1, "test_tool", false));
        assert_eq!(state.pending_count(), 1);
        assert!(state.remove(0));
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn test_metrics_warning_levels() {
        let mut state = AgentMetricsState::default();
        assert_eq!(state.warning_level(), WarningLevel::Ok);

        state.tokens_used = 5000;
        assert_eq!(state.warning_level(), WarningLevel::Caution);

        state.tokens_used = 8000;
        assert_eq!(state.warning_level(), WarningLevel::Warning);

        state.tokens_used = 9500;
        assert_eq!(state.warning_level(), WarningLevel::Critical);
    }
}
