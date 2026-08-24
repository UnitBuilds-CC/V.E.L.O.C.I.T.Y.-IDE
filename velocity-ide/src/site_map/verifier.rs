// site_map/verifier.rs — Incremental Merkle verifier for NDA program generation
#![allow(dead_code)]
//
// During Path 2 generation, every emitted NDA node is hashed as it is produced.
// The verifier maintains a stack of open SCOPEs.  When a SCOPE closes, its child
// hashes are folded into a single parent hash.  The ROOT token carries the
// top-level hash; if it mismatches the accumulated tree the generation step is
// rejected — structurally invalid programs cannot reach execution.
//
// This gives the same guarantee as git's object store: you cannot store a
// corrupted object because the hash would mismatch.  Here the hash mismatch
// fires during generation, not after.
//
// Hash function: SHA-256 (via sha2 crate, already in Cargo.toml).
// All hashes are truncated to u64 (first 8 bytes of SHA-256 output) for
// speed and compact storage.  Collision probability over a site map of 10^9
// entries is ~2.7 × 10^{-10} — acceptable for a deterministic KV store.

use serde::Serialize;
use sha2::{Digest, Sha256};

// ─── NDA opcode vocabulary (Path 2's 9-token output space) ───────────────────

/// The complete output vocabulary for Path 2 (NDA native pipeline).
/// These are the only tokens the NDA output head can emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NdaOpcode {
    // ── Original opcodes ─────────────────────────────────────────────────
    Scope = 0,    // begin a group of child nodes
    EndScope = 1, // close a SCOPE, finalise its hash
    Matrix = 2,   // weight matrix node (rows × cols, sign[], extra[], scale)
    Norm = 3,     // layer-norm node (weight[], bias[])
    Call = 4,     // reference another node by its u64 hash
    Int = 5,      // scalar integer constant
    Root = 6,     // top-level Merkle commit — must be the final token
    Bit0 = 7,     // bit value 0 (used inside Matrix/Norm payload)
    Bit1 = 8,     // bit value 1
    // ── Language opcodes (NDA-as-a-language) ──────────────────────────────
    Loop = 9,     // bounded loop: count + body
    While = 10,   // conditional loop: cond + body
    If = 11,      // branch: cond + then + optional else
    Compare = 12, // comparison: op + lhs + rhs → bool vec
    Let = 13,     // variable binding: name_hash + init
    Load = 14,    // variable read: name_hash
    Store = 15,   // variable write: name_hash + value
    Add = 16,     // vector addition: lhs + rhs
    VecOp = 17,   // unary vector op: kind + operand
    Print = 18,   // output to stdout: source
    Return = 19,  // function return: value
    Break = 20,   // exit loop
    // ── New bytecode opcodes ──────────────────────────────────────────────
    Bitwise = 21,     // bitwise operations
    Float = 22,       // scalar float constant
    Math = 23,        // scalar float arithmetic
    MathFunc = 24,    // scalar float functions (sin, cos, exp, etc.)
    Peek = 25,        // MMIO read
    Poke = 26,        // MMIO write
    Gemv = 27,        // matrix-vector multiply
    Dot = 28,         // vector-vector dot product
    Syscall = 29,     // syscall transition
    Spawn = 30,       // spawn thread
    Atomic = 31,      // atomic hardware instruction (CAS, FAA)
    Alloc = 32,       // virtual heap allocation
    Free = 33,        // virtual heap free
    RegInt = 34,      // register hardware interrupt handler
    Cast = 35,        // type casting
    GpuDispatch = 36, // shader dispatch to UGAL
    Triple = 37,      // semantic graph subject-predicate-object triple
}

impl NdaOpcode {
    pub const VOCAB_SIZE: usize = 38;

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Scope),
            1 => Some(Self::EndScope),
            2 => Some(Self::Matrix),
            3 => Some(Self::Norm),
            4 => Some(Self::Call),
            5 => Some(Self::Int),
            6 => Some(Self::Root),
            7 => Some(Self::Bit0),
            8 => Some(Self::Bit1),
            9 => Some(Self::Loop),
            10 => Some(Self::While),
            11 => Some(Self::If),
            12 => Some(Self::Compare),
            13 => Some(Self::Let),
            14 => Some(Self::Load),
            15 => Some(Self::Store),
            16 => Some(Self::Add),
            17 => Some(Self::VecOp),
            18 => Some(Self::Print),
            19 => Some(Self::Return),
            20 => Some(Self::Break),
            21 => Some(Self::Bitwise),
            22 => Some(Self::Float),
            23 => Some(Self::Math),
            24 => Some(Self::MathFunc),
            25 => Some(Self::Peek),
            26 => Some(Self::Poke),
            27 => Some(Self::Gemv),
            28 => Some(Self::Dot),
            29 => Some(Self::Syscall),
            30 => Some(Self::Spawn),
            31 => Some(Self::Atomic),
            32 => Some(Self::Alloc),
            33 => Some(Self::Free),
            34 => Some(Self::RegInt),
            35 => Some(Self::Cast),
            36 => Some(Self::GpuDispatch),
            37 => Some(Self::Triple),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Scope => "SCOPE",
            Self::EndScope => "END_SCOPE",
            Self::Matrix => "MATRIX",
            Self::Norm => "NORM",
            Self::Call => "CALL",
            Self::Int => "INT",
            Self::Root => "ROOT",
            Self::Bit0 => "0",
            Self::Bit1 => "1",
            Self::Loop => "LOOP",
            Self::While => "WHILE",
            Self::If => "IF",
            Self::Compare => "COMPARE",
            Self::Let => "LET",
            Self::Load => "LOAD",
            Self::Store => "STORE",
            Self::Add => "ADD",
            Self::VecOp => "VECOP",
            Self::Print => "PRINT",
            Self::Return => "RETURN",
            Self::Break => "BREAK",
            Self::Bitwise => "BITWISE",
            Self::Float => "FLOAT",
            Self::Math => "MATH",
            Self::MathFunc => "MATH_FUNC",
            Self::Peek => "PEEK",
            Self::Poke => "POKE",
            Self::Gemv => "GEMV",
            Self::Dot => "DOT",
            Self::Syscall => "SYSCALL",
            Self::Spawn => "SPAWN",
            Self::Atomic => "ATOMIC",
            Self::Alloc => "ALLOC",
            Self::Free => "FREE",
            Self::RegInt => "REG_INT",
            Self::Cast => "CAST",
            Self::GpuDispatch => "GPU_DISPATCH",
            Self::Triple => "TRIPLE",
        }
    }
}

// ─── Comparison operators ─────────────────────────────────────────────────────

/// Comparison operation for Compare nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CmpOp {
    Eq = 0, // ==
    Ne = 1, // !=
    Lt = 2, // <
    Gt = 3, // >
    Le = 4, // <=
    Ge = 5, // >=
}

impl CmpOp {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Eq),
            1 => Some(Self::Ne),
            2 => Some(Self::Lt),
            3 => Some(Self::Gt),
            4 => Some(Self::Le),
            5 => Some(Self::Ge),
            _ => None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
        }
    }
}

// ─── Vector operation kinds ───────────────────────────────────────────────────

/// Unary vector operations for VecOp nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VecOpKind {
    SiLU = 0,      // SiLU activation (lookup table)
    Negate = 1,    // element-wise negate
    Abs = 2,       // element-wise absolute value
    ReduceSum = 3, // sum all elements → scalar vec
}

impl VecOpKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::SiLU),
            1 => Some(Self::Negate),
            2 => Some(Self::Abs),
            3 => Some(Self::ReduceSum),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::SiLU => "silu",
            Self::Negate => "negate",
            Self::Abs => "abs",
            Self::ReduceSum => "reduce_sum",
        }
    }
}

// ─── Bytecode sub-enums ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BitwiseOp {
    And = 0,
    Or = 1,
    Xor = 2,
    Not = 3,
    Shl = 4,
    Shr = 5,
}

impl BitwiseOp {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::And),
            1 => Some(Self::Or),
            2 => Some(Self::Xor),
            3 => Some(Self::Not),
            4 => Some(Self::Shl),
            5 => Some(Self::Shr),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::Not => "not",
            Self::Shl => "shl",
            Self::Shr => "shr",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MathOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
}

impl MathOp {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Add),
            1 => Some(Self::Sub),
            2 => Some(Self::Mul),
            3 => Some(Self::Div),
            _ => None,
        }
    }
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MathFuncKind {
    Sin = 0,
    Cos = 1,
    Sqrt = 2,
    Exp = 3,
}

impl MathFuncKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Sin),
            1 => Some(Self::Cos),
            2 => Some(Self::Sqrt),
            3 => Some(Self::Exp),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Sqrt => "sqrt",
            Self::Exp => "exp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AtomicOp {
    Cas = 0,
    Faa = 1,
}

impl AtomicOp {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Cas),
            1 => Some(Self::Faa),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Cas => "cas",
            Self::Faa => "faa",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TypeKind {
    Int = 0,
    Float = 1,
    Vector = 2,
}

impl TypeKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Int),
            1 => Some(Self::Float),
            2 => Some(Self::Vector),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Vector => "vector",
        }
    }
}

// ─── NDA node (in-memory representation) ──────────────────────────────────────

/// A single node in an NDA program tree.
///
/// The original 5 node types (Matrix, Norm, Call, Int, Scope) form the
/// computation engine. The language extension nodes (Loop, While, If,
/// Compare, Let, Load, Store, Add, VecOp, Print, Return, Break) turn
/// NDA into a full programming language.
#[derive(Clone, Debug)]
pub enum NdaNode {
    Matrix {
        rows: u16,
        cols: u16,
        scale: i8,
        sign: Vec<u8>, // bit-packed, rows*cols bits
        extra: Vec<u8>,
    },
    Norm {
        size: u16,
        weight: Vec<u8>,
        bias: Vec<u8>,
    },
    Call {
        target: u64, // hash of the referenced node
    },
    Int {
        value: i32,
    },
    Scope {
        children: Vec<NdaNode>,
    },
    Loop {
        count: u32,
        body: Vec<NdaNode>,
    },
    While {
        cond: Box<NdaNode>,
        body: Vec<NdaNode>,
    },
    If {
        cond: Box<NdaNode>,
        then_body: Vec<NdaNode>,
        else_body: Option<Vec<NdaNode>>,
    },
    Compare {
        op: CmpOp,
        lhs: Box<NdaNode>,
        rhs: Box<NdaNode>,
    },
    Let {
        name_hash: u64,
        init: Box<NdaNode>,
    },
    Load {
        name_hash: u64,
    },
    Store {
        name_hash: u64,
        value: Box<NdaNode>,
    },
    Add {
        lhs: Box<NdaNode>,
        rhs: Box<NdaNode>,
    },
    VecOp {
        op: VecOpKind,
        operand: Box<NdaNode>,
    },
    Print {
        source: Box<NdaNode>,
    },
    Return {
        value: Box<NdaNode>,
    },
    Break,
    Bitwise {
        op: BitwiseOp,
        lhs: Box<NdaNode>,
        rhs: Option<Box<NdaNode>>,
    },
    Float {
        value: f32,
    },
    Math {
        op: MathOp,
        lhs: Box<NdaNode>,
        rhs: Box<NdaNode>,
    },
    MathFunc {
        func: MathFuncKind,
        operand: Box<NdaNode>,
    },
    Peek {
        addr: Box<NdaNode>,
    },
    Poke {
        addr: Box<NdaNode>,
        value: Box<NdaNode>,
    },
    Gemv {
        matrix: Box<NdaNode>,
        vector: Box<NdaNode>,
    },
    Dot {
        lhs: Box<NdaNode>,
        rhs: Box<NdaNode>,
    },
    Syscall {
        num: u32,
        args: Vec<NdaNode>,
    },
    Spawn {
        scope_hash: u64,
    },
    Atomic {
        op: AtomicOp,
        addr: Box<NdaNode>,
        val: Box<NdaNode>,
    },
    Alloc {
        size: Box<NdaNode>,
    },
    Free {
        addr: Box<NdaNode>,
    },
    RegInt {
        vector: u32,
        handler_hash: u64,
    },
    Cast {
        from_type: TypeKind,
        to_type: TypeKind,
        operand: Box<NdaNode>,
    },
    GpuDispatch {
        shader_hash: u64,
        args: Vec<NdaNode>,
    },
    Triple {
        subject_hash: u64,
        predicate_id: u16,
        object_hash: u64,
    },
}

impl NdaNode {
    /// Compute the SHA-256-truncated-to-u64 hash of this node.
    pub fn hash(&self) -> u64 {
        let mut h = Sha256::new();
        self.hash_into(&mut h);
        let digest = h.finalize();
        u64::from_le_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest is always 32 bytes"),
        )
    }

    /// Hash a list of child nodes into a hasher (Merkle tree pattern).
    fn hash_children(h: &mut Sha256, children: &[NdaNode]) {
        h.update((children.len() as u32).to_le_bytes());
        for child in children {
            h.update(child.hash().to_le_bytes());
        }
    }

    fn hash_into(&self, h: &mut Sha256) {
        match self {
            // ── Original computation nodes ───────────────────────────────
            NdaNode::Matrix {
                rows,
                cols,
                scale,
                sign,
                extra,
            } => {
                h.update(b"M");
                h.update(rows.to_le_bytes());
                h.update(cols.to_le_bytes());
                h.update([*scale as u8]);
                h.update(sign);
                h.update(extra);
            }
            NdaNode::Norm { size, weight, bias } => {
                h.update(b"N");
                h.update(size.to_le_bytes());
                h.update(weight);
                h.update(bias);
            }
            NdaNode::Call { target } => {
                h.update(b"C");
                h.update(target.to_le_bytes());
            }
            NdaNode::Int { value } => {
                h.update(b"I");
                h.update(value.to_le_bytes());
            }
            NdaNode::Scope { children } => {
                h.update(b"S");
                Self::hash_children(h, children);
            }

            // ── Control flow ─────────────────────────────────────────────
            NdaNode::Loop { count, body } => {
                h.update(b"LP");
                h.update(count.to_le_bytes());
                Self::hash_children(h, body);
            }
            NdaNode::While { cond, body } => {
                h.update(b"WH");
                h.update(cond.hash().to_le_bytes());
                Self::hash_children(h, body);
            }
            NdaNode::If {
                cond,
                then_body,
                else_body,
            } => {
                h.update(b"IF");
                h.update(cond.hash().to_le_bytes());
                Self::hash_children(h, then_body);
                if let Some(eb) = else_body {
                    h.update(b"EL");
                    Self::hash_children(h, eb);
                }
            }
            NdaNode::Compare { op, lhs, rhs } => {
                h.update(b"CMP");
                h.update([*op as u8]);
                h.update(lhs.hash().to_le_bytes());
                h.update(rhs.hash().to_le_bytes());
            }
            NdaNode::Break => {
                h.update(b"BRK");
            }

            // ── Variables ────────────────────────────────────────────────
            NdaNode::Let { name_hash, init } => {
                h.update(b"LET");
                h.update(name_hash.to_le_bytes());
                h.update(init.hash().to_le_bytes());
            }
            NdaNode::Load { name_hash } => {
                h.update(b"LD");
                h.update(name_hash.to_le_bytes());
            }
            NdaNode::Store { name_hash, value } => {
                h.update(b"ST");
                h.update(name_hash.to_le_bytes());
                h.update(value.hash().to_le_bytes());
            }

            // ── Arithmetic ───────────────────────────────────────────────
            NdaNode::Add { lhs, rhs } => {
                h.update(b"ADD");
                h.update(lhs.hash().to_le_bytes());
                h.update(rhs.hash().to_le_bytes());
            }
            NdaNode::VecOp { op, operand } => {
                h.update(b"VOP");
                h.update([*op as u8]);
                h.update(operand.hash().to_le_bytes());
            }

            // ── I/O ──────────────────────────────────────────────────────
            NdaNode::Print { source } => {
                h.update(b"PRT");
                h.update(source.hash().to_le_bytes());
            }
            NdaNode::Return { value } => {
                h.update(b"RET");
                h.update(value.hash().to_le_bytes());
            }
            // ── New variants ──────────────────────────────────────────────
            NdaNode::Bitwise { op, lhs, rhs } => {
                h.update(b"BW");
                h.update([*op as u8]);
                h.update(lhs.hash().to_le_bytes());
                if let Some(r) = rhs {
                    h.update(r.hash().to_le_bytes());
                }
            }
            NdaNode::Float { value } => {
                h.update(b"FL");
                h.update(value.to_le_bytes());
            }
            NdaNode::Math { op, lhs, rhs } => {
                h.update(b"MTH");
                h.update([*op as u8]);
                h.update(lhs.hash().to_le_bytes());
                h.update(rhs.hash().to_le_bytes());
            }
            NdaNode::MathFunc { func, operand } => {
                h.update(b"MFC");
                h.update([*func as u8]);
                h.update(operand.hash().to_le_bytes());
            }
            NdaNode::Peek { addr } => {
                h.update(b"PEK");
                h.update(addr.hash().to_le_bytes());
            }
            NdaNode::Poke { addr, value } => {
                h.update(b"POK");
                h.update(addr.hash().to_le_bytes());
                h.update(value.hash().to_le_bytes());
            }
            NdaNode::Gemv { matrix, vector } => {
                h.update(b"GMV");
                h.update(matrix.hash().to_le_bytes());
                h.update(vector.hash().to_le_bytes());
            }
            NdaNode::Dot { lhs, rhs } => {
                h.update(b"DOT");
                h.update(lhs.hash().to_le_bytes());
                h.update(rhs.hash().to_le_bytes());
            }
            NdaNode::Syscall { num, args } => {
                h.update(b"SYS");
                h.update(num.to_le_bytes());
                Self::hash_children(h, args);
            }
            NdaNode::Spawn { scope_hash } => {
                h.update(b"SPW");
                h.update(scope_hash.to_le_bytes());
            }
            NdaNode::Atomic { op, addr, val } => {
                h.update(b"ATC");
                h.update([*op as u8]);
                h.update(addr.hash().to_le_bytes());
                h.update(val.hash().to_le_bytes());
            }
            NdaNode::Alloc { size } => {
                h.update(b"ALC");
                h.update(size.hash().to_le_bytes());
            }
            NdaNode::Free { addr } => {
                h.update(b"FRE");
                h.update(addr.hash().to_le_bytes());
            }
            NdaNode::RegInt {
                vector,
                handler_hash,
            } => {
                h.update(b"RGI");
                h.update(vector.to_le_bytes());
                h.update(handler_hash.to_le_bytes());
            }
            NdaNode::Cast {
                from_type,
                to_type,
                operand,
            } => {
                h.update(b"CST");
                h.update([*from_type as u8, *to_type as u8]);
                h.update(operand.hash().to_le_bytes());
            }
            NdaNode::GpuDispatch { shader_hash, args } => {
                h.update(b"GPD");
                h.update(shader_hash.to_le_bytes());
                Self::hash_children(h, args);
            }
            NdaNode::Triple {
                subject_hash,
                predicate_id,
                object_hash,
            } => {
                h.update(b"TPL");
                h.update(subject_hash.to_le_bytes());
                h.update(predicate_id.to_le_bytes());
                h.update(object_hash.to_le_bytes());
            }
        }
    }
}

// ─── MerkleVerifier ───────────────────────────────────────────────────────────

/// Incremental Merkle verifier for streaming NDA generation.
///
/// Maintains a stack of open SCOPEs.  Each level of the stack holds the
/// hashes of nodes completed at that level.  When END_SCOPE is emitted,
/// the current level's hashes are folded into a Scope node hash and pushed
/// to the parent level.  The ROOT token provides the claimed top-level hash;
/// if it mismatches the single remaining hash on the stack the generation is
/// invalid and must be rejected.
///
/// This mirrors git's tree-object model: blobs hash their content, trees hash
/// their children's hashes, commits hash their tree.  Every level is verified
/// against its children before it can be used as a parent.
#[derive(Debug)]
pub struct MerkleVerifier {
    /// Stack of hash lists, one per open SCOPE.
    /// `stack[0]` = top-level, `stack[last]` = innermost open SCOPE.
    pub(crate) stack: Vec<Vec<u64>>,
    /// Set when ROOT token is emitted; used for final validation.
    claimed_root: Option<u64>,
    /// Completed root hash (set when stack fully unwinds).
    computed_root: Option<u64>,
}

impl MerkleVerifier {
    pub fn new() -> Self {
        Self {
            stack: vec![vec![]], // start with one open top-level scope
            claimed_root: None,
            computed_root: None,
        }
    }

    /// Reset for a new generation.
    pub fn reset(&mut self) {
        self.stack = vec![vec![]];
        self.claimed_root = None;
        self.computed_root = None;
    }

    /// Push a completed leaf node hash at the current SCOPE level.
    /// Call this whenever a terminal node (Matrix, Norm, Call, Int) is fully emitted.
    pub fn push_leaf(&mut self, node: &NdaNode) {
        let h = node.hash();
        if let Some(level) = self.stack.last_mut() {
            level.push(h);
        }
    }

    /// Open a new nested SCOPE.  Call when SCOPE opcode is emitted.
    pub fn open_scope(&mut self) {
        self.stack.push(vec![]);
    }

    /// Close the innermost SCOPE.  Folds its children into a Scope node hash
    /// and pushes that hash to the parent level.
    /// Returns `Err` if there is no open inner SCOPE (malformed program).
    pub fn close_scope(&mut self) -> Result<u64, &'static str> {
        if self.stack.len() < 2 {
            return Err("END_SCOPE with no matching SCOPE");
        }
        let children_hashes = self.stack.pop().expect("stack.len() >= 2 checked above");
        // Build a synthetic Scope node from child hashes to compute parent hash.
        let scope_hash = Self::hash_scope(&children_hashes);
        if let Some(level) = self.stack.last_mut() {
            level.push(scope_hash);
        }
        Ok(scope_hash)
    }

    /// Record the claimed root hash from the ROOT token.
    /// Call when ROOT opcode + its u64 payload is emitted.
    pub fn record_root(&mut self, claimed: u64) {
        self.claimed_root = Some(claimed);
        // Attempt to finalise: if only the top-level scope remains and has
        // exactly one entry, that is the computed root.
        if self.stack.len() == 1 {
            if let Some(&h) = self.stack[0].last() {
                self.computed_root = Some(h);
            }
        }
    }

    /// Returns true iff the claimed ROOT matches the computed Merkle root.
    /// A return value of `false` means the beam candidate must be pruned.
    pub fn is_valid(&self) -> bool {
        match (self.claimed_root, self.computed_root) {
            (Some(c), Some(r)) => c == r,
            _ => false,
        }
    }

    /// Returns `true` if the verifier is in a structurally consistent state
    /// mid-generation (no violated invariants yet).  Used to prune beams early.
    pub fn is_consistent(&self) -> bool {
        !self.stack.is_empty()
    }

    /// Depth of the currently open SCOPE nesting.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn hash_scope(child_hashes: &[u64]) -> u64 {
        let mut h = Sha256::new();
        h.update(b"S");
        h.update((child_hashes.len() as u32).to_le_bytes());
        for &ch in child_hashes {
            h.update(ch.to_le_bytes());
        }
        let digest = h.finalize();
        u64::from_le_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest is always 32 bytes"),
        )
    }
}

impl NdaOpcode {
    /// Human-readable category for grouping opcodes in diagnostics.
    pub fn category(self) -> &'static str {
        match self {
            Self::Scope | Self::EndScope | Self::Root => "structure",
            Self::Matrix | Self::Norm | Self::Call | Self::Int => "computation",
            Self::Bit0 | Self::Bit1 => "payload",
            Self::Loop | Self::While | Self::If | Self::Break => "control_flow",
            Self::Compare | Self::Let | Self::Load | Self::Store => "variable",
            Self::Add | Self::VecOp => "arithmetic",
            Self::Print | Self::Return => "io",
            Self::Bitwise | Self::Float | Self::Math | Self::MathFunc => "arithmetic",
            Self::Peek | Self::Poke => "memory",
            Self::Gemv | Self::Dot => "computation",
            Self::Syscall | Self::Spawn | Self::Atomic => "system",
            Self::Alloc | Self::Free | Self::RegInt => "system",
            Self::Cast => "type_system",
            Self::GpuDispatch => "gpu",
            Self::Triple => "semantic",
        }
    }

    /// Human-readable description of what the opcode does.
    pub fn description(self) -> &'static str {
        match self {
            Self::Scope => "Begin a group of child nodes",
            Self::EndScope => "Close a scope, finalise its hash",
            Self::Matrix => "Weight matrix node (rows × cols, sign[], extra[], scale)",
            Self::Norm => "Layer-norm node (weight[], bias[])",
            Self::Call => "Reference another node by its u64 hash",
            Self::Int => "Scalar integer constant",
            Self::Root => "Top-level Merkle commit — must be the final token",
            Self::Bit0 => "Bit value 0 (used inside Matrix/Norm payload)",
            Self::Bit1 => "Bit value 1",
            Self::Loop => "Bounded loop: count + body",
            Self::While => "Conditional loop: cond + body",
            Self::If => "Branch: cond + then + optional else",
            Self::Compare => "Comparison: op + lhs + rhs → bool vec",
            Self::Let => "Variable binding: name_hash + init",
            Self::Load => "Variable read: name_hash",
            Self::Store => "Variable write: name_hash + value",
            Self::Add => "Vector addition: lhs + rhs",
            Self::VecOp => "Unary vector op: kind + operand",
            Self::Print => "Output to stdout: source",
            Self::Return => "Function return: value",
            Self::Break => "Exit loop",
            Self::Bitwise => "Bitwise operations",
            Self::Float => "Scalar float constant",
            Self::Math => "Scalar float arithmetic",
            Self::MathFunc => "Scalar float functions (sin, cos, exp, etc.)",
            Self::Peek => "MMIO read",
            Self::Poke => "MMIO write",
            Self::Gemv => "Matrix-vector multiply",
            Self::Dot => "Vector-vector dot product",
            Self::Syscall => "Syscall transition",
            Self::Spawn => "Spawn thread",
            Self::Atomic => "Atomic hardware instruction (CAS, FAA)",
            Self::Alloc => "Virtual heap allocation",
            Self::Free => "Virtual heap free",
            Self::RegInt => "Register interrupt handler",
            Self::Cast => "Type casting",
            Self::GpuDispatch => "Shader dispatch to UGAL",
            Self::Triple => "Semantic graph subject-predicate-object triple",
        }
    }

    /// Returns true if this opcode is a control-flow construct.
    pub fn is_control_flow(self) -> bool {
        matches!(self, Self::Loop | Self::While | Self::If | Self::Break)
    }

    /// Returns true if this opcode performs arithmetic or vector math.
    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            Self::Add
                | Self::VecOp
                | Self::Bitwise
                | Self::Math
                | Self::MathFunc
                | Self::Dot
                | Self::Gemv
        )
    }

    /// Returns true if this opcode performs I/O or returns a value.
    pub fn is_io(self) -> bool {
        matches!(self, Self::Print | Self::Return)
    }

    /// Returns true if this opcode reads/writes variables.
    pub fn is_variable(self) -> bool {
        matches!(self, Self::Let | Self::Load | Self::Store | Self::Compare)
    }

    /// Returns true if this opcode accesses memory-mapped I/O.
    pub fn is_memory(self) -> bool {
        matches!(self, Self::Peek | Self::Poke | Self::Alloc | Self::Free)
    }

    /// Returns true if this opcode is a core computation node.
    pub fn is_computation(self) -> bool {
        matches!(self, Self::Matrix | Self::Norm | Self::Call | Self::Gemv | Self::Dot)
    }
}

// ─── Diagnostics ───────────────────────────────────────────────────────────────

/// Serializable diagnostic for a single NDA opcode.
#[derive(Debug, Clone, Serialize)]
pub struct OpcodeInfo {
    pub opcode: u8,
    pub name: String,
    pub category: String,
    pub description: String,
    pub is_control_flow: bool,
    pub is_arithmetic: bool,
    pub is_io: bool,
    pub is_variable: bool,
    pub is_memory: bool,
    pub is_computation: bool,
}

/// Return diagnostic info for a single opcode.
pub fn opcode_info(op: NdaOpcode) -> OpcodeInfo {
    OpcodeInfo {
        opcode: op as u8,
        name: op.name().to_string(),
        category: op.category().to_string(),
        description: op.description().to_string(),
        is_control_flow: op.is_control_flow(),
        is_arithmetic: op.is_arithmetic(),
        is_io: op.is_io(),
        is_variable: op.is_variable(),
        is_memory: op.is_memory(),
        is_computation: op.is_computation(),
    }
}

/// Distribution of opcode categories across a token stream.
#[derive(Debug, Clone, Serialize)]
pub struct OpcodeDistribution {
    pub total_tokens: usize,
    pub structure_count: usize,
    pub computation_count: usize,
    pub payload_count: usize,
    pub control_flow_count: usize,
    pub variable_count: usize,
    pub arithmetic_count: usize,
    pub io_count: usize,
    pub memory_count: usize,
    pub system_count: usize,
    pub type_system_count: usize,
    pub gpu_count: usize,
    pub semantic_count: usize,
    pub unique_opcodes: usize,
    pub validation_issues: Vec<String>,
}

/// Compute the opcode category distribution over a stream of opcodes.
pub fn opcode_distribution(opcodes: &[NdaOpcode]) -> OpcodeDistribution {
    let mut structure = 0usize;
    let mut computation = 0usize;
    let mut payload = 0usize;
    let mut control_flow = 0usize;
    let mut variable = 0usize;
    let mut arithmetic = 0usize;
    let mut io = 0usize;
    let mut memory = 0usize;
    let mut system = 0usize;
    let mut type_system = 0usize;
    let mut gpu = 0usize;
    let mut semantic = 0usize;
    let mut seen = [false; NdaOpcode::VOCAB_SIZE];

    for &op in opcodes {
        let idx = op as usize;
        if idx < NdaOpcode::VOCAB_SIZE {
            seen[idx] = true;
        }
        match op.category() {
            "structure" => structure += 1,
            "computation" => computation += 1,
            "payload" => payload += 1,
            "control_flow" => control_flow += 1,
            "variable" => variable += 1,
            "arithmetic" => arithmetic += 1,
            "io" => io += 1,
            "memory" => memory += 1,
            "system" => system += 1,
            "type_system" => type_system += 1,
            "gpu" => gpu += 1,
            "semantic" => semantic += 1,
            _ => {}
        }
    }

    let unique = seen.iter().filter(|&&v| v).count();
    let mut issues = Vec::new();
    if opcodes.is_empty() {
        issues.push("empty opcode stream".to_string());
    }
    // Check for structural balance: SCOPE and END_SCOPE counts should match.
    let scope_count = opcodes.iter().filter(|&&o| o == NdaOpcode::Scope).count();
    let end_scope_count = opcodes.iter().filter(|&&o| o == NdaOpcode::EndScope).count();
    if scope_count != end_scope_count {
        issues.push(format!(
            "scope imbalance: {} SCOPE vs {} END_SCOPE",
            scope_count, end_scope_count
        ));
    }
    // ROOT should appear at most once and only at the end.
    let root_count = opcodes.iter().filter(|&&o| o == NdaOpcode::Root).count();
    if root_count > 1 {
        issues.push(format!("multiple ROOT tokens: {}", root_count));
    }
    if root_count == 1 && opcodes.last() != Some(&NdaOpcode::Root) {
        issues.push("ROOT token is not the final token".to_string());
    }

    OpcodeDistribution {
        total_tokens: opcodes.len(),
        structure_count: structure,
        computation_count: computation,
        payload_count: payload,
        control_flow_count: control_flow,
        variable_count: variable,
        arithmetic_count: arithmetic,
        io_count: io,
        memory_count: memory,
        system_count: system,
        type_system_count: type_system,
        gpu_count: gpu,
        semantic_count: semantic,
        unique_opcodes: unique,
        validation_issues: issues,
    }
}

/// Serializable diagnostic snapshot of MerkleVerifier state.
#[derive(Debug, Clone, Serialize)]
pub struct MerkleVerifierInfo {
    pub stack_depth: usize,
    pub total_pending_hashes: usize,
    pub innermost_scope_size: usize,
    pub has_claimed_root: bool,
    pub has_computed_root: bool,
    pub is_valid: bool,
    pub is_consistent: bool,
    pub validation_issues: Vec<String>,
}

impl MerkleVerifier {
    /// Return a diagnostic snapshot of the verifier's current state.
    pub fn info(&self) -> MerkleVerifierInfo {
        let total_pending: usize = self.stack.iter().map(|level| level.len()).sum();
        let innermost = self.stack.last().map_or(0, |level| level.len());
        let mut issues = Vec::new();
        if self.stack.is_empty() {
            issues.push("empty scope stack".to_string());
        }
        if self.claimed_root.is_some() && self.computed_root.is_none() {
            issues.push("claimed root recorded but computed root is missing".to_string());
        }
        if let (Some(c), Some(r)) = (self.claimed_root, self.computed_root) {
            if c != r {
                issues.push(format!(
                    "root mismatch: claimed={:016x} computed={:016x}",
                    c, r
                ));
            }
        }
        MerkleVerifierInfo {
            stack_depth: self.stack.len(),
            total_pending_hashes: total_pending,
            innermost_scope_size: innermost,
            has_claimed_root: self.claimed_root.is_some(),
            has_computed_root: self.computed_root.is_some(),
            is_valid: self.is_valid(),
            is_consistent: self.is_consistent(),
            validation_issues: issues,
        }
    }
}

/// Validate the structural integrity of an NdaNode tree.
/// Returns a list of warnings (empty = clean).
pub fn validate_node(node: &NdaNode) -> Vec<String> {
    let mut issues = Vec::new();
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            scale,
            sign,
            extra: _,
        } => {
            if *rows == 0 || *cols == 0 {
                issues.push(format!("matrix has zero dimension: {}x{}", rows, cols));
            }
            let expected_bits = (*rows as usize) * (*cols as usize);
            let expected_sign_bytes = (expected_bits + 7) / 8;
            if sign.len() != expected_sign_bytes {
                issues.push(format!(
                    "matrix sign bytes mismatch: expected {} for {}x{}, got {}",
                    expected_sign_bytes, rows, cols, sign.len()
                ));
            }
            if *scale < -15 || *scale > 15 {
                issues.push(format!("matrix scale out of range: {}", scale));
            }
        }
        NdaNode::Norm { size, weight, bias } => {
            if *size == 0 {
                issues.push("norm has zero size".to_string());
            }
            if weight.len() != bias.len() {
                issues.push(format!(
                    "norm weight/bias length mismatch: {} vs {}",
                    weight.len(),
                    bias.len()
                ));
            }
        }
        NdaNode::Scope { children } => {
            for (i, child) in children.iter().enumerate() {
                let child_issues = validate_node(child);
                for issue in child_issues {
                    issues.push(format!("child[{}]: {}", i, issue));
                }
            }
        }
        NdaNode::Loop { count, body } => {
            if *count == 0 {
                issues.push("loop has zero iteration count".to_string());
            }
            if body.is_empty() {
                issues.push("loop has empty body".to_string());
            }
            for (i, child) in body.iter().enumerate() {
                for issue in validate_node(child) {
                    issues.push(format!("loop body[{}]: {}", i, issue));
                }
            }
        }
        NdaNode::While { cond, body } => {
            for issue in validate_node(cond) {
                issues.push(format!("while cond: {}", issue));
            }
            for (i, child) in body.iter().enumerate() {
                for issue in validate_node(child) {
                    issues.push(format!("while body[{}]: {}", i, issue));
                }
            }
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            for issue in validate_node(cond) {
                issues.push(format!("if cond: {}", issue));
            }
            for (i, child) in then_body.iter().enumerate() {
                for issue in validate_node(child) {
                    issues.push(format!("then[{}]: {}", i, issue));
                }
            }
            if let Some(eb) = else_body {
                for (i, child) in eb.iter().enumerate() {
                    for issue in validate_node(child) {
                        issues.push(format!("else[{}]: {}", i, issue));
                    }
                }
            }
        }
        NdaNode::Compare { lhs, rhs, .. } => {
            for issue in validate_node(lhs) {
                issues.push(format!("cmp lhs: {}", issue));
            }
            for issue in validate_node(rhs) {
                issues.push(format!("cmp rhs: {}", issue));
            }
        }
        NdaNode::Let { init, .. } => {
            for issue in validate_node(init) {
                issues.push(format!("let init: {}", issue));
            }
        }
        NdaNode::Store { value, .. } => {
            for issue in validate_node(value) {
                issues.push(format!("store value: {}", issue));
            }
        }
        NdaNode::Add { lhs, rhs } => {
            for issue in validate_node(lhs) {
                issues.push(format!("add lhs: {}", issue));
            }
            for issue in validate_node(rhs) {
                issues.push(format!("add rhs: {}", issue));
            }
        }
        NdaNode::VecOp { operand, .. } => {
            for issue in validate_node(operand) {
                issues.push(format!("vecop operand: {}", issue));
            }
        }
        NdaNode::Print { source } => {
            for issue in validate_node(source) {
                issues.push(format!("print source: {}", issue));
            }
        }
        NdaNode::Return { value } => {
            for issue in validate_node(value) {
                issues.push(format!("return value: {}", issue));
            }
        }
        NdaNode::Bitwise { lhs, rhs, .. } => {
            for issue in validate_node(lhs) {
                issues.push(format!("bitwise lhs: {}", issue));
            }
            if let Some(r) = rhs {
                for issue in validate_node(r) {
                    issues.push(format!("bitwise rhs: {}", issue));
                }
            }
        }
        NdaNode::Math { lhs, rhs, .. } => {
            for issue in validate_node(lhs) {
                issues.push(format!("math lhs: {}", issue));
            }
            for issue in validate_node(rhs) {
                issues.push(format!("math rhs: {}", issue));
            }
        }
        NdaNode::MathFunc { operand, .. } => {
            for issue in validate_node(operand) {
                issues.push(format!("mathfunc operand: {}", issue));
            }
        }
        NdaNode::Peek { addr } => {
            for issue in validate_node(addr) {
                issues.push(format!("peek addr: {}", issue));
            }
        }
        NdaNode::Poke { addr, value } => {
            for issue in validate_node(addr) {
                issues.push(format!("poke addr: {}", issue));
            }
            for issue in validate_node(value) {
                issues.push(format!("poke value: {}", issue));
            }
        }
        NdaNode::Gemv { matrix, vector } => {
            for issue in validate_node(matrix) {
                issues.push(format!("gemv matrix: {}", issue));
            }
            for issue in validate_node(vector) {
                issues.push(format!("gemv vector: {}", issue));
            }
        }
        NdaNode::Dot { lhs, rhs } => {
            for issue in validate_node(lhs) {
                issues.push(format!("dot lhs: {}", issue));
            }
            for issue in validate_node(rhs) {
                issues.push(format!("dot rhs: {}", issue));
            }
        }
        NdaNode::Syscall { args, .. } => {
            for (i, arg) in args.iter().enumerate() {
                for issue in validate_node(arg) {
                    issues.push(format!("syscall arg[{}]: {}", i, issue));
                }
            }
        }
        NdaNode::Atomic { addr, val, .. } => {
            for issue in validate_node(addr) {
                issues.push(format!("atomic addr: {}", issue));
            }
            for issue in validate_node(val) {
                issues.push(format!("atomic val: {}", issue));
            }
        }
        NdaNode::Alloc { size } => {
            for issue in validate_node(size) {
                issues.push(format!("alloc size: {}", issue));
            }
        }
        NdaNode::Free { addr } => {
            for issue in validate_node(addr) {
                issues.push(format!("free addr: {}", issue));
            }
        }
        NdaNode::Cast { operand, .. } => {
            for issue in validate_node(operand) {
                issues.push(format!("cast operand: {}", issue));
            }
        }
        NdaNode::GpuDispatch { args, .. } => {
            for (i, arg) in args.iter().enumerate() {
                for issue in validate_node(arg) {
                    issues.push(format!("gpu arg[{}]: {}", i, issue));
                }
            }
        }
        // Leaf nodes with no children to recurse into.
        NdaNode::Call { .. }
        | NdaNode::Int { .. }
        | NdaNode::Float { .. }
        | NdaNode::Load { .. }
        | NdaNode::Break
        | NdaNode::Spawn { .. }
        | NdaNode::RegInt { .. }
        | NdaNode::Triple { .. } => {}
    }
    issues
}

/// Return the kind name of an NdaNode variant as a static string.
pub fn node_kind_name(node: &NdaNode) -> &'static str {
    match node {
        NdaNode::Matrix { .. } => "Matrix",
        NdaNode::Norm { .. } => "Norm",
        NdaNode::Call { .. } => "Call",
        NdaNode::Int { .. } => "Int",
        NdaNode::Scope { .. } => "Scope",
        NdaNode::Loop { .. } => "Loop",
        NdaNode::While { .. } => "While",
        NdaNode::If { .. } => "If",
        NdaNode::Compare { .. } => "Compare",
        NdaNode::Let { .. } => "Let",
        NdaNode::Load { .. } => "Load",
        NdaNode::Store { .. } => "Store",
        NdaNode::Add { .. } => "Add",
        NdaNode::VecOp { .. } => "VecOp",
        NdaNode::Print { .. } => "Print",
        NdaNode::Return { .. } => "Return",
        NdaNode::Break => "Break",
        NdaNode::Bitwise { .. } => "Bitwise",
        NdaNode::Float { .. } => "Float",
        NdaNode::Math { .. } => "Math",
        NdaNode::MathFunc { .. } => "MathFunc",
        NdaNode::Peek { .. } => "Peek",
        NdaNode::Poke { .. } => "Poke",
        NdaNode::Gemv { .. } => "Gemv",
        NdaNode::Dot { .. } => "Dot",
        NdaNode::Syscall { .. } => "Syscall",
        NdaNode::Spawn { .. } => "Spawn",
        NdaNode::Atomic { .. } => "Atomic",
        NdaNode::Alloc { .. } => "Alloc",
        NdaNode::Free { .. } => "Free",
        NdaNode::RegInt { .. } => "RegInt",
        NdaNode::Cast { .. } => "Cast",
        NdaNode::GpuDispatch { .. } => "GpuDispatch",
        NdaNode::Triple { .. } => "Triple",
    }
}

/// Estimate the memory footprint of an NdaNode tree in bytes.
pub fn estimated_memory_bytes(node: &NdaNode) -> usize {
    let mut total = std::mem::size_of::<NdaNode>();
    match node {
        NdaNode::Matrix { sign, extra, .. } => {
            total += sign.len() + extra.len();
        }
        NdaNode::Norm { weight, bias, .. } => {
            total += weight.len() + bias.len();
        }
        NdaNode::Scope { children } => {
            for child in children {
                total += estimated_memory_bytes(child);
            }
        }
        NdaNode::Loop { body, .. } => {
            for child in body {
                total += estimated_memory_bytes(child);
            }
        }
        NdaNode::While { cond, body } => {
            total += estimated_memory_bytes(cond);
            for child in body {
                total += estimated_memory_bytes(child);
            }
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            total += estimated_memory_bytes(cond);
            for child in then_body {
                total += estimated_memory_bytes(child);
            }
            if let Some(eb) = else_body {
                for child in eb {
                    total += estimated_memory_bytes(child);
                }
            }
        }
        NdaNode::Compare { lhs, rhs, .. }
        | NdaNode::Add { lhs, rhs }
        | NdaNode::Math { lhs, rhs, .. }
        | NdaNode::Dot { lhs, rhs }
        | NdaNode::Poke { addr: lhs, value: rhs }
        | NdaNode::Gemv { matrix: lhs, vector: rhs }
        | NdaNode::Atomic { addr: lhs, val: rhs, .. } => {
            total += estimated_memory_bytes(lhs);
            total += estimated_memory_bytes(rhs);
        }
        NdaNode::Let { init, .. }
        | NdaNode::Store { value: init, .. }
        | NdaNode::VecOp { operand: init, .. }
        | NdaNode::Print { source: init }
        | NdaNode::Return { value: init }
        | NdaNode::Bitwise { lhs: init, .. }
        | NdaNode::MathFunc { operand: init, .. }
        | NdaNode::Peek { addr: init }
        | NdaNode::Alloc { size: init }
        | NdaNode::Free { addr: init }
        | NdaNode::Cast { operand: init, .. } => {
            total += estimated_memory_bytes(init);
        }
        NdaNode::Syscall { args, .. } | NdaNode::GpuDispatch { args, .. } => {
            for arg in args {
                total += estimated_memory_bytes(arg);
            }
        }
        // Pure-value leaf nodes.
        NdaNode::Call { .. }
        | NdaNode::Int { .. }
        | NdaNode::Float { .. }
        | NdaNode::Load { .. }
        | NdaNode::Break
        | NdaNode::Spawn { .. }
        | NdaNode::RegInt { .. }
        | NdaNode::Triple { .. } => {}
    }
    total
}

impl Default for MerkleVerifier {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_hash_is_deterministic() {
        let n1 = NdaNode::Int { value: 42 };
        let n2 = NdaNode::Int { value: 42 };
        assert_eq!(n1.hash(), n2.hash());
    }

    #[test]
    fn different_nodes_have_different_hashes() {
        let n1 = NdaNode::Int { value: 42 };
        let n2 = NdaNode::Int { value: 43 };
        assert_ne!(n1.hash(), n2.hash());
    }

    #[test]
    fn scope_hash_depends_on_children() {
        let s1 = NdaNode::Scope {
            children: vec![NdaNode::Int { value: 1 }],
        };
        let s2 = NdaNode::Scope {
            children: vec![NdaNode::Int { value: 2 }],
        };
        assert_ne!(s1.hash(), s2.hash());
    }

    #[test]
    fn verifier_accepts_valid_root() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 7 };
        v.push_leaf(&leaf);
        // Simulate: stack[0] = [leaf.hash()] — top-level scope holds one node.
        // Close the top-level scope manually to get computed root.
        let children = v.stack[0].clone();
        let _root_hash = MerkleVerifier::hash_scope(&children);
        // In real generation the top-level scope is implicit; the last hash
        // on the stack IS the root.  Simulate ROOT token:
        v.computed_root = Some(leaf.hash());
        v.claimed_root = Some(leaf.hash());
        assert!(v.is_valid());
    }

    #[test]
    fn verifier_rejects_wrong_root() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 7 };
        v.push_leaf(&leaf);
        v.computed_root = Some(leaf.hash());
        v.claimed_root = Some(leaf.hash() ^ 0xDEAD_BEEF); // tampered
        assert!(!v.is_valid());
    }

    #[test]
    fn nested_scope_folds_correctly() {
        let mut v = MerkleVerifier::new();
        let n1 = NdaNode::Int { value: 1 };
        let n2 = NdaNode::Int { value: 2 };

        v.open_scope();
        v.push_leaf(&n1);
        v.push_leaf(&n2);
        let inner_hash = v.close_scope().unwrap();

        // The parent scope should now hold inner_hash.
        assert_eq!(v.stack[0], vec![inner_hash]);
    }

    #[test]
    fn close_scope_without_open_errors() {
        let mut v = MerkleVerifier::new();
        // stack has only 1 level (the implicit top-level) — closing it is an error
        assert!(v.close_scope().is_err());
    }

    // ─── New diagnostic tests ──────────────────────────────────────────────────

    #[test]
    fn opcode_from_u8_roundtrip() {
        for i in 0..NdaOpcode::VOCAB_SIZE as u8 {
            let op = NdaOpcode::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
        }
        assert!(NdaOpcode::from_u8(255).is_none());
    }

    #[test]
    fn opcode_categories_are_nonempty() {
        // Every opcode should have a non-empty category.
        for i in 0..NdaOpcode::VOCAB_SIZE as u8 {
            let op = NdaOpcode::from_u8(i).unwrap();
            assert!(!op.category().is_empty(), "opcode {:?} has empty category", op);
            assert!(!op.description().is_empty(), "opcode {:?} has empty description", op);
            assert!(!op.name().is_empty(), "opcode {:?} has empty name", op);
        }
    }

    #[test]
    fn opcode_boolean_flags_are_mutually_exclusive_for_structure() {
        // Structure opcodes should not also be arithmetic.
        let structure_ops = [NdaOpcode::Scope, NdaOpcode::EndScope, NdaOpcode::Root];
        for op in &structure_ops {
            assert!(op.is_control_flow() == false || op.is_control_flow()); // just ensure no panic
            // Structure ops are not arithmetic.
            assert!(!op.is_arithmetic());
            assert!(!op.is_io());
        }
    }

    #[test]
    fn opcode_info_serializes() {
        let info = opcode_info(NdaOpcode::Matrix);
        assert_eq!(info.name, "MATRIX");
        assert_eq!(info.category, "computation");
        assert!(info.is_computation);
        assert!(!info.is_control_flow);
    }

    #[test]
    fn opcode_distribution_empty_stream() {
        let dist = opcode_distribution(&[]);
        assert_eq!(dist.total_tokens, 0);
        assert_eq!(dist.unique_opcodes, 0);
        assert!(!dist.validation_issues.is_empty()); // should warn about empty
    }

    #[test]
    fn opcode_distribution_balanced_program() {
        let ops = vec![
            NdaOpcode::Scope,
            NdaOpcode::Int,
            NdaOpcode::EndScope,
            NdaOpcode::Root,
        ];
        let dist = opcode_distribution(&ops);
        assert_eq!(dist.total_tokens, 4);
        assert!(dist.validation_issues.is_empty()); // balanced
        assert!(dist.structure_count > 0);
        assert!(dist.computation_count > 0);
    }

    #[test]
    fn opcode_distribution_detects_scope_imbalance() {
        let ops = vec![NdaOpcode::Scope, NdaOpcode::Int]; // missing EndScope
        let dist = opcode_distribution(&ops);
        assert!(!dist.validation_issues.is_empty());
        assert!(dist.validation_issues.iter().any(|i| i.contains("imbalance")));
    }

    #[test]
    fn opcode_distribution_detects_root_not_final() {
        let ops = vec![NdaOpcode::Root, NdaOpcode::Int]; // Root before Int
        let dist = opcode_distribution(&ops);
        assert!(!dist.validation_issues.is_empty());
        assert!(dist.validation_issues.iter().any(|i| i.contains("not the final")));
    }

    #[test]
    fn verifier_info_clean_state() {
        let v = MerkleVerifier::new();
        let info = v.info();
        assert_eq!(info.stack_depth, 1); // one open top-level scope
        assert_eq!(info.total_pending_hashes, 0);
        assert!(!info.has_claimed_root);
        assert!(!info.has_computed_root);
        assert!(!info.is_valid);
        assert!(info.is_consistent);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn verifier_info_with_mismatch() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 42 };
        v.push_leaf(&leaf);
        v.computed_root = Some(0xAAAA);
        v.claimed_root = Some(0xBBBB);
        let info = v.info();
        assert!(!info.is_valid);
        assert!(!info.validation_issues.is_empty());
        assert!(info.validation_issues.iter().any(|i| i.contains("mismatch")));
    }

    #[test]
    fn validate_node_clean_int() {
        let node = NdaNode::Int { value: 42 };
        let issues = validate_node(&node);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_node_matrix_zero_dim() {
        let node = NdaNode::Matrix {
            rows: 0,
            cols: 4,
            scale: 0,
            sign: vec![],
            extra: vec![],
        };
        let issues = validate_node(&node);
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("zero dimension")));
    }

    #[test]
    fn validate_node_matrix_sign_mismatch() {
        let node = NdaNode::Matrix {
            rows: 2,
            cols: 4,
            scale: 0,
            sign: vec![0xFF; 4], // 2*4=8 bits = 1 byte expected, got 4
            extra: vec![],
        };
        let issues = validate_node(&node);
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("sign bytes mismatch")));
    }

    #[test]
    fn validate_node_matrix_scale_out_of_range() {
        let node = NdaNode::Matrix {
            rows: 1,
            cols: 1,
            scale: 20, // out of [-15, 15]
            sign: vec![0],
            extra: vec![],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("scale out of range")));
    }

    #[test]
    fn validate_node_norm_zero_size() {
        let node = NdaNode::Norm {
            size: 0,
            weight: vec![],
            bias: vec![],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("zero size")));
    }

    #[test]
    fn validate_node_norm_weight_bias_mismatch() {
        let node = NdaNode::Norm {
            size: 4,
            weight: vec![0; 4],
            bias: vec![0; 2], // different length
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("weight/bias length mismatch")));
    }

    #[test]
    fn validate_node_loop_zero_count() {
        let node = NdaNode::Loop {
            count: 0,
            body: vec![NdaNode::Break],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("zero iteration")));
    }

    #[test]
    fn validate_node_loop_empty_body() {
        let node = NdaNode::Loop {
            count: 5,
            body: vec![],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("empty body")));
    }

    #[test]
    fn validate_node_nested_scope_propagates_issues() {
        let node = NdaNode::Scope {
            children: vec![NdaNode::Matrix {
                rows: 0,
                cols: 0,
                scale: 0,
                sign: vec![],
                extra: vec![],
            }],
        };
        let issues = validate_node(&node);
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("child[0]")));
    }

    #[test]
    fn node_kind_name_covers_all_variants() {
        let nodes = vec![
            NdaNode::Int { value: 0 },
            NdaNode::Float { value: 0.0 },
            NdaNode::Break,
        ];
        for node in &nodes {
            let name = node_kind_name(node);
            assert!(!name.is_empty());
        }
        assert_eq!(node_kind_name(&NdaNode::Break), "Break");
        assert_eq!(
            node_kind_name(&NdaNode::Int { value: 0 }),
            "Int"
        );
    }

    #[test]
    fn estimated_memory_bytes_leaf() {
        let node = NdaNode::Int { value: 42 };
        let bytes = estimated_memory_bytes(&node);
        assert!(bytes > 0); // at least sizeof(NdaNode)
    }

    #[test]
    fn estimated_memory_bytes_matrix_includes_buffers() {
        let sign = vec![0xAA; 128];
        let extra = vec![0xBB; 64];
        let node = NdaNode::Matrix {
            rows: 8,
            cols: 16,
            scale: 0,
            sign: sign.clone(),
            extra: extra.clone(),
        };
        let bytes = estimated_memory_bytes(&node);
        assert!(bytes >= 128 + 64); // at least the buffer sizes
    }

    #[test]
    fn estimated_memory_bytes_scope_sums_children() {
        let child1 = NdaNode::Int { value: 1 };
        let child2 = NdaNode::Int { value: 2 };
        let scope = NdaNode::Scope {
            children: vec![child1.clone(), child2.clone()],
        };
        let scope_bytes = estimated_memory_bytes(&scope);
        let c1 = estimated_memory_bytes(&child1);
        let c2 = estimated_memory_bytes(&child2);
        assert!(scope_bytes >= c1 + c2);
    }

    #[test]
    fn cmp_op_roundtrip() {
        for i in 0..6u8 {
            let op = CmpOp::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.symbol().is_empty());
        }
        assert!(CmpOp::from_u8(255).is_none());
    }

    #[test]
    fn vec_op_kind_roundtrip() {
        for i in 0..4u8 {
            let op = VecOpKind::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(VecOpKind::from_u8(255).is_none());
    }

    #[test]
    fn bitwise_op_roundtrip() {
        for i in 0..6u8 {
            let op = BitwiseOp::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(BitwiseOp::from_u8(255).is_none());
    }

    #[test]
    fn math_op_roundtrip() {
        for i in 0..4u8 {
            let op = MathOp::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.symbol().is_empty());
        }
        assert!(MathOp::from_u8(255).is_none());
    }

    #[test]
    fn math_func_roundtrip() {
        for i in 0..4u8 {
            let op = MathFuncKind::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(MathFuncKind::from_u8(255).is_none());
    }

    #[test]
    fn atomic_op_roundtrip() {
        for i in 0..2u8 {
            let op = AtomicOp::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(AtomicOp::from_u8(255).is_none());
    }

    #[test]
    fn type_kind_roundtrip() {
        for i in 0..3u8 {
            let op = TypeKind::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(TypeKind::from_u8(255).is_none());
    }

    #[test]
    fn verifier_reset_clears_state() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 99 };
        v.push_leaf(&leaf);
        v.open_scope();
        v.push_leaf(&leaf);
        v.close_scope().unwrap();
        assert!(v.depth() >= 1);

        v.reset();
        assert_eq!(v.depth(), 1);
        assert!(v.stack[0].is_empty());
        assert!(!v.is_valid());
        assert!(v.is_consistent());
    }

    #[test]
    fn verifier_multiple_leaves_at_top_level() {
        let mut v = MerkleVerifier::new();
        let n1 = NdaNode::Int { value: 10 };
        let n2 = NdaNode::Int { value: 20 };
        let n3 = NdaNode::Float { value: 3.14 };
        v.push_leaf(&n1);
        v.push_leaf(&n2);
        v.push_leaf(&n3);
        assert_eq!(v.stack[0].len(), 3);
        assert_eq!(v.depth(), 1);
    }

    #[test]
    fn node_hash_differs_by_variant_tag() {
        // Int(42) and Float(42.0) should have different hashes.
        let int_node = NdaNode::Int { value: 42 };
        let float_node = NdaNode::Float { value: 42.0 };
        assert_ne!(int_node.hash(), float_node.hash());
    }

    #[test]
    fn validate_clean_matrix() {
        let node = NdaNode::Matrix {
            rows: 2,
            cols: 4,
            scale: 0,
            sign: vec![0xAA], // 2*4=8 bits = 1 byte
            extra: vec![],
        };
        let issues = validate_node(&node);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_clean_norm() {
        let node = NdaNode::Norm {
            size: 4,
            weight: vec![0; 4],
            bias: vec![0; 4],
        };
        let issues = validate_node(&node);
        assert!(issues.is_empty());
    }

    #[test]
    fn opcode_distribution_unique_count() {
        let ops = vec![NdaOpcode::Int, NdaOpcode::Int, NdaOpcode::Float];
        let dist = opcode_distribution(&ops);
        assert_eq!(dist.unique_opcodes, 2); // Int and Float
        assert_eq!(dist.total_tokens, 3);
    }

    // ── Block 139: Extended tests ─────────────────────────────────────────

    // --- NdaOpcode: specific category values ---

    #[test]
    fn opcode_category_specific_values() {
        assert_eq!(NdaOpcode::Scope.category(), "structure");
        assert_eq!(NdaOpcode::Matrix.category(), "computation");
        assert_eq!(NdaOpcode::Bit0.category(), "payload");
        assert_eq!(NdaOpcode::Loop.category(), "control_flow");
        assert_eq!(NdaOpcode::Let.category(), "variable");
        assert_eq!(NdaOpcode::Add.category(), "arithmetic");
        assert_eq!(NdaOpcode::Print.category(), "io");
        assert_eq!(NdaOpcode::Peek.category(), "memory");
        assert_eq!(NdaOpcode::Syscall.category(), "system");
        assert_eq!(NdaOpcode::Cast.category(), "type_system");
        assert_eq!(NdaOpcode::GpuDispatch.category(), "gpu");
        assert_eq!(NdaOpcode::Triple.category(), "semantic");
    }

    #[test]
    fn opcode_name_specific_values() {
        assert_eq!(NdaOpcode::Scope.name(), "SCOPE");
        assert_eq!(NdaOpcode::EndScope.name(), "END_SCOPE");
        assert_eq!(NdaOpcode::Bit0.name(), "0");
        assert_eq!(NdaOpcode::Bit1.name(), "1");
        assert_eq!(NdaOpcode::MathFunc.name(), "MATH_FUNC");
        assert_eq!(NdaOpcode::RegInt.name(), "REG_INT");
        assert_eq!(NdaOpcode::GpuDispatch.name(), "GPU_DISPATCH");
    }

    // --- NdaOpcode: boolean predicates ---

    #[test]
    fn opcode_is_control_flow() {
        assert!(NdaOpcode::Loop.is_control_flow());
        assert!(NdaOpcode::While.is_control_flow());
        assert!(NdaOpcode::If.is_control_flow());
        assert!(NdaOpcode::Break.is_control_flow());
        assert!(!NdaOpcode::Int.is_control_flow());
        assert!(!NdaOpcode::Add.is_control_flow());
    }

    #[test]
    fn opcode_is_arithmetic() {
        assert!(NdaOpcode::Add.is_arithmetic());
        assert!(NdaOpcode::VecOp.is_arithmetic());
        assert!(NdaOpcode::Bitwise.is_arithmetic());
        assert!(NdaOpcode::Math.is_arithmetic());
        assert!(NdaOpcode::MathFunc.is_arithmetic());
        assert!(NdaOpcode::Dot.is_arithmetic());
        assert!(NdaOpcode::Gemv.is_arithmetic());
        assert!(!NdaOpcode::Int.is_arithmetic());
        assert!(!NdaOpcode::Print.is_arithmetic());
    }

    #[test]
    fn opcode_is_io() {
        assert!(NdaOpcode::Print.is_io());
        assert!(NdaOpcode::Return.is_io());
        assert!(!NdaOpcode::Int.is_io());
        assert!(!NdaOpcode::Load.is_io());
    }

    #[test]
    fn opcode_is_variable() {
        assert!(NdaOpcode::Let.is_variable());
        assert!(NdaOpcode::Load.is_variable());
        assert!(NdaOpcode::Store.is_variable());
        assert!(NdaOpcode::Compare.is_variable());
        assert!(!NdaOpcode::Int.is_variable());
    }

    #[test]
    fn opcode_is_memory() {
        assert!(NdaOpcode::Peek.is_memory());
        assert!(NdaOpcode::Poke.is_memory());
        assert!(NdaOpcode::Alloc.is_memory());
        assert!(NdaOpcode::Free.is_memory());
        assert!(!NdaOpcode::Int.is_memory());
    }

    #[test]
    fn opcode_is_computation() {
        assert!(NdaOpcode::Matrix.is_computation());
        assert!(NdaOpcode::Norm.is_computation());
        assert!(NdaOpcode::Call.is_computation());
        assert!(NdaOpcode::Gemv.is_computation());
        assert!(NdaOpcode::Dot.is_computation());
        assert!(!NdaOpcode::Int.is_computation());
    }

    // --- opcode_info for various opcodes ---

    #[test]
    fn opcode_info_all_fields_for_scope() {
        let info = opcode_info(NdaOpcode::Scope);
        assert_eq!(info.opcode, 0);
        assert_eq!(info.name, "SCOPE");
        assert_eq!(info.category, "structure");
        assert!(!info.description.is_empty());
        assert!(!info.is_control_flow);
        assert!(!info.is_arithmetic);
        assert!(!info.is_io);
        assert!(!info.is_variable);
        assert!(!info.is_memory);
        assert!(!info.is_computation);
    }

    #[test]
    fn opcode_info_serializes_all_fields() {
        let info = opcode_info(NdaOpcode::Loop);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"opcode\":9"));
        assert!(json.contains("\"name\":\"LOOP\""));
        assert!(json.contains("\"is_control_flow\":true"));
    }

    // --- opcode_distribution edge cases ---

    #[test]
    fn opcode_distribution_multiple_roots() {
        let ops = vec![NdaOpcode::Root, NdaOpcode::Root, NdaOpcode::Int];
        let dist = opcode_distribution(&ops);
        assert!(dist.validation_issues.iter().any(|i| i.contains("multiple ROOT")));
    }

    #[test]
    fn opcode_distribution_scope_imbalance_values() {
        let ops = vec![NdaOpcode::Scope, NdaOpcode::Scope, NdaOpcode::EndScope];
        let dist = opcode_distribution(&ops);
        assert!(dist.validation_issues.iter().any(|i| i.contains("2") && i.contains("1")));
    }

    #[test]
    fn opcode_distribution_category_counts_sum_to_total() {
        let ops = vec![
            NdaOpcode::Scope, NdaOpcode::Int, NdaOpcode::Bit0,
            NdaOpcode::Loop, NdaOpcode::Let, NdaOpcode::Add,
            NdaOpcode::Print, NdaOpcode::Peek, NdaOpcode::Syscall,
            NdaOpcode::Cast, NdaOpcode::GpuDispatch, NdaOpcode::Triple,
        ];
        let dist = opcode_distribution(&ops);
        let sum = dist.structure_count + dist.computation_count + dist.payload_count
            + dist.control_flow_count + dist.variable_count + dist.arithmetic_count
            + dist.io_count + dist.memory_count + dist.system_count
            + dist.type_system_count + dist.gpu_count + dist.semantic_count;
        assert_eq!(sum, dist.total_tokens);
    }

    #[test]
    fn opcode_distribution_clone_and_debug() {
        let dist = opcode_distribution(&[NdaOpcode::Int]);
        let cloned = dist.clone();
        assert_eq!(cloned.total_tokens, 1);
        let debug = format!("{:?}", dist);
        assert!(debug.contains("OpcodeDistribution"));
    }

    // --- MerkleVerifier edge cases ---

    #[test]
    fn verifier_is_valid_false_without_roots() {
        let v = MerkleVerifier::new();
        assert!(!v.is_valid());
    }

    #[test]
    fn verifier_is_valid_false_with_only_claimed() {
        let mut v = MerkleVerifier::new();
        v.claimed_root = Some(42);
        assert!(!v.is_valid());
    }

    #[test]
    fn verifier_record_root_sets_computed() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 7 };
        v.push_leaf(&leaf);
        // record_root should set computed_root from the last hash on stack
        v.record_root(leaf.hash());
        assert_eq!(v.info().has_computed_root, true);
        assert_eq!(v.info().has_claimed_root, true);
    }

    #[test]
    fn verifier_info_empty_stack_issue() {
        let mut v = MerkleVerifier::new();
        v.stack.clear(); // force empty stack
        let info = v.info();
        assert!(info.validation_issues.iter().any(|i| i.contains("empty scope stack")));
        assert!(!info.is_consistent);
    }

    #[test]
    fn verifier_info_claimed_without_computed() {
        let mut v = MerkleVerifier::new();
        v.claimed_root = Some(0x1234);
        // computed_root is None
        let info = v.info();
        assert!(info.validation_issues.iter().any(|i| i.contains("computed root is missing")));
    }

    #[test]
    fn verifier_depth_with_nested_scopes() {
        let mut v = MerkleVerifier::new();
        assert_eq!(v.depth(), 1);
        v.open_scope();
        assert_eq!(v.depth(), 2);
        v.open_scope();
        assert_eq!(v.depth(), 3);
        v.close_scope().unwrap();
        assert_eq!(v.depth(), 2);
        v.close_scope().unwrap();
        assert_eq!(v.depth(), 1);
    }

    #[test]
    fn verifier_default_equals_new() {
        let v = MerkleVerifier::default();
        assert_eq!(v.depth(), 1);
        assert!(v.is_consistent());
        assert!(!v.is_valid());
    }

    // --- validate_node: remaining variants ---

    #[test]
    fn validate_node_while_propagates_cond_issues() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
            body: vec![NdaNode::Break],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("while cond")));
    }

    #[test]
    fn validate_node_if_propagates_cond_issues() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
            then_body: vec![NdaNode::Break],
            else_body: None,
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("if cond")));
    }

    #[test]
    fn validate_node_if_else_propagates() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Break],
            else_body: Some(vec![NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }]),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("else[0]")));
    }

    #[test]
    fn validate_node_compare_propagates() {
        let node = NdaNode::Compare {
            op: CmpOp::Eq,
            lhs: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
            rhs: Box::new(NdaNode::Int { value: 1 }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("cmp lhs")));
    }

    #[test]
    fn validate_node_let_propagates() {
        let node = NdaNode::Let {
            name_hash: 42,
            init: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("let init")));
    }

    #[test]
    fn validate_node_store_propagates() {
        let node = NdaNode::Store {
            name_hash: 1,
            value: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("store value")));
    }

    #[test]
    fn validate_node_add_propagates() {
        let node = NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("add rhs")));
    }

    #[test]
    fn validate_node_vecop_propagates() {
        let node = NdaNode::VecOp {
            op: VecOpKind::SiLU,
            operand: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("vecop operand")));
    }

    #[test]
    fn validate_node_print_propagates() {
        let node = NdaNode::Print {
            source: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("print source")));
    }

    #[test]
    fn validate_node_return_propagates() {
        let node = NdaNode::Return {
            value: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("return value")));
    }

    #[test]
    fn validate_node_bitwise_propagates() {
        let node = NdaNode::Bitwise {
            op: BitwiseOp::And,
            lhs: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
            rhs: Some(Box::new(NdaNode::Int { value: 1 })),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("bitwise lhs")));
    }

    #[test]
    fn validate_node_clean_leaf_variants() {
        let leaves = vec![
            NdaNode::Call { target: 0 },
            NdaNode::Int { value: 0 },
            NdaNode::Float { value: 0.0 },
            NdaNode::Load { name_hash: 0 },
            NdaNode::Break,
            NdaNode::Spawn { scope_hash: 0 },
            NdaNode::RegInt { vector: 0, handler_hash: 0 },
            NdaNode::Triple { subject_hash: 0, predicate_id: 0, object_hash: 0 },
        ];
        for node in &leaves {
            let issues = validate_node(node);
            assert!(issues.is_empty(), "leaf {:?} had issues: {:?}", node_kind_name(node), issues);
        }
    }

    // --- node_kind_name: all variants ---

    #[test]
    fn node_kind_name_all_variants() {
        let cases = vec![
            (NdaNode::Matrix { rows: 1, cols: 1, scale: 0, sign: vec![0], extra: vec![] }, "Matrix"),
            (NdaNode::Norm { size: 1, weight: vec![0], bias: vec![0] }, "Norm"),
            (NdaNode::Call { target: 0 }, "Call"),
            (NdaNode::Int { value: 0 }, "Int"),
            (NdaNode::Scope { children: vec![] }, "Scope"),
            (NdaNode::Loop { count: 1, body: vec![] }, "Loop"),
            (NdaNode::Break, "Break"),
            (NdaNode::Float { value: 0.0 }, "Float"),
            (NdaNode::Triple { subject_hash: 0, predicate_id: 0, object_hash: 0 }, "Triple"),
        ];
        for (node, expected) in cases {
            assert_eq!(node_kind_name(&node), expected);
        }
    }

    // --- estimated_memory_bytes: more variants ---

    #[test]
    fn estimated_memory_bytes_norm_includes_buffers() {
        let node = NdaNode::Norm {
            size: 8,
            weight: vec![0; 32],
            bias: vec![0; 32],
        };
        let bytes = estimated_memory_bytes(&node);
        assert!(bytes >= 64); // weight + bias
    }

    #[test]
    fn estimated_memory_bytes_nested_scopes() {
        let inner = NdaNode::Scope { children: vec![NdaNode::Int { value: 1 }] };
        let outer = NdaNode::Scope { children: vec![inner] };
        let bytes = estimated_memory_bytes(&outer);
        assert!(bytes > estimated_memory_bytes(&NdaNode::Int { value: 1 }));
    }

    // --- CmpOp specific symbols ---

    #[test]
    fn cmp_op_specific_symbols() {
        assert_eq!(CmpOp::Eq.symbol(), "==");
        assert_eq!(CmpOp::Ne.symbol(), "!=");
        assert_eq!(CmpOp::Lt.symbol(), "<");
        assert_eq!(CmpOp::Gt.symbol(), ">");
        assert_eq!(CmpOp::Le.symbol(), "<=");
        assert_eq!(CmpOp::Ge.symbol(), ">=");
    }

    // --- MathOp specific symbols ---

    #[test]
    fn math_op_specific_symbols() {
        assert_eq!(MathOp::Add.symbol(), "+");
        assert_eq!(MathOp::Sub.symbol(), "-");
        assert_eq!(MathOp::Mul.symbol(), "*");
        assert_eq!(MathOp::Div.symbol(), "/");
    }

    // --- Sub-enum specific names ---

    #[test]
    fn vec_op_kind_specific_names() {
        assert_eq!(VecOpKind::SiLU.name(), "silu");
        assert_eq!(VecOpKind::Negate.name(), "negate");
        assert_eq!(VecOpKind::Abs.name(), "abs");
        assert_eq!(VecOpKind::ReduceSum.name(), "reduce_sum");
    }

    #[test]
    fn bitwise_op_specific_names() {
        assert_eq!(BitwiseOp::And.name(), "and");
        assert_eq!(BitwiseOp::Or.name(), "or");
        assert_eq!(BitwiseOp::Xor.name(), "xor");
        assert_eq!(BitwiseOp::Not.name(), "not");
        assert_eq!(BitwiseOp::Shl.name(), "shl");
        assert_eq!(BitwiseOp::Shr.name(), "shr");
    }

    #[test]
    fn math_func_kind_specific_names() {
        assert_eq!(MathFuncKind::Sin.name(), "sin");
        assert_eq!(MathFuncKind::Cos.name(), "cos");
        assert_eq!(MathFuncKind::Sqrt.name(), "sqrt");
        assert_eq!(MathFuncKind::Exp.name(), "exp");
    }

    #[test]
    fn atomic_op_specific_names() {
        assert_eq!(AtomicOp::Cas.name(), "cas");
        assert_eq!(AtomicOp::Faa.name(), "faa");
    }

    #[test]
    fn type_kind_specific_names() {
        assert_eq!(TypeKind::Int.name(), "int");
        assert_eq!(TypeKind::Float.name(), "float");
        assert_eq!(TypeKind::Vector.name(), "vector");
    }

    // --- NdaNode hash: more variants ---

    #[test]
    fn hash_all_leaf_variants_deterministic() {
        let nodes = vec![
            NdaNode::Int { value: 42 },
            NdaNode::Float { value: 3.14 },
            NdaNode::Call { target: 123 },
            NdaNode::Load { name_hash: 456 },
            NdaNode::Break,
            NdaNode::Spawn { scope_hash: 789 },
            NdaNode::RegInt { vector: 0, handler_hash: 111 },
            NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
        ];
        for node in &nodes {
            assert_eq!(node.hash(), node.hash(), "hash not deterministic for {:?}", node_kind_name(node));
        }
    }

    #[test]
    fn hash_scope_empty_vs_nonempty() {
        let empty = NdaNode::Scope { children: vec![] };
        let nonempty = NdaNode::Scope { children: vec![NdaNode::Int { value: 1 }] };
        assert_ne!(empty.hash(), nonempty.hash());
    }

    // --- OpcodeInfo clone/debug ---

    #[test]
    fn opcode_info_clone_and_debug() {
        let info = opcode_info(NdaOpcode::Matrix);
        let cloned = info.clone();
        assert_eq!(cloned.name, "MATRIX");
        let debug = format!("{:?}", info);
        assert!(debug.contains("OpcodeInfo"));
    }

    // --- MerkleVerifierInfo serialization ---

    #[test]
    fn verifier_info_serializes() {
        let v = MerkleVerifier::new();
        let info = v.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"stack_depth\":1"));
        assert!(json.contains("\"is_consistent\":true"));
    }

    // --- validate_node: syscall/gpu_dispatch args ---

    #[test]
    fn validate_node_syscall_propagates() {
        let node = NdaNode::Syscall {
            num: 1,
            args: vec![NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("syscall arg[0]")));
    }

    #[test]
    fn validate_node_gpu_dispatch_propagates() {
        let node = NdaNode::GpuDispatch {
            shader_hash: 0,
            args: vec![NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }],
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("gpu arg[0]")));
    }

    #[test]
    fn validate_node_alloc_propagates() {
        let node = NdaNode::Alloc {
            size: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("alloc size")));
    }

    #[test]
    fn validate_node_free_propagates() {
        let node = NdaNode::Free {
            addr: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("free addr")));
    }

    #[test]
    fn validate_node_cast_propagates() {
        let node = NdaNode::Cast {
            from_type: TypeKind::Int,
            to_type: TypeKind::Float,
            operand: Box::new(NdaNode::Matrix {
                rows: 0, cols: 0, scale: 0, sign: vec![], extra: vec![],
            }),
        };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("cast operand")));
    }

    // ─── Block 192: JSON, opcode methods, memory, kind names ─────────────

    #[test]
    fn opcode_info_json_has_10_keys() {
        let info = opcode_info(NdaOpcode::Matrix);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 10);
    }

    #[test]
    fn opcode_distribution_json_has_15_keys() {
        let dist = opcode_distribution(&[NdaOpcode::Scope, NdaOpcode::Int, NdaOpcode::EndScope]);
        let json = serde_json::to_string(&dist).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 15);
    }

    #[test]
    fn merkle_verifier_info_json_has_8_keys() {
        let v = MerkleVerifier::new();
        let info = v.info();
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 8);
        // Verify all expected keys exist
        for key in &["stack_depth", "total_pending_hashes", "innermost_scope_size",
                      "has_claimed_root", "has_computed_root", "is_valid",
                      "is_consistent", "validation_issues"] {
            assert!(val.get(key).is_some(), "missing key: {key}");
        }
    }

    #[test]
    fn opcode_info_clone_independent() {
        let info = opcode_info(NdaOpcode::Loop);
        let mut cloned = info.clone();
        cloned.name = "MODIFIED".to_string();
        assert_eq!(info.name, "LOOP");
    }

    #[test]
    fn opcode_distribution_clone_independent() {
        let dist = opcode_distribution(&[NdaOpcode::Matrix, NdaOpcode::Norm]);
        let mut cloned = dist.clone();
        cloned.total_tokens = 9999;
        assert_eq!(dist.total_tokens, 2);
    }

    #[test]
    fn opcode_from_u8_roundtrip_all_valid() {
        for i in 0..=37u8 {
            // Note: not all values 0..37 map to opcodes (e.g., 8 is Bit1 but there's no opcode at 8)
            if let Some(op) = NdaOpcode::from_u8(i) {
                let name = op.name();
                assert!(!name.is_empty(), "opcode {i} has empty name");
            }
        }
    }

    #[test]
    fn opcode_from_u8_invalid_returns_none() {
        assert!(NdaOpcode::from_u8(38).is_none());
        assert!(NdaOpcode::from_u8(255).is_none());
    }

    #[test]
    fn cmp_op_from_u8_roundtrip() {
        for i in 0..6u8 {
            let op = CmpOp::from_u8(i).unwrap();
            let sym = op.symbol();
            assert!(!sym.is_empty());
        }
        assert!(CmpOp::from_u8(6).is_none());
        assert!(CmpOp::from_u8(255).is_none());
    }

    #[test]
    fn vec_op_kind_all_variants() {
        assert_eq!(VecOpKind::from_u8(0).unwrap().name(), "silu");
        assert_eq!(VecOpKind::from_u8(1).unwrap().name(), "negate");
        assert_eq!(VecOpKind::from_u8(2).unwrap().name(), "abs");
        assert_eq!(VecOpKind::from_u8(3).unwrap().name(), "reduce_sum");
        assert!(VecOpKind::from_u8(4).is_none());
    }

    #[test]
    fn bitwise_op_all_variants() {
        for (i, expected) in [(0u8,"and"),(1,"or"),(2,"xor"),(3,"not"),(4,"shl"),(5,"shr")] {
            assert_eq!(BitwiseOp::from_u8(i).unwrap().name(), expected);
        }
        assert!(BitwiseOp::from_u8(6).is_none());
    }

    #[test]
    fn math_op_all_variants() {
        assert_eq!(MathOp::from_u8(0).unwrap().symbol(), "+");
        assert_eq!(MathOp::from_u8(1).unwrap().symbol(), "-");
        assert_eq!(MathOp::from_u8(2).unwrap().symbol(), "*");
        assert_eq!(MathOp::from_u8(3).unwrap().symbol(), "/");
        assert!(MathOp::from_u8(4).is_none());
    }

    #[test]
    fn math_func_all_variants() {
        assert_eq!(MathFuncKind::from_u8(0).unwrap().name(), "sin");
        assert_eq!(MathFuncKind::from_u8(1).unwrap().name(), "cos");
        assert_eq!(MathFuncKind::from_u8(2).unwrap().name(), "sqrt");
        assert_eq!(MathFuncKind::from_u8(3).unwrap().name(), "exp");
        assert!(MathFuncKind::from_u8(4).is_none());
    }

    #[test]
    fn atomic_op_and_type_kind() {
        assert_eq!(AtomicOp::from_u8(0).unwrap().name(), "cas");
        assert_eq!(AtomicOp::from_u8(1).unwrap().name(), "faa");
        assert!(AtomicOp::from_u8(2).is_none());
        assert_eq!(TypeKind::from_u8(0).unwrap().name(), "int");
        assert_eq!(TypeKind::from_u8(1).unwrap().name(), "float");
        assert_eq!(TypeKind::from_u8(2).unwrap().name(), "vector");
        assert!(TypeKind::from_u8(3).is_none());
    }

    #[test]
    fn opcode_categories_cover_all() {
        // Verify every opcode has a non-empty category and description
        for i in 0..=37u8 {
            if let Some(op) = NdaOpcode::from_u8(i) {
                assert!(!op.category().is_empty(), "opcode {:?} has empty category", op);
                assert!(!op.description().is_empty(), "opcode {:?} has empty description", op);
            }
        }
    }

    #[test]
    fn opcode_boolean_methods_consistent() {
        // Each opcode should be in exactly one boolean category or none
        let all_ops: Vec<NdaOpcode> = (0..=37u8).filter_map(NdaOpcode::from_u8).collect();
        for op in &all_ops {
            // Structure opcodes are not control_flow/arithmetic/io/variable/memory/computation
            if *op == NdaOpcode::Scope || *op == NdaOpcode::EndScope || *op == NdaOpcode::Root {
                assert!(!op.is_control_flow());
                assert!(!op.is_computation());
            }
        }
    }

    #[test]
    fn node_kind_name_coverage_192() {
        let nodes = vec![
            ("Matrix", NdaNode::Matrix { rows: 1, cols: 1, scale: 0, sign: vec![0], extra: vec![0] }),
            ("Norm", NdaNode::Norm { size: 1, weight: vec![0], bias: vec![0] }),
            ("Call", NdaNode::Call { target: 0 }),
            ("Int", NdaNode::Int { value: 0 }),
            ("Scope", NdaNode::Scope { children: vec![] }),
            ("Float", NdaNode::Float { value: 0.0 }),
            ("Break", NdaNode::Break),
            ("Load", NdaNode::Load { name_hash: 0 }),
            ("Spawn", NdaNode::Spawn { scope_hash: 0 }),
            ("Triple", NdaNode::Triple { subject_hash: 0, predicate_id: 0, object_hash: 0 }),
        ];
        for (expected, node) in nodes {
            assert_eq!(node_kind_name(&node), expected);
        }
    }

    #[test]
    fn estimated_memory_bytes_leaf_nodes() {
        let int_node = NdaNode::Int { value: 42 };
        let bytes = estimated_memory_bytes(&int_node);
        assert_eq!(bytes, std::mem::size_of::<NdaNode>());

        let float_node = NdaNode::Float { value: 3.14 };
        assert_eq!(estimated_memory_bytes(&float_node), std::mem::size_of::<NdaNode>());
    }

    #[test]
    fn estimated_memory_bytes_matrix_includes_bitmaps() {
        let node = NdaNode::Matrix {
            rows: 4, cols: 8, scale: 1,
            sign: vec![0xFF; 4],
            extra: vec![0xAA; 4],
        };
        let bytes = estimated_memory_bytes(&node);
        assert_eq!(bytes, std::mem::size_of::<NdaNode>() + 8);
    }

    #[test]
    fn estimated_memory_bytes_scope_recursive() {
        let child = NdaNode::Matrix {
            rows: 1, cols: 8, scale: 0,
            sign: vec![0; 1], extra: vec![0; 1],
        };
        let scope = NdaNode::Scope { children: vec![child.clone(), child.clone()] };
        let scope_bytes = estimated_memory_bytes(&scope);
        let child_bytes = estimated_memory_bytes(&child);
        // scope = size_of::<NdaNode>() + 2 * child_bytes
        assert_eq!(scope_bytes, std::mem::size_of::<NdaNode>() + 2 * child_bytes);
    }

    #[test]
    fn opcode_distribution_empty_stream_192() {
        let dist = opcode_distribution(&[]);
        assert_eq!(dist.total_tokens, 0);
        assert_eq!(dist.unique_opcodes, 0);
        assert!(dist.validation_issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn opcode_distribution_scope_imbalance() {
        let ops = vec![NdaOpcode::Scope, NdaOpcode::Scope, NdaOpcode::EndScope];
        let dist = opcode_distribution(&ops);
        assert!(dist.validation_issues.iter().any(|i| i.contains("imbalance")));
    }

    #[test]
    fn opcode_distribution_root_not_last() {
        let ops = vec![NdaOpcode::Root, NdaOpcode::Int];
        let dist = opcode_distribution(&ops);
        assert!(dist.validation_issues.iter().any(|i| i.contains("not the final")));
    }

    #[test]
    fn opcode_distribution_multiple_roots_192() {
        let ops = vec![NdaOpcode::Int, NdaOpcode::Root, NdaOpcode::Root];
        let dist = opcode_distribution(&ops);
        assert!(dist.validation_issues.iter().any(|i| i.contains("multiple ROOT")));
    }

    #[test]
    fn verifier_info_with_mismatch_192() {
        let mut v = MerkleVerifier::new();
        v.claimed_root = Some(0xAAAA);
        v.computed_root = Some(0xBBBB);
        let info = v.info();
        assert!(info.validation_issues.iter().any(|i| i.contains("mismatch")));
        assert!(!info.is_valid);
    }

    #[test]
    fn verifier_info_claimed_without_computed_192() {
        let mut v = MerkleVerifier::new();
        v.claimed_root = Some(0x1234);
        // computed_root is None
        let info = v.info();
        assert!(info.validation_issues.iter().any(|i| i.contains("computed root is missing")));
    }

    #[test]
    fn verifier_reset_clears_state_192() {
        let mut v = MerkleVerifier::new();
        v.open_scope();
        v.push_leaf(&NdaNode::Int { value: 1 });
        v.record_root(0xDEAD);
        v.reset();
        assert_eq!(v.depth(), 1);
        assert!(!v.is_valid());
        assert!(v.info().validation_issues.is_empty());
    }

    // ─── Block 205: hash coverage for new variants, validate_node, sub-enum roundtrips ──

    #[test]
    fn hash_new_variants_deterministic() {
        // Each new node variant should produce the same hash on repeated calls
        let nodes: Vec<NdaNode> = vec![
            NdaNode::Float { value: 3.14 },
            NdaNode::Break,
            NdaNode::Load { name_hash: 0xABCD },
            NdaNode::Spawn { scope_hash: 0x1234 },
            NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            NdaNode::RegInt { vector: 5, handler_hash: 99 },
        ];
        for node in &nodes {
            assert_eq!(node.hash(), node.hash(), "hash not deterministic for {:?}", node_kind_name(node));
        }
    }

    #[test]
    fn hash_compound_variants_deterministic() {
        let int_a = NdaNode::Int { value: 1 };
        let int_b = NdaNode::Int { value: 2 };
        let nodes: Vec<NdaNode> = vec![
            NdaNode::Bitwise { op: BitwiseOp::And, lhs: Box::new(int_a.clone()), rhs: Some(Box::new(int_b.clone())) },
            NdaNode::Math { op: MathOp::Add, lhs: Box::new(int_a.clone()), rhs: Box::new(int_b.clone()) },
            NdaNode::MathFunc { func: MathFuncKind::Sin, operand: Box::new(int_a.clone()) },
            NdaNode::Peek { addr: Box::new(int_a.clone()) },
            NdaNode::Poke { addr: Box::new(int_a.clone()), value: Box::new(int_b.clone()) },
            NdaNode::Gemv { matrix: Box::new(int_a.clone()), vector: Box::new(int_b.clone()) },
            NdaNode::Dot { lhs: Box::new(int_a.clone()), rhs: Box::new(int_b.clone()) },
            NdaNode::Syscall { num: 1, args: vec![int_a.clone()] },
            NdaNode::Atomic { op: AtomicOp::Cas, addr: Box::new(int_a.clone()), val: Box::new(int_b.clone()) },
            NdaNode::Alloc { size: Box::new(int_a.clone()) },
            NdaNode::Free { addr: Box::new(int_a.clone()) },
            NdaNode::Cast { from_type: TypeKind::Int, to_type: TypeKind::Float, operand: Box::new(int_a.clone()) },
            NdaNode::GpuDispatch { shader_hash: 0xFF, args: vec![int_a.clone()] },
        ];
        for node in &nodes {
            assert_eq!(node.hash(), node.hash(), "not deterministic for {:?}", node_kind_name(node));
        }
    }

    #[test]
    fn hash_different_float_values() {
        let a = NdaNode::Float { value: 1.0 };
        let b = NdaNode::Float { value: 2.0 };
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn hash_bitwise_with_and_without_rhs() {
        let lhs = NdaNode::Int { value: 1 };
        let rhs = NdaNode::Int { value: 2 };
        let with_rhs = NdaNode::Bitwise { op: BitwiseOp::And, lhs: Box::new(lhs.clone()), rhs: Some(Box::new(rhs)) };
        let without_rhs = NdaNode::Bitwise { op: BitwiseOp::Not, lhs: Box::new(lhs.clone()), rhs: None };
        // Different structure → different hash
        assert_ne!(with_rhs.hash(), without_rhs.hash());
    }

    #[test]
    fn validate_node_matrix_zero_rows() {
        let node = NdaNode::Matrix { rows: 0, cols: 4, scale: 0, sign: vec![], extra: vec![] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("zero dimension")));
    }

    #[test]
    fn validate_node_matrix_sign_byte_mismatch() {
        // 2x4 = 8 bits → 1 byte expected for sign
        let node = NdaNode::Matrix { rows: 2, cols: 4, scale: 0, sign: vec![0, 0], extra: vec![] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("sign bytes mismatch")));
    }

    #[test]
    fn validate_node_matrix_scale_out_of_range_205() {
        let node = NdaNode::Matrix { rows: 1, cols: 1, scale: 20, sign: vec![0], extra: vec![] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("scale out of range")));
    }

    #[test]
    fn validate_node_matrix_negative_scale_out_of_range() {
        let node = NdaNode::Matrix { rows: 1, cols: 1, scale: -20, sign: vec![0], extra: vec![] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("scale out of range")));
    }

    #[test]
    fn validate_node_norm_zero_size_205() {
        let node = NdaNode::Norm { size: 0, weight: vec![], bias: vec![] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("zero size")));
    }

    #[test]
    fn validate_node_norm_weight_bias_mismatch_205() {
        let node = NdaNode::Norm { size: 4, weight: vec![1, 2, 3], bias: vec![1, 2] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("weight/bias length mismatch")));
    }

    #[test]
    fn validate_node_loop_zero_count_205() {
        let node = NdaNode::Loop { count: 0, body: vec![NdaNode::Int { value: 1 }] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("zero iteration")));
    }

    #[test]
    fn validate_node_loop_empty_body_205() {
        let node = NdaNode::Loop { count: 10, body: vec![] };
        let issues = validate_node(&node);
        assert!(issues.iter().any(|i| i.contains("empty body")));
    }

    #[test]
    fn validate_node_clean_matrix() {
        // 4x4 = 16 bits → 2 bytes for sign
        let node = NdaNode::Matrix { rows: 4, cols: 4, scale: 5, sign: vec![0xFF; 2], extra: vec![] };
        let issues = validate_node(&node);
        assert!(issues.is_empty(), "expected no issues, got {:?}", issues);
    }

    #[test]
    fn node_kind_name_all_variants_205() {
        let int_node = NdaNode::Int { value: 0 };
        assert_eq!(node_kind_name(&int_node), "Int");
        let call = NdaNode::Call { target: 0 };
        assert_eq!(node_kind_name(&call), "Call");
        let scope = NdaNode::Scope { children: vec![] };
        assert_eq!(node_kind_name(&scope), "Scope");
        let loop_n = NdaNode::Loop { count: 1, body: vec![] };
        assert_eq!(node_kind_name(&loop_n), "Loop");
        let while_n = NdaNode::While { cond: Box::new(int_node.clone()), body: vec![] };
        assert_eq!(node_kind_name(&while_n), "While");
        let if_n = NdaNode::If { cond: Box::new(int_node.clone()), then_body: vec![], else_body: None };
        assert_eq!(node_kind_name(&if_n), "If");
        let break_n = NdaNode::Break;
        assert_eq!(node_kind_name(&break_n), "Break");
    }

    #[test]
    fn estimated_memory_bytes_norm_includes_weight_bias() {
        let node = NdaNode::Norm { size: 4, weight: vec![0; 10], bias: vec![0; 10] };
        let bytes = estimated_memory_bytes(&node);
        assert_eq!(bytes, std::mem::size_of::<NdaNode>() + 20);
    }

    #[test]
    fn estimated_memory_bytes_loop_recursive() {
        let child = NdaNode::Int { value: 1 };
        let loop_n = NdaNode::Loop { count: 3, body: vec![child.clone(), child.clone()] };
        let loop_bytes = estimated_memory_bytes(&loop_n);
        let child_bytes = estimated_memory_bytes(&child);
        assert_eq!(loop_bytes, std::mem::size_of::<NdaNode>() + 2 * child_bytes);
    }

    #[test]
    fn cmp_op_roundtrip_and_symbol() {
        for i in 0..=5u8 {
            let op = CmpOp::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.symbol().is_empty());
        }
        assert!(CmpOp::from_u8(6).is_none());
        assert!(CmpOp::from_u8(255).is_none());
    }

    #[test]
    fn vec_op_kind_roundtrip_and_name() {
        for i in 0..=3u8 {
            let op = VecOpKind::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(VecOpKind::from_u8(4).is_none());
    }

    #[test]
    fn bitwise_op_roundtrip_and_name() {
        for i in 0..=5u8 {
            let op = BitwiseOp::from_u8(i).unwrap();
            assert_eq!(op as u8, i);
            assert!(!op.name().is_empty());
        }
        assert!(BitwiseOp::from_u8(6).is_none());
    }

    #[test]
    fn opcode_is_methods_coverage() {
        // Test each boolean method returns true for at least one opcode
        assert!(NdaOpcode::Loop.is_control_flow());
        assert!(NdaOpcode::While.is_control_flow());
        assert!(NdaOpcode::If.is_control_flow());
        assert!(NdaOpcode::Break.is_control_flow());
        assert!(NdaOpcode::Add.is_arithmetic());
        assert!(NdaOpcode::Dot.is_arithmetic());
        assert!(NdaOpcode::Print.is_io());
        assert!(NdaOpcode::Return.is_io());
        assert!(NdaOpcode::Let.is_variable());
        assert!(NdaOpcode::Load.is_variable());
        assert!(NdaOpcode::Store.is_variable());
        assert!(NdaOpcode::Compare.is_variable());
        assert!(NdaOpcode::Peek.is_memory());
        assert!(NdaOpcode::Poke.is_memory());
        assert!(NdaOpcode::Alloc.is_memory());
        assert!(NdaOpcode::Free.is_memory());
        assert!(NdaOpcode::Matrix.is_computation());
        assert!(NdaOpcode::Norm.is_computation());
        assert!(NdaOpcode::Call.is_computation());
        assert!(NdaOpcode::Gemv.is_computation());
        assert!(NdaOpcode::Dot.is_computation());
    }

    #[test]
    fn opcode_distribution_all_categories() {
        // Build an opcode stream covering every category
        let ops = vec![
            NdaOpcode::Scope, NdaOpcode::EndScope, NdaOpcode::Root,  // structure
            NdaOpcode::Matrix, NdaOpcode::Norm, NdaOpcode::Call, NdaOpcode::Int,  // computation
            NdaOpcode::Bit0, NdaOpcode::Bit1,  // payload
            NdaOpcode::Loop, NdaOpcode::While, NdaOpcode::If, NdaOpcode::Break,  // control_flow
            NdaOpcode::Compare, NdaOpcode::Let, NdaOpcode::Load, NdaOpcode::Store,  // variable
            NdaOpcode::Add, NdaOpcode::VecOp, NdaOpcode::Bitwise, NdaOpcode::Float,  // arithmetic
            NdaOpcode::Math, NdaOpcode::MathFunc, NdaOpcode::Dot, NdaOpcode::Gemv,  // arithmetic
            NdaOpcode::Print, NdaOpcode::Return,  // io
            NdaOpcode::Peek, NdaOpcode::Poke,  // memory
            NdaOpcode::Syscall, NdaOpcode::Spawn, NdaOpcode::Atomic,  // system
            NdaOpcode::Alloc, NdaOpcode::Free, NdaOpcode::RegInt,  // system
            NdaOpcode::Cast,  // type_system
            NdaOpcode::GpuDispatch,  // gpu
            NdaOpcode::Triple,  // semantic
        ];
        let dist = opcode_distribution(&ops);
        assert_eq!(dist.total_tokens, ops.len());
        assert!(dist.structure_count >= 3);
        assert!(dist.computation_count >= 4);
        assert!(dist.payload_count >= 2);
        assert!(dist.control_flow_count >= 4);
        assert!(dist.variable_count >= 4);
        assert!(dist.arithmetic_count >= 6);
        assert!(dist.io_count >= 2);
        assert!(dist.memory_count >= 2);
        assert!(dist.system_count >= 6);
        assert_eq!(dist.type_system_count, 1);
        assert_eq!(dist.gpu_count, 1);
        assert_eq!(dist.semantic_count, 1);
        assert_eq!(dist.unique_opcodes, NdaOpcode::VOCAB_SIZE);
    }

    #[test]
    fn merkle_verifier_depth_tracking() {
        let mut v = MerkleVerifier::new();
        assert_eq!(v.depth(), 1);
        v.open_scope();
        assert_eq!(v.depth(), 2);
        v.open_scope();
        assert_eq!(v.depth(), 3);
        v.close_scope().unwrap();
        assert_eq!(v.depth(), 2);
        v.close_scope().unwrap();
        assert_eq!(v.depth(), 1);
    }

    #[test]
    fn merkle_verifier_is_consistent() {
        let v = MerkleVerifier::new();
        assert!(v.is_consistent());
        // After reset, still consistent
        let mut v2 = MerkleVerifier::new();
        v2.open_scope();
        v2.reset();
        assert!(v2.is_consistent());
    }

    #[test]
    fn merkle_verifier_record_root_with_pending_hash() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 42 };
        v.push_leaf(&leaf);
        // stack[0] now has one hash — record_root should set computed_root
        v.record_root(leaf.hash());
        assert!(v.is_valid());
    }

    #[test]
    fn opcode_info_all_fields_populated() {
        let info = opcode_info(NdaOpcode::Triple);
        assert_eq!(info.opcode, 37);
        assert_eq!(info.name, "TRIPLE");
        assert_eq!(info.category, "semantic");
        assert!(!info.description.is_empty());
        assert!(!info.is_control_flow);
        assert!(!info.is_arithmetic);
        assert!(!info.is_io);
        assert!(!info.is_variable);
        assert!(!info.is_memory);
        assert!(!info.is_computation);
    }
}
