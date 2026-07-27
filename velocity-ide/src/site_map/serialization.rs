use anyhow::Result;

use super::verifier::{AtomicOp, BitwiseOp, CmpOp, NdaNode, TypeKind, VecOpKind};

/// Minimal NDA node serialisation for disk storage.
/// Format: 1-byte opcode tag + payload.
pub fn serialise_node(node: &NdaNode) -> Vec<u8> {
    let mut buf = Vec::new();
    write_node(node, &mut buf);
    buf
}

pub fn deserialise_node(data: &[u8], offset: &mut usize) -> Result<NdaNode> {
    if *offset >= data.len() {
        anyhow::bail!("EOF");
    }
    let tag = data[*offset];
    *offset += 1;
    match tag {
        b'M' => {
            if *offset + 5 > data.len() {
                anyhow::bail!("Truncated Matrix");
            }
            let rows = u16::from_le_bytes(data[*offset..*offset + 2].try_into().unwrap());
            let cols = u16::from_le_bytes(data[*offset + 2..*offset + 4].try_into().unwrap());
            let scale = data[*offset + 4] as i8;
            *offset += 5;
            let bitmap_bytes = rows as usize * (cols as usize).div_ceil(8);
            if *offset + 2 * bitmap_bytes > data.len() {
                anyhow::bail!("Truncated Matrix bitmaps");
            }
            let sign = data[*offset..*offset + bitmap_bytes].to_vec();
            *offset += bitmap_bytes;
            let extra = data[*offset..*offset + bitmap_bytes].to_vec();
            *offset += bitmap_bytes;
            Ok(NdaNode::Matrix {
                rows,
                cols,
                scale,
                sign,
                extra,
            })
        }
        b'N' => {
            if *offset + 2 > data.len() {
                anyhow::bail!("Truncated Norm");
            }
            let size = u16::from_le_bytes(data[*offset..*offset + 2].try_into().unwrap());
            *offset += 2;
            let bitmap_bytes = (size as usize).div_ceil(8);
            if *offset + 2 * bitmap_bytes > data.len() {
                anyhow::bail!("Truncated Norm bitmaps");
            }
            let weight = data[*offset..*offset + bitmap_bytes].to_vec();
            *offset += bitmap_bytes;
            let bias = data[*offset..*offset + bitmap_bytes].to_vec();
            *offset += bitmap_bytes;
            Ok(NdaNode::Norm { size, weight, bias })
        }
        b'C' => {
            if *offset < data.len() && data[*offset] == b'M' {
                *offset += 1;
                if *offset >= data.len() || data[*offset] != b'P' {
                    anyhow::bail!("Invalid Compare tag");
                }
                *offset += 1;
                if *offset >= data.len() {
                    anyhow::bail!("Truncated Compare op");
                }
                let op_val = data[*offset];
                *offset += 1;
                let op = CmpOp::from_u8(op_val).ok_or_else(|| anyhow::anyhow!("Invalid CmpOp"))?;
                let lhs = deserialise_node(data, offset)?;
                let rhs = deserialise_node(data, offset)?;
                Ok(NdaNode::Compare {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                })
            } else if *offset < data.len() && data[*offset] == b'S' {
                *offset += 1;
                if *offset + 2 > data.len() {
                    anyhow::bail!("Truncated Cast types");
                }
                let from_val = data[*offset];
                let to_val = data[*offset + 1];
                *offset += 2;
                let from_type = TypeKind::from_u8(from_val)
                    .ok_or_else(|| anyhow::anyhow!("Invalid from_type"))?;
                let to_type = TypeKind::from_u8(to_val)
                    .ok_or_else(|| anyhow::anyhow!("Invalid to_type"))?;
                let operand = deserialise_node(data, offset)?;
                Ok(NdaNode::Cast {
                    from_type,
                    to_type,
                    operand: Box::new(operand),
                })
            } else {
                if *offset + 8 > data.len() {
                    anyhow::bail!("Truncated Call");
                }
                let target = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
                *offset += 8;
                Ok(NdaNode::Call { target })
            }
        }
        b'I' => {
            if *offset < data.len() && data[*offset] == b'F' {
                *offset += 1;
                let cond = deserialise_node(data, offset)?;
                if *offset + 4 > data.len() {
                    anyhow::bail!("Truncated If then len");
                }
                let then_len =
                    u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap()) as usize;
                *offset += 4;
                let mut then_body = Vec::with_capacity(then_len);
                for _ in 0..then_len {
                    then_body.push(deserialise_node(data, offset)?);
                }
                let mut else_body = None;
                if *offset < data.len() {
                    let has_else = data[*offset];
                    if has_else == 1 {
                        *offset += 1;
                        if *offset + 4 > data.len() {
                            anyhow::bail!("Truncated If else len");
                        }
                        let else_len =
                            u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap())
                                as usize;
                        *offset += 4;
                        let mut eb = Vec::with_capacity(else_len);
                        for _ in 0..else_len {
                            eb.push(deserialise_node(data, offset)?);
                        }
                        else_body = Some(eb);
                    } else if has_else == 0 {
                        *offset += 1;
                    }
                }
                Ok(NdaNode::If {
                    cond: Box::new(cond),
                    then_body,
                    else_body,
                })
            } else {
                if *offset + 4 > data.len() {
                    anyhow::bail!("Truncated Int");
                }
                let value = i32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
                *offset += 4;
                Ok(NdaNode::Int { value })
            }
        }
        b'S' => {
            if *offset < data.len() && data[*offset] == b'T' {
                *offset += 1;
                if *offset + 8 > data.len() {
                    anyhow::bail!("Truncated Store name_hash");
                }
                let name_hash = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
                *offset += 8;
                let value = deserialise_node(data, offset)?;
                Ok(NdaNode::Store {
                    name_hash,
                    value: Box::new(value),
                })
            } else {
                if *offset + 4 > data.len() {
                    anyhow::bail!("Truncated Scope");
                }
                let len =
                    u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap()) as usize;
                *offset += 4;
                let mut children = Vec::with_capacity(len);
                for _ in 0..len {
                    children.push(deserialise_node(data, offset)?);
                }
                Ok(NdaNode::Scope { children })
            }
        }
        b'L' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated L tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'P' => {
                    if *offset + 8 > data.len() {
                        anyhow::bail!("Truncated Loop count/len");
                    }
                    let count = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
                    let len =
                        u32::from_le_bytes(data[*offset + 4..*offset + 8].try_into().unwrap())
                            as usize;
                    *offset += 8;
                    let mut body = Vec::with_capacity(len);
                    for _ in 0..len {
                        body.push(deserialise_node(data, offset)?);
                    }
                    Ok(NdaNode::Loop { count, body })
                }
                b'T' => {
                    if *offset + 8 > data.len() {
                        anyhow::bail!("Truncated Let name_hash");
                    }
                    let name_hash =
                        u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
                    *offset += 8;
                    let init = deserialise_node(data, offset)?;
                    Ok(NdaNode::Let {
                        name_hash,
                        init: Box::new(init),
                    })
                }
                b'D' => {
                    if *offset + 8 > data.len() {
                        anyhow::bail!("Truncated Load name_hash");
                    }
                    let name_hash =
                        u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
                    *offset += 8;
                    Ok(NdaNode::Load { name_hash })
                }
                _ => anyhow::bail!("Unknown subtag L{}", sub),
            }
        }
        b'W' => {
            if *offset >= data.len() || data[*offset] != b'H' {
                anyhow::bail!("Invalid While tag");
            }
            *offset += 1;
            let cond = deserialise_node(data, offset)?;
            if *offset + 4 > data.len() {
                anyhow::bail!("Truncated While body len");
            }
            let len = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap()) as usize;
            *offset += 4;
            let mut body = Vec::with_capacity(len);
            for _ in 0..len {
                body.push(deserialise_node(data, offset)?);
            }
            Ok(NdaNode::While {
                cond: Box::new(cond),
                body,
            })
        }
        b'B' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated B tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'K' => Ok(NdaNode::Break),
                b'W' => {
                    if *offset + 1 > data.len() {
                        anyhow::bail!("Truncated Bitwise op");
                    }
                    let op_val = data[*offset];
                    *offset += 1;
                    let op = BitwiseOp::from_u8(op_val)
                        .ok_or_else(|| anyhow::anyhow!("Invalid BitwiseOp"))?;
                    let lhs = deserialise_node(data, offset)?;
                    if *offset >= data.len() {
                        anyhow::bail!("Truncated Bitwise has_rhs");
                    }
                    let has_rhs = data[*offset];
                    *offset += 1;
                    let rhs = if has_rhs == 1 {
                        Some(Box::new(deserialise_node(data, offset)?))
                    } else {
                        None
                    };
                    Ok(NdaNode::Bitwise {
                        op,
                        lhs: Box::new(lhs),
                        rhs,
                    })
                }
                _ => anyhow::bail!("Unknown subtag B{}", sub),
            }
        }
        b'F' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated F tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'L' => {
                    if *offset + 4 > data.len() {
                        anyhow::bail!("Truncated Float");
                    }
                    let value = f32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
                    *offset += 4;
                    Ok(NdaNode::Float { value })
                }
                b'R' => {
                    let addr = deserialise_node(data, offset)?;
                    Ok(NdaNode::Free {
                        addr: Box::new(addr),
                    })
                }
                _ => anyhow::bail!("Unknown subtag F{}", sub),
            }
        }
        b'G' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated G tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'M' => {
                    let matrix = deserialise_node(data, offset)?;
                    let vector = deserialise_node(data, offset)?;
                    Ok(NdaNode::Gemv {
                        matrix: Box::new(matrix),
                        vector: Box::new(vector),
                    })
                }
                b'D' => {
                    if *offset + 12 > data.len() {
                        anyhow::bail!("Truncated GpuDispatch");
                    }
                    let shader_hash =
                        u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
                    let len =
                        u32::from_le_bytes(data[*offset + 8..*offset + 12].try_into().unwrap())
                            as usize;
                    *offset += 12;
                    let mut args = Vec::with_capacity(len);
                    for _ in 0..len {
                        args.push(deserialise_node(data, offset)?);
                    }
                    Ok(NdaNode::GpuDispatch { shader_hash, args })
                }
                _ => anyhow::bail!("Unknown subtag G{}", sub),
            }
        }
        b'D' => {
            if *offset >= data.len() || data[*offset] != b'T' {
                anyhow::bail!("Invalid Dot tag");
            }
            *offset += 1;
            let lhs = deserialise_node(data, offset)?;
            let rhs = deserialise_node(data, offset)?;
            Ok(NdaNode::Dot {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        b'A' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated A tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'D' => {
                    let lhs = deserialise_node(data, offset)?;
                    let rhs = deserialise_node(data, offset)?;
                    Ok(NdaNode::Add {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    })
                }
                b'T' => {
                    if *offset + 1 > data.len() {
                        anyhow::bail!("Truncated Atomic op");
                    }
                    let op_val = data[*offset];
                    *offset += 1;
                    let op = AtomicOp::from_u8(op_val)
                        .ok_or_else(|| anyhow::anyhow!("Invalid AtomicOp"))?;
                    let addr = deserialise_node(data, offset)?;
                    let val = deserialise_node(data, offset)?;
                    Ok(NdaNode::Atomic {
                        op,
                        addr: Box::new(addr),
                        val: Box::new(val),
                    })
                }
                b'L' => {
                    let size = deserialise_node(data, offset)?;
                    Ok(NdaNode::Alloc {
                        size: Box::new(size),
                    })
                }
                _ => anyhow::bail!("Unknown subtag A{}", sub),
            }
        }
        b'V' => {
            if *offset >= data.len() || data[*offset] != b'O' {
                anyhow::bail!("Invalid VecOp tag");
            }
            *offset += 1;
            if *offset >= data.len() {
                anyhow::bail!("Truncated VecOp op");
            }
            let op_val = data[*offset];
            *offset += 1;
            let op =
                VecOpKind::from_u8(op_val).ok_or_else(|| anyhow::anyhow!("Invalid VecOpKind"))?;
            let operand = deserialise_node(data, offset)?;
            Ok(NdaNode::VecOp {
                op,
                operand: Box::new(operand),
            })
        }
        b'P' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated P tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'R' => {
                    let source = deserialise_node(data, offset)?;
                    Ok(NdaNode::Print {
                        source: Box::new(source),
                    })
                }
                b'K' => {
                    let addr = deserialise_node(data, offset)?;
                    Ok(NdaNode::Peek {
                        addr: Box::new(addr),
                    })
                }
                b'O' => {
                    let addr = deserialise_node(data, offset)?;
                    let value = deserialise_node(data, offset)?;
                    Ok(NdaNode::Poke {
                        addr: Box::new(addr),
                        value: Box::new(value),
                    })
                }
                _ => anyhow::bail!("Unknown subtag P{}", sub),
            }
        }
        b'R' => {
            if *offset >= data.len() {
                anyhow::bail!("Truncated R tag");
            }
            let sub = data[*offset];
            *offset += 1;
            match sub {
                b'T' => {
                    let value = deserialise_node(data, offset)?;
                    Ok(NdaNode::Return {
                        value: Box::new(value),
                    })
                }
                b'I' => {
                    if *offset + 12 > data.len() {
                        anyhow::bail!("Truncated RegInt");
                    }
                    let vector = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
                    let handler_hash =
                        u64::from_le_bytes(data[*offset + 4..*offset + 12].try_into().unwrap());
                    *offset += 12;
                    Ok(NdaNode::RegInt {
                        vector,
                        handler_hash,
                    })
                }
                _ => anyhow::bail!("Unknown subtag R{}", sub),
            }
        }
        b'T' => {
            if *offset + 18 > data.len() {
                anyhow::bail!("Truncated Triple");
            }
            let subject_hash = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
            let predicate_id = u16::from_le_bytes(data[*offset + 8..*offset + 10].try_into().unwrap());
            let object_hash =
                u64::from_le_bytes(data[*offset + 10..*offset + 18].try_into().unwrap());
            *offset += 18;
            Ok(NdaNode::Triple {
                subject_hash,
                predicate_id,
                object_hash,
            })
        }
        _ => anyhow::bail!("Unknown tag {}", tag),
    }
}

pub fn write_node(node: &NdaNode, buf: &mut Vec<u8>) {
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            scale,
            sign,
            extra,
        } => {
            buf.push(b'M');
            buf.extend_from_slice(&rows.to_le_bytes());
            buf.extend_from_slice(&cols.to_le_bytes());
            buf.push(*scale as u8);
            buf.extend_from_slice(sign);
            buf.extend_from_slice(extra);
        }
        NdaNode::Norm { size, weight, bias } => {
            buf.push(b'N');
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(weight);
            buf.extend_from_slice(bias);
        }
        NdaNode::Call { target } => {
            buf.push(b'C');
            buf.extend_from_slice(&target.to_le_bytes());
        }
        NdaNode::Int { value } => {
            buf.push(b'I');
            buf.extend_from_slice(&value.to_le_bytes());
        }
        NdaNode::Scope { children } => {
            buf.push(b'S');
            buf.extend_from_slice(&(children.len() as u32).to_le_bytes());
            for child in children {
                write_node(child, buf);
            }
        }
        NdaNode::Loop { count, body } => {
            buf.push(b'L');
            buf.push(b'P');
            buf.extend_from_slice(&count.to_le_bytes());
            buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
            for child in body {
                write_node(child, buf);
            }
        }
        NdaNode::While { cond, body } => {
            buf.push(b'W');
            buf.push(b'H');
            write_node(cond, buf);
            buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
            for child in body {
                write_node(child, buf);
            }
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            buf.push(b'I');
            buf.push(b'F');
            write_node(cond, buf);
            buf.extend_from_slice(&(then_body.len() as u32).to_le_bytes());
            for child in then_body {
                write_node(child, buf);
            }
            if let Some(eb) = else_body {
                buf.push(1u8);
                buf.extend_from_slice(&(eb.len() as u32).to_le_bytes());
                for child in eb {
                    write_node(child, buf);
                }
            } else {
                buf.push(0u8);
            }
        }
        NdaNode::Compare { op, lhs, rhs } => {
            buf.push(b'C');
            buf.push(b'M');
            buf.push(b'P');
            buf.push(*op as u8);
            write_node(lhs, buf);
            write_node(rhs, buf);
        }
        NdaNode::Break => {
            buf.push(b'B');
            buf.push(b'K');
        }
        NdaNode::Let { name_hash, init } => {
            buf.push(b'L');
            buf.push(b'T');
            buf.extend_from_slice(&name_hash.to_le_bytes());
            write_node(init, buf);
        }
        NdaNode::Load { name_hash } => {
            buf.push(b'L');
            buf.push(b'D');
            buf.extend_from_slice(&name_hash.to_le_bytes());
        }
        NdaNode::Store { name_hash, value } => {
            buf.push(b'S');
            buf.push(b'T');
            buf.extend_from_slice(&name_hash.to_le_bytes());
            write_node(value, buf);
        }
        NdaNode::Add { lhs, rhs } => {
            buf.push(b'A');
            buf.push(b'D');
            write_node(lhs, buf);
            write_node(rhs, buf);
        }
        NdaNode::VecOp { op, operand } => {
            buf.push(b'V');
            buf.push(b'O');
            buf.push(*op as u8);
            write_node(operand, buf);
        }
        NdaNode::Print { source } => {
            buf.push(b'P');
            buf.push(b'R');
            write_node(source, buf);
        }
        NdaNode::Return { value } => {
            buf.push(b'R');
            buf.push(b'T');
            write_node(value, buf);
        }
        NdaNode::Bitwise { op, lhs, rhs } => {
            buf.push(b'B');
            buf.push(b'W');
            buf.push(*op as u8);
            write_node(lhs, buf);
            if let Some(r) = rhs {
                buf.push(1u8);
                write_node(r, buf);
            } else {
                buf.push(0u8);
            }
        }
        NdaNode::Float { value } => {
            buf.push(b'F');
            buf.push(b'L');
            buf.extend_from_slice(&value.to_le_bytes());
        }
        NdaNode::Math { op, lhs, rhs } => {
            buf.push(b'M');
            buf.push(b'H');
            buf.push(*op as u8);
            write_node(lhs, buf);
            write_node(rhs, buf);
        }
        NdaNode::MathFunc { func, operand } => {
            buf.push(b'M');
            buf.push(b'F');
            buf.push(*func as u8);
            write_node(operand, buf);
        }
        NdaNode::Peek { addr } => {
            buf.push(b'P');
            buf.push(b'K');
            write_node(addr, buf);
        }
        NdaNode::Poke { addr, value } => {
            buf.push(b'P');
            buf.push(b'O');
            write_node(addr, buf);
            write_node(value, buf);
        }
        NdaNode::Gemv { matrix, vector } => {
            buf.push(b'G');
            buf.push(b'M');
            write_node(matrix, buf);
            write_node(vector, buf);
        }
        NdaNode::Dot { lhs, rhs } => {
            buf.push(b'D');
            buf.push(b'T');
            write_node(lhs, buf);
            write_node(rhs, buf);
        }
        NdaNode::Syscall { num, args } => {
            buf.push(b'S');
            buf.push(b'C');
            buf.extend_from_slice(&num.to_le_bytes());
            buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
            for arg in args {
                write_node(arg, buf);
            }
        }
        NdaNode::Spawn { scope_hash } => {
            buf.push(b'S');
            buf.push(b'W');
            buf.extend_from_slice(&scope_hash.to_le_bytes());
        }
        NdaNode::Atomic { op, addr, val } => {
            buf.push(b'A');
            buf.push(b'T');
            buf.push(*op as u8);
            write_node(addr, buf);
            write_node(val, buf);
        }
        NdaNode::Alloc { size } => {
            buf.push(b'A');
            buf.push(b'L');
            write_node(size, buf);
        }
        NdaNode::Free { addr } => {
            buf.push(b'F');
            buf.push(b'R');
            write_node(addr, buf);
        }
        NdaNode::RegInt {
            vector,
            handler_hash,
        } => {
            buf.push(b'R');
            buf.push(b'I');
            buf.extend_from_slice(&vector.to_le_bytes());
            buf.extend_from_slice(&handler_hash.to_le_bytes());
        }
        NdaNode::Cast {
            from_type,
            to_type,
            operand,
        } => {
            buf.push(b'C');
            buf.push(b'S');
            buf.push(*from_type as u8);
            buf.push(*to_type as u8);
            write_node(operand, buf);
        }
        NdaNode::GpuDispatch { shader_hash, args } => {
            buf.push(b'G');
            buf.push(b'D');
            buf.extend_from_slice(&shader_hash.to_le_bytes());
            buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
            for arg in args {
                write_node(arg, buf);
            }
        }
        NdaNode::Triple {
            subject_hash,
            predicate_id,
            object_hash,
        } => {
            buf.push(b'T');
            buf.extend_from_slice(&subject_hash.to_le_bytes());
            buf.extend_from_slice(&predicate_id.to_le_bytes());
            buf.extend_from_slice(&object_hash.to_le_bytes());
        }
    }
}
