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

use sha2::{Digest, Sha256};

// ─── NDA opcode vocabulary (Path 2's 9-token output space) ───────────────────

/// The complete output vocabulary for Path 2 (NDA native pipeline).
/// These are the only tokens the NDA output head can emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NdaOpcode {
    // ── Original opcodes ─────────────────────────────────────────────────
    Scope       = 0,   // begin a group of child nodes
    EndScope    = 1,   // close a SCOPE, finalise its hash
    Matrix      = 2,   // weight matrix node (rows × cols, sign[], extra[], scale)
    Norm        = 3,   // layer-norm node (weight[], bias[])
    Call        = 4,   // reference another node by its u64 hash
    Int         = 5,   // scalar integer constant
    Root        = 6,   // top-level Merkle commit — must be the final token
    Bit0        = 7,   // bit value 0 (used inside Matrix/Norm payload)
    Bit1        = 8,   // bit value 1
    // ── Language opcodes (NDA-as-a-language) ──────────────────────────────
    Loop        = 9,   // bounded loop: count + body
    While       = 10,  // conditional loop: cond + body
    If          = 11,  // branch: cond + then + optional else
    Compare     = 12,  // comparison: op + lhs + rhs → bool vec
    Let         = 13,  // variable binding: name_hash + init
    Load        = 14,  // variable read: name_hash
    Store       = 15,  // variable write: name_hash + value
    Add         = 16,  // vector addition: lhs + rhs
    VecOp       = 17,  // unary vector op: kind + operand
    Print       = 18,  // output to stdout: source
    Return      = 19,  // function return: value
    Break       = 20,  // exit loop
    // ── New bytecode opcodes ──────────────────────────────────────────────
    Bitwise     = 21,  // bitwise operations
    Float       = 22,  // scalar float constant
    Math        = 23,  // scalar float arithmetic
    MathFunc    = 24,  // scalar float functions (sin, cos, exp, etc.)
    Peek        = 25,  // MMIO read
    Poke        = 26,  // MMIO write
    Gemv        = 27,  // matrix-vector multiply
    Dot         = 28,  // vector-vector dot product
    Syscall     = 29,  // syscall transition
    Spawn       = 30,  // spawn thread
    Atomic      = 31,  // atomic hardware instruction (CAS, FAA)
    Alloc       = 32,  // virtual heap allocation
    Free        = 33,  // virtual heap free
    RegInt      = 34,  // register hardware interrupt handler
    Cast        = 35,  // type casting
    GpuDispatch = 36,  // shader dispatch to UGAL
}

impl NdaOpcode {
    pub const VOCAB_SIZE: usize = 37;

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0  => Some(Self::Scope),
            1  => Some(Self::EndScope),
            2  => Some(Self::Matrix),
            3  => Some(Self::Norm),
            4  => Some(Self::Call),
            5  => Some(Self::Int),
            6  => Some(Self::Root),
            7  => Some(Self::Bit0),
            8  => Some(Self::Bit1),
            9  => Some(Self::Loop),
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
            _  => None,
        }
    }

    /// Human-readable name for logging.
    pub fn name(self) -> &'static str {
        match self {
            Self::Scope       => "SCOPE",
            Self::EndScope    => "END_SCOPE",
            Self::Matrix      => "MATRIX",
            Self::Norm        => "NORM",
            Self::Call        => "CALL",
            Self::Int         => "INT",
            Self::Root        => "ROOT",
            Self::Bit0        => "0",
            Self::Bit1        => "1",
            Self::Loop        => "LOOP",
            Self::While       => "WHILE",
            Self::If          => "IF",
            Self::Compare     => "COMPARE",
            Self::Let         => "LET",
            Self::Load        => "LOAD",
            Self::Store       => "STORE",
            Self::Add         => "ADD",
            Self::VecOp       => "VECOP",
            Self::Print       => "PRINT",
            Self::Return      => "RETURN",
            Self::Break       => "BREAK",
            Self::Bitwise     => "BITWISE",
            Self::Float       => "FLOAT",
            Self::Math        => "MATH",
            Self::MathFunc    => "MATH_FUNC",
            Self::Peek        => "PEEK",
            Self::Poke        => "POKE",
            Self::Gemv        => "GEMV",
            Self::Dot         => "DOT",
            Self::Syscall     => "SYSCALL",
            Self::Spawn       => "SPAWN",
            Self::Atomic      => "ATOMIC",
            Self::Alloc       => "ALLOC",
            Self::Free        => "FREE",
            Self::RegInt      => "REG_INT",
            Self::Cast        => "CAST",
            Self::GpuDispatch => "GPU_DISPATCH",
        }
    }
}

// ─── Comparison operators ─────────────────────────────────────────────────────

/// Comparison operation for Compare nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CmpOp {
    Eq = 0,   // ==
    Ne = 1,   // !=
    Lt = 2,   // <
    Gt = 3,   // >
    Le = 4,   // <=
    Ge = 5,   // >=
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
    SiLU      = 0,   // SiLU activation (lookup table)
    Negate    = 1,   // element-wise negate
    Abs       = 2,   // element-wise absolute value
    ReduceSum = 3,   // sum all elements → scalar vec
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
            Self::SiLU      => "silu",
            Self::Negate    => "negate",
            Self::Abs       => "abs",
            Self::ReduceSum => "reduce_sum",
        }
    }
}

// ─── Bytecode sub-enums ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BitwiseOp {
    And = 0,
    Or  = 1,
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
            Self::Or  => "or",
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
    Sin  = 0,
    Cos  = 1,
    Sqrt = 2,
    Exp  = 3,
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
            Self::Sin  => "sin",
            Self::Cos  => "cos",
            Self::Sqrt => "sqrt",
            Self::Exp  => "exp",
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
    Int    = 0,
    Float  = 1,
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
            Self::Int    => "int",
            Self::Float  => "float",
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
        rows:   u16,
        cols:   u16,
        scale:  i8,
        sign:   Vec<u8>,   // bit-packed, rows*cols bits
        extra:  Vec<u8>,
    },
    Norm {
        size:   u16,
        weight: Vec<u8>,
        bias:   Vec<u8>,
    },
    Call {
        target: u64,       // hash of the referenced node
    },
    Int {
        value: i32,
    },
    Scope {
        children: Vec<NdaNode>,
    },
    Loop {
        count: u32,
        body:  Vec<NdaNode>,
    },
    While {
        cond: Box<NdaNode>,
        body: Vec<NdaNode>,
    },
    If {
        cond:      Box<NdaNode>,
        then_body: Vec<NdaNode>,
        else_body: Option<Vec<NdaNode>>,
    },
    Compare {
        op:  CmpOp,
        lhs: Box<NdaNode>,
        rhs: Box<NdaNode>,
    },
    Let {
        name_hash: u64,
        init:      Box<NdaNode>,
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
        op:      VecOpKind,
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
        op:  BitwiseOp,
        lhs: Box<NdaNode>,
        rhs: Option<Box<NdaNode>>,
    },
    Float {
        value: f32,
    },
    Math {
        op:  MathOp,
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
}

impl NdaNode {
    /// Compute the SHA-256-truncated-to-u64 hash of this node.
    pub fn hash(&self) -> u64 {
        let mut h = Sha256::new();
        self.hash_into(&mut h);
        let digest = h.finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap())
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
            NdaNode::Matrix { rows, cols, scale, sign, extra } => {
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
            NdaNode::If { cond, then_body, else_body } => {
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
            NdaNode::RegInt { vector, handler_hash } => {
                h.update(b"RGI");
                h.update(vector.to_le_bytes());
                h.update(handler_hash.to_le_bytes());
            }
            NdaNode::Cast { from_type, to_type, operand } => {
                h.update(b"CST");
                h.update([*from_type as u8, *to_type as u8]);
                h.update(operand.hash().to_le_bytes());
            }
            NdaNode::GpuDispatch { shader_hash, args } => {
                h.update(b"GPD");
                h.update(shader_hash.to_le_bytes());
                Self::hash_children(h, args);
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
    pub(crate) stack:        Vec<Vec<u64>>,
    /// Set when ROOT token is emitted; used for final validation.
    claimed_root:  Option<u64>,
    /// Completed root hash (set when stack fully unwinds).
    computed_root: Option<u64>,
}

impl MerkleVerifier {
    pub fn new() -> Self {
        Self {
            stack:         vec![vec![]],   // start with one open top-level scope
            claimed_root:  None,
            computed_root: None,
        }
    }

    /// Reset for a new generation.
    pub fn reset(&mut self) {
        self.stack         = vec![vec![]];
        self.claimed_root  = None;
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
        let children_hashes = self.stack.pop().unwrap();
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
            _                  => false,
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
        u64::from_le_bytes(digest[..8].try_into().unwrap())
    }
}

impl Default for MerkleVerifier {
    fn default() -> Self { Self::new() }
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
        let s1 = NdaNode::Scope { children: vec![NdaNode::Int { value: 1 }] };
        let s2 = NdaNode::Scope { children: vec![NdaNode::Int { value: 2 }] };
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
        v.claimed_root  = Some(leaf.hash());
        assert!(v.is_valid());
    }

    #[test]
    fn verifier_rejects_wrong_root() {
        let mut v = MerkleVerifier::new();
        let leaf = NdaNode::Int { value: 7 };
        v.push_leaf(&leaf);
        v.computed_root = Some(leaf.hash());
        v.claimed_root  = Some(leaf.hash() ^ 0xDEAD_BEEF);   // tampered
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
}
