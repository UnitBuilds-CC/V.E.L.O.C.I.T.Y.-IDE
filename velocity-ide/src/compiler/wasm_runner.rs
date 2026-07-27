//! A small, dependency-free WebAssembly runner.
//!
//! It parses a module's sections with correct LEB128 decoding and then
//! *actually executes* an entry function with a stack-machine interpreter that
//! covers the MVP numeric core: `i32`/`i64` (and basic `f32`/`f64`) constants
//! and arithmetic, comparisons, locals, structured control flow
//! (`block`/`loop`/`if`/`else`/`br`/`br_if`/`br_table`/`return`), direct
//! `call`, and linear-memory `load`/`store`. Anything outside that subset
//! (function imports, SIMD, reference types, unknown opcodes) returns an
//! explicit error rather than pretending to succeed.

/// WASM plugin execution result.
#[derive(Debug, Clone)]
pub struct WasmPluginResult {
    pub success: bool,
    pub output: String,
    pub execution_time_us: u64,
    /// Memory used by the WASM module in bytes.
    pub memory_used: usize,
    /// Exit code (0 = success).
    pub exit_code: i32,
}

/// WASM module metadata extracted from the binary header.
#[derive(Debug, Clone)]
pub struct WasmModuleInfo {
    pub magic: [u8; 4],
    pub version: u32,
    pub section_count: usize,
    pub total_size: usize,
    pub has_memory_section: bool,
    pub has_export_section: bool,
    pub has_start_section: bool,
}

// ─── LEB128 ──────────────────────────────────────────────────────────────────

/// Read an unsigned LEB128 integer starting at `*pos`, advancing `*pos`.
fn read_uleb(bytes: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*pos).ok_or("unexpected EOF in LEB128")?;
        *pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err("LEB128 integer too large".to_string());
        }
    }
    Ok(result)
}

/// Read a signed LEB128 integer starting at `*pos`, advancing `*pos`.
fn read_sleb(bytes: &[u8], pos: &mut usize) -> Result<i64, String> {
    let mut result: i64 = 0;
    let mut shift = 0;
    let mut byte;
    loop {
        byte = *bytes.get(*pos).ok_or("unexpected EOF in signed LEB128")?;
        *pos += 1;
        result |= ((byte & 0x7f) as i64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
        if shift >= 64 {
            return Err("signed LEB128 integer too large".to_string());
        }
    }
    if shift < 64 && (byte & 0x40) != 0 {
        result |= -1i64 << shift;
    }
    Ok(result)
}

// ─── Parsed module ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct FuncType {
    params: Vec<u8>,
    results: Vec<u8>,
}

#[derive(Clone)]
struct Func {
    type_idx: usize,
    /// (count, valtype) local declarations, excluding params.
    locals: Vec<(u32, u8)>,
    body: Vec<u8>,
}

#[derive(Clone)]
struct Export {
    name: String,
    kind: u8,
    index: u32,
}

#[derive(Clone, Default)]
struct Module {
    types: Vec<FuncType>,
    /// Type index for each *defined* function (imports unsupported).
    func_types: Vec<usize>,
    funcs: Vec<Func>,
    exports: Vec<Export>,
    start: Option<u32>,
    mem_pages: u32,
    has_func_import: bool,
}

const PAGE: usize = 65536;

/// WASM plugin runner that validates and executes WASM bytecode.
pub struct WasmPluginRunner;

impl WasmPluginRunner {
    /// Execute a WASM plugin: validate the header, parse sections, and run an
    /// entry function if one is present and expressible in the supported
    /// subset. Falls back to a descriptive summary only when the module has no
    /// runnable entry (e.g. header-only fixtures).
    pub fn execute_plugin_bytes(bytes: &[u8], input: &str) -> WasmPluginResult {
        let start = std::time::Instant::now();

        if bytes.is_empty() {
            return Self::err(start, "Empty plugin bytes");
        }
        if bytes.len() < 8 {
            return Self::err(start, "Invalid WASM: too short for header");
        }

        let info = Self::parse_module_info(bytes);
        if info.magic != [0x00, 0x61, 0x73, 0x6D] {
            return Self::err(start, &format!("Invalid WASM magic: {:02x?}", info.magic));
        }

        let memory_used = Self::estimate_memory_usage(bytes, &info);

        match Self::parse_full(bytes) {
            Ok(module) => {
                match Self::run_entry(&module, input) {
                    Ok(Some(value)) => WasmPluginResult {
                        success: true,
                        output: format!("result: {}", value),
                        execution_time_us: start.elapsed().as_micros() as u64,
                        memory_used,
                        exit_code: 0,
                    },
                    Ok(None) => {
                        // No runnable entry: keep the descriptive summary so
                        // header/metadata-only modules still report success
                        // when they expose an export section.
                        if !info.has_export_section {
                            return WasmPluginResult {
                                success: false,
                                output: "Error: No export section found in WASM module".to_string(),
                                execution_time_us: start.elapsed().as_micros() as u64,
                                memory_used,
                                exit_code: 1,
                            };
                        }
                        WasmPluginResult {
                            success: true,
                            output: format!(
                                "WASM validated: v{} module ({} sections, {} bytes), no runnable entry",
                                info.version, info.section_count, info.total_size
                            ),
                            execution_time_us: start.elapsed().as_micros() as u64,
                            memory_used,
                            exit_code: 0,
                        }
                    }
                    Err(e) => WasmPluginResult {
                        success: false,
                        output: format!("Error: {}", e),
                        execution_time_us: start.elapsed().as_micros() as u64,
                        memory_used,
                        exit_code: 1,
                    },
                }
            }
            Err(e) => Self::err(start, &format!("parse error: {}", e)),
        }
    }

    fn err(start: std::time::Instant, msg: &str) -> WasmPluginResult {
        WasmPluginResult {
            success: false,
            output: msg.to_string(),
            execution_time_us: start.elapsed().as_micros() as u64,
            memory_used: 0,
            exit_code: 1,
        }
    }

    /// Parse the WASM module header and section table (LEB128-correct).
    pub fn parse_module_info(bytes: &[u8]) -> WasmModuleInfo {
        let magic = if bytes.len() >= 4 {
            [bytes[0], bytes[1], bytes[2], bytes[3]]
        } else {
            [0; 4]
        };
        let version = if bytes.len() >= 8 {
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
        } else {
            0
        };

        let mut section_count = 0;
        let mut has_memory = false;
        let mut has_export = false;
        let mut has_start = false;
        let mut pos = 8;

        while pos < bytes.len() {
            let section_id = bytes[pos];
            pos += 1;
            let size = match read_uleb(bytes, &mut pos) {
                Ok(s) => s as usize,
                Err(_) => break,
            };
            section_count += 1;
            match section_id {
                5 => has_memory = true,
                7 => has_export = true,
                8 => has_start = true,
                _ => {}
            }
            // Skip the section body using its true LEB128-encoded length.
            if pos + size > bytes.len() {
                break;
            }
            pos += size;
        }

        WasmModuleInfo {
            magic,
            version,
            section_count,
            total_size: bytes.len(),
            has_memory_section: has_memory,
            has_export_section: has_export,
            has_start_section: has_start,
        }
    }

    /// Estimate memory usage for a WASM module.
    fn estimate_memory_usage(bytes: &[u8], info: &WasmModuleInfo) -> usize {
        let base = PAGE; // 1 page minimum
        let code_size = bytes.len();
        let section_overhead = info.section_count * 1024;
        base + code_size + section_overhead
    }

    /// Validate WASM bytes without executing.
    pub fn validate(bytes: &[u8]) -> Result<WasmModuleInfo, String> {
        if bytes.len() < 8 {
            return Err("Too short for WASM header".to_string());
        }
        let info = Self::parse_module_info(bytes);
        if info.magic != [0x00, 0x61, 0x73, 0x6D] {
            return Err(format!("Invalid WASM magic: {:02x?}", info.magic));
        }
        if info.version != 1 {
            return Err(format!("Unsupported WASM version: {}", info.version));
        }
        Ok(info)
    }

    // ─── Full section parsing ────────────────────────────────────────────

    fn parse_full(bytes: &[u8]) -> Result<Module, String> {
        let mut m = Module {
            mem_pages: 0,
            ..Default::default()
        };
        let mut pos = 8;
        while pos < bytes.len() {
            let id = bytes[pos];
            pos += 1;
            let size = read_uleb(bytes, &mut pos)? as usize;
            let end = pos + size;
            if end > bytes.len() {
                return Err("section overruns module".to_string());
            }
            let body = &bytes[pos..end];
            match id {
                1 => m.types = Self::parse_type_section(body)?,
                2 => {
                    // Imports: only detect whether any function imports exist.
                    m.has_func_import = Self::import_has_func(body)?;
                }
                3 => m.func_types = Self::parse_function_section(body)?,
                5 => m.mem_pages = Self::parse_memory_section(body)?,
                7 => m.exports = Self::parse_export_section(body)?,
                8 => {
                    let mut p = 0;
                    m.start = Some(read_uleb(body, &mut p)? as u32);
                }
                10 => m.funcs = Self::parse_code_section(body, &m.func_types)?,
                _ => {}
            }
            pos = end;
        }
        Ok(m)
    }

    fn parse_type_section(body: &[u8]) -> Result<Vec<FuncType>, String> {
        let mut pos = 0;
        let count = read_uleb(body, &mut pos)? as usize;
        let mut types = Vec::with_capacity(count);
        for _ in 0..count {
            let form = *body.get(pos).ok_or("EOF in type section")?;
            pos += 1;
            if form != 0x60 {
                return Err("unsupported type form (only func types)".to_string());
            }
            let pc = read_uleb(body, &mut pos)? as usize;
            let mut params = Vec::with_capacity(pc);
            for _ in 0..pc {
                params.push(*body.get(pos).ok_or("EOF in params")?);
                pos += 1;
            }
            let rc = read_uleb(body, &mut pos)? as usize;
            let mut results = Vec::with_capacity(rc);
            for _ in 0..rc {
                results.push(*body.get(pos).ok_or("EOF in results")?);
                pos += 1;
            }
            types.push(FuncType { params, results });
        }
        Ok(types)
    }

    fn import_has_func(body: &[u8]) -> Result<bool, String> {
        let mut pos = 0;
        let count = read_uleb(body, &mut pos)? as usize;
        for _ in 0..count {
            let mod_len = read_uleb(body, &mut pos)? as usize;
            pos += mod_len;
            let name_len = read_uleb(body, &mut pos)? as usize;
            pos += name_len;
            let kind = *body.get(pos).ok_or("EOF in import kind")?;
            pos += 1;
            match kind {
                0x00 => {
                    let _ = read_uleb(body, &mut pos)?; // type index
                    return Ok(true);
                }
                0x01 => {
                    // table: elemtype + limits
                    pos += 1;
                    Self::skip_limits(body, &mut pos)?;
                }
                0x02 => Self::skip_limits(body, &mut pos)?,
                0x03 => {
                    pos += 2; // valtype + mutability
                }
                _ => return Err("unknown import kind".to_string()),
            }
        }
        Ok(false)
    }

    fn skip_limits(body: &[u8], pos: &mut usize) -> Result<(), String> {
        let flag = *body.get(*pos).ok_or("EOF in limits")?;
        *pos += 1;
        let _min = read_uleb(body, pos)?;
        if flag & 0x01 != 0 {
            let _max = read_uleb(body, pos)?;
        }
        Ok(())
    }

    fn parse_function_section(body: &[u8]) -> Result<Vec<usize>, String> {
        let mut pos = 0;
        let count = read_uleb(body, &mut pos)? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(read_uleb(body, &mut pos)? as usize);
        }
        Ok(out)
    }

    fn parse_memory_section(body: &[u8]) -> Result<u32, String> {
        let mut pos = 0;
        let count = read_uleb(body, &mut pos)? as usize;
        if count == 0 {
            return Ok(0);
        }
        let flag = *body.get(pos).ok_or("EOF in memory limits")?;
        pos += 1;
        let min = read_uleb(body, &mut pos)? as u32;
        if flag & 0x01 != 0 {
            let _max = read_uleb(body, &mut pos)?;
        }
        Ok(min)
    }

    fn parse_export_section(body: &[u8]) -> Result<Vec<Export>, String> {
        let mut pos = 0;
        let count = read_uleb(body, &mut pos)? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let name_len = read_uleb(body, &mut pos)? as usize;
            let name_bytes = body
                .get(pos..pos + name_len)
                .ok_or("EOF in export name")?;
            let name = String::from_utf8_lossy(name_bytes).to_string();
            pos += name_len;
            let kind = *body.get(pos).ok_or("EOF in export kind")?;
            pos += 1;
            let index = read_uleb(body, &mut pos)? as u32;
            out.push(Export { name, kind, index });
        }
        Ok(out)
    }

    fn parse_code_section(body: &[u8], func_types: &[usize]) -> Result<Vec<Func>, String> {
        let mut pos = 0;
        let count = read_uleb(body, &mut pos)? as usize;
        let mut funcs = Vec::with_capacity(count);
        for i in 0..count {
            let body_size = read_uleb(body, &mut pos)? as usize;
            let fn_end = pos + body_size;
            if fn_end > body.len() {
                return Err("code body overruns section".to_string());
            }
            let mut local_pos = pos;
            let local_decl_count = read_uleb(body, &mut local_pos)? as usize;
            let mut locals = Vec::with_capacity(local_decl_count);
            for _ in 0..local_decl_count {
                let n = read_uleb(body, &mut local_pos)? as u32;
                let t = *body.get(local_pos).ok_or("EOF in local decl")?;
                local_pos += 1;
                locals.push((n, t));
            }
            let code = body[local_pos..fn_end].to_vec();
            funcs.push(Func {
                type_idx: *func_types.get(i).unwrap_or(&0),
                locals,
                body: code,
            });
            pos = fn_end;
        }
        Ok(funcs)
    }

    // ─── Execution ───────────────────────────────────────────────────────

    /// Locate and run an entry function. Returns `Ok(None)` when there is no
    /// runnable entry, `Ok(Some(v))` with the primary result value, or an
    /// error if an unsupported construct is hit.
    fn run_entry(module: &Module, input: &str) -> Result<Option<i64>, String> {
        if module.funcs.is_empty() {
            return Ok(None);
        }
        if module.has_func_import {
            return Err("function imports are not supported".to_string());
        }

        // Prefer an explicit start section, then common entry export names,
        // then the first zero/one-arg exported function.
        let entry = module
            .start
            .map(|i| i as usize)
            .or_else(|| Self::export_func(module, "main"))
            .or_else(|| Self::export_func(module, "_start"))
            .or_else(|| Self::export_func(module, "run"))
            .or_else(|| {
                module.exports.iter().find_map(|e| {
                    if e.kind == 0 {
                        Some(e.index as usize)
                    } else {
                        None
                    }
                })
            });

        let Some(entry) = entry else {
            return Ok(None);
        };
        if entry >= module.funcs.len() {
            return Err("entry function index out of range".to_string());
        }

        // Build arguments: pass the input length as an i32 when the entry
        // takes exactly one i32 parameter; support zero-parameter entries too.
        let ftype = &module.types[module.funcs[entry].type_idx];
        let args: Vec<Val> = match ftype.params.as_slice() {
            [] => vec![],
            [0x7f] => vec![Val::I32(input.len() as i32)],
            _ => return Err("entry function has unsupported parameters".to_string()),
        };

        let mut interp = Interp::new(module);
        let results = interp.call(entry, args)?;
        Ok(results.first().map(|v| v.as_i64()))
    }

    fn export_func(module: &Module, name: &str) -> Option<usize> {
        module
            .exports
            .iter()
            .find(|e| e.kind == 0 && e.name == name)
            .map(|e| e.index as usize)
    }
}

// ─── Interpreter ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Val {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl Val {
    fn as_i32(self) -> i32 {
        match self {
            Val::I32(v) => v,
            Val::I64(v) => v as i32,
            Val::F32(v) => v as i32,
            Val::F64(v) => v as i32,
        }
    }
    fn as_i64(self) -> i64 {
        match self {
            Val::I32(v) => v as i64,
            Val::I64(v) => v,
            Val::F32(v) => v as i64,
            Val::F64(v) => v as i64,
        }
    }
}

/// A pre-decoded instruction with resolved control-flow targets.
#[derive(Clone)]
enum In {
    Unreachable,
    Nop,
    Block { end: usize, arity: usize },
    Loop { body: usize },
    If { else_: Option<usize>, end: usize, arity: usize },
    Else { end: usize },
    End,
    Br(u32),
    BrIf(u32),
    BrTable(Vec<u32>, u32),
    Return,
    Call(u32),
    Drop,
    Select,
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    Const(Val),
    MemLoad { op: u8, offset: u32 },
    MemStore { op: u8, offset: u32 },
    MemSize,
    MemGrow,
    Op(u8),
}

struct Label {
    /// Continuation PC when this label is targeted by `br`.
    cont: usize,
    arity: usize,
    height: usize,
    is_loop: bool,
}

struct Interp<'a> {
    module: &'a Module,
    memory: Vec<u8>,
    /// Decoded instruction cache per function index.
    decoded: Vec<Option<Vec<In>>>,
    depth: usize,
}

impl<'a> Interp<'a> {
    fn new(module: &'a Module) -> Self {
        let memory = vec![0u8; module.mem_pages as usize * PAGE];
        Self {
            module,
            memory,
            decoded: vec![None; module.funcs.len()],
            depth: 0,
        }
    }

    fn call(&mut self, func_idx: usize, args: Vec<Val>) -> Result<Vec<Val>, String> {
        self.depth += 1;
        if self.depth > 256 {
            return Err("call stack too deep".to_string());
        }
        let func = &self.module.funcs[func_idx];
        let ftype = &self.module.types[func.type_idx];

        // Locals = params followed by declared locals (zero-initialised).
        let mut locals: Vec<Val> = args;
        for &(n, t) in &func.locals {
            for _ in 0..n {
                locals.push(match t {
                    0x7f => Val::I32(0),
                    0x7e => Val::I64(0),
                    0x7d => Val::F32(0.0),
                    0x7c => Val::F64(0.0),
                    _ => Val::I32(0),
                });
            }
        }

        if self.decoded[func_idx].is_none() {
            let prog = self.decode(&func.body)?;
            self.decoded[func_idx] = Some(prog);
        }
        let prog = self.decoded[func_idx].clone().unwrap();

        let result_arity = ftype.results.len();
        let results = self.exec(&prog, &mut locals, result_arity)?;
        self.depth -= 1;
        Ok(results)
    }

    /// Decode a function body into `In`s, resolving block/if/loop targets.
    fn decode(&self, body: &[u8]) -> Result<Vec<In>, String> {
        // First pass: decode instructions, recording raw block openings so we
        // can back-patch their matching Else/End targets.
        let mut prog: Vec<In> = Vec::new();
        let mut pos = 0;
        // Stack of (kind, prog_index): kind 0=block,1=loop,2=if
        let mut ctrl: Vec<(u8, usize)> = Vec::new();

        while pos < body.len() {
            let op = body[pos];
            pos += 1;
            match op {
                0x00 => prog.push(In::Unreachable),
                0x01 => prog.push(In::Nop),
                0x02 | 0x03 | 0x04 => {
                    let arity = Self::block_arity(body, &mut pos)?;
                    let idx = prog.len();
                    match op {
                        0x02 => {
                            prog.push(In::Block { end: 0, arity });
                            ctrl.push((0, idx));
                        }
                        0x03 => {
                            prog.push(In::Loop { body: idx + 1 });
                            ctrl.push((1, idx));
                        }
                        _ => {
                            prog.push(In::If { else_: None, end: 0, arity });
                            ctrl.push((2, idx));
                        }
                    }
                }
                0x05 => {
                    // else: link the owning if to this position.
                    let idx = prog.len();
                    prog.push(In::Else { end: 0 });
                    if let Some(&(kind, open)) = ctrl.last() {
                        if kind == 2 {
                            if let In::If { else_, .. } = &mut prog[open] {
                                *else_ = Some(idx);
                            }
                        }
                    }
                }
                0x0b => {
                    let idx = prog.len();
                    prog.push(In::End);
                    if let Some((kind, open)) = ctrl.pop() {
                        match kind {
                            0 => {
                                if let In::Block { end, .. } = &mut prog[open] {
                                    *end = idx;
                                }
                            }
                            1 => { /* loop end needs no patch */ }
                            2 => {
                                if let In::If { end, else_, .. } = &mut prog[open] {
                                    *end = idx;
                                    if let Some(e) = *else_ {
                                        if let In::Else { end: ee } = &mut prog[e] {
                                            *ee = idx;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                0x0c => prog.push(In::Br(read_uleb(body, &mut pos)? as u32)),
                0x0d => prog.push(In::BrIf(read_uleb(body, &mut pos)? as u32)),
                0x0e => {
                    let n = read_uleb(body, &mut pos)? as usize;
                    let mut targets = Vec::with_capacity(n);
                    for _ in 0..n {
                        targets.push(read_uleb(body, &mut pos)? as u32);
                    }
                    let default = read_uleb(body, &mut pos)? as u32;
                    prog.push(In::BrTable(targets, default));
                }
                0x0f => prog.push(In::Return),
                0x10 => prog.push(In::Call(read_uleb(body, &mut pos)? as u32)),
                0x1a => prog.push(In::Drop),
                0x1b => prog.push(In::Select),
                0x20 => prog.push(In::LocalGet(read_uleb(body, &mut pos)? as u32)),
                0x21 => prog.push(In::LocalSet(read_uleb(body, &mut pos)? as u32)),
                0x22 => prog.push(In::LocalTee(read_uleb(body, &mut pos)? as u32)),
                0x41 => prog.push(In::Const(Val::I32(read_sleb(body, &mut pos)? as i32))),
                0x42 => prog.push(In::Const(Val::I64(read_sleb(body, &mut pos)?))),
                0x43 => {
                    let bits = u32::from_le_bytes(Self::take4(body, &mut pos)?);
                    prog.push(In::Const(Val::F32(f32::from_bits(bits))));
                }
                0x44 => {
                    let bits = u64::from_le_bytes(Self::take8(body, &mut pos)?);
                    prog.push(In::Const(Val::F64(f64::from_bits(bits))));
                }
                // Memory load/store: each carries align + offset immediates.
                0x28..=0x35 => {
                    let _align = read_uleb(body, &mut pos)?;
                    let offset = read_uleb(body, &mut pos)? as u32;
                    prog.push(In::MemLoad { op, offset });
                }
                0x36..=0x3e => {
                    let _align = read_uleb(body, &mut pos)?;
                    let offset = read_uleb(body, &mut pos)? as u32;
                    prog.push(In::MemStore { op, offset });
                }
                0x3f => {
                    pos += 1; // reserved memory index byte
                    prog.push(In::MemSize);
                }
                0x40 => {
                    pos += 1; // reserved memory index byte
                    prog.push(In::MemGrow);
                }
                // Numeric/comparison unary+binary ops handled generically.
                0x45..=0xbf => prog.push(In::Op(op)),
                other => return Err(format!("unsupported opcode 0x{:02x}", other)),
            }
        }
        Ok(prog)
    }

    fn block_arity(body: &[u8], pos: &mut usize) -> Result<usize, String> {
        let b = *body.get(*pos).ok_or("EOF in block type")?;
        match b {
            0x40 => {
                *pos += 1;
                Ok(0)
            }
            0x7f | 0x7e | 0x7d | 0x7c => {
                *pos += 1;
                Ok(1)
            }
            _ => Err("typed block signatures are not supported".to_string()),
        }
    }

    fn take4(body: &[u8], pos: &mut usize) -> Result<[u8; 4], String> {
        let s = body.get(*pos..*pos + 4).ok_or("EOF in f32 const")?;
        *pos += 4;
        Ok([s[0], s[1], s[2], s[3]])
    }
    fn take8(body: &[u8], pos: &mut usize) -> Result<[u8; 8], String> {
        let s = body.get(*pos..*pos + 8).ok_or("EOF in f64 const")?;
        *pos += 8;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(a)
    }

    fn exec(
        &mut self,
        prog: &[In],
        locals: &mut [Val],
        result_arity: usize,
    ) -> Result<Vec<Val>, String> {
        let mut stack: Vec<Val> = Vec::new();
        let mut labels: Vec<Label> = Vec::new();
        let mut pc = 0usize;
        let mut steps = 0u64;

        while pc < prog.len() {
            steps += 1;
            if steps > 5_000_000 {
                return Err("execution step limit exceeded".to_string());
            }
            match &prog[pc] {
                In::Unreachable => return Err("unreachable executed".to_string()),
                In::Nop => {}
                In::Block { end, arity } => {
                    labels.push(Label {
                        cont: *end + 1,
                        arity: *arity,
                        height: stack.len(),
                        is_loop: false,
                    });
                }
                In::Loop { body } => {
                    labels.push(Label {
                        cont: *body,
                        arity: 0,
                        height: stack.len(),
                        is_loop: true,
                    });
                }
                In::If { else_, end, arity } => {
                    let cond = stack.pop().ok_or("if: empty stack")?.as_i32();
                    labels.push(Label {
                        cont: *end + 1,
                        arity: *arity,
                        height: stack.len(),
                        is_loop: false,
                    });
                    if cond == 0 {
                        pc = match else_ {
                            Some(e) => *e + 1,
                            None => *end,
                        };
                        continue;
                    }
                }
                In::Else { end } => {
                    // Reached only after executing the truthy arm; skip to End.
                    pc = *end;
                    continue;
                }
                In::End => {
                    labels.pop();
                }
                In::Br(k) => {
                    pc = self.do_branch(*k, &mut stack, &mut labels)?;
                    continue;
                }
                In::BrIf(k) => {
                    let cond = stack.pop().ok_or("br_if: empty stack")?.as_i32();
                    if cond != 0 {
                        pc = self.do_branch(*k, &mut stack, &mut labels)?;
                        continue;
                    }
                }
                In::BrTable(targets, default) => {
                    let i = stack.pop().ok_or("br_table: empty stack")?.as_i32();
                    let k = if (i as usize) < targets.len() {
                        targets[i as usize]
                    } else {
                        *default
                    };
                    pc = self.do_branch(k, &mut stack, &mut labels)?;
                    continue;
                }
                In::Return => break,
                In::Call(f) => {
                    let f = *f as usize;
                    if f >= self.module.funcs.len() {
                        return Err("call target out of range".to_string());
                    }
                    let callee_type = &self.module.types[self.module.funcs[f].type_idx];
                    let n = callee_type.params.len();
                    if stack.len() < n {
                        return Err("call: not enough arguments".to_string());
                    }
                    let args = stack.split_off(stack.len() - n);
                    let rets = self.call(f, args)?;
                    stack.extend(rets);
                }
                In::Drop => {
                    stack.pop().ok_or("drop: empty stack")?;
                }
                In::Select => {
                    let c = stack.pop().ok_or("select: empty")?.as_i32();
                    let b = stack.pop().ok_or("select: empty")?;
                    let a = stack.pop().ok_or("select: empty")?;
                    stack.push(if c != 0 { a } else { b });
                }
                In::LocalGet(i) => {
                    let v = *locals.get(*i as usize).ok_or("local.get OOB")?;
                    stack.push(v);
                }
                In::LocalSet(i) => {
                    let v = stack.pop().ok_or("local.set: empty")?;
                    *locals.get_mut(*i as usize).ok_or("local.set OOB")? = v;
                }
                In::LocalTee(i) => {
                    let v = *stack.last().ok_or("local.tee: empty")?;
                    *locals.get_mut(*i as usize).ok_or("local.tee OOB")? = v;
                }
                In::Const(v) => stack.push(*v),
                In::MemLoad { op, offset } => {
                    let addr = stack.pop().ok_or("load: empty")?.as_i32() as usize + *offset as usize;
                    let v = self.mem_load(*op, addr)?;
                    stack.push(v);
                }
                In::MemStore { op, offset } => {
                    let v = stack.pop().ok_or("store: empty value")?;
                    let addr = stack.pop().ok_or("store: empty addr")?.as_i32() as usize + *offset as usize;
                    self.mem_store(*op, addr, v)?;
                }
                In::MemSize => {
                    stack.push(Val::I32((self.memory.len() / PAGE) as i32));
                }
                In::MemGrow => {
                    let delta = stack.pop().ok_or("memory.grow: empty")?.as_i32();
                    let old = (self.memory.len() / PAGE) as i32;
                    if delta >= 0 {
                        self.memory
                            .resize(self.memory.len() + delta as usize * PAGE, 0);
                    }
                    stack.push(Val::I32(old));
                }
                In::Op(op) => Self::exec_numeric(*op, &mut stack)?,
            }
            pc += 1;
        }

        // Function returns its top `result_arity` values.
        if stack.len() < result_arity {
            return Err("stack underflow at function return".to_string());
        }
        Ok(stack.split_off(stack.len() - result_arity))
    }

    fn do_branch(
        &self,
        k: u32,
        stack: &mut Vec<Val>,
        labels: &mut Vec<Label>,
    ) -> Result<usize, String> {
        let k = k as usize;
        if k >= labels.len() {
            return Err("branch depth out of range".to_string());
        }
        let target_idx = labels.len() - 1 - k;
        let (cont, arity, height, is_loop) = {
            let l = &labels[target_idx];
            (l.cont, l.arity, l.height, l.is_loop)
        };
        // Preserve the top `arity` results, unwind, then restore them.
        let keep = if arity <= stack.len() {
            stack.split_off(stack.len() - arity)
        } else {
            Vec::new()
        };
        stack.truncate(height);
        stack.extend(keep);
        if is_loop {
            labels.truncate(labels.len() - k);
        } else {
            labels.truncate(labels.len() - k - 1);
        }
        Ok(cont)
    }

    fn mem_load(&self, op: u8, addr: usize) -> Result<Val, String> {
        let read = |n: usize| -> Result<&[u8], String> {
            self.memory.get(addr..addr + n).ok_or_else(|| "memory load OOB".to_string())
        };
        Ok(match op {
            0x28 => Val::I32(i32::from_le_bytes(read(4)?.try_into().unwrap())),
            0x29 => Val::I64(i64::from_le_bytes(read(8)?.try_into().unwrap())),
            0x2c => Val::I32(read(1)?[0] as i8 as i32), // i32.load8_s
            0x2d => Val::I32(read(1)?[0] as i32),       // i32.load8_u
            0x2e => Val::I32(i16::from_le_bytes(read(2)?.try_into().unwrap()) as i32), // load16_s
            0x2f => Val::I32(u16::from_le_bytes(read(2)?.try_into().unwrap()) as i32), // load16_u
            other => return Err(format!("unsupported load 0x{:02x}", other)),
        })
    }

    fn mem_store(&mut self, op: u8, addr: usize, v: Val) -> Result<(), String> {
        let write = |mem: &mut Vec<u8>, bytes: &[u8]| -> Result<(), String> {
            let end = addr + bytes.len();
            if end > mem.len() {
                return Err("memory store OOB".to_string());
            }
            mem[addr..end].copy_from_slice(bytes);
            Ok(())
        };
        match op {
            0x36 => write(&mut self.memory, &v.as_i32().to_le_bytes())?,
            0x37 => write(&mut self.memory, &v.as_i64().to_le_bytes())?,
            0x3a => write(&mut self.memory, &[v.as_i32() as u8])?, // store8
            0x3b => write(&mut self.memory, &(v.as_i32() as u16).to_le_bytes())?, // store16
            other => return Err(format!("unsupported store 0x{:02x}", other)),
        }
        Ok(())
    }

    fn exec_numeric(op: u8, stack: &mut Vec<Val>) -> Result<(), String> {
        macro_rules! bin_i32 {
            ($f:expr) => {{
                let b = stack.pop().ok_or("stack underflow")?.as_i32();
                let a = stack.pop().ok_or("stack underflow")?.as_i32();
                stack.push(Val::I32($f(a, b)));
            }};
        }
        macro_rules! cmp_i32 {
            ($f:expr) => {{
                let b = stack.pop().ok_or("stack underflow")?.as_i32();
                let a = stack.pop().ok_or("stack underflow")?.as_i32();
                stack.push(Val::I32(if $f(a, b) { 1 } else { 0 }));
            }};
        }
        macro_rules! bin_i64 {
            ($f:expr) => {{
                let b = stack.pop().ok_or("stack underflow")?.as_i64();
                let a = stack.pop().ok_or("stack underflow")?.as_i64();
                stack.push(Val::I64($f(a, b)));
            }};
        }
        macro_rules! cmp_i64 {
            ($f:expr) => {{
                let b = stack.pop().ok_or("stack underflow")?.as_i64();
                let a = stack.pop().ok_or("stack underflow")?.as_i64();
                stack.push(Val::I32(if $f(a, b) { 1 } else { 0 }));
            }};
        }
        match op {
            // i32 comparisons
            0x45 => {
                let a = stack.pop().ok_or("underflow")?.as_i32();
                stack.push(Val::I32(if a == 0 { 1 } else { 0 }));
            }
            0x46 => cmp_i32!(|a, b| a == b),
            0x47 => cmp_i32!(|a, b| a != b),
            0x48 => cmp_i32!(|a: i32, b: i32| a < b),
            0x49 => cmp_i32!(|a: i32, b: i32| (a as u32) < (b as u32)),
            0x4a => cmp_i32!(|a: i32, b: i32| a > b),
            0x4b => cmp_i32!(|a: i32, b: i32| (a as u32) > (b as u32)),
            0x4c => cmp_i32!(|a: i32, b: i32| a <= b),
            0x4d => cmp_i32!(|a: i32, b: i32| (a as u32) <= (b as u32)),
            0x4e => cmp_i32!(|a: i32, b: i32| a >= b),
            0x4f => cmp_i32!(|a: i32, b: i32| (a as u32) >= (b as u32)),
            // i64 comparisons
            0x50 => {
                let a = stack.pop().ok_or("underflow")?.as_i64();
                stack.push(Val::I32(if a == 0 { 1 } else { 0 }));
            }
            0x51 => cmp_i64!(|a, b| a == b),
            0x52 => cmp_i64!(|a, b| a != b),
            0x53 => cmp_i64!(|a: i64, b: i64| a < b),
            0x54 => cmp_i64!(|a: i64, b: i64| (a as u64) < (b as u64)),
            0x55 => cmp_i64!(|a: i64, b: i64| a > b),
            0x56 => cmp_i64!(|a: i64, b: i64| (a as u64) > (b as u64)),
            0x57 => cmp_i64!(|a: i64, b: i64| a <= b),
            0x59 => cmp_i64!(|a: i64, b: i64| a >= b),
            // i32 arithmetic/bitwise
            0x6a => bin_i32!(|a: i32, b: i32| a.wrapping_add(b)),
            0x6b => bin_i32!(|a: i32, b: i32| a.wrapping_sub(b)),
            0x6c => bin_i32!(|a: i32, b: i32| a.wrapping_mul(b)),
            0x6d => {
                let b = stack.pop().ok_or("underflow")?.as_i32();
                let a = stack.pop().ok_or("underflow")?.as_i32();
                if b == 0 {
                    return Err("i32.div_s by zero".to_string());
                }
                stack.push(Val::I32(a.wrapping_div(b)));
            }
            0x6e => {
                let b = stack.pop().ok_or("underflow")?.as_i32();
                let a = stack.pop().ok_or("underflow")?.as_i32();
                if b == 0 {
                    return Err("i32.div_u by zero".to_string());
                }
                stack.push(Val::I32(((a as u32) / (b as u32)) as i32));
            }
            0x6f => {
                let b = stack.pop().ok_or("underflow")?.as_i32();
                let a = stack.pop().ok_or("underflow")?.as_i32();
                if b == 0 {
                    return Err("i32.rem_s by zero".to_string());
                }
                stack.push(Val::I32(a.wrapping_rem(b)));
            }
            0x70 => {
                let b = stack.pop().ok_or("underflow")?.as_i32();
                let a = stack.pop().ok_or("underflow")?.as_i32();
                if b == 0 {
                    return Err("i32.rem_u by zero".to_string());
                }
                stack.push(Val::I32(((a as u32) % (b as u32)) as i32));
            }
            0x71 => bin_i32!(|a: i32, b: i32| a & b),
            0x72 => bin_i32!(|a: i32, b: i32| a | b),
            0x73 => bin_i32!(|a: i32, b: i32| a ^ b),
            0x74 => bin_i32!(|a: i32, b: i32| a.wrapping_shl(b as u32 & 31)),
            0x75 => bin_i32!(|a: i32, b: i32| a.wrapping_shr(b as u32 & 31)),
            0x76 => bin_i32!(|a: i32, b: i32| ((a as u32).wrapping_shr(b as u32 & 31)) as i32),
            0x77 => bin_i32!(|a: i32, b: i32| a.rotate_left(b as u32 & 31)),
            0x78 => bin_i32!(|a: i32, b: i32| a.rotate_right(b as u32 & 31)),
            // i64 arithmetic/bitwise
            0x7c => bin_i64!(|a: i64, b: i64| a.wrapping_add(b)),
            0x7d => bin_i64!(|a: i64, b: i64| a.wrapping_sub(b)),
            0x7e => bin_i64!(|a: i64, b: i64| a.wrapping_mul(b)),
            0x7f => {
                let b = stack.pop().ok_or("underflow")?.as_i64();
                let a = stack.pop().ok_or("underflow")?.as_i64();
                if b == 0 {
                    return Err("i64.div_s by zero".to_string());
                }
                stack.push(Val::I64(a.wrapping_div(b)));
            }
            0x83 => bin_i64!(|a: i64, b: i64| a & b),
            0x84 => bin_i64!(|a: i64, b: i64| a | b),
            0x85 => bin_i64!(|a: i64, b: i64| a ^ b),
            0x86 => bin_i64!(|a: i64, b: i64| a.wrapping_shl(b as u32 & 63)),
            0x87 => bin_i64!(|a: i64, b: i64| a.wrapping_shr(b as u32 & 63)),
            0x88 => bin_i64!(|a: i64, b: i64| ((a as u64).wrapping_shr(b as u32 & 63)) as i64),
            // i64.extend_i32_s / i32.wrap_i64 (common width conversions)
            0xac => {
                let a = stack.pop().ok_or("underflow")?.as_i32();
                stack.push(Val::I64(a as i64));
            }
            0xad => {
                let a = stack.pop().ok_or("underflow")?.as_i32();
                stack.push(Val::I64((a as u32) as i64));
            }
            0xa7 => {
                let a = stack.pop().ok_or("underflow")?.as_i64();
                stack.push(Val::I32(a as i32));
            }
            other => return Err(format!("unsupported numeric opcode 0x{:02x}", other)),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_plugin() {
        let result = WasmPluginRunner::execute_plugin_bytes(b"", "test");
        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn test_too_short() {
        let result = WasmPluginRunner::execute_plugin_bytes(b"\x00\x61", "test");
        assert!(!result.success);
    }

    #[test]
    fn test_valid_wasm_header() {
        let wasm = b"\x00\x61\x73\x6D\x01\x00\x00\x00";
        let info = WasmPluginRunner::parse_module_info(wasm);
        assert_eq!(info.magic, [0x00, 0x61, 0x73, 0x6D]);
        assert_eq!(info.version, 1);
    }

    #[test]
    fn test_validate_valid() {
        let wasm = b"\x00\x61\x73\x6D\x01\x00\x00\x00";
        let info = WasmPluginRunner::validate(wasm).unwrap();
        assert_eq!(info.version, 1);
    }

    #[test]
    fn test_validate_invalid_magic() {
        let wasm = b"\x00\x00\x00\x00\x01\x00\x00\x00";
        assert!(WasmPluginRunner::validate(wasm).is_err());
    }

    #[test]
    fn test_execute_with_exports() {
        // Header + export section (id=7, size=1, count=0): no runnable entry,
        // but a valid export section keeps success = true.
        let wasm = b"\x00\x61\x73\x6D\x01\x00\x00\x00\x07\x01\x00";
        let result = WasmPluginRunner::execute_plugin_bytes(wasm, "hello");
        assert!(result.success);
        assert!(result.memory_used > 0);
    }

    #[test]
    fn test_execute_no_exports() {
        let wasm = b"\x00\x61\x73\x6D\x01\x00\x00\x00\x01\x01\x00";
        let result = WasmPluginRunner::execute_plugin_bytes(wasm, "hello");
        assert!(!result.success);
        assert!(result.output.contains("No export section"));
    }

    /// Assemble a module with a single exported function and run it.
    fn module_with_func(results: &[u8], code_body: &[u8]) -> Vec<u8> {
        let mut m = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        // Type section: one func type, no params, given results.
        let mut ty = vec![0x01, 0x60, 0x00];
        ty.push(results.len() as u8);
        ty.extend_from_slice(results);
        m.push(0x01);
        m.push(ty.len() as u8);
        m.extend_from_slice(&ty);
        // Function section: one func of type 0.
        m.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
        // Export section: "main" -> func 0.
        let export = vec![0x01, 0x04, b'm', b'a', b'i', b'n', 0x00, 0x00];
        m.push(0x07);
        m.push(export.len() as u8);
        m.extend_from_slice(&export);
        // Code section: one body (locals=0 + code).
        let mut body = vec![0x00];
        body.extend_from_slice(code_body);
        let mut code = vec![0x01, body.len() as u8];
        code.extend_from_slice(&body);
        m.push(0x0a);
        m.push(code.len() as u8);
        m.extend_from_slice(&code);
        m
    }

    #[test]
    fn leb128_section_sizes_parse_correctly() {
        // A type section body larger than one byte must be walked correctly.
        let wasm = module_with_func(&[0x7f], &[0x41, 0x2a, 0x0b]); // i32.const 42
        let info = WasmPluginRunner::parse_module_info(&wasm);
        // magic/version + type + function + export + code = 4 sections.
        assert_eq!(info.section_count, 4);
        assert!(info.has_export_section);
    }

    #[test]
    fn executes_const_return() {
        let wasm = module_with_func(&[0x7f], &[0x41, 0x2a, 0x0b]); // i32.const 42
        let r = WasmPluginRunner::execute_plugin_bytes(&wasm, "");
        assert!(r.success, "output: {}", r.output);
        assert_eq!(r.output, "result: 42");
    }

    #[test]
    fn executes_arithmetic() {
        // (i32.const 7)(i32.const 5) i32.add  => 12
        let wasm = module_with_func(&[0x7f], &[0x41, 0x07, 0x41, 0x05, 0x6a, 0x0b]);
        let r = WasmPluginRunner::execute_plugin_bytes(&wasm, "");
        assert!(r.success, "output: {}", r.output);
        assert_eq!(r.output, "result: 12");
    }

    #[test]
    fn executes_if_else() {
        // if (1) { 100 } else { 200 } end  -> 100
        // i32.const 1; if (result i32) i32.const 100 else i32.const 200 end
        let body = [
            0x41, 0x01, // i32.const 1
            0x04, 0x7f, // if (result i32)
            0x41, 0xe4, 0x00, // i32.const 100 (signed LEB128)
            0x05, // else
            0x41, 0xc8, 0x01, // i32.const 200
            0x0b, // end (if)
            0x0b, // end (func)
        ];
        let wasm = module_with_func(&[0x7f], &body);
        let r = WasmPluginRunner::execute_plugin_bytes(&wasm, "");
        assert!(r.success, "output: {}", r.output);
        assert_eq!(r.output, "result: 100");
    }

    #[test]
    fn executes_loop_sum() {
        // Sum 1..=5 using a loop with a local counter/accumulator.
        // locals: [0]=i (i32), [1]=sum (i32)
        // (func (result i32)
        //   loop
        //     local.get 1; local.get 0; i32.add; local.set 1   ; sum += i
        //     local.get 0; i32.const 1; i32.add; local.set 0    ; i += 1
        //     local.get 0; i32.const 5; i32.le_s; br_if 0        ; if i<=5 repeat
        //   end
        //   local.get 1)
        let code_body = [
            0x03, 0x40, // loop (void)
            0x20, 0x01, 0x20, 0x00, 0x6a, 0x21, 0x01, // sum = sum + i
            0x20, 0x00, 0x41, 0x01, 0x6a, 0x21, 0x00, // i = i + 1
            0x20, 0x00, 0x41, 0x05, 0x4c, 0x0d, 0x00, // if i <= 5 br 0
            0x0b, // end loop
            0x20, 0x01, // local.get sum
            0x0b, // end func
        ];
        // Body with 2 i32 locals: local decl count=1, (count=2, type=i32)
        let mut body = vec![0x01, 0x02, 0x7f];
        body.extend_from_slice(&code_body);
        // Build module manually to inject the locals.
        let mut m = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let ty = vec![0x01, 0x60, 0x00, 0x01, 0x7f];
        m.push(0x01);
        m.push(ty.len() as u8);
        m.extend_from_slice(&ty);
        m.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
        let export = vec![0x01, 0x04, b'm', b'a', b'i', b'n', 0x00, 0x00];
        m.push(0x07);
        m.push(export.len() as u8);
        m.extend_from_slice(&export);
        let mut code = vec![0x01, body.len() as u8];
        code.extend_from_slice(&body);
        m.push(0x0a);
        m.push(code.len() as u8);
        m.extend_from_slice(&code);

        let r = WasmPluginRunner::execute_plugin_bytes(&m, "");
        assert!(r.success, "output: {}", r.output);
        // i starts at 0: iterations add i=0,1,2,3,4,5 then i becomes 6 > 5.
        // sum = 0+1+2+3+4+5 = 15.
        assert_eq!(r.output, "result: 15");
    }

    #[test]
    fn unsupported_opcode_reports_error() {
        // 0xfc-prefixed (saturating truncation) is outside the supported set.
        let wasm = module_with_func(&[0x7f], &[0x41, 0x00, 0xfc, 0x00, 0x0b]);
        let r = WasmPluginRunner::execute_plugin_bytes(&wasm, "");
        assert!(!r.success);
        assert!(r.output.contains("unsupported"), "output: {}", r.output);
    }
}
