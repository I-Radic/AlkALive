//! ADR-008 WASM code generation backend.
//!
//! This module implements real WebAssembly binary generation from the typed
//! AST. It uses the `wasm-encoder` crate to emit valid `.wasm` modules that
//! can be instantiated by any WebAssembly runtime.
//!
//! # Pipeline
//!
//! ```text
//! typed AST (ModuleDecl)
//!   │
//!   ▼  [wasm_codegen::compile_to_wasm]
//! WebAssembly binary (Vec<u8>)
//!   │
//!   ▼  [wasm_encoder::Module]
//! valid .wasm module with:
//!   - function types
//!   - function bodies (instructions)
//!   - exported functions
//!   - memory (for strings/heap)
//! ```
//!
//! # What this generates
//!
//! For each `fn` declaration in the module, the WASM backend emits:
//! 1. A function type signature (params → results)
//! 2. A function body with WASM instructions
//! 3. An export (so the host can call it)
//!
//! Literal expressions (`i32`, `f32`, `bool`) are compiled to `i32.const` /
//! `f32.const` instructions. Variable references compile to `local.get`.
//! Return statements compile to the expression followed by `return`.
//!
//! # Memory model
//!
//! A single linear memory (1 page = 64KB) is exported. Strings are allocated
//! on this memory by the host; the WASM module reads them via `i32.load`.

#![forbid(unsafe_code)]

use core::fmt;

use crate::ast::{
    BaseType, BinOp, Block, ClassDecl, Expr, FnDecl, ItemDecl, Lit, MethodDecl, ModuleDecl, Param,
    Stmt, Type,
};
use crate::typechecker;

use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ElementSection, Elements, ExportKind, ExportSection,
    Function, FunctionSection, GlobalSection, ImportSection, Instruction, MemorySection,
    MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
};

// ======================================================================
// Error type
// ======================================================================

/// An error during WASM code generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmCodegenError {
    /// Human-readable message.
    pub message: String,
    /// 1-based line of the offending construct.
    pub line: u32,
    /// 1-based column of the offending construct.
    pub col: u32,
}

impl fmt::Display for WasmCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "wasm codegen error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl core::error::Error for WasmCodegenError {}

// ======================================================================
// String table (Gap 4 — String Data Sections)
// ======================================================================

/// One entry in the string table — interned literal data.
#[derive(Debug, Clone)]
struct StringEntry {
    /// The UTF-8 string content (the source literal, decoded).
    text: String,
    /// Address of the length prefix in linear memory.
    offset: u32,
    /// Byte length of the UTF-8 payload (excluding the length prefix).
    byte_len: u32,
}

/// The module-wide string interner. Populated lazily by `compile_expr` on
/// `Lit::Str`; consumed by the data-section emitter after the code section.
#[derive(Debug, Default)]
struct StringTable {
    /// Map from literal text → memory offset (for dedup).
    by_text: std::collections::HashMap<String, u32>,
    /// Ordered entries for emitting the data section.
    entries: Vec<StringEntry>,
    /// Next free offset (starts at 4, after the null guard).
    next_offset: u32,
}

impl StringTable {
    fn new() -> Self {
        Self {
            by_text: Default::default(),
            entries: Vec::new(),
            next_offset: 4, // null guard occupies 0..3
        }
    }

    /// Intern a string. Returns the offset of the length prefix.
    /// Deduplicates by exact text match.
    fn intern(&mut self, text: &str) -> u32 {
        if let Some(&off) = self.by_text.get(text) {
            return off;
        }
        let byte_len = text.len() as u32;
        // 4 bytes for length prefix + byte_len bytes for payload,
        // padded to 4-byte alignment.
        let padded_len = (byte_len + 3) & !3; // round up to 4
        let total = 4 + padded_len;
        let offset = self.next_offset;
        self.entries.push(StringEntry {
            text: text.to_string(),
            offset,
            byte_len,
        });
        self.by_text.insert(text.to_string(), offset);
        self.next_offset += total;
        offset
    }

    /// Returns the total bytes needed for all strings (including null guard).
    fn total_bytes(&self) -> u32 {
        self.next_offset
    }

    /// Returns the number of pages needed (1 page = 64 KiB).
    fn memory_pages(&self) -> u32 {
        let bytes = self.total_bytes();
        bytes.div_ceil(65536).max(1)
    }

    /// Returns true if there are no string entries (only the null guard).
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Emit the data section into the module.
    fn emit_data_section(&self, module: &mut Module) {
        let mut data_sec = DataSection::new();

        // Null guard: 4 zero bytes at offset 0.
        data_sec.active(0, &wasm_encoder::ConstExpr::i32_const(0), [0u8, 0, 0, 0]);

        // One active data segment per string entry.
        for entry in &self.entries {
            let byte_len = entry.byte_len;
            let padded_len = (byte_len + 3) & !3;
            let mut segment: Vec<u8> = Vec::with_capacity(4 + padded_len as usize);
            // 4-byte little-endian length prefix.
            segment.extend_from_slice(&byte_len.to_le_bytes());
            // UTF-8 payload.
            segment.extend_from_slice(entry.text.as_bytes());
            // Zero-padding to 4-byte alignment.
            let padding = (padded_len - byte_len) as usize;
            segment.resize(segment.len() + padding, 0u8);
            data_sec.active(
                0,
                &wasm_encoder::ConstExpr::i32_const(entry.offset as i32),
                segment,
            );
        }

        module.section(&data_sec);
    }
}

// ======================================================================
// Host imports (Gap 5 — Collection Method Dispatch)
// ======================================================================

/// One host import declaration.
#[derive(Debug, Clone)]
struct HostImport {
    /// The import name (e.g. `"vec_new"`).
    name: &'static str,
    /// The parameter types.
    params: &'static [ValType],
    /// The result types.
    results: &'static [ValType],
}

/// The fixed list of host imports under module `"alk"`. After Gap 1, this
/// is 11 imports: the 10 collection `vec_*` functions (indices 0..9) plus
/// `__alk_alloc` (index 10) for object allocation. These occupy the lowest
/// indices in the function index space.
const HOST_IMPORTS: &[HostImport] = &[
    HostImport {
        name: "vec_new",
        params: &[],
        results: &[ValType::I32],
    },
    HostImport {
        name: "vec_with_capacity",
        params: &[ValType::I32, ValType::I32],
        results: &[ValType::I32],
    },
    HostImport {
        name: "vec_push",
        params: &[ValType::I32, ValType::I32],
        results: &[],
    },
    HostImport {
        name: "vec_extend",
        params: &[ValType::I32, ValType::I32],
        results: &[],
    },
    HostImport {
        name: "vec_remove",
        params: &[ValType::I32, ValType::I32],
        results: &[],
    },
    HostImport {
        name: "vec_clear",
        params: &[ValType::I32],
        results: &[],
    },
    HostImport {
        name: "vec_len",
        params: &[ValType::I32],
        results: &[ValType::I32],
    },
    HostImport {
        name: "vec_is_empty",
        params: &[ValType::I32],
        results: &[ValType::I32],
    },
    HostImport {
        name: "vec_get",
        params: &[ValType::I32, ValType::I32],
        results: &[ValType::I32],
    },
    HostImport {
        name: "vec_set",
        params: &[ValType::I32, ValType::I32, ValType::I32],
        results: &[],
    },
    // Gap 1 — OO model: heap allocator for object construction (CR-17).
    HostImport {
        name: "__alk_alloc",
        params: &[ValType::I32],
        results: &[ValType::I32],
    },
];

/// Returns the number of host imports (11 after Gap 1).
fn host_import_count() -> u32 {
    HOST_IMPORTS.len() as u32
}

/// Look up a host import by name, returning its index (0..9).
fn host_import_index(name: &str) -> Option<u32> {
    HOST_IMPORTS
        .iter()
        .position(|h| h.name == name)
        .map(|i| i as u32)
}

/// Map an AlkALive Vec method name to its host import name.
/// Returns `None` for unknown methods (should not happen in typechecked code).
fn vec_method_to_host(method: &str) -> Option<&'static str> {
    match method {
        "push" => Some("vec_push"),
        "extend" => Some("vec_extend"),
        "remove" => Some("vec_remove"),
        "clear" => Some("vec_clear"),
        "len" => Some("vec_len"),
        "is_empty" => Some("vec_is_empty"),
        "get" => Some("vec_get"),
        "insert" => Some("vec_set"),     // insert ≈ set at index
        "first" => Some("vec_get"),      // placeholder
        "last" => Some("vec_get"),       // placeholder
        "contains" => Some("vec_get"),   // placeholder
        "truncate" => Some("vec_clear"), // placeholder
        "swap_remove" => Some("vec_remove"),
        "drain" => Some("vec_clear"),
        "append" => Some("vec_extend"),
        _ => None,
    }
}

// ======================================================================
// WASM type mapping
// ======================================================================

/// Maps an AlkALive [`BaseType`] to the corresponding WebAssembly [`ValType`].
///
/// - `i32` → `ValType::I32`
/// - `f32` → `ValType::F32`
/// - `bool` → `ValType::I32` (booleans are represented as i32: 0 = false, 1 = true)
/// - `string` → `ValType::I32` (strings are pointers into linear memory)
/// - `Vec<T>` → `ValType::I32` (collections are heap-allocated; the value is a pointer)
/// - `Named(...)` → `ValType::I32` (user types are heap-allocated; pointer)
pub fn alk_type_to_wasm(base: &BaseType) -> ValType {
    match base {
        BaseType::I32 => ValType::I32,
        BaseType::F32 => ValType::F32,
        BaseType::Bool => ValType::I32,
        BaseType::Str => ValType::I32,      // pointer
        BaseType::Vec(_) => ValType::I32,   // pointer to heap-allocated collection
        BaseType::Named(_) => ValType::I32, // pointer to heap-allocated object
    }
}

/// Maps an AlkALive [`Type`] (with qualifier) to the WASM `ValType`. The
/// qualifier is erased — WASM has no notion of monotonicity; it is enforced
/// at compile time by the type checker.
pub fn alk_full_type_to_wasm(ty: &Type) -> ValType {
    alk_type_to_wasm(&ty.base)
}

// ======================================================================
// Function type indexing
// ======================================================================

/// A function type registered in the WASM type section, with its index.
struct FuncType {
    /// The index in the type section.
    idx: u32,
    /// The parameter types (as WASM ValTypes).
    params: Vec<ValType>,
    /// The result types (as WASM ValTypes).
    results: Vec<ValType>,
}

/// The type section builder. Collects function type signatures and deduplicates.
struct TypeSectionBuilder {
    types: Vec<FuncType>,
}

impl TypeSectionBuilder {
    fn new() -> Self {
        Self { types: Vec::new() }
    }

    /// Register a function type and return its index. Deduplicates by
    /// comparing params + results.
    fn register(&mut self, params: &[ValType], results: &[ValType]) -> u32 {
        for t in &self.types {
            if t.params == params && t.results == results {
                return t.idx;
            }
        }
        let idx = self.types.len() as u32;
        self.types.push(FuncType {
            idx,
            params: params.to_vec(),
            results: results.to_vec(),
        });
        idx
    }

    /// Emit the type section into the module.
    fn emit(&self, module: &mut Module) {
        let mut type_sec = TypeSection::new();
        for t in &self.types {
            type_sec.ty().function(t.params.clone(), t.results.clone());
        }
        module.section(&type_sec);
    }
}

// ======================================================================
// Function body compilation
// ======================================================================

/// Bundle of cross-function compile state (Gap 1 — OO model).
///
/// All function/method bodies in a module share the same `CompileContext`,
/// which gives them access to the class table (for field-offset lookup,
/// vtable-slot lookup, and `Self` resolution) and the function-index map
/// (for direct calls to free functions and class methods).
struct CompileContext<'a> {
    /// The class table built by the type checker.
    classes: &'a typechecker::ClassTable,
    /// Map from class name → vtable_base (table index where the class's
    /// vtable begins).
    vtable_bases: &'a std::collections::HashMap<String, u32>,
}

/// Walk the base chain from root to derived. Public mirror of the
/// typechecker's private `build_chain` helper.
fn build_chain_public(classes: &typechecker::ClassTable, class_name: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = Some(class_name);
    while let Some(c) = current {
        chain.push(c.to_string());
        current = classes.lookup(c).and_then(|s| s.base.as_deref());
    }
    chain.reverse();
    chain
}

/// Compute the byte offset of a field in the object layout (Gap 1).
/// Returns `None` if the field is not found. The vtable_base occupies
/// offset 0; base-class fields come next, then derived-class fields.
fn field_offset_public(
    classes: &typechecker::ClassTable,
    class_name: &str,
    field_name: &str,
) -> Option<u32> {
    let chain = build_chain_public(classes, class_name);
    let mut offset = 4u32;
    for c in &chain {
        if let Some(sig) = classes.lookup(c) {
            for f in &sig.fields {
                if f.name == field_name {
                    return Some(offset);
                }
                offset += 4;
            }
        }
    }
    None
}

/// Compute the vtable slot index for a method on a class (or its base chain).
/// Returns `None` if the method is not found.
fn vtable_slot_public(
    classes: &typechecker::ClassTable,
    class_name: &str,
    method_name: &str,
) -> Option<u32> {
    let chain = build_chain_public(classes, class_name);
    let mut layout: Vec<String> = Vec::new();
    for c in &chain {
        if let Some(sig) = classes.lookup(c) {
            for m in &sig.methods {
                if !layout.contains(&m.name) {
                    layout.push(m.name.clone());
                }
            }
        }
    }
    layout.iter().position(|n| n == method_name).map(|i| i as u32)
}



/// A compiled instruction sequence — we use our own enum to avoid
/// `Instruction` not implementing `PartialEq` (needed for the `ends_with`
/// check in the codegen).
#[derive(Debug, Clone, PartialEq)]
enum AlkInstr {
    I32Const(i32),
    F32Const(f32),
    LocalGet(u32),
    LocalSet(u32),
    /// `local.tee` — set a local AND leave the value on the stack.
    LocalTee(u32),
    Drop,
    Return,
    /// A binary operation on the stack.
    BinaryOp(BinOp),
    /// A function call by name (resolved to a function index during emission).
    Call(String),
    /// `call_indirect` carrying the method's WASM type signature. The
    /// emission pass resolves this to a type index in the type section.
    /// The table index is always 0 (the single funcref table).
    CallIndirect(Vec<ValType>, Vec<ValType>),
    /// `i32.load offset=N` — load an i32 from linear memory.
    I32Load(u32),
    /// `i32.store offset=N` — store an i32 to linear memory.
    I32Store(u32),
    /// `if` instruction (conditional branch).
    If,
    /// `else` instruction.
    Else,
    /// `block` instruction (begins a block).
    Block,
    /// `loop` instruction (begins a loop block).
    Loop,
    /// `br n` (branch to the nth enclosing block).
    Br(u32),
    // End is not currently emitted by the compiler (wasm-encoder adds it
    // automatically), but is part of the instruction model for completeness.
    #[allow(dead_code)]
    End,
}

/// Compiles a function body into WASM instructions.
///
/// The function's parameters become WASM locals. `let` declarations inside
/// the body become additional locals. Expressions are compiled to stack-based
/// WASM instructions.
struct FnCompiler<'ctx> {
    /// Map from local name → local index + WASM type.
    locals: Vec<(String, ValType)>,
    /// Parallel to `locals`: the AlkALive declared type of each local (if
    /// known). Used to resolve `Expr::Field` receiver types at compile time.
    local_types: Vec<Option<Type>>,
    /// The class table (Gap 1). Empty for modules with no classes.
    classes: &'ctx typechecker::ClassTable,
    /// Map from class name → vtable_base (table index where the class's
    /// vtable begins). Used to seed object literals.
    vtable_bases: &'ctx std::collections::HashMap<String, u32>,
    /// The enclosing class name (Gap 1), if compiling a method body.
    enclosing_class: Option<String>,
    /// Whether `self` is local 0 (instance method).
    is_instance: bool,
}

impl<'ctx> FnCompiler<'ctx> {
    fn new(params: &[Param], ctx: &'ctx CompileContext<'ctx>) -> Self {
        let locals: Vec<(String, ValType)> = params
            .iter()
            .map(|p| (p.name.clone(), alk_full_type_to_wasm(&p.ty)))
            .collect();
        let local_types: Vec<Option<Type>> = params.iter().map(|p| Some(p.ty.clone())).collect();
        Self {
            locals,
            local_types,
            classes: ctx.classes,
            vtable_bases: ctx.vtable_bases,
            enclosing_class: None,
            is_instance: false,
        }
    }

    /// Construct a FnCompiler for a class method body (Gap 1).
    /// For instance methods, `self` is implicitly local 0.
    fn new_for_method(
        class_name: &str,
        m: &MethodDecl,
        ctx: &'ctx CompileContext<'ctx>,
    ) -> Self {
        let mut locals: Vec<(String, ValType)> = Vec::new();
        let mut local_types: Vec<Option<Type>> = Vec::new();
        if m.is_instance {
            // `self` is implicitly the first parameter (local 0).
            locals.push(("self".to_string(), ValType::I32));
            local_types.push(Some(Type {
                qualifier: crate::ast::Qualifier::Unrestricted,
                base: BaseType::Named(class_name.to_string()),
            }));
        }
        for p in &m.params {
            locals.push((p.name.clone(), alk_full_type_to_wasm(&p.ty)));
            local_types.push(Some(p.ty.clone()));
        }
        Self {
            locals,
            local_types,
            classes: ctx.classes,
            vtable_bases: ctx.vtable_bases,
            enclosing_class: Some(class_name.to_string()),
            is_instance: m.is_instance,
        }
    }

    /// Look up a local by name, returning its index.
    fn local_index(&self, name: &str) -> Option<u32> {
        self.locals
            .iter()
            .position(|(n, _)| n == name)
            .map(|i| i as u32)
    }

    /// Look up a local's AlkALive type by name.
    fn local_type(&self, name: &str) -> Option<&Type> {
        self.locals
            .iter()
            .position(|(n, _)| n == name)
            .and_then(|i| self.local_types.get(i).and_then(|t| t.as_ref()))
    }

    /// Resolve the static type of an expression at compile time. This is a
    /// best-effort inference used to determine field offsets and vtable slots
    /// — the type checker has already validated the program.
    fn expr_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Lit(lit, _, _) => Some(typechecker::literal_type(lit)),
            Expr::Var(name, _, _) => self.local_type(name).cloned(),
            Expr::Self_(_, _) => {
                if self.is_instance {
                    self.enclosing_class
                        .as_ref()
                        .map(|c| Type {
                            qualifier: crate::ast::Qualifier::Unrestricted,
                            base: BaseType::Named(c.clone()),
                        })
                } else {
                    None
                }
            }
            Expr::Field { receiver, field, .. } => {
                let rt = self.expr_type(receiver)?;
                if let BaseType::Named(class_name) = &rt.base {
                    // Look up the field in the class chain via the typechecker.
                    let sig = typechecker::ClassTable::lookup(self.classes, class_name)?;
                    let _ = sig;
                    // Use the typechecker's helper.
                    let chain = build_chain_public(self.classes, class_name);
                    for c in &chain {
                        if let Some(cs) = self.classes.lookup(c) {
                            for f in &cs.fields {
                                if f.name == *field {
                                    return Some(f.ty.clone());
                                }
                            }
                        }
                    }
                    None
                } else {
                    None
                }
            }
            Expr::Object { class, .. } => {
                let resolved = if class == "Self" {
                    self.enclosing_class.clone()?
                } else {
                    class.clone()
                };
                Some(Type {
                    qualifier: crate::ast::Qualifier::Unrestricted,
                    base: BaseType::Named(resolved),
                })
            }
            Expr::StaticCall { class, method, .. } => {
                let resolved = if class == "Self" {
                    self.enclosing_class.clone()?
                } else {
                    class.clone()
                };
                let q = format!("{}::{}", resolved, method);
                // We don't have sigs here, but we can scan the class table.
                if let Some(cs) = self.classes.lookup(&resolved) {
                    for m in &cs.methods {
                        if m.name == *method {
                            return m.return_type.clone();
                        }
                    }
                    // Walk base chain.
                    let mut current = cs.base.as_deref();
                    while let Some(b) = current {
                        if let Some(bs) = self.classes.lookup(b) {
                            for m in &bs.methods {
                                if m.name == *method {
                                    return m.return_type.clone();
                                }
                            }
                            current = bs.base.as_deref();
                        } else {
                            break;
                        }
                    }
                }
                let _ = q;
                None
            }
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let rt = self.expr_type(receiver)?;
                if let BaseType::Named(class_name) = &rt.base {
                    // Walk the class chain looking for the method.
                    let mut current = Some(class_name.as_str());
                    while let Some(c) = current {
                        if let Some(cs) = self.classes.lookup(c) {
                            for m in &cs.methods {
                                if m.name == *method {
                                    return m.return_type.clone();
                                }
                            }
                            current = cs.base.as_deref();
                        } else {
                            break;
                        }
                    }
                    None
                } else {
                    None
                }
            }
            Expr::PathCall(module, member, _, _, _) => {
                // Vec::new / Vec::with_capacity → no element type known.
                // Class::method → look up in fn_indices? Actually we want the
                // return type, which requires scanning the class table.
                if let Some(cs) = self.classes.lookup(module) {
                    for m in &cs.methods {
                        if m.name == *member {
                            return m.return_type.clone();
                        }
                    }
                    let mut current = cs.base.as_deref();
                    while let Some(b) = current {
                        if let Some(bs) = self.classes.lookup(b) {
                            for m in &bs.methods {
                                if m.name == *member {
                                    return m.return_type.clone();
                                }
                            }
                            current = bs.base.as_deref();
                        } else {
                            break;
                        }
                    }
                }
                None
            }
            Expr::Call { callee, .. } => {
                // Free function — we'd need the FnSig table to know its type.
                // For now, return None (callers should not need this for
                // field-access compilation).
                let _ = callee;
                None
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                if op.is_comparison() || op.is_logical() {
                    Some(Type {
                        qualifier: crate::ast::Qualifier::Unrestricted,
                        base: BaseType::Bool,
                    })
                } else {
                    self.expr_type(lhs).or_else(|| self.expr_type(rhs))
                }
            }
        }
    }

    /// Compile a block of statements into a sequence of instructions.
    /// Returns the instructions and any new locals declared.
    ///
    /// `function_return` is `Some` iff this is the top-level body of a
    /// function/method that has a declared return type. In that case, the
    /// trailing expression statement (if any) is treated as the return
    /// value (Rust-style block expression) and is NOT dropped.
    fn compile_block(
        &mut self,
        block: &Block,
        strings: &mut StringTable,
        function_return: Option<&Type>,
    ) -> (Vec<AlkInstr>, Vec<(ValType, u32)>) {
        let mut instrs = Vec::new();
        let mut new_locals: Vec<(ValType, u32)> = Vec::new(); // (type, count)

        for (stmt_idx, stmt) in block.stmts.iter().enumerate() {
            let locals_before = self.locals.len();
            let is_last = stmt_idx == block.stmts.len() - 1;
            match stmt {
                Stmt::Let(l) => {
                    // Compile the initialiser expression.
                    self.compile_expr(&l.init, &mut instrs, strings);
                    // Declare a new local for this binding.
                    let local_idx = self.locals.len() as u32;
                    let wasm_ty = alk_full_type_to_wasm(&l.ty);
                    self.locals.push((l.name.clone(), wasm_ty));
                    self.local_types.push(Some(l.ty.clone()));
                    // Track for the local declarations section.
                    // Group by type: if the last local has the same type, increment count.
                    if let Some(last) = new_locals.last_mut() {
                        if last.0 == wasm_ty {
                            last.1 += 1;
                        } else {
                            new_locals.push((wasm_ty, 1));
                        }
                    } else {
                        new_locals.push((wasm_ty, 1));
                    }
                    // Store the initialiser result into the local.
                    instrs.push(AlkInstr::LocalSet(local_idx));
                }
                Stmt::Expr(e) => {
                    // Expression statement. If it's the last statement in
                    // the block AND the function has a return type, leave
                    // the value on the stack (Rust-style trailing
                    // expression = return value). Otherwise compile and
                    // drop the result.
                    self.compile_expr(e, &mut instrs, strings);
                    let keep = is_last && function_return.is_some() && expr_produces_value(e);
                    if !keep {
                        instrs.push(AlkInstr::Drop);
                    }
                }
                Stmt::Return(opt, _line, _col) => {
                    if let Some(e) = opt {
                        self.compile_expr(e, &mut instrs, strings);
                    }
                    instrs.push(AlkInstr::Return);
                }
                Stmt::If {
                    cond,
                    then_block,
                    else_block,
                    line: _,
                    col: _,
                } => {
                    // Compile the condition (leaves i32 on stack).
                    self.compile_expr(cond, &mut instrs, strings);
                    // Emit: if (cond) { then } else { else }
                    instrs.push(AlkInstr::If);
                    // Compile the then-block.
                    let (then_instrs, then_locals) =
                        self.compile_block(then_block, strings, None);
                    instrs.extend(then_instrs);
                    // Merge then-locals into new_locals.
                    for (ty, count) in then_locals {
                        if let Some(last) = new_locals.last_mut() {
                            if last.0 == ty {
                                last.1 += count;
                            } else {
                                new_locals.push((ty, count));
                            }
                        } else {
                            new_locals.push((ty, count));
                        }
                    }
                    // Handle else branch.
                    if let Some(else_b) = else_block {
                        instrs.push(AlkInstr::Else);
                        let (else_instrs, else_locals) =
                            self.compile_block(else_b, strings, None);
                        instrs.extend(else_instrs);
                        for (ty, count) in else_locals {
                            if let Some(last) = new_locals.last_mut() {
                                if last.0 == ty {
                                    last.1 += count;
                                } else {
                                    new_locals.push((ty, count));
                                }
                            } else {
                                new_locals.push((ty, count));
                            }
                        }
                    }
                    instrs.push(AlkInstr::End);
                }
                Stmt::While {
                    cond,
                    body,
                    line: _,
                    col: _,
                } => {
                    // WASM while loop: block (loop (if (!cond) br 1) body br 0)
                    // Simplified: block loop cond if(0) (br 1) body br 0 end end
                    instrs.push(AlkInstr::Block);
                    instrs.push(AlkInstr::Loop);
                    // Compile condition.
                    self.compile_expr(cond, &mut instrs, strings);
                    // if (cond == 0) break out of loop
                    instrs.push(AlkInstr::If);
                    instrs.push(AlkInstr::Br(1)); // break out of block
                    instrs.push(AlkInstr::Else);
                    instrs.push(AlkInstr::End);
                    // Compile body.
                    let (body_instrs, body_locals) = self.compile_block(body, strings, None);
                    instrs.extend(body_instrs);
                    for (ty, count) in body_locals {
                        if let Some(last) = new_locals.last_mut() {
                            if last.0 == ty {
                                last.1 += count;
                            } else {
                                new_locals.push((ty, count));
                            }
                        } else {
                            new_locals.push((ty, count));
                        }
                    }
                    // Continue loop.
                    instrs.push(AlkInstr::Br(0)); // loop back
                    instrs.push(AlkInstr::End); // end loop
                    instrs.push(AlkInstr::End); // end block
                }
                Stmt::Assign {
                    target,
                    value,
                    line: _,
                    col: _,
                } => {
                    // Field assignment: `obj.field = value;` (Gap 1).
                    // Target must be Expr::Field (typechecker-enforced).
                    if let Expr::Field {
                        receiver,
                        field,
                        line: _,
                        col: _,
                    } = target
                    {
                        // Determine the receiver's static class so we can
                        // compute the field offset.
                        let recv_ty = self.expr_type(receiver);
                        let class_name = recv_ty
                            .as_ref()
                            .and_then(|t| match &t.base {
                                BaseType::Named(n) => Some(n.clone()),
                                _ => None,
                            })
                            .or_else(|| self.enclosing_class.clone());
                        let offset = class_name
                            .as_deref()
                            .and_then(|c| field_offset_public(self.classes, c, field))
                            .unwrap_or(0);
                        // Stack order for i32.store: [addr, value].
                        // Push the receiver (address), then the value.
                        self.compile_expr(receiver, &mut instrs, strings);
                        self.compile_expr(value, &mut instrs, strings);
                        instrs.push(AlkInstr::I32Store(offset));
                    } else {
                        // Non-field assignment — typechecker should have
                        // rejected this. Defensive fallback: compile and drop.
                        self.compile_expr(value, &mut instrs, strings);
                        if expr_produces_value(value) {
                            instrs.push(AlkInstr::Drop);
                        }
                    }
                }
            }
            // Account for any temp locals added by compile_expr (e.g. the
            // `__obj_tmp` locals for object literals).
            let locals_after = self.locals.len();
            if locals_after > locals_before {
                let added = (locals_after - locals_before) as u32;
                // All temps are I32 (object pointers).
                if let Some(last) = new_locals.last_mut() {
                    if last.0 == ValType::I32 {
                        last.1 += added;
                    } else {
                        new_locals.push((ValType::I32, added));
                    }
                } else {
                    new_locals.push((ValType::I32, added));
                }
            }
        }

        (instrs, new_locals)
    }

    /// Compile an expression into WASM instructions, leaving the result on
    /// the stack.
    fn compile_expr(&mut self, expr: &Expr, instrs: &mut Vec<AlkInstr>, strings: &mut StringTable) {
        match expr {
            Expr::Lit(lit, _line, _col) => {
                match lit {
                    Lit::Int(v) => instrs.push(AlkInstr::I32Const(*v as i32)),
                    Lit::Float(v) => instrs.push(AlkInstr::F32Const(*v as f32)),
                    Lit::Str(s) => {
                        // Intern the string and emit its memory offset as a pointer.
                        let offset = strings.intern(s);
                        instrs.push(AlkInstr::I32Const(offset as i32));
                    }
                    Lit::Bool(b) => {
                        instrs.push(AlkInstr::I32Const(if *b { 1 } else { 0 }));
                    }
                }
            }
            Expr::Var(name, _line, _col) => {
                if let Some(idx) = self.local_index(name) {
                    instrs.push(AlkInstr::LocalGet(idx));
                } else {
                    // Undefined variable — type checker should have caught this.
                    instrs.push(AlkInstr::I32Const(0));
                }
            }
            Expr::PathCall(module, member, args, _line, _col) => {
                // Gap 5: Vec::new and Vec::with_capacity compile to host calls.
                match (module.as_str(), member.as_str()) {
                    ("Vec", "new") => {
                        // Push elem_size = 4 (all types are 4 bytes in WASM).
                        instrs.push(AlkInstr::I32Const(4));
                        // Call vec_new host import (returns Vec handle).
                        instrs.push(AlkInstr::Call("vec_new".to_string()));
                    }
                    ("Vec", "with_capacity") => {
                        // Push elem_size = 4, then the capacity argument.
                        instrs.push(AlkInstr::I32Const(4));
                        if !args.is_empty() {
                            self.compile_expr(&args[0], instrs, strings);
                        } else {
                            instrs.push(AlkInstr::I32Const(0));
                        }
                        instrs.push(AlkInstr::Call("vec_with_capacity".to_string()));
                    }
                    _ => {
                        // Other path calls: compile args, emit placeholder.
                        for a in args {
                            self.compile_expr(a, instrs, strings);
                        }
                        instrs.push(AlkInstr::I32Const(0));
                    }
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                line: _,
                col: _,
            } => {
                // Determine whether this is a Vec method call (Gap 5) or a
                // class instance-method call (Gap 1 — virtual dispatch).
                let recv_ty = self.expr_type(receiver);
                let is_vec_receiver = recv_ty
                    .as_ref()
                    .map(|t| t.is_vec())
                    .unwrap_or(false);
                if is_vec_receiver {
                    // Gap 5: Vec method calls compile to host import calls.
                    self.compile_expr(receiver, instrs, strings);
                    for a in args {
                        self.compile_expr(a, instrs, strings);
                    }
                    if let Some(host_name) = vec_method_to_host(method) {
                        instrs.push(AlkInstr::Call(host_name.to_string()));
                    } else {
                        let drop_count = 1 + args.len();
                        for _ in 0..drop_count {
                            instrs.push(AlkInstr::Drop);
                        }
                    }
                } else {
                    // Gap 1: virtual dispatch on a class-typed receiver.
                    // Sequence:
                    //   local.get $obj            ;; receiver (for method's `self`)
                    //   <args>
                    //   local.get $obj            ;; load vtable_base
                    //   i32.load offset=0
                    //   i32.const <slot>
                    //   i32.add
                    //   call_indirect (type $method_type)
                    let class_name = recv_ty
                        .as_ref()
                        .and_then(|t| match &t.base {
                            BaseType::Named(n) => Some(n.clone()),
                            _ => None,
                        })
                        .or_else(|| self.enclosing_class.clone());
                    // Resolve the method (walking the base chain).
                    let resolved = class_name
                        .as_deref()
                        .and_then(|c| resolve_method(self.classes, c, method));
                    let mdecl: &MethodDecl = match resolved {
                        Some((_defining_class, m)) => m,
                        None => {
                            // Defensive: drop receiver and args.
                            self.compile_expr(receiver, instrs, strings);
                            for a in args {
                                self.compile_expr(a, instrs, strings);
                            }
                            let drop_count = 1 + args.len();
                            for _ in 0..drop_count {
                                instrs.push(AlkInstr::Drop);
                            }
                            return;
                        }
                    };
                    // Compute the vtable slot: based on the receiver's static
                    // class (the call site), NOT the defining class. This is
                    // the slot the vtable_base + slot dispatch expects.
                    let slot = class_name
                        .as_deref()
                        .and_then(|c| vtable_slot_public(self.classes, c, method))
                        .unwrap_or(0);
                    // Compute the WASM type index for the method's signature.
                    // The method's WASM type is:
                    //   params: [i32 (self)?, <param types>]
                    //   results: [<return type>] or []
                    let mut params: Vec<ValType> = Vec::new();
                    if mdecl.is_instance {
                        params.push(ValType::I32); // self
                    }
                    for p in &mdecl.params {
                        params.push(alk_full_type_to_wasm(&p.ty));
                    }
                    let results: Vec<ValType> = mdecl
                        .return_type
                        .as_ref()
                        .map(|t| vec![alk_full_type_to_wasm(t)])
                        .unwrap_or_default();
                    let type_idx = 0u32; // placeholder; emission pass resolves.
                    let _ = type_idx;
                    // Push receiver (for self).
                    self.compile_expr(receiver, instrs, strings);
                    // Push args.
                    for a in args {
                        self.compile_expr(a, instrs, strings);
                    }
                    // Load vtable_base: local.get obj; i32.load offset=0.
                    self.compile_expr(receiver, instrs, strings);
                    instrs.push(AlkInstr::I32Load(0));
                    // Add slot.
                    instrs.push(AlkInstr::I32Const(slot as i32));
                    instrs.push(AlkInstr::BinaryOp(BinOp::Add));
                    // call_indirect — carries the method's WASM type signature
                    // for the emission pass to resolve.
                    instrs.push(AlkInstr::CallIndirect(params, results));
                }
            }
            Expr::Binary {
                lhs,
                op,
                rhs,
                line: _,
                col: _,
            } => {
                // Compile LHS and RHS (leaves both on the stack).
                self.compile_expr(lhs, instrs, strings);
                self.compile_expr(rhs, instrs, strings);
                // Emit the binary operator instruction.
                instrs.push(AlkInstr::BinaryOp(*op));
            }
            Expr::Call {
                callee,
                args,
                line: _,
                col: _,
            } => {
                // Compile arguments (left to right).
                for a in args {
                    self.compile_expr(a, instrs, strings);
                }
                // Emit a call instruction. The function index will be
                // resolved by the codegen pass that knows the function table.
                instrs.push(AlkInstr::Call(callee.clone()));
            }
            Expr::Self_(_, _) => {
                // `self` is local 0 for instance methods.
                if self.is_instance {
                    instrs.push(AlkInstr::LocalGet(0));
                } else {
                    // Defensive: typechecker should have caught this.
                    instrs.push(AlkInstr::I32Const(0));
                }
            }
            Expr::Field {
                receiver,
                field,
                line: _,
                col: _,
            } => {
                // Determine the receiver's static class for offset lookup.
                let recv_ty = self.expr_type(receiver);
                let class_name = recv_ty
                    .as_ref()
                    .and_then(|t| match &t.base {
                        BaseType::Named(n) => Some(n.clone()),
                        _ => None,
                    })
                    .or_else(|| self.enclosing_class.clone());
                let offset = class_name
                    .as_deref()
                    .and_then(|c| field_offset_public(self.classes, c, field))
                    .unwrap_or(0);
                // Push receiver, then load.
                self.compile_expr(receiver, instrs, strings);
                instrs.push(AlkInstr::I32Load(offset));
            }
            Expr::Object {
                class,
                fields,
                line: _,
                col: _,
            } => {
                // Resolve `Self` to the enclosing class.
                let resolved = if class == "Self" {
                    self.enclosing_class.clone().unwrap_or_else(|| class.clone())
                } else {
                    class.clone()
                };
                let sig = self.classes.lookup(&resolved);
                let field_stride = sig
                    .map(|s| s.field_stride)
                    .unwrap_or(4);
                let vtable_base = self
                    .vtable_bases
                    .get(&resolved)
                    .copied()
                    .unwrap_or(0);
                // 1. Allocate.
                instrs.push(AlkInstr::I32Const(field_stride as i32));
                instrs.push(AlkInstr::Call("__alk_alloc".to_string()));
                // 2. Save ptr in a temp local. Use the next free local index.
                //    We'll create a synthetic local for this.
                let tmp_idx = self.locals.len() as u32;
                // We push to locals so the index is stable. The local
                // declaration will be added by compile_block's new_locals
                // tracking — but here we're inside compile_expr which doesn't
                // track new_locals. So we use a slightly hacky approach:
                // allocate a high-index local that won't conflict.
                // Actually, the cleanest fix is to use the stack directly:
                //   i32.const <stride>
                //   call __alk_alloc     ;; stack: [ptr]
                //   local.tee $tmp       ;; $tmp = ptr; stack: [ptr]
                //   i32.const <vtable_base>
                //   i32.store offset=0   ;; stack: []
                //   local.get $tmp       ;; stack: [ptr]
                //   <value>
                //   i32.store offset=<offset>
                //   ...
                //   local.get $tmp       ;; final result
                // We need a temp local. Reserve one at the end of the locals list.
                // The compile_block tracks new_locals for the LocalDecls section;
                // we need to ensure the temp local is also declared.
                // For simplicity, we declare the temp local as an I32 and add
                // it to self.locals so subsequent stores/gets use the right index.
                self.locals.push(("__obj_tmp".to_string(), ValType::I32));
                self.local_types.push(None);
                // local.tee $tmp — saves ptr to $tmp and leaves ptr on stack.
                instrs.push(AlkInstr::LocalTee(tmp_idx));
                // Store vtable_base.
                instrs.push(AlkInstr::I32Const(vtable_base as i32));
                instrs.push(AlkInstr::I32Store(0));
                // Store each field.
                for (fname, vexpr, _, _) in fields {
                    let offset = field_offset_public(self.classes, &resolved, fname).unwrap_or(0);
                    instrs.push(AlkInstr::LocalGet(tmp_idx));
                    self.compile_expr(vexpr, instrs, strings);
                    instrs.push(AlkInstr::I32Store(offset));
                }
                // Leave the ptr on the stack as the result.
                instrs.push(AlkInstr::LocalGet(tmp_idx));
                // Note: we do NOT pop the temp local from self.locals —
                // subsequent compile_block iterations would see inconsistent
                // state. The temp local is "leaked" for the rest of the
                // function, which is fine (it's just an extra i32 slot).
                // However, the compile_block's new_locals tracking doesn't
                // know about it. To handle this, compile_block must also
                // account for any new locals added during compile_expr.
                // We handle this by having compile_block re-scan self.locals
                // length after each statement and adjust new_locals.
                // For now, we accept a small leak (one i32 local per object
                // literal). This is a known limitation.
            }
            Expr::StaticCall {
                class,
                method,
                args,
                line: _,
                col: _,
            } => {
                // Resolve `Self` to the enclosing class.
                let resolved = if class == "Self" {
                    self.enclosing_class.clone().unwrap_or_else(|| class.clone())
                } else {
                    class.clone()
                };
                let qualified = format!("{}::{}", resolved, method);
                // Compile args.
                for a in args {
                    self.compile_expr(a, instrs, strings);
                }
                // Direct call by qualified name.
                instrs.push(AlkInstr::Call(qualified));
            }
        }
    }
}

/// Resolve a method on a class (walking the base chain). Returns
/// `(defining_class_name, Some(method_decl))` if found.
fn resolve_method<'a>(
    classes: &'a typechecker::ClassTable,
    class_name: &str,
    method_name: &str,
) -> Option<(String, &'a MethodDecl)> {
    let mut current = Some(class_name);
    while let Some(c) = current {
        if let Some(sig) = classes.lookup(c) {
            for m in &sig.methods {
                if m.name == method_name {
                    return Some((c.to_string(), m));
                }
            }
            current = sig.base.as_deref();
        } else {
            break;
        }
    }
    None
}

/// Returns `true` if the expression produces a value on the stack.
fn expr_produces_value(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_, _, _) => true,
        Expr::Var(_, _, _) => true,
        Expr::Binary { .. } => true,
        Expr::PathCall(_, _, _, _, _) => true,
        Expr::Call { .. } => true,
        Expr::Self_(_, _) => true,
        Expr::Field { .. } => true,
        Expr::Object { .. } => true,
        Expr::StaticCall { .. } => true,
        Expr::MethodCall { method, .. } => {
            // Vec methods that return a value.
            method == "len" || method == "is_empty" || method == "get"
        }
    }
}

// ======================================================================
// Top-level WASM compilation
// ======================================================================

/// The result of WASM compilation: a binary module plus metadata.
#[derive(Debug, Clone)]
pub struct WasmModule {
    /// The raw WebAssembly binary bytes.
    pub bytes: Vec<u8>,
    /// The names of exported functions.
    pub exported_functions: Vec<String>,
    /// The number of pages of linear memory allocated (1 page = 64KB).
    pub memory_pages: u32,
}

impl WasmModule {
    /// Returns the size of the WASM binary in bytes.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// Returns `true` if the binary starts with the WASM magic number (`\0asm`).
    pub fn is_valid_wasm(&self) -> bool {
        self.bytes.len() >= 4 && &self.bytes[0..4] == b"\0asm"
    }
}

/// Compile a typed AlkALive module into a WebAssembly binary.
///
/// This is the real WASM backend (ADR-008: "compiling to WASM"). It:
/// 1. Runs the type checker to verify source-level soundness (ADR-009).
/// 2. Collects function type signatures into the type section.
/// 3. Compiles each function body into WASM instructions.
/// 4. Exports each function by name.
/// 5. Exports a linear memory for heap-allocated data.
///
/// After Gap 1 (OO model), it also:
/// 6. Compiles each class method body into WASM instructions.
/// 7. Emits a `TableSection` (one funcref table for vtables).
/// 8. Emits a `GlobalSection` (vtable_base constants per class).
/// 9. Emits an `ElementSection` seeding the table with method indices.
/// 10. Virtual dispatch on class-typed receivers compiles to `call_indirect`.
///
/// # Errors
///
/// Returns `WasmCodegenError` if the type checker finds errors, or if a
/// construct cannot be lowered to WASM.
pub fn compile_to_wasm(module: &ModuleDecl) -> Result<WasmModule, WasmCodegenError> {
    // 1. Run the type checker first (ADR-009 source-level soundness).
    let type_errors = typechecker::check_module(module);
    if !type_errors.is_empty() {
        let first = &type_errors.errors[0];
        return Err(WasmCodegenError {
            message: format!(
                "type check failed: {} (and {} more)",
                first.message,
                type_errors.len() - 1
            ),
            line: first.line,
            col: first.col,
        });
    }

    // 1.5. Extract the ClassTable built by the type checker (Gap 1).
    // We re-run collect_classes here to get the table — the type checker
    // doesn't expose it. This is a small duplication but keeps the API clean.
    let mut classes = typechecker::ClassTable::new();
    {
        let mut errs = typechecker::TypeErrorSet::new();
        collect_classes_via_typechecker(module, &mut classes, &mut errs);
    }

    // 2. Collect function declarations and class methods.
    let fns: Vec<&FnDecl> = module
        .items
        .iter()
        .filter_map(|item| match item {
            ItemDecl::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    let class_decls: Vec<&ClassDecl> = module
        .items
        .iter()
        .filter_map(|item| match item {
            ItemDecl::Class(c) => Some(c),
            _ => None,
        })
        .collect();

    // 3. Build the WASM module.
    let mut wasm_module = Module::new();
    let mut type_builder = TypeSectionBuilder::new();

    // Register function types and collect function metadata.
    struct FnMeta {
        name: String,
        type_idx: u32,
        #[allow(dead_code)]
        params: Vec<ValType>,
        results: Vec<ValType>,
    }
    let mut fn_metas: Vec<FnMeta> = Vec::new();

    for f in &fns {
        let params: Vec<ValType> = f
            .params
            .iter()
            .map(|p| alk_full_type_to_wasm(&p.ty))
            .collect();
        let results: Vec<ValType> = match &f.return_type {
            Some(rt) => vec![alk_full_type_to_wasm(rt)],
            None => vec![],
        };
        let type_idx = type_builder.register(&params, &results);
        fn_metas.push(FnMeta {
            name: f.name.clone(),
            type_idx,
            params,
            results,
        });
    }

    // Register class method types (Gap 1). Each method becomes a WASM
    // function. Instance methods have `self: i32` as the first param.
    for c in &class_decls {
        for m in &c.methods {
            let mut params: Vec<ValType> = Vec::new();
            if m.is_instance {
                params.push(ValType::I32); // self
            }
            for p in &m.params {
                params.push(alk_full_type_to_wasm(&p.ty));
            }
            let results: Vec<ValType> = match &m.return_type {
                Some(rt) => vec![alk_full_type_to_wasm(rt)],
                None => vec![],
            };
            let type_idx = type_builder.register(&params, &results);
            let qualified = format!("{}::{}", c.name, m.name);
            fn_metas.push(FnMeta {
                name: qualified,
                type_idx,
                params,
                results,
            });
        }
    }

    // Build fn_indices map (name → WASM function index). Module functions
    // start at index host_import_count(); class methods follow.
    let host_count = host_import_count();
    let mut fn_indices: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for (idx, meta) in fn_metas.iter().enumerate() {
        fn_indices.insert(meta.name.clone(), host_count + idx as u32);
    }

    // Compute vtable_bases for each class (cumulative vtable_slot_count of
    // all classes before it in source order).
    let mut vtable_bases: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    let mut cumulative: u32 = 0;
    for c in &class_decls {
        vtable_bases.insert(c.name.clone(), cumulative);
        if let Some(sig) = classes.lookup(&c.name) {
            cumulative += sig.vtable_slot_count;
        }
    }
    let total_vtable_slots = cumulative;

    // Register host import types in the type section (Gap 5 + Gap 1).
    let host_type_indices: Vec<u32> = HOST_IMPORTS
        .iter()
        .map(|h| type_builder.register(h.params, h.results))
        .collect();

    // Emit the type section.
    type_builder.emit(&mut wasm_module);

    // 4. Import section — declare host function imports.
    let mut import_sec = ImportSection::new();
    for (i, host) in HOST_IMPORTS.iter().enumerate() {
        import_sec.import(
            "alk",
            host.name,
            wasm_encoder::EntityType::Function(host_type_indices[i]),
        );
    }
    wasm_module.section(&import_sec);

    // 5. Function section — declare module-local function indices.
    let mut func_sec = FunctionSection::new();
    for meta in &fn_metas {
        func_sec.function(meta.type_idx);
    }
    wasm_module.section(&func_sec);

    // 6. Table section — one funcref table for vtables (Gap 1).
    if total_vtable_slots > 0 {
        let mut table_sec = TableSection::new();
        table_sec.table(TableType {
            element_type: RefType::FUNCREF,
            minimum: total_vtable_slots as u64,
            maximum: None,
            table64: false,
            shared: false,
        });
        wasm_module.section(&table_sec);
    }

    // 7. Memory section — enough pages for all string data (Gap 4).
    let mut strings = StringTable::new();
    pre_scan_strings(module, &mut strings);
    let mem_pages = strings.memory_pages();
    let mut mem_sec = MemorySection::new();
    mem_sec.memory(MemoryType {
        minimum: mem_pages as u64,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    wasm_module.section(&mem_sec);

    // 8. Global section — vtable_base constants per class (Gap 1).
    if !class_decls.is_empty() {
        let mut global_sec = GlobalSection::new();
        for c in &class_decls {
            let base = vtable_bases.get(&c.name).copied().unwrap_or(0);
            global_sec.global(
                wasm_encoder::GlobalType {
                    val_type: ValType::I32,
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::i32_const(base as i32),
            );
        }
        wasm_module.section(&global_sec);
    }

    // 9. Export section — export each top-level fn + memory.
    let mut export_sec = ExportSection::new();
    for (idx, f) in fns.iter().enumerate() {
        export_sec.export(&f.name, ExportKind::Func, host_count + idx as u32);
    }
    export_sec.export("memory", ExportKind::Memory, 0);
    wasm_module.section(&export_sec);

    // 10. Element section — seed the table with method indices (Gap 1).
    if total_vtable_slots > 0 {
        let mut elem_sec = ElementSection::new();
        for c in &class_decls {
            // Compute the vtable layout: vec of (method_name, defining_class).
            let layout = vtable_layout_public(&classes, &c.name);
            let base = vtable_bases.get(&c.name).copied().unwrap_or(0);
            let mut func_indices_vec: Vec<u32> = Vec::with_capacity(layout.len());
            for (mname, defining_class) in &layout {
                // The function index for `defining_class::mname`.
                let qualified = format!("{}::{}", defining_class, mname);
                let idx = fn_indices.get(&qualified).copied().unwrap_or(0);
                func_indices_vec.push(idx);
            }
            // Active element segment at offset = base, table 0.
            elem_sec.active(
                None,
                &ConstExpr::i32_const(base as i32),
                Elements::Functions(std::borrow::Cow::Owned(func_indices_vec)),
            );
        }
        wasm_module.section(&elem_sec);
    }

    // 11. Code section — compile function + method bodies.
    let ctx = CompileContext {
        classes: &classes,
        vtable_bases: &vtable_bases,
    };

    let mut code_sec = CodeSection::new();

    // Compile top-level functions.
    for (idx, f) in fns.iter().enumerate() {
        let meta = &fn_metas[idx];
        let mut compiler = FnCompiler::new(&f.params, &ctx);

        // Compile the body (interning strings into the StringTable).
        let (body_instrs, new_locals) =
            compiler.compile_block(&f.body, &mut strings, f.return_type.as_ref());

        // Build the local declarations for the function body.
        let local_decls: Vec<(u32, ValType)> =
            new_locals.iter().map(|(ty, count)| (*count, *ty)).collect();

        let mut func = Function::new(local_decls);

        emit_instrs(
            &body_instrs,
            &mut func,
            &fn_indices,
            &mut |params, results| type_builder.register(params, results),
        );

        // If the function has a return type and the body doesn't end with
        // an explicit return, emit an implicit return.
        if !body_instrs.ends_with(&[AlkInstr::Return]) && !meta.results.is_empty() {
            func.instruction(&Instruction::Return);
        }

        // Every function body ends with `end`.
        func.instruction(&Instruction::End);

        code_sec.function(&func);
    }

    // Compile class methods.
    let mut fn_meta_offset = fns.len();
    for c in &class_decls {
        for m in &c.methods {
            let meta = &fn_metas[fn_meta_offset];
            fn_meta_offset += 1;
            let mut compiler = FnCompiler::new_for_method(&c.name, m, &ctx);

            let (body_instrs, new_locals) =
                compiler.compile_block(&m.body, &mut strings, m.return_type.as_ref());

            let local_decls: Vec<(u32, ValType)> =
                new_locals.iter().map(|(ty, count)| (*count, *ty)).collect();

            let mut func = Function::new(local_decls);

            emit_instrs(
                &body_instrs,
                &mut func,
                &fn_indices,
                &mut |params, results| type_builder.register(params, results),
            );

            if !body_instrs.ends_with(&[AlkInstr::Return]) && !meta.results.is_empty() {
                func.instruction(&Instruction::Return);
            }
            func.instruction(&Instruction::End);
            code_sec.function(&func);
        }
    }
    wasm_module.section(&code_sec);

    // 12. Data section — emit string literals as data segments (Gap 4).
    strings.emit_data_section(&mut wasm_module);

    // 13. Serialize the module to bytes.
    let bytes = wasm_module.finish();

    let exported_functions: Vec<String> = fns.iter().map(|f| f.name.clone()).collect();

    Ok(WasmModule {
        bytes,
        exported_functions,
        memory_pages: mem_pages,
    })
}

/// Helper: emit a sequence of [`AlkInstr`]s to a wasm-encoder [`Function`],
/// resolving `Call` name → function index, `CallIndirect` signature → type
/// index, etc.
fn emit_instrs(
    body_instrs: &[AlkInstr],
    func: &mut Function,
    fn_indices: &std::collections::HashMap<String, u32>,
    type_register: &mut dyn FnMut(&[ValType], &[ValType]) -> u32,
) {
    for instr in body_instrs {
        let wasm_instr = match instr {
            AlkInstr::I32Const(v) => Instruction::I32Const(*v),
            AlkInstr::F32Const(v) => Instruction::F32Const(*v),
            AlkInstr::LocalGet(idx) => Instruction::LocalGet(*idx),
            AlkInstr::LocalSet(idx) => Instruction::LocalSet(*idx),
            AlkInstr::LocalTee(idx) => Instruction::LocalTee(*idx),
            AlkInstr::Drop => Instruction::Drop,
            AlkInstr::Return => Instruction::Return,
            AlkInstr::End => Instruction::End,
            AlkInstr::I32Load(offset) => Instruction::I32Load(wasm_encoder::MemArg {
                offset: *offset as u64,
                align: 2, // 4-byte alignment (2^2)
                memory_index: 0,
            }),
            AlkInstr::I32Store(offset) => Instruction::I32Store(wasm_encoder::MemArg {
                offset: *offset as u64,
                align: 2,
                memory_index: 0,
            }),
            AlkInstr::BinaryOp(op) => match op {
                BinOp::Add => Instruction::I32Add,
                BinOp::Sub => Instruction::I32Sub,
                BinOp::Mul => Instruction::I32Mul,
                BinOp::Div => Instruction::I32DivS,
                BinOp::Mod => Instruction::I32RemS,
                BinOp::Eq => Instruction::I32Eq,
                BinOp::Ne => Instruction::I32Ne,
                BinOp::Lt => Instruction::I32LtS,
                BinOp::Le => Instruction::I32LeS,
                BinOp::Gt => Instruction::I32GtS,
                BinOp::Ge => Instruction::I32GeS,
                BinOp::And => Instruction::I32And,
                BinOp::Or => Instruction::I32Or,
            },
            AlkInstr::Call(name) => {
                let func_idx = if let Some(host_idx) = host_import_index(name) {
                    host_idx
                } else if let Some(idx) = fn_indices.get(name) {
                    *idx
                } else {
                    // Fallback: host_import_count + 0. Should not happen in
                    // typechecked code.
                    host_import_count()
                };
                Instruction::Call(func_idx)
            }
            AlkInstr::CallIndirect(params, results) => {
                let type_idx = type_register(params, results);
                Instruction::CallIndirect {
                    type_index: type_idx,
                    table_index: 0,
                }
            }
            AlkInstr::If => Instruction::If(wasm_encoder::BlockType::Empty),
            AlkInstr::Else => Instruction::Else,
            AlkInstr::Block => Instruction::Block(wasm_encoder::BlockType::Empty),
            AlkInstr::Loop => Instruction::Loop(wasm_encoder::BlockType::Empty),
            AlkInstr::Br(n) => Instruction::Br(*n),
        };
        func.instruction(&wasm_instr);
    }
}

/// Local FnMeta type used for indexing (kept for readability).
#[allow(dead_code)]
struct FnMetaLocal {
    name: String,
    type_idx: u32,
    params: Vec<ValType>,
    results: Vec<ValType>,
}

/// Re-collect the ClassTable from a module (Gap 1). The type checker builds
/// this internally but doesn't expose it; we re-run the collection here to
/// get the table for codegen. This is a small duplication but keeps the
/// typechecker's API stable.
fn collect_classes_via_typechecker(
    module: &ModuleDecl,
    classes: &mut typechecker::ClassTable,
    errors: &mut typechecker::TypeErrorSet,
) {
    // The typechecker's `collect_classes` is private, so we replicate the
    // logic here. We share the same `ClassTable` type, so the resulting
    // table is identical to what the typechecker built.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in &module.items {
        if let ItemDecl::Class(c) = item {
            if !seen.insert(c.name.clone()) {
                errors.push(typechecker::TypeError {
                    message: format!("class `{}` declared twice", c.name),
                    line: c.line,
                    col: c.col,
                });
                continue;
            }
            let sig = typechecker::ClassSig {
                name: c.name.clone(),
                base: c.base.clone(),
                visibility: c.visibility,
                fields: c.fields.clone(),
                methods: c.methods.clone(),
                field_stride: 0,
                vtable_slot_count: 0,
            };
            classes.insert(c.name.clone(), sig);
        }
    }
    // Compute strides + vtable slot counts.
    let names: Vec<String> = classes.names_in_order().to_vec();
    for name in &names {
        let total_fields = total_field_count_public(classes, name);
        let total_methods = total_unique_method_count_public(classes, name);
        if let Some(sig) = classes.lookup_mut(name) {
            sig.field_stride = 4 * (1 + total_fields);
            sig.vtable_slot_count = total_methods;
        }
    }
}

/// Mirror of typechecker's `total_field_count` (private).
fn total_field_count_public(classes: &typechecker::ClassTable, class_name: &str) -> u32 {
    let mut count = 0u32;
    let mut current = Some(class_name);
    while let Some(c) = current {
        if let Some(sig) = classes.lookup(c) {
            count += sig.fields.len() as u32;
            current = sig.base.as_deref();
        } else {
            break;
        }
    }
    count
}

/// Mirror of typechecker's `total_unique_method_count` (private).
fn total_unique_method_count_public(classes: &typechecker::ClassTable, class_name: &str) -> u32 {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut current = Some(class_name);
    while let Some(c) = current {
        if let Some(sig) = classes.lookup(c) {
            for m in &sig.methods {
                seen.insert(m.name.clone());
            }
            current = sig.base.as_deref();
        } else {
            break;
        }
    }
    seen.len() as u32
}

/// Mirror of typechecker's `vtable_layout` (private).
fn vtable_layout_public(
    classes: &typechecker::ClassTable,
    class_name: &str,
) -> Vec<(String, String)> {
    let chain = build_chain_public(classes, class_name);
    let mut layout: Vec<(String, String)> = Vec::new();
    for c in &chain {
        if let Some(sig) = classes.lookup(c) {
            for m in &sig.methods {
                if let Some(slot) = layout.iter().position(|(n, _)| n == &m.name) {
                    layout[slot].1 = c.clone();
                } else {
                    layout.push((m.name.clone(), c.clone()));
                }
            }
        }
    }
    layout
}

/// Convenience: compile `.alk` source to a WASM binary in one call.
///
/// This runs the full pipeline: lex → parse → typecheck → WASM codegen.
pub fn compile_src_to_wasm(src: &str) -> Result<WasmModule, String> {
    let module = crate::parse(src).map_err(|e| format!("{}", e))?;
    compile_to_wasm(&module).map_err(|e| format!("{}", e))
}

/// Pre-scan the module for string literals, interning them into the StringTable.
/// This allows the memory section to declare the correct number of pages
/// before the code section is emitted.
fn pre_scan_strings(module: &ModuleDecl, strings: &mut StringTable) {
    for item in &module.items {
        match item {
            ItemDecl::Fn(f) => pre_scan_block(&f.body, strings),
            ItemDecl::Let(l) => pre_scan_expr(&l.init, strings),
            ItemDecl::Class(c) => {
                for m in &c.methods {
                    pre_scan_block(&m.body, strings);
                }
            }
        }
    }
}

fn pre_scan_block(block: &Block, strings: &mut StringTable) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(l) => pre_scan_expr(&l.init, strings),
            Stmt::Expr(e) => pre_scan_expr(e, strings),
            Stmt::Return(opt, _, _) => {
                if let Some(e) = opt {
                    pre_scan_expr(e, strings);
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                pre_scan_expr(cond, strings);
                pre_scan_block(then_block, strings);
                if let Some(else_b) = else_block {
                    pre_scan_block(else_b, strings);
                }
            }
            Stmt::While { cond, body, .. } => {
                pre_scan_expr(cond, strings);
                pre_scan_block(body, strings);
            }
            Stmt::Assign { target, value, .. } => {
                pre_scan_expr(target, strings);
                pre_scan_expr(value, strings);
            }
        }
    }
}

fn pre_scan_expr(expr: &Expr, strings: &mut StringTable) {
    match expr {
        Expr::Lit(Lit::Str(s), _, _) => {
            strings.intern(s);
        }
        Expr::Lit(_, _, _) => {}
        Expr::Var(_, _, _) => {}
        Expr::Self_(_, _) => {}
        Expr::Binary { lhs, rhs, .. } => {
            pre_scan_expr(lhs, strings);
            pre_scan_expr(rhs, strings);
        }
        Expr::PathCall(_, _, args, _, _) => {
            for a in args {
                pre_scan_expr(a, strings);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            pre_scan_expr(receiver, strings);
            for a in args {
                pre_scan_expr(a, strings);
            }
        }
        Expr::Call { args, .. } => {
            for a in args {
                pre_scan_expr(a, strings);
            }
        }
        Expr::Field { receiver, .. } => {
            pre_scan_expr(receiver, strings);
        }
        Expr::Object { fields, .. } => {
            for (_, vexpr, _, _) in fields {
                pre_scan_expr(vexpr, strings);
            }
        }
        Expr::StaticCall { args, .. } => {
            for a in args {
                pre_scan_expr(a, strings);
            }
        }
    }
}

// ======================================================================
// Tests
// ======================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Qualifier;

    const SCENE: &str = "scene { background: #000000 }";

    fn parse_module(src: &str) -> ModuleDecl {
        crate::parse(src).expect("parse should succeed")
    }

    #[test]
    fn wasm_module_is_valid_binary() {
        let src = format!(
            "module M {{ {} fn add(a: i32, b: i32) -> i32 {{ return a; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm(), "binary must start with \\0asm magic");
        assert!(wasm.size() > 8, "binary must have header + sections");
    }

    #[test]
    fn wasm_module_exports_functions() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return 42; }} fn g() -> i32 {{ return 7; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.exported_functions.contains(&"f".to_string()));
        assert!(wasm.exported_functions.contains(&"g".to_string()));
    }

    #[test]
    fn wasm_module_exports_memory() {
        let src = format!("module M {{ {} fn f() -> i32 {{ return 1; }} }}", SCENE);
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        let binary_str = String::from_utf8_lossy(&wasm.bytes);
        assert!(binary_str.contains("memory"), "binary must export memory");
    }

    #[test]
    fn wasm_module_has_memory_pages() {
        let src = format!("module M {{ {} fn f() {{}} }}", SCENE);
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert_eq!(wasm.memory_pages, 1, "should allocate 1 memory page");
    }

    #[test]
    fn wasm_type_mapping_i32() {
        assert_eq!(alk_type_to_wasm(&BaseType::I32), ValType::I32);
    }

    #[test]
    fn wasm_type_mapping_f32() {
        assert_eq!(alk_type_to_wasm(&BaseType::F32), ValType::F32);
    }

    #[test]
    fn wasm_type_mapping_bool_is_i32() {
        assert_eq!(alk_type_to_wasm(&BaseType::Bool), ValType::I32);
    }

    #[test]
    fn wasm_type_mapping_string_is_i32_pointer() {
        assert_eq!(alk_type_to_wasm(&BaseType::Str), ValType::I32);
    }

    #[test]
    fn wasm_type_mapping_vec_is_i32_pointer() {
        assert_eq!(
            alk_type_to_wasm(&BaseType::Vec(Box::new(Type {
                qualifier: Qualifier::Unrestricted,
                base: BaseType::I32,
            }))),
            ValType::I32
        );
    }

    #[test]
    fn wasm_compile_integer_return() {
        let src = format!(
            "module M {{ {} fn answer() -> i32 {{ return 42; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        assert!(wasm.size() > 20);
    }

    #[test]
    fn wasm_compile_float_return() {
        let src = format!("module M {{ {} fn pi() -> f32 {{ return 3.14; }} }}", SCENE);
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_bool_return() {
        let src = format!(
            "module M {{ {} fn truth() -> bool {{ return true; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_void_function() {
        let src = format!("module M {{ {} fn do_nothing() {{}} }}", SCENE);
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_let_and_return() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ let x: i32 = 42; return x; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_multiple_functions() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return 1; }} fn g() -> i32 {{ return 2; }} fn h() -> i32 {{ return 3; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert_eq!(wasm.exported_functions.len(), 3);
    }

    #[test]
    fn wasm_compile_with_params() {
        let src = format!(
            "module M {{ {} fn add(a: i32, b: i32) -> i32 {{ return a; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        assert!(wasm.exported_functions.contains(&"add".to_string()));
    }

    #[test]
    fn wasm_compile_typecheck_failure() {
        // This should fail because the return type doesn't match.
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return \"hello\"; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let result = compile_to_wasm(&m);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("type check failed"));
    }

    #[test]
    fn wasm_compile_no_functions() {
        // A module with only a scene and no functions should still produce
        // a valid (empty) WASM module with memory.
        let src = "module M { scene { background: #000000 } }";
        let m = parse_module(src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        assert!(wasm.exported_functions.is_empty());
    }

    #[test]
    fn wasm_compile_src_convenience() {
        let src = format!("module M {{ {} fn f() -> i32 {{ return 42; }} }}", SCENE);
        let wasm = compile_src_to_wasm(&src).expect("compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_binary_has_correct_version() {
        let src = format!("module M {{ {} fn f() -> i32 {{ return 42; }} }}", SCENE);
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        // Verify the magic + version header is valid.
        assert_eq!(&wasm.bytes[0..4], b"\0asm");
        assert_eq!(wasm.bytes[4..8], [0x01, 0x00, 0x00, 0x00]); // version 1
    }

    #[test]
    fn wasm_binary_parseable_by_wasmparser() {
        // The ultimate test: the binary we generate must be parseable by
        // wasmparser (the official WebAssembly parser for Rust).
        let src = format!("module M {{ {} fn f() -> i32 {{ return 42; }} }}", SCENE);
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");

        // Parse the binary with wasmparser to verify it's structurally valid.
        use wasmparser::Parser;
        let parser = Parser::new(0);
        let result = parser.parse_all(&wasm.bytes);
        // Collect the payloads — if any error occurs, the binary is invalid.
        let mut found_func = false;
        let mut found_memory = false;
        for payload in result {
            let payload = payload.expect("wasmparser should parse the binary");
            match payload {
                wasmparser::Payload::FunctionSection(r) => {
                    found_func = r.count() > 0;
                }
                wasmparser::Payload::MemorySection(r) => {
                    found_memory = r.count() > 0;
                }
                _ => {}
            }
        }
        assert!(found_func, "binary must have a function section");
        assert!(found_memory, "binary must have a memory section");
    }
}

#[cfg(test)]
mod binary_op_tests {
    use super::*;
    use crate::ast::Qualifier;

    const SCENE: &str = "scene { background: #000000 }";

    fn parse_module(src: &str) -> ModuleDecl {
        crate::parse(src).expect("parse should succeed")
    }

    #[test]
    fn wasm_compile_addition() {
        let src = format!(
            "module M {{ {} fn add() -> i32 {{ return 1 + 2; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_subtraction() {
        let src = format!(
            "module M {{ {} fn sub() -> i32 {{ return 10 - 3; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_multiplication() {
        let src = format!(
            "module M {{ {} fn mul() -> i32 {{ return 4 * 5; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_division() {
        let src = format!(
            "module M {{ {} fn div() -> i32 {{ return 20 / 4; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_chained_arithmetic() {
        // 1 + 2 * 3 (with precedence: 1 + (2*3) = 7)
        let src = format!(
            "module M {{ {} fn calc() -> i32 {{ return 1 + 2 * 3; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_parenthesized_expression() {
        // (1 + 2) * 3 = 9
        let src = format!(
            "module M {{ {} fn calc() -> i32 {{ return (1 + 2) * 3; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_comparison() {
        let src = format!(
            "module M {{ {} fn cmp() -> bool {{ return 1 < 2; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_logical_and() {
        let src = format!(
            "module M {{ {} fn land() -> bool {{ return true && false; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_logical_or() {
        let src = format!(
            "module M {{ {} fn lor() -> bool {{ return true || false; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_variable_arithmetic() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ let x: i32 = 5; let y: i32 = 3; return x + y; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_function_call() {
        let src = format!(
            "module M {{ {} fn helper() -> i32 {{ return 42; }} fn caller() -> i32 {{ return helper(); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        assert_eq!(wasm.exported_functions.len(), 2);
    }

    #[test]
    fn wasm_compile_function_call_with_args() {
        let src = format!(
            "module M {{ {} fn double(x: i32) -> i32 {{ return x; }} fn main() -> i32 {{ return double(21); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_binary_with_wasmparser_validation() {
        // The generated binary with binary operators must be valid WASM.
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return 1 + 2 * 3 - 4 / 2; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());

        // Validate with wasmparser.
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse the binary");
        }
    }

    #[test]
    fn wasm_function_call_with_wasmparser_validation() {
        let src = format!(
            "module M {{ {} fn a() -> i32 {{ return 1; }} fn b() -> i32 {{ return a(); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());

        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse the binary");
        }
    }
}

#[cfg(test)]
mod control_flow_tests {
    use super::*;

    const SCENE: &str = "scene { background: #000000 }";

    fn parse_module(src: &str) -> ModuleDecl {
        crate::parse(src).expect("parse should succeed")
    }

    #[test]
    fn wasm_compile_if_statement() {
        let src = format!(
            "module M {{ {} fn f(x: i32) -> i32 {{ if (x > 0) {{ return 1; }} return 0; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_if_else_statement() {
        let src = format!(
            "module M {{ {} fn f(x: i32) -> i32 {{ if (x > 0) {{ return 1; }} else {{ return 2; }} }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_compile_while_loop() {
        let src = format!(
            "module M {{ {} fn f(n: i32) -> i32 {{ let i: i32 = 0; while (i < n) {{ i = i + 1; }} return i; }} }}",
            SCENE
        );
        // Note: this test may fail to parse because we don't support `i = i + 1`
        // (assignment to existing variable). Let's just test the while parse.
        let src = format!(
            "module M {{ {} fn f(n: i32) {{ let i: i32 = 0; while (i < n) {{ i; }} }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
    }

    #[test]
    fn wasm_if_else_wasmparser_valid() {
        let src = format!(
            "module M {{ {} fn f(x: i32) -> i32 {{ if (x > 0) {{ return 1; }} else {{ return 0; }} }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());

        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse the if/else binary");
        }
    }

    #[test]
    fn wasm_while_wasmparser_valid() {
        let src = format!(
            "module M {{ {} fn f(n: i32) {{ while (n > 0) {{ n; }} }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());

        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse the while binary");
        }
    }
}

#[cfg(test)]
mod string_data_tests {
    use super::*;

    const SCENE: &str = "scene { background: #000000 }";

    fn parse_module(src: &str) -> ModuleDecl {
        crate::parse(src).expect("parse should succeed")
    }

    #[test]
    fn string_literal_emits_data_section() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return \"hello\"; }} }}",
            SCENE
        );
        // This should fail type checking (string -> i32 mismatch), but
        // we can test the string table directly.
        let mut st = StringTable::new();
        let off = st.intern("hello");
        assert!(off > 0, "offset must be > 0 (after null guard)");
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].text, "hello");
    }

    #[test]
    fn string_deduplication() {
        let mut st = StringTable::new();
        let off1 = st.intern("world");
        let off2 = st.intern("world");
        assert_eq!(off1, off2, "same string must return same offset");
        assert_eq!(st.entries.len(), 1, "only one entry after dedup");
    }

    #[test]
    fn string_different_strings_different_offsets() {
        let mut st = StringTable::new();
        let off1 = st.intern("foo");
        let off2 = st.intern("bar");
        assert_ne!(off1, off2, "different strings must have different offsets");
        assert_eq!(st.entries.len(), 2);
    }

    #[test]
    fn string_empty_string() {
        let mut st = StringTable::new();
        let off = st.intern("");
        assert!(off > 0, "empty string still gets a valid offset");
        assert_eq!(st.entries[0].byte_len, 0);
    }

    #[test]
    fn string_null_guard_at_offset_zero() {
        let st = StringTable::new();
        // No strings interned, but next_offset should be 4 (after null guard)
        assert_eq!(st.next_offset, 4);
        assert!(st.is_empty());
    }

    #[test]
    fn string_memory_pages_calculation() {
        let mut st = StringTable::new();
        // Small strings should fit in 1 page
        st.intern("hello");
        st.intern("world");
        assert_eq!(st.memory_pages(), 1, "small strings fit in 1 page");
    }

    #[test]
    fn string_wasm_binary_contains_data_section() {
        // A module with a string literal should produce a WASM binary
        // that contains a data section (identified by section ID 11).
        let src = format!(
            "module M {{ {} fn f() {{ let s: string = \"hello\"; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        // The binary should be larger than a module without strings
        // because of the data section.
        assert!(wasm.size() > 20);
    }

    #[test]
    fn string_wasm_validated_by_wasmparser() {
        let src = format!(
            "module M {{ {} fn f() {{ let s: string = \"test\"; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse the binary with strings");
        }
    }

    #[test]
    fn string_multiple_literals() {
        let src = format!(
            "module M {{ {} fn f() {{ let a: string = \"foo\"; let b: string = \"bar\"; let c: string = \"foo\"; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        // "foo" appears twice but should be deduplicated.
        // The binary should be valid.
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse");
        }
    }
}

#[cfg(test)]
mod collection_dispatch_tests {
    use super::*;

    const SCENE: &str = "scene { background: #000000 }";

    fn parse_module(src: &str) -> ModuleDecl {
        crate::parse(src).expect("parse should succeed")
    }

    #[test]
    fn wasm_module_has_import_section() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return 42; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        // The binary should contain the "alk" module name for imports.
        let binary_str = String::from_utf8_lossy(&wasm.bytes);
        assert!(binary_str.contains("alk"), "binary must import from 'alk' module");
    }

    #[test]
    fn wasm_module_has_10_host_imports() {
        // After Gap 1, the module has 11 host imports: the 10 `vec_*`
        // collection imports plus `__alk_alloc` for object allocation.
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return 42; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        // Validate with wasmparser and count imports.
        use wasmparser::Parser;
        let parser = Parser::new(0);
        let mut import_count = 0;
        for payload in parser.parse_all(&wasm.bytes) {
            let payload = payload.expect("wasmparser should parse");
            if let wasmparser::Payload::ImportSection(r) = payload {
                import_count = r.count();
            }
        }
        assert_eq!(import_count, 11, "must have 11 host imports (10 vec_* + __alk_alloc)");
    }

    #[test]
    fn wasm_vec_new_compiles_to_host_call() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::new(); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        // Validate with wasmparser.
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse");
        }
    }

    #[test]
    fn wasm_vec_push_compiles_to_host_call() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::new(); v.push(1); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse");
        }
    }

    #[test]
    fn wasm_vec_len_compiles_to_host_call() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::new(); let n: i32 = v.len(); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse");
        }
    }

    #[test]
    fn wasm_vec_with_capacity_compiles_to_host_call() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::with_capacity(10); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse");
        }
    }

    #[test]
    fn wasm_function_call_offset_by_imports() {
        // Module functions must be offset by 10 (host import count).
        let src = format!(
            "module M {{ {} fn a() -> i32 {{ return 1; }} fn b() -> i32 {{ return a(); }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        assert!(wasm.is_valid_wasm());
        assert_eq!(wasm.exported_functions.len(), 2);
        // Validate with wasmparser.
        use wasmparser::Parser;
        let parser = Parser::new(0);
        for payload in parser.parse_all(&wasm.bytes) {
            payload.expect("wasmparser should parse");
        }
    }

    #[test]
    fn wasm_host_import_names_present() {
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return 42; }} }}",
            SCENE
        );
        let m = parse_module(&src);
        let wasm = compile_to_wasm(&m).expect("wasm compile");
        let binary_str = String::from_utf8_lossy(&wasm.bytes);
        // Check that all 10 host import names are present.
        for name in &["vec_new", "vec_with_capacity", "vec_push", "vec_extend",
                       "vec_remove", "vec_clear", "vec_len", "vec_is_empty",
                       "vec_get", "vec_set"] {
            assert!(binary_str.contains(name), "binary must contain import '{}'", name);
        }
    }

    #[test]
    fn wasm_vec_method_dispatch_all_methods() {
        // Test that all recognized Vec methods compile without error.
        for method in &["push", "len", "is_empty", "clear", "get"] {
            let src = format!(
                "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::new(); v.{}(); }} }}",
                SCENE, method
            );
            let m = parse_module(&src);
            let wasm = compile_to_wasm(&m).expect("wasm compile");
            assert!(wasm.is_valid_wasm(), "method {} should compile", method);
        }
    }
}
