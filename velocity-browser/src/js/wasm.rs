//! From-scratch WebAssembly interpreter with stack machine, local variables,
//! memory operations, and bytecode dispatch.

#[derive(Debug, Clone, PartialEq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

/// Wasm opcode constants (subset matching the official spec).
#[allow(non_upper_case_globals)]
mod op {
    pub const UNREACHABLE: u8 = 0x00;
    pub const NOP: u8 = 0x01;
    pub const BLOCK: u8 = 0x02;
    pub const LOOP: u8 = 0x03;
    pub const IF: u8 = 0x04;
    pub const ELSE: u8 = 0x05;
    pub const END: u8 = 0x0B;
    pub const BR: u8 = 0x0C;
    pub const BR_IF: u8 = 0x0D;
    pub const RETURN: u8 = 0x0F;
    pub const LOCAL_GET: u8 = 0x20;
    pub const LOCAL_SET: u8 = 0x21;
    pub const LOCAL_TEE: u8 = 0x22;
    pub const I32_CONST: u8 = 0x41;
    pub const I64_CONST: u8 = 0x42;
    pub const F32_CONST: u8 = 0x43;
    pub const F64_CONST: u8 = 0x44;
    // i32 comparison
    pub const I32_EQZ: u8 = 0x45;
    pub const I32_EQ: u8 = 0x46;
    pub const I32_NE: u8 = 0x47;
    pub const I32_LT_S: u8 = 0x48;
    pub const I32_LT_U: u8 = 0x49;
    pub const I32_GT_S: u8 = 0x4A;
    pub const I32_GT_U: u8 = 0x4B;
    pub const I32_LE_S: u8 = 0x4C;
    pub const I32_LE_U: u8 = 0x4D;
    pub const I32_GE_S: u8 = 0x4E;
    pub const I32_GE_U: u8 = 0x4F;
    // i64 comparison
    pub const I64_EQZ: u8 = 0x50;
    pub const I64_EQ: u8 = 0x51;
    pub const I64_NE: u8 = 0x52;
    pub const I64_LT_S: u8 = 0x53;
    pub const I64_GT_S: u8 = 0x55;
    pub const I64_LE_S: u8 = 0x57;
    pub const I64_GE_S: u8 = 0x59;
    // i32 arithmetic
    pub const I32_ADD: u8 = 0x6A;
    pub const I32_SUB: u8 = 0x6B;
    pub const I32_MUL: u8 = 0x6C;
    pub const I32_DIV_S: u8 = 0x6D;
    pub const I32_DIV_U: u8 = 0x6E;
    pub const I32_REM_S: u8 = 0x6F;
    pub const I32_REM_U: u8 = 0x70;
    pub const I32_AND: u8 = 0x71;
    pub const I32_OR: u8 = 0x72;
    pub const I32_XOR: u8 = 0x73;
    pub const I32_SHL: u8 = 0x74;
    pub const I32_SHR_S: u8 = 0x75;
    pub const I32_SHR_U: u8 = 0x76;
    pub const I32_ROTL: u8 = 0x77;
    pub const I32_ROTR: u8 = 0x78;
    pub const I32_CLZ: u8 = 0x79;
    pub const I32_CTZ: u8 = 0x7A;
    pub const I32_POPCNT: u8 = 0x7B;
    // i64 arithmetic
    pub const I64_ADD: u8 = 0x7C;
    pub const I64_SUB: u8 = 0x7D;
    pub const I64_MUL: u8 = 0x7E;
    pub const I64_DIV_S: u8 = 0x7F;
    pub const I64_AND: u8 = 0x83;
    pub const I64_OR: u8 = 0x84;
    pub const I64_XOR: u8 = 0x85;
    // f32 arithmetic
    pub const F32_ABS: u8 = 0x8B;
    pub const F32_NEG: u8 = 0x8C;
    pub const F32_CEIL: u8 = 0x8D;
    pub const F32_FLOOR: u8 = 0x8E;
    pub const F32_SQRT: u8 = 0x91;
    pub const F32_ADD: u8 = 0x92;
    pub const F32_SUB: u8 = 0x93;
    pub const F32_MUL: u8 = 0x94;
    pub const F32_DIV: u8 = 0x95;
    pub const F32_MIN: u8 = 0x96;
    pub const F32_MAX: u8 = 0x97;
    // f64 arithmetic
    pub const F64_ABS: u8 = 0x99;
    pub const F64_NEG: u8 = 0x9A;
    pub const F64_CEIL: u8 = 0x9B;
    pub const F64_FLOOR: u8 = 0x9C;
    pub const F64_SQRT: u8 = 0x9F;
    pub const F64_ADD: u8 = 0xA0;
    pub const F64_SUB: u8 = 0xA1;
    pub const F64_MUL: u8 = 0xA2;
    pub const F64_DIV: u8 = 0xA3;
    pub const F64_MIN: u8 = 0xA4;
    pub const F64_MAX: u8 = 0xA5;
    // Conversions
    pub const I32_WRAP_I64: u8 = 0xA7;
    pub const I64_EXTEND_I32_S: u8 = 0xAC;
    pub const F32_CONVERT_I32_S: u8 = 0xB2;
    pub const F64_CONVERT_I32_S: u8 = 0xB7;
    pub const I32_TRUNC_F32_S: u8 = 0xA8;
    // Memory
    pub const I32_LOAD: u8 = 0x28;
    pub const I64_LOAD: u8 = 0x29;
    pub const F32_LOAD: u8 = 0x2A;
    pub const F64_LOAD: u8 = 0x2B;
    pub const I32_STORE: u8 = 0x36;
    pub const I64_STORE: u8 = 0x37;
    pub const F32_STORE: u8 = 0x38;
    pub const F64_STORE: u8 = 0x39;
    pub const I32_LOAD8_U: u8 = 0x2D;
    pub const I32_LOAD16_U: u8 = 0x2F;
    pub const I32_STORE8: u8 = 0x3A;
    pub const I32_STORE16: u8 = 0x3B;
}

pub struct WasmInterpreter {
    pub memory: Vec<u8>,
    pub stack: Vec<WasmValue>,
    pub locals: Vec<WasmValue>,
    label_stack: Vec<usize>,
    pub pc: usize,
}

type WasmResult = Result<(), String>;

impl WasmInterpreter {
    pub fn new(initial_memory_pages: usize) -> Self {
        Self {
            memory: vec![0u8; initial_memory_pages * 64 * 1024],
            stack: Vec::new(),
            locals: Vec::new(),
            label_stack: Vec::new(),
            pc: 0,
        }
    }

    /// Initialize locals for a function with the given values.
    pub fn init_locals(&mut self, values: Vec<WasmValue>) {
        self.locals = values;
    }

    // ── Stack helpers ──

    fn pop_i32(&mut self) -> Result<i32, String> {
        match self.stack.pop() {
            Some(WasmValue::I32(v)) => Ok(v),
            other => Err(format!("expected i32 on stack, got {:?}", other)),
        }
    }
    fn pop_i64(&mut self) -> Result<i64, String> {
        match self.stack.pop() {
            Some(WasmValue::I64(v)) => Ok(v),
            other => Err(format!("expected i64 on stack, got {:?}", other)),
        }
    }
    fn pop_f32(&mut self) -> Result<f32, String> {
        match self.stack.pop() {
            Some(WasmValue::F32(v)) => Ok(v),
            other => Err(format!("expected f32 on stack, got {:?}", other)),
        }
    }
    fn pop_f64(&mut self) -> Result<f64, String> {
        match self.stack.pop() {
            Some(WasmValue::F64(v)) => Ok(v),
            other => Err(format!("expected f64 on stack, got {:?}", other)),
        }
    }
    fn push(&mut self, v: WasmValue) {
        self.stack.push(v);
    }

    // ── Memory helpers ──

    fn load_bytes(&self, addr: usize, n: usize) -> Result<&[u8], String> {
        if addr
            .checked_add(n)
            .is_none_or(|end| end > self.memory.len())
        {
            return Err("memory out of bounds".into());
        }
        Ok(&self.memory[addr..addr + n])
    }
    fn store_bytes(&mut self, addr: usize, bytes: &[u8]) -> WasmResult {
        let n = bytes.len();
        if addr
            .checked_add(n)
            .is_none_or(|end| end > self.memory.len())
        {
            return Err("memory out of bounds".into());
        }
        self.memory[addr..addr + n].copy_from_slice(bytes);
        Ok(())
    }

    /// Read a u32 immediate (LEB128-simplified as 4-byte LE) from code at pc.
    fn read_u32(code: &[u8], pc: &mut usize) -> Result<u32, String> {
        if *pc + 4 > code.len() {
            return Err("unexpected end of code".into());
        }
        let val = u32::from_le_bytes([code[*pc], code[*pc + 1], code[*pc + 2], code[*pc + 3]]);
        *pc += 4;
        Ok(val)
    }
    fn read_i32(code: &[u8], pc: &mut usize) -> Result<i32, String> {
        Self::read_u32(code, pc).map(|v| v as i32)
    }
    fn read_i64(code: &[u8], pc: &mut usize) -> Result<i64, String> {
        if *pc + 8 > code.len() {
            return Err("unexpected end of code".into());
        }
        let val = i64::from_le_bytes([
            code[*pc],
            code[*pc + 1],
            code[*pc + 2],
            code[*pc + 3],
            code[*pc + 4],
            code[*pc + 5],
            code[*pc + 6],
            code[*pc + 7],
        ]);
        *pc += 8;
        Ok(val)
    }
    fn read_f32(code: &[u8], pc: &mut usize) -> Result<f32, String> {
        if *pc + 4 > code.len() {
            return Err("unexpected end of code".into());
        }
        let val = f32::from_le_bytes([code[*pc], code[*pc + 1], code[*pc + 2], code[*pc + 3]]);
        *pc += 4;
        Ok(val)
    }
    fn read_f64(code: &[u8], pc: &mut usize) -> Result<f64, String> {
        if *pc + 8 > code.len() {
            return Err("unexpected end of code".into());
        }
        let val = f64::from_le_bytes([
            code[*pc],
            code[*pc + 1],
            code[*pc + 2],
            code[*pc + 3],
            code[*pc + 4],
            code[*pc + 5],
            code[*pc + 6],
            code[*pc + 7],
        ]);
        *pc += 8;
        Ok(val)
    }

    /// Execute a bytecode program. Returns the top-of-stack value (if any).
    pub fn execute_program(&mut self, code: &[u8]) -> Result<Option<WasmValue>, String> {
        self.pc = 0;
        while self.pc < code.len() {
            let opcode = code[self.pc];
            self.pc += 1;
            self.execute_instruction(opcode, code)?;
        }
        Ok(self.stack.last().cloned())
    }

    /// Execute a single opcode.
    pub fn execute_instruction(&mut self, opcode: u8, code: &[u8]) -> WasmResult {
        match opcode {
            op::NOP => {}
            op::UNREACHABLE => return Err("unreachable executed".into()),

            // ── Control flow ──
            op::BLOCK => {
                let _block_type = code.get(self.pc).copied().unwrap_or(0x40);
                self.pc += 1;
                self.label_stack.push(self.pc); // mark block start
            }
            op::LOOP => {
                let _block_type = code.get(self.pc).copied().unwrap_or(0x40);
                self.pc += 1;
                self.label_stack.push(self.pc);
            }
            op::IF => {
                let _block_type = code.get(self.pc).copied().unwrap_or(0x40);
                self.pc += 1;
                let cond = self.pop_i32()?;
                if cond != 0 {
                    self.label_stack.push(self.pc);
                } else {
                    // Skip to ELSE or END
                    let mut depth = 1;
                    while self.pc < code.len() && depth > 0 {
                        match code[self.pc] {
                            op::IF => {
                                depth += 1;
                                self.pc += 2;
                            }
                            op::ELSE if depth == 1 => {
                                depth = 0;
                                self.pc += 1;
                            }
                            op::END => {
                                depth -= 1;
                                if depth > 0 {
                                    self.pc += 1;
                                }
                            }
                            _ => {
                                self.pc += 1;
                            }
                        }
                    }
                    if depth == 0 {
                        self.label_stack.push(self.pc);
                    }
                }
            }
            op::ELSE => {
                // If we reach ELSE during normal execution, skip to END
                let mut depth = 1;
                while self.pc < code.len() && depth > 0 {
                    match code[self.pc] {
                        op::IF => {
                            depth += 1;
                            self.pc += 1;
                        }
                        op::END => {
                            depth -= 1;
                            if depth > 0 {
                                self.pc += 1;
                            }
                        }
                        _ => {
                            self.pc += 1;
                        }
                    }
                }
            }
            op::END => {
                self.label_stack.pop();
            }
            op::BR => {
                let _label = Self::read_u32(code, &mut self.pc)?;
                // Simple: jump to END of current block
                let mut depth = 1;
                while self.pc < code.len() && depth > 0 {
                    match code[self.pc] {
                        op::BLOCK | op::LOOP | op::IF => {
                            depth += 1;
                            self.pc += 2;
                        }
                        op::END => {
                            depth -= 1;
                            if depth > 0 {
                                self.pc += 1;
                            }
                        }
                        _ => {
                            self.pc += 1;
                        }
                    }
                }
                self.label_stack.pop();
            }
            op::BR_IF => {
                let _label = Self::read_u32(code, &mut self.pc)?;
                let cond = self.pop_i32()?;
                if cond != 0 {
                    let mut depth = 1;
                    while self.pc < code.len() && depth > 0 {
                        match code[self.pc] {
                            op::BLOCK | op::LOOP | op::IF => {
                                depth += 1;
                                self.pc += 2;
                            }
                            op::END => {
                                depth -= 1;
                                if depth > 0 {
                                    self.pc += 1;
                                }
                            }
                            _ => {
                                self.pc += 1;
                            }
                        }
                    }
                    self.label_stack.pop();
                }
            }
            op::RETURN => {
                // Stop execution
                self.pc = code.len();
            }

            // ── Local variables ──
            op::LOCAL_GET => {
                let idx = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self
                    .locals
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| format!("local {} out of range", idx))?;
                self.push(val);
            }
            op::LOCAL_SET => {
                let idx = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.stack.pop().ok_or("stack underflow")?;
                if idx < self.locals.len() {
                    self.locals[idx] = val;
                } else {
                    return Err(format!("local {} out of range", idx));
                }
            }
            op::LOCAL_TEE => {
                let idx = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.stack.last().cloned().ok_or("stack underflow")?;
                if idx < self.locals.len() {
                    self.locals[idx] = val;
                } else {
                    return Err(format!("local {} out of range", idx));
                }
            }

            // ── Constants ──
            op::I32_CONST => {
                let v = Self::read_i32(code, &mut self.pc)?;
                self.push(WasmValue::I32(v));
            }
            op::I64_CONST => {
                let v = Self::read_i64(code, &mut self.pc)?;
                self.push(WasmValue::I64(v));
            }
            op::F32_CONST => {
                let v = Self::read_f32(code, &mut self.pc)?;
                self.push(WasmValue::F32(v));
            }
            op::F64_CONST => {
                let v = Self::read_f64(code, &mut self.pc)?;
                self.push(WasmValue::F64(v));
            }

            // ── i32 comparison ──
            op::I32_EQZ => {
                let a = self.pop_i32()?;
                self.push(WasmValue::I32(if a == 0 { 1 } else { 0 }));
            }
            op::I32_EQ => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if a == b { 1 } else { 0 }));
            }
            op::I32_NE => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if a != b { 1 } else { 0 }));
            }
            op::I32_LT_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if a < b { 1 } else { 0 }));
            }
            op::I32_LT_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32(if a < b { 1 } else { 0 }));
            }
            op::I32_GT_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if a > b { 1 } else { 0 }));
            }
            op::I32_GT_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32(if a > b { 1 } else { 0 }));
            }
            op::I32_LE_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if a <= b { 1 } else { 0 }));
            }
            op::I32_LE_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32(if a <= b { 1 } else { 0 }));
            }
            op::I32_GE_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if a >= b { 1 } else { 0 }));
            }
            op::I32_GE_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32(if a >= b { 1 } else { 0 }));
            }

            // ── i64 comparison ──
            op::I64_EQZ => {
                let a = self.pop_i64()?;
                self.push(WasmValue::I32(if a == 0 { 1 } else { 0 }));
            }
            op::I64_EQ => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I32(if a == b { 1 } else { 0 }));
            }
            op::I64_NE => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I32(if a != b { 1 } else { 0 }));
            }
            op::I64_LT_S => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I32(if a < b { 1 } else { 0 }));
            }
            op::I64_GT_S => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I32(if a > b { 1 } else { 0 }));
            }
            op::I64_LE_S => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I32(if a <= b { 1 } else { 0 }));
            }
            op::I64_GE_S => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I32(if a >= b { 1 } else { 0 }));
            }

            // ── i32 arithmetic ──
            op::I32_ADD => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a.wrapping_add(b)));
            }
            op::I32_SUB => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a.wrapping_sub(b)));
            }
            op::I32_MUL => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a.wrapping_mul(b)));
            }
            op::I32_DIV_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if b != 0 {
                    a.wrapping_div(b)
                } else {
                    return Err("i32.div_s by zero".into());
                }));
            }
            op::I32_DIV_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32(if b != 0 {
                    (a / b) as i32
                } else {
                    return Err("i32.div_u by zero".into());
                }));
            }
            op::I32_REM_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(if b != 0 {
                    a.wrapping_rem(b)
                } else {
                    return Err("i32.rem_s by zero".into());
                }));
            }
            op::I32_REM_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32((a % b) as i32));
            }
            op::I32_AND => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a & b));
            }
            op::I32_OR => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a | b));
            }
            op::I32_XOR => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a ^ b));
            }
            op::I32_SHL => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()?);
                self.push(WasmValue::I32(a.wrapping_shl(b % 32)));
            }
            op::I32_SHR_S => {
                let (b, a) = (self.pop_i32()?, self.pop_i32()?);
                self.push(WasmValue::I32(a.wrapping_shr((b % 32) as u32)));
            }
            op::I32_SHR_U => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()? as u32);
                self.push(WasmValue::I32(a.wrapping_shr(b % 32) as i32));
            }
            op::I32_ROTL => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()?);
                self.push(WasmValue::I32(a.rotate_left(b % 32)));
            }
            op::I32_ROTR => {
                let (b, a) = (self.pop_i32()? as u32, self.pop_i32()?);
                self.push(WasmValue::I32(a.rotate_right(b % 32)));
            }
            op::I32_CLZ => {
                let a = self.pop_i32()?;
                self.push(WasmValue::I32(a.leading_zeros() as i32));
            }
            op::I32_CTZ => {
                let a = self.pop_i32()?;
                self.push(WasmValue::I32(a.trailing_zeros() as i32));
            }
            op::I32_POPCNT => {
                let a = self.pop_i32()?;
                self.push(WasmValue::I32(a.count_ones() as i32));
            }

            // ── i64 arithmetic ──
            op::I64_ADD => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(a.wrapping_add(b)));
            }
            op::I64_SUB => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(a.wrapping_sub(b)));
            }
            op::I64_MUL => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(a.wrapping_mul(b)));
            }
            op::I64_DIV_S => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(if b != 0 {
                    a.wrapping_div(b)
                } else {
                    return Err("i64.div_s by zero".into());
                }));
            }
            op::I64_AND => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(a & b));
            }
            op::I64_OR => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(a | b));
            }
            op::I64_XOR => {
                let (b, a) = (self.pop_i64()?, self.pop_i64()?);
                self.push(WasmValue::I64(a ^ b));
            }

            // ── f32 arithmetic ──
            op::F32_ABS => {
                let a = self.pop_f32()?;
                self.push(WasmValue::F32(a.abs()));
            }
            op::F32_NEG => {
                let a = self.pop_f32()?;
                self.push(WasmValue::F32(-a));
            }
            op::F32_CEIL => {
                let a = self.pop_f32()?;
                self.push(WasmValue::F32(a.ceil()));
            }
            op::F32_FLOOR => {
                let a = self.pop_f32()?;
                self.push(WasmValue::F32(a.floor()));
            }
            op::F32_SQRT => {
                let a = self.pop_f32()?;
                self.push(WasmValue::F32(a.sqrt()));
            }
            op::F32_ADD => {
                let (b, a) = (self.pop_f32()?, self.pop_f32()?);
                self.push(WasmValue::F32(a + b));
            }
            op::F32_SUB => {
                let (b, a) = (self.pop_f32()?, self.pop_f32()?);
                self.push(WasmValue::F32(a - b));
            }
            op::F32_MUL => {
                let (b, a) = (self.pop_f32()?, self.pop_f32()?);
                self.push(WasmValue::F32(a * b));
            }
            op::F32_DIV => {
                let (b, a) = (self.pop_f32()?, self.pop_f32()?);
                self.push(WasmValue::F32(a / b));
            }
            op::F32_MIN => {
                let (b, a) = (self.pop_f32()?, self.pop_f32()?);
                self.push(WasmValue::F32(a.min(b)));
            }
            op::F32_MAX => {
                let (b, a) = (self.pop_f32()?, self.pop_f32()?);
                self.push(WasmValue::F32(a.max(b)));
            }

            // ── f64 arithmetic ──
            op::F64_ABS => {
                let a = self.pop_f64()?;
                self.push(WasmValue::F64(a.abs()));
            }
            op::F64_NEG => {
                let a = self.pop_f64()?;
                self.push(WasmValue::F64(-a));
            }
            op::F64_CEIL => {
                let a = self.pop_f64()?;
                self.push(WasmValue::F64(a.ceil()));
            }
            op::F64_FLOOR => {
                let a = self.pop_f64()?;
                self.push(WasmValue::F64(a.floor()));
            }
            op::F64_SQRT => {
                let a = self.pop_f64()?;
                self.push(WasmValue::F64(a.sqrt()));
            }
            op::F64_ADD => {
                let (b, a) = (self.pop_f64()?, self.pop_f64()?);
                self.push(WasmValue::F64(a + b));
            }
            op::F64_SUB => {
                let (b, a) = (self.pop_f64()?, self.pop_f64()?);
                self.push(WasmValue::F64(a - b));
            }
            op::F64_MUL => {
                let (b, a) = (self.pop_f64()?, self.pop_f64()?);
                self.push(WasmValue::F64(a * b));
            }
            op::F64_DIV => {
                let (b, a) = (self.pop_f64()?, self.pop_f64()?);
                self.push(WasmValue::F64(a / b));
            }
            op::F64_MIN => {
                let (b, a) = (self.pop_f64()?, self.pop_f64()?);
                self.push(WasmValue::F64(a.min(b)));
            }
            op::F64_MAX => {
                let (b, a) = (self.pop_f64()?, self.pop_f64()?);
                self.push(WasmValue::F64(a.max(b)));
            }

            // ── Conversions ──
            op::I32_WRAP_I64 => {
                let a = self.pop_i64()?;
                self.push(WasmValue::I32(a as i32));
            }
            op::I64_EXTEND_I32_S => {
                let a = self.pop_i32()?;
                self.push(WasmValue::I64(a as i64));
            }
            op::F32_CONVERT_I32_S => {
                let a = self.pop_i32()?;
                self.push(WasmValue::F32(a as f32));
            }
            op::F64_CONVERT_I32_S => {
                let a = self.pop_i32()?;
                self.push(WasmValue::F64(a as f64));
            }
            op::I32_TRUNC_F32_S => {
                let a = self.pop_f32()?;
                self.push(WasmValue::I32(a as i32));
            }

            // ── Memory ──
            op::I32_LOAD => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let addr = self.pop_i32()? as usize + offset;
                let bytes = self.load_bytes(addr, 4)?;
                self.push(WasmValue::I32(i32::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])));
            }
            op::I64_LOAD => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let addr = self.pop_i32()? as usize + offset;
                let bytes = self.load_bytes(addr, 8)?;
                let mut arr = [0u8; 8];
                arr.copy_from_slice(bytes);
                self.push(WasmValue::I64(i64::from_le_bytes(arr)));
            }
            op::F32_LOAD => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let addr = self.pop_i32()? as usize + offset;
                let bytes = self.load_bytes(addr, 4)?;
                self.push(WasmValue::F32(f32::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])));
            }
            op::F64_LOAD => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let addr = self.pop_i32()? as usize + offset;
                let bytes = self.load_bytes(addr, 8)?;
                let mut arr = [0u8; 8];
                arr.copy_from_slice(bytes);
                self.push(WasmValue::F64(f64::from_le_bytes(arr)));
            }
            op::I32_LOAD8_U => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let addr = self.pop_i32()? as usize + offset;
                let bytes = self.load_bytes(addr, 1)?;
                self.push(WasmValue::I32(bytes[0] as i32));
            }
            op::I32_LOAD16_U => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let addr = self.pop_i32()? as usize + offset;
                let bytes = self.load_bytes(addr, 2)?;
                self.push(WasmValue::I32(
                    u16::from_le_bytes([bytes[0], bytes[1]]) as i32
                ));
            }
            op::I32_STORE => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.pop_i32()?;
                let addr = self.pop_i32()? as usize + offset;
                self.store_bytes(addr, &val.to_le_bytes())?;
            }
            op::I64_STORE => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.pop_i64()?;
                let addr = self.pop_i32()? as usize + offset;
                self.store_bytes(addr, &val.to_le_bytes())?;
            }
            op::F32_STORE => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.pop_f32()?;
                let addr = self.pop_i32()? as usize + offset;
                self.store_bytes(addr, &val.to_le_bytes())?;
            }
            op::F64_STORE => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.pop_f64()?;
                let addr = self.pop_i32()? as usize + offset;
                self.store_bytes(addr, &val.to_le_bytes())?;
            }
            op::I32_STORE8 => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.pop_i32()?;
                let addr = self.pop_i32()? as usize + offset;
                self.store_bytes(addr, &[val as u8])?;
            }
            op::I32_STORE16 => {
                let _align = Self::read_u32(code, &mut self.pc)?;
                let offset = Self::read_u32(code, &mut self.pc)? as usize;
                let val = self.pop_i32()?;
                let addr = self.pop_i32()? as usize + offset;
                self.store_bytes(addr, &(val as u16).to_le_bytes())?;
            }

            _ => return Err(format!("unimplemented opcode 0x{:02x}", opcode)),
        }
        Ok(())
    }

    // ── Legacy single-step API ──

    pub fn execute_i32_add(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.execute_instruction(op::I32_ADD, &[])
            .map_err(|e| e.into())
    }

    pub fn write_memory(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.store_bytes(offset, bytes).map_err(|e| e.into())
    }

    pub fn read_memory(
        &self,
        offset: usize,
        len: usize,
    ) -> Result<&[u8], Box<dyn std::error::Error + Send + Sync>> {
        self.load_bytes(offset, len).map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i32_arithmetic_program() {
        let mut wasm = WasmInterpreter::new(1);
        // (3 + 7) * 2 = 20
        let code: Vec<u8> = vec![
            op::I32_CONST,
            3,
            0,
            0,
            0,
            op::I32_CONST,
            7,
            0,
            0,
            0,
            op::I32_ADD,
            op::I32_CONST,
            2,
            0,
            0,
            0,
            op::I32_MUL,
        ];
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::I32(20)));
    }

    #[test]
    fn locals_and_branching() {
        let mut wasm = WasmInterpreter::new(1);
        wasm.init_locals(vec![WasmValue::I32(10), WasmValue::I32(20)]);
        // if local[0] < local[1] then result = local[1] else result = local[0]
        let code: Vec<u8> = vec![
            op::LOCAL_GET,
            0,
            0,
            0,
            0, // push local[0] = 10
            op::LOCAL_GET,
            1,
            0,
            0,
            0,            // push local[1] = 20
            op::I32_LT_S, // 10 < 20 → 1
            op::IF,
            0x40, // if
            op::LOCAL_GET,
            1,
            0,
            0,
            0, // push local[1] = 20
            op::ELSE,
            op::LOCAL_GET,
            0,
            0,
            0,
            0, // push local[0] = 10
            op::END,
        ];
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::I32(20)));
    }

    #[test]
    fn memory_load_store() {
        let mut wasm = WasmInterpreter::new(1);
        let code: Vec<u8> = vec![
            // Store 42 at address 0
            op::I32_CONST,
            0,
            0,
            0,
            0, // address = 0
            op::I32_CONST,
            42,
            0,
            0,
            0, // value = 42
            op::I32_STORE,
            2,
            0,
            0,
            0,
            0,
            0,
            0,
            0, // align=2, offset=0
            // Load from address 0
            op::I32_CONST,
            0,
            0,
            0,
            0, // address = 0
            op::I32_LOAD,
            2,
            0,
            0,
            0,
            0,
            0,
            0,
            0, // align=2, offset=0
        ];
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::I32(42)));
    }

    #[test]
    fn bitwise_operations() {
        let mut wasm = WasmInterpreter::new(1);
        let code: Vec<u8> = vec![
            op::I32_CONST,
            0xFF,
            0,
            0,
            0,
            op::I32_CONST,
            0x0F,
            0,
            0,
            0,
            op::I32_AND,
        ];
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::I32(0x0F)));
    }

    #[test]
    fn f64_sqrt() {
        let mut wasm = WasmInterpreter::new(1);
        let code: Vec<u8> = Vec::new();
        let mut code = code;
        code.push(op::F64_CONST);
        code.extend_from_slice(&144.0f64.to_le_bytes());
        code.push(op::F64_SQRT);
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::F64(12.0)));
    }

    #[test]
    fn i32_clz_ctz_popcnt() {
        let mut wasm = WasmInterpreter::new(1);
        // clz(8) = 28 (0b1000 → 28 leading zeros in 32-bit)
        let code = vec![op::I32_CONST, 8, 0, 0, 0, op::I32_CLZ];
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::I32(28)));
    }

    #[test]
    fn conversion_wrap_extend() {
        let mut wasm = WasmInterpreter::new(1);
        // i64 → i32 wrap: 0x1_0000_0007 → 7
        let code = vec![op::I64_CONST, 7, 0, 0, 0, 0, 0, 0, 0, op::I32_WRAP_I64];
        let result = wasm.execute_program(&code).unwrap();
        assert_eq!(result, Some(WasmValue::I32(7)));
    }
}
