use crate::site_map::NdaNode;
use super::types::VarRegistry;
use super::x86_emitter::X86Emitter;

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
        match node {
            NdaNode::Store { name_hash, value } => {
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
            _ => {}
        }
    }

    if let (Some((i_hash, step)), Some((sum_hash, added_hash))) = (increment_var, accumulator_var)
    {
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

            let modrm_mov = 0xC0 | ((i_reg as u8 & 7) << 3) | 0;
            emitter.emit_slice(&[0x44, 0x89, modrm_mov]);

            emitter.emit(0x69);
            emitter.emit(0xC0);
            emitter.emit_slice(&(count as i32).to_le_bytes());

            emitter.emit(0x05);
            emitter.emit_slice(&sum_step.to_le_bytes());

            let modrm_add_sum = 0xC0 | (0 << 3) | (sum_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x01, modrm_add_sum]);

            let modrm_add_i = 0xC0 | (0 << 3) | (i_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x81, modrm_add_i]);
            emitter.emit_slice(&n_c.to_le_bytes());

            return Ok(true);
        }
    }

    Ok(false)
}
