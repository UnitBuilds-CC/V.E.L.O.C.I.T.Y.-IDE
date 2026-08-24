use serde::Serialize;

use super::types::VarRegistry;
use super::x86_emitter::X86Emitter;
use crate::site_map::NdaNode;

pub fn detect_and_compile_symbolic_loop(
    count: u32,
    body: &[NdaNode],
    emitter: &mut X86Emitter,
    registry: &VarRegistry,
) -> Result<bool, String> {
    if body.len() != 2 {
        return Ok(false);
    }

    let mut increment_var = None;
    let mut accumulator_var = None;

    for node in body {
        if let NdaNode::Store { name_hash, value } = node {
            if let NdaNode::Add { lhs, rhs } = &**value {
                let mut is_inc = false;
                let mut step = 0i32;
                if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                    if l_hash == name_hash {
                        if let NdaNode::Int { value: val } = &**rhs {
                            is_inc = true;
                            step = *val;
                        }
                    }
                }
                if !is_inc {
                    if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                        if r_hash == name_hash {
                            if let NdaNode::Int { value: val } = &**lhs {
                                is_inc = true;
                                step = *val;
                            }
                        }
                    }
                }
                if is_inc {
                    increment_var = Some((*name_hash, step));
                    continue;
                }

                let mut is_acc = false;
                let mut other_var = None;
                if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                    if l_hash == name_hash {
                        if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                            is_acc = true;
                            other_var = Some(*r_hash);
                        }
                    }
                }
                if !is_acc {
                    if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                        if r_hash == name_hash {
                            if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                                is_acc = true;
                                other_var = Some(*l_hash);
                            }
                        }
                    }
                }
                if is_acc {
                    accumulator_var = Some((*name_hash, other_var.unwrap()));
                    continue;
                }
            }
        }
    }

    if let (Some((i_hash, step)), Some((sum_hash, added_hash))) = (increment_var, accumulator_var) {
        if added_hash == i_hash && sum_hash != i_hash {
            let i_slot = registry.get_or_create_slot(i_hash);
            let sum_slot = registry.get_or_create_slot(sum_hash);
            if i_slot >= 4 || sum_slot >= 4 {
                return Ok(false);
            }
            let i_reg = 12 + i_slot;
            let sum_reg = 12 + sum_slot;

            let n = count as i64;
            let n_c = (n * step as i64) as i32;
            let sum_step = (step as i64 * n * (n - 1) / 2) as i32;

            let modrm_mov = 0xC0 | ((i_reg as u8 & 7) << 3);
            emitter.emit_slice(&[0x44, 0x89, modrm_mov]);

            emitter.emit(0x69);
            emitter.emit(0xC0);
            emitter.emit_slice(&(count as i32).to_le_bytes());

            emitter.emit(0x05);
            emitter.emit_slice(&sum_step.to_le_bytes());

            let modrm_add_sum = 0xC0 | (sum_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x01, modrm_add_sum]);

            let modrm_add_i = 0xC0 | (i_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x81, modrm_add_i]);
            emitter.emit_slice(&n_c.to_le_bytes());

            return Ok(true);
        }
    }

    Ok(false)
}

// ─── Diagnostics ───────────────────────────────────────────────────────────────

/// Describes a detected symbolic loop pattern.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SymbolicLoopPattern {
    /// The variable being incremented each iteration.
    pub increment_var_hash: u64,
    /// The step value added per iteration.
    pub increment_step: i32,
    /// The accumulator variable being summed into.
    pub accumulator_var_hash: u64,
    /// The other variable being added to the accumulator.
    pub added_var_hash: u64,
    /// Whether this pattern can be native-compiled.
    pub is_native_eligible: bool,
}

/// Diagnostic snapshot of a loop body analysis.
#[derive(Debug, Clone, Serialize)]
pub struct LoopAnalysisInfo {
    pub body_node_count: usize,
    pub has_increment_pattern: bool,
    pub has_accumulator_pattern: bool,
    pub detected_pattern: Option<SymbolicLoopPattern>,
    pub validation_issues: Vec<String>,
}

/// Analyze a loop body for symbolic optimization opportunities without emitting code.
pub fn analyze_loop_body(count: u32, body: &[NdaNode]) -> LoopAnalysisInfo {
    let mut issues = validate_symbolic_loop_params(count, body);
    let mut increment_var = None;
    let mut accumulator_var = None;

    for node in body {
        if let NdaNode::Store { name_hash, value } = node {
            if let NdaNode::Add { lhs, rhs } = &**value {
                // Check increment pattern: var = var + Int(step)
                let mut is_inc = false;
                let mut step = 0i32;
                if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                    if l_hash == name_hash {
                        if let NdaNode::Int { value: val } = &**rhs {
                            is_inc = true;
                            step = *val;
                        }
                    }
                }
                if !is_inc {
                    if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                        if r_hash == name_hash {
                            if let NdaNode::Int { value: val } = &**lhs {
                                is_inc = true;
                                step = *val;
                            }
                        }
                    }
                }
                if is_inc {
                    increment_var = Some((*name_hash, step));
                    continue;
                }

                // Check accumulator pattern: var = var + other_var
                let mut is_acc = false;
                let mut other_var = None;
                if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                    if l_hash == name_hash {
                        if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                            is_acc = true;
                            other_var = Some(*r_hash);
                        }
                    }
                }
                if !is_acc {
                    if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                        if r_hash == name_hash {
                            if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                                is_acc = true;
                                other_var = Some(*l_hash);
                            }
                        }
                    }
                }
                if is_acc {
                    accumulator_var = Some((*name_hash, other_var.unwrap()));
                }
            }
        }
    }

    let pattern = match (increment_var, accumulator_var) {
        (Some((i_hash, step)), Some((sum_hash, added_hash))) => {
            let eligible = added_hash == i_hash && sum_hash != i_hash;
            if eligible {
                Some(SymbolicLoopPattern {
                    increment_var_hash: i_hash,
                    increment_step: step,
                    accumulator_var_hash: sum_hash,
                    added_var_hash: added_hash,
                    is_native_eligible: true,
                })
            } else {
                None
            }
        }
        _ => None,
    };

    LoopAnalysisInfo {
        body_node_count: body.len(),
        has_increment_pattern: increment_var.is_some(),
        has_accumulator_pattern: accumulator_var.is_some(),
        detected_pattern: pattern,
        validation_issues: issues,
    }
}

/// Validate symbolic loop parameters without executing.
pub fn validate_symbolic_loop_params(count: u32, body: &[NdaNode]) -> Vec<String> {
    let mut issues = Vec::new();

    if count == 0 {
        issues.push("loop count is 0 (no iterations)".to_string());
    }

    if body.is_empty() {
        issues.push("loop body is empty".to_string());
    }

    if body.len() > 100 {
        issues.push(format!(
            "loop body has {} nodes (too large for symbolic optimization)",
            body.len()
        ));
    }

    // Check for potential overflow in closed-form computation
    let n = count as i64;
    if n > 100_000 {
        issues.push(format!(
            "loop count {} is large; closed-form i64 arithmetic may overflow",
            count
        ));
    }

    issues
}

/// Compute the closed-form result of a symbolic loop without executing it.
/// Returns (final_i_value, accumulated_sum_delta) for the given pattern.
pub fn symbolic_loop_closed_form(count: u32, step: i32) -> (i64, i64) {
    let n = count as i64;
    let s = step as i64;
    let final_i = n * s;
    let sum_delta = s * n * (n - 1) / 2;
    (final_i, sum_delta)
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_inc_store(var_hash: u64, step: i32) -> NdaNode {
        NdaNode::Store {
            name_hash: var_hash,
            value: Box::new(NdaNode::Add {
                lhs: Box::new(NdaNode::Load { name_hash: var_hash }),
                rhs: Box::new(NdaNode::Int { value: step }),
            }),
        }
    }

    fn make_acc_store(sum_hash: u64, added_hash: u64) -> NdaNode {
        NdaNode::Store {
            name_hash: sum_hash,
            value: Box::new(NdaNode::Add {
                lhs: Box::new(NdaNode::Load { name_hash: sum_hash }),
                rhs: Box::new(NdaNode::Load { name_hash: added_hash }),
            }),
        }
    }

    #[test]
    fn analyze_loop_body_empty() {
        let info = analyze_loop_body(10, &[]);
        assert_eq!(info.body_node_count, 0);
        assert!(!info.has_increment_pattern);
        assert!(!info.has_accumulator_pattern);
        assert!(info.detected_pattern.is_none());
        assert!(info.validation_issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn analyze_loop_body_zero_count() {
        let body = vec![make_inc_store(0x01, 1)];
        let info = analyze_loop_body(0, &body);
        assert!(info.validation_issues.iter().any(|i| i.contains("count is 0")));
    }

    #[test]
    fn analyze_loop_body_increment_only() {
        let body = vec![
            make_inc_store(0x01, 2),
            NdaNode::Int { value: 42 },
        ];
        let info = analyze_loop_body(10, &body);
        assert!(info.has_increment_pattern);
        assert!(!info.has_accumulator_pattern);
        assert!(info.detected_pattern.is_none());
    }

    #[test]
    fn analyze_loop_body_full_pattern() {
        let i_hash: u64 = 0xAAAA;
        let sum_hash: u64 = 0xBBBB;
        let body = vec![
            make_inc_store(i_hash, 1),
            make_acc_store(sum_hash, i_hash),
        ];
        let info = analyze_loop_body(10, &body);
        assert!(info.has_increment_pattern);
        assert!(info.has_accumulator_pattern);
        assert!(info.detected_pattern.is_some());
        let pat = info.detected_pattern.unwrap();
        assert_eq!(pat.increment_var_hash, i_hash);
        assert_eq!(pat.increment_step, 1);
        assert_eq!(pat.accumulator_var_hash, sum_hash);
        assert_eq!(pat.added_var_hash, i_hash);
        assert!(pat.is_native_eligible);
    }

    #[test]
    fn analyze_loop_body_wrong_pattern() {
        // accumulator adds a different var, not the increment var
        let i_hash: u64 = 0xAAAA;
        let sum_hash: u64 = 0xBBBB;
        let other_hash: u64 = 0xCCCC;
        let body = vec![
            make_inc_store(i_hash, 1),
            make_acc_store(sum_hash, other_hash), // adds other_hash, not i_hash
        ];
        let info = analyze_loop_body(10, &body);
        assert!(info.detected_pattern.is_none()); // not eligible
    }

    #[test]
    fn validate_symbolic_loop_clean() {
        let body = vec![make_inc_store(0x01, 1)];
        let issues = validate_symbolic_loop_params(10, &body);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_symbolic_loop_zero_count() {
        let body = vec![make_inc_store(0x01, 1)];
        let issues = validate_symbolic_loop_params(0, &body);
        assert!(issues.iter().any(|i| i.contains("count is 0")));
    }

    #[test]
    fn validate_symbolic_loop_empty_body() {
        let issues = validate_symbolic_loop_params(10, &[]);
        assert!(issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn validate_symbolic_loop_large_count() {
        let body = vec![make_inc_store(0x01, 1)];
        let issues = validate_symbolic_loop_params(200_000, &body);
        assert!(issues.iter().any(|i| i.contains("overflow")));
    }

    #[test]
    fn symbolic_loop_closed_form_basic() {
        // count=5, step=1: i goes 0,1,2,3,4 -> final_i=5, sum=0+1+2+3+4=10
        let (final_i, sum_delta) = symbolic_loop_closed_form(5, 1);
        assert_eq!(final_i, 5);
        assert_eq!(sum_delta, 10);
    }

    #[test]
    fn symbolic_loop_closed_form_step2() {
        // count=4, step=2: i goes 0,2,4,6 -> final_i=8, sum=0+2+4+6=12
        let (final_i, sum_delta) = symbolic_loop_closed_form(4, 2);
        assert_eq!(final_i, 8);
        assert_eq!(sum_delta, 12);
    }

    #[test]
    fn symbolic_loop_closed_form_single() {
        let (final_i, sum_delta) = symbolic_loop_closed_form(1, 3);
        assert_eq!(final_i, 3);
        assert_eq!(sum_delta, 0); // n*(n-1)/2 = 1*0/2 = 0
    }

    #[test]
    fn symbolic_loop_closed_form_zero() {
        let (final_i, sum_delta) = symbolic_loop_closed_form(0, 5);
        assert_eq!(final_i, 0);
        assert_eq!(sum_delta, 0);
    }

    // ── Block 110: expanded tests ────────────────────────────────────────────

    #[test]
    fn closed_form_negative_step() {
        // count=4, step=-1: i goes 0,-1,-2,-3 -> final_i=-4, sum=0+(-1)+(-2)+(-3)=-6
        let (final_i, sum_delta) = symbolic_loop_closed_form(4, -1);
        assert_eq!(final_i, -4);
        assert_eq!(sum_delta, -6);
    }

    #[test]
    fn closed_form_zero_step() {
        let (final_i, sum_delta) = symbolic_loop_closed_form(10, 0);
        assert_eq!(final_i, 0);
        assert_eq!(sum_delta, 0);
    }

    #[test]
    fn closed_form_large_count() {
        let (final_i, sum_delta) = symbolic_loop_closed_form(1000, 1);
        assert_eq!(final_i, 1000);
        assert_eq!(sum_delta, 1000 * 999 / 2);
    }

    #[test]
    fn validate_body_too_large() {
        let body: Vec<NdaNode> = (0..101).map(|_| NdaNode::Int { value: 0 }).collect();
        let issues = validate_symbolic_loop_params(10, &body);
        assert!(issues.iter().any(|i| i.contains("too large")));
    }

    #[test]
    fn validate_body_exactly_100_ok() {
        let body: Vec<NdaNode> = (0..100).map(|_| NdaNode::Int { value: 0 }).collect();
        let issues = validate_symbolic_loop_params(10, &body);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_multiple_issues() {
        let issues = validate_symbolic_loop_params(0, &[]);
        assert!(issues.len() >= 2); // zero count + empty body
    }

    #[test]
    fn analyze_reversed_increment() {
        // Int + Load instead of Load + Int
        let var_hash: u64 = 0x01;
        let body = vec![
            NdaNode::Store {
                name_hash: var_hash,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Int { value: 3 }),
                    rhs: Box::new(NdaNode::Load { name_hash: var_hash }),
                }),
            },
            NdaNode::Int { value: 0 },
        ];
        let info = analyze_loop_body(10, &body);
        assert!(info.has_increment_pattern);
        let pat = info.detected_pattern;
        // only increment, no accumulator → no full pattern
        assert!(pat.is_none());
    }

    #[test]
    fn analyze_reversed_accumulator() {
        // Load + Load reversed for accumulator
        let i_hash: u64 = 0xAAAA;
        let sum_hash: u64 = 0xBBBB;
        let body = vec![
            make_inc_store(i_hash, 1),
            NdaNode::Store {
                name_hash: sum_hash,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: i_hash }),
                    rhs: Box::new(NdaNode::Load { name_hash: sum_hash }),
                }),
            },
        ];
        let info = analyze_loop_body(10, &body);
        assert!(info.has_increment_pattern);
        assert!(info.has_accumulator_pattern);
        assert!(info.detected_pattern.is_some());
    }

    #[test]
    fn analyze_same_var_for_inc_and_acc_not_eligible() {
        // sum_hash == i_hash → not eligible
        let var_hash: u64 = 0xAAAA;
        let body = vec![
            make_inc_store(var_hash, 1),
            make_acc_store(var_hash, var_hash),
        ];
        let info = analyze_loop_body(10, &body);
        assert!(info.has_increment_pattern);
        assert!(info.has_accumulator_pattern);
        // But pattern should be None because sum_hash == i_hash
        assert!(info.detected_pattern.is_none());
    }

    #[test]
    fn detect_body_too_long() {
        let body = vec![
            make_inc_store(0x01, 1),
            make_acc_store(0x02, 0x01),
            NdaNode::Int { value: 99 },
        ];
        let mut emitter = X86Emitter::new();
        let registry = VarRegistry::new();
        let result = detect_and_compile_symbolic_loop(10, &body, &mut emitter, &registry).unwrap();
        assert!(!result); // body.len() != 2 → false
    }

    #[test]
    fn detect_body_wrong_shape() {
        let body = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Int { value: 2 },
        ];
        let mut emitter = X86Emitter::new();
        let registry = VarRegistry::new();
        let result = detect_and_compile_symbolic_loop(10, &body, &mut emitter, &registry).unwrap();
        assert!(!result);
    }

    #[test]
    fn detect_eligible_pattern_emits_bytes() {
        let i_hash: u64 = 0xAAAA;
        let sum_hash: u64 = 0xBBBB;
        let body = vec![
            make_inc_store(i_hash, 1),
            make_acc_store(sum_hash, i_hash),
        ];
        let mut emitter = X86Emitter::new();
        let registry = VarRegistry::new();
        let result = detect_and_compile_symbolic_loop(10, &body, &mut emitter, &registry).unwrap();
        assert!(result);
        assert!(!emitter.buf.is_empty());
    }

    #[test]
    fn pattern_struct_equality() {
        let p1 = SymbolicLoopPattern {
            increment_var_hash: 1, increment_step: 2,
            accumulator_var_hash: 3, added_var_hash: 1,
            is_native_eligible: true,
        };
        let p2 = p1.clone();
        assert_eq!(p1, p2);
    }

    #[test]
    fn pattern_struct_serializes() {
        let p = SymbolicLoopPattern {
            increment_var_hash: 0xAA, increment_step: 3,
            accumulator_var_hash: 0xBB, added_var_hash: 0xAA,
            is_native_eligible: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"increment_step\":3"));
        assert!(json.contains("\"is_native_eligible\":true"));
    }

    #[test]
    fn loop_analysis_info_serializes() {
        let info = LoopAnalysisInfo {
            body_node_count: 2,
            has_increment_pattern: true,
            has_accumulator_pattern: false,
            detected_pattern: None,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"body_node_count\":2"));
        assert!(json.contains("\"has_increment_pattern\":true"));
    }

    #[test]
    fn loop_analysis_info_with_pattern() {
        let i_hash: u64 = 0x01;
        let sum_hash: u64 = 0x02;
        let body = vec![
            make_inc_store(i_hash, 1),
            make_acc_store(sum_hash, i_hash),
        ];
        let info = analyze_loop_body(5, &body);
        assert!(info.detected_pattern.is_some());
        let pat = info.detected_pattern.unwrap();
        assert_eq!(pat.increment_step, 1);
        assert!(info.validation_issues.is_empty());
    }
}
