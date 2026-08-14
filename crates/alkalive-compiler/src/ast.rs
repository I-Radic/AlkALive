//! Abstract Syntax Tree for the AlkALive `.alk` Hello-World subset.
//!
//! The AST is a faithful, lossless representation of the source: every
//! token consumed by the parser lands in some AST node, and no semantic
//! validation (defaults, type-checking) is performed here. That work is
//! deferred to [`crate::codegen`], which lowers the AST to a [`crate::ir::SceneIR`].
//!
//! # Attributes (ADR-027 Phase 1)
//!
//! The AST optionally carries [`Attribute`] annotations on declarations:
//!
//! - `@monotone` / `@antitone` — leading attributes on a node declaration
//!   (e.g. `@monotone text "X" { ... }`).
//! - `#![deny(monotonicity)]` — a file-level attribute on the module that
//!   upgrades monotonicity lint warnings into hard errors.
//!
//! Attributes are stored verbatim (name + source position) and are not
//! semantically interpreted by the AST. The lint pass in [`crate::lints`]
//! is responsible for consuming them.
//!
//! Grammar (informal):
//! ```text
//! ModuleDecl    := ShebangAttr* 'module' Ident '{' SceneDecl? '}'
//! ShebangAttr   := '#!' '[' Ident ( '(' Ident ')' )? ']'
//! SceneDecl     := 'scene' '{' SceneItem* '}'
//! SceneItem     := Attr* ( BackgroundProp | TextNode | InputFieldNode )
//! Attr          := '@' Ident ( '(' Ident ')' )?
//! BackgroundProp:= 'background' ':' HexColor
//! TextNode      := 'text' String '{' NodeProp* '}'
//! InputFieldNode:= 'input-field' '{' NodeProp* '}'
//! NodeProp      := ColorProp | FontSizeProp | RotationProp
//!                | PositionProp | PlaceholderProp
//! ColorProp     := 'color' ':' ColorValue
//! ColorValue    := HexColor | Ident          // #RRGGBB or 'gold'
//! FontSizeProp  := 'font-size' ':' Number
//! RotationProp  := 'rotation' ':' 'y-axis' Number
//! PositionProp  := 'position' ':' PositionValue
//! PositionValue := 'center' | 'below' Ident | Number Number
//! PlaceholderProp:='placeholder' ':' String
//! ```

#![forbid(unsafe_code)]

use core::fmt;

/// A source-level attribute annotation (ADR-027 Phase 1).
///
/// Attributes are deliberately untyped: the parser stores the textual
/// name (e.g. `"monotone"`, `"antitone"`, or `"deny(monotonicity)"` for
/// file-level shebang attributes) along with the source position of the
/// introducer (`@` or `#!`). Semantic interpretation happens in the
/// [`crate::lints`] module.
///
/// # Examples
///
/// - `@monotone` becomes `Attribute { name: "monotone", line, col }`.
/// - `@antitone` becomes `Attribute { name: "antitone", line, col }`.
/// - `#![deny(monotonicity)]` becomes a single `Attribute` on the module
///   with `name: "deny(monotonicity)"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// The attribute name as written in source, including any
    /// parenthesised argument for shebang attributes. For `@monotone`
    /// this is the bare string `"monotone"`; for `#![deny(monotonicity)]`
    /// this is `"deny(monotonicity)"` (the entire bracketed payload).
    pub name: String,
    /// 1-based line where the attribute's introducer (`@` or `#!`) appears.
    pub line: u32,
    /// 1-based column where the attribute's introducer appears.
    pub col: u32,
}

impl Attribute {
    /// Convenience constructor for tests and external builders.
    pub fn new(name: impl Into<String>, line: u32, col: u32) -> Self {
        Self {
            name: name.into(),
            line,
            col,
        }
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.name)
    }
}

/// Top-level module declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    /// Module name as written in source (e.g. `"HelloWorld"`).
    pub name: String,
    /// Optional scene block. `None` if the module declares no `scene { ... }`.
    pub scene: Option<SceneDecl>,
    /// File-level attributes (e.g. `#![deny(monotonicity)]`) attached to
    /// the module. Collected from the very start of the source file.
    pub attributes: Vec<Attribute>,
    /// Top-level items declared in the module body alongside the scene:
    /// `fn` declarations and `let` collection declarations.
    /// (ADR-027 Phase 2 — the language is extended with functions and typed
    /// collections so that monotonicity qualifiers have something to annotate.)
    pub items: Vec<ItemDecl>,
    /// 1-based line where the `module` keyword appears.
    pub line: u32,
    /// 1-based column where the `module` keyword appears.
    pub col: u32,
}

impl ModuleDecl {
    /// Returns `true` iff the module carries the `#![deny(monotonicity)]`
    /// file-level attribute. The attribute name is stored as
    /// `"deny(monotonicity)"` by the shebang-attribute parser.
    pub fn denies_monotonicity(&self) -> bool {
        self.attributes
            .iter()
            .any(|a| a.name == "deny(monotonicity)")
    }
}

// ======================================================================
// ADR-027 Phase 2 — Type system, functions, collections, expressions
// ======================================================================

/// A top-level item declared inside a module body (alongside the scene block).
#[derive(Debug, Clone, PartialEq)]
pub enum ItemDecl {
    /// `fn name(params) -> ReturnType { body }`
    Fn(FnDecl),
    /// `let name: Type = init;`
    Let(LetDecl),
}

/// A function declaration.
///
/// ```text
/// fn name(param: Type, ...) -> ReturnType { body }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    /// Function name.
    pub name: String,
    /// Typed parameters.
    pub params: Vec<Param>,
    /// Optional return type. `None` means the function returns unit `()`.
    pub return_type: Option<Type>,
    /// The function body block.
    pub body: Block,
    /// Attributes attached to this declaration (e.g. `@monotone`).
    pub attrs: Vec<Attribute>,
    /// 1-based line of the `fn` keyword.
    pub line: u32,
    /// 1-based column of the `fn` keyword.
    pub col: u32,
}

/// A typed function parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Parameter name.
    pub name: String,
    /// Parameter type (may carry a monotonicity qualifier).
    pub ty: Type,
    /// 1-based line of the parameter name.
    pub line: u32,
    /// 1-based column of the parameter name.
    pub col: u32,
}

/// A typed `let` binding (collection declaration).
///
/// ```text
/// let name: Type = init;
/// ```
/// The `Type` may carry a `monotone` / `antitone` qualifier.
#[derive(Debug, Clone, PartialEq)]
pub struct LetDecl {
    /// Binding name.
    pub name: String,
    /// Declared type (may carry a monotonicity qualifier).
    pub ty: Type,
    /// Initialiser expression.
    pub init: Expr,
    /// Attributes attached to this declaration (e.g. `@monotone`).
    pub attrs: Vec<Attribute>,
    /// 1-based line of the `let` keyword.
    pub line: u32,
    /// 1-based column of the `let` keyword.
    pub col: u32,
}

/// A brace-delimited block of statements.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Statements in source order.
    pub stmts: Vec<Stmt>,
    /// 1-based line of the opening `{`.
    pub line: u32,
    /// 1-based column of the opening `{`.
    pub col: u32,
}

/// A statement inside a function body.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `let name: Type = init;`
    Let(LetDecl),
    /// An expression statement (typically a method call like `x.push(e);`).
    Expr(Expr),
    /// `return expr;` or `return;`.
    Return(Option<Expr>, u32, u32),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    And,
    /// `||`
    Or,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Mod => write!(f, "%"),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Le => write!(f, "<="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Ge => write!(f, ">="),
            BinOp::And => write!(f, "&&"),
            BinOp::Or => write!(f, "||"),
        }
    }
}

impl BinOp {
    /// Returns the precedence level (higher = binds tighter).
    /// 1: `||`
    /// 2: `&&`
    /// 3: `==` `!=` `<` `<=` `>` `>=`
    /// 4: `+` `-`
    /// 5: `*` `/` `%`
    pub fn precedence(&self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 3,
            BinOp::Add | BinOp::Sub => 4,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 5,
        }
    }

    /// Returns `true` if this is a comparison operator (==, !=, <, <=, >, >=).
    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    /// Returns `true` if this is a logical operator (&&, ||).
    pub fn is_logical(&self) -> bool {
        matches!(self, BinOp::And | BinOp::Or)
    }
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal value.
    Lit(Lit, u32, u32),
    /// A variable reference.
    Var(String, u32, u32),
    /// A binary operation: `lhs op rhs`.
    Binary {
        /// Left-hand side.
        lhs: Box<Expr>,
        /// The operator.
        op: BinOp,
        /// Right-hand side.
        rhs: Box<Expr>,
        /// 1-based line of the operator.
        line: u32,
        /// 1-based column of the operator.
        col: u32,
    },
    /// A path-qualified call like `Vec::new()`.
    ///
    /// Stored as `(module, member, args, line, col)`.
    PathCall(String, String, Vec<Expr>, u32, u32),
    /// A method call `receiver.method(args)`.
    MethodCall {
        /// The receiver expression (often a `Var`).
        receiver: Box<Expr>,
        /// Method name (e.g. `push`, `remove`, `len`).
        method: String,
        /// Argument expressions.
        args: Vec<Expr>,
        /// 1-based line of the method name.
        line: u32,
        /// 1-based column of the method name.
        col: u32,
    },
    /// A function call `name(args)`.
    Call {
        /// The function name.
        callee: String,
        /// Argument expressions.
        args: Vec<Expr>,
        /// 1-based line of the callee.
        line: u32,
        /// 1-based column of the callee.
        col: u32,
    },
}

/// A literal value.
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    /// An integer literal (stored as `i64`).
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A string literal.
    Str(String),
    /// A boolean literal.
    Bool(bool),
}

/// A type, optionally carrying a monotonicity qualifier.
///
/// (ADR-027 Phase 2 — `monotone` / `antitone` are first-class type qualifiers.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    /// The monotonicity qualifier on this type.
    pub qualifier: Qualifier,
    /// The base (unqualified) type.
    pub base: BaseType,
}

/// The monotonicity qualifier on a type.
///
/// - `Monotone`: the collection may only grow (no shrink operations).
/// - `Antitone`: the collection may only shrink (no grow operations).
/// - `Unrestricted`: no monotonicity constraint (the default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Qualifier {
    /// No monotonicity constraint.
    #[default]
    Unrestricted,
    /// Collection only grows.
    Monotone,
    /// Collection only shrinks.
    Antitone,
}

impl fmt::Display for Qualifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Qualifier::Unrestricted => write!(f, "unrestricted"),
            Qualifier::Monotone => write!(f, "monotone"),
            Qualifier::Antitone => write!(f, "antitone"),
        }
    }
}

/// The base (unqualified) type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseType {
    /// `i32`
    I32,
    /// `f32`
    F32,
    /// `string`
    Str,
    /// `bool`
    Bool,
    /// `Vec<T>` — a growable collection.
    Vec(Box<Type>),
    /// A named user type (forward reference; currently unused).
    Named(String),
}

impl Type {
    /// Convenience: is this a `Vec<T>` (any element type, any qualifier)?
    pub fn is_vec(&self) -> bool {
        matches!(self.base, BaseType::Vec(_))
    }

    /// Convenience: construct an unrestricted `Vec<T>`.
    pub fn vec(elem: Type) -> Self {
        Type {
            qualifier: Qualifier::Unrestricted,
            base: BaseType::Vec(Box::new(elem)),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.qualifier {
            Qualifier::Unrestricted => {}
            Qualifier::Monotone => write!(f, "monotone ")?,
            Qualifier::Antitone => write!(f, "antitone ")?,
        }
        match &self.base {
            BaseType::I32 => write!(f, "i32"),
            BaseType::F32 => write!(f, "f32"),
            BaseType::Str => write!(f, "string"),
            BaseType::Bool => write!(f, "bool"),
            BaseType::Vec(elem) => write!(f, "Vec<{}>", elem),
            BaseType::Named(n) => write!(f, "{}", n),
        }
    }
}

/// A `scene { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDecl {
    /// Optional `background: #RRGGBB` property. `None` if not declared.
    pub background: Option<Color>,
    /// Ordered list of child nodes (text / input-field) in source order.
    pub nodes: Vec<NodeDecl>,
    /// Attributes attached to the `scene` block itself (rare; usually
    /// attributes appear on individual nodes rather than the scene).
    pub attributes: Vec<Attribute>,
    /// 1-based line of the `scene` keyword.
    pub line: u32,
    /// 1-based column of the `scene` keyword.
    pub col: u32,
}

/// A node declaration inside a scene.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeDecl {
    /// A `text "..." { ... }` node.
    Text(TextNode),
    /// An `input-field { ... }` node.
    InputField(InputFieldNode),
}

impl NodeDecl {
    /// Returns a reference to the attributes carried by this node, if any.
    /// Useful for the lint pass which walks nodes uniformly.
    pub fn attributes(&self) -> &[Attribute] {
        match self {
            NodeDecl::Text(t) => &t.attributes,
            NodeDecl::InputField(f) => &f.attributes,
        }
    }

    /// Returns a mutable reference to the attributes carried by this node.
    pub fn attributes_mut(&mut self) -> &mut Vec<Attribute> {
        match self {
            NodeDecl::Text(t) => &mut t.attributes,
            NodeDecl::InputField(f) => &mut f.attributes,
        }
    }
}

/// `text "content" { color: ..., font-size: ..., rotation: ..., position: ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct TextNode {
    /// The string literal between the quotes.
    pub content: String,
    /// Optional `color:` property.
    pub color: Option<Color>,
    /// Optional `font-size:` property.
    pub font_size: Option<f32>,
    /// Optional `rotation:` property.
    pub rotation: Option<RotationDecl>,
    /// Optional `position:` property.
    pub position: Option<PositionDecl>,
    /// Leading attributes (e.g. `@monotone`) attached to this text node.
    pub attributes: Vec<Attribute>,
    /// 1-based line of the `text` keyword.
    pub line: u32,
    /// 1-based column of the `text` keyword.
    pub col: u32,
}

/// `input-field { placeholder: ..., position: ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct InputFieldNode {
    /// Optional `placeholder:` property.
    pub placeholder: Option<String>,
    /// Optional `position:` property.
    pub position: Option<PositionDecl>,
    /// Leading attributes (e.g. `@antitone`) attached to this input field.
    pub attributes: Vec<Attribute>,
    /// 1-based line of the `input-field` keyword.
    pub line: u32,
    /// 1-based column of the `input-field` keyword.
    pub col: u32,
}

/// `rotation: y-axis 0.5` — rotate around an axis at `speed` rad/s.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationDecl {
    /// Axis identifier as written (`"y-axis"`). Reserved for future
    /// `x-axis` / `z-axis` support.
    pub axis: String,
    /// Rotation speed in radians per second.
    pub speed: f32,
}

/// A color value: either a hex literal or a named color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Color {
    /// `#RRGGBB` literal. The three bytes are `(R, G, B)`.
    Hex(u8, u8, u8),
    /// A named color (e.g. `gold`). The string is the identifier as written.
    Named(String),
}

/// A position value.
#[derive(Debug, Clone, PartialEq)]
pub enum PositionDecl {
    /// `center`
    Center,
    /// `below <ref>` — place below the named node.
    Below(String),
    /// `x y` — explicit coordinates.
    Custom(f32, f32),
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Color::Hex(r, g, b) => write!(f, "#{:02X}{:02X}{:02X}", r, g, b),
            Color::Named(name) => write!(f, "{}", name),
        }
    }
}

impl fmt::Display for PositionDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PositionDecl::Center => write!(f, "center"),
            PositionDecl::Below(name) => write!(f, "below {}", name),
            PositionDecl::Custom(x, y) => write!(f, "{} {}", x, y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_hex_display() {
        assert_eq!(format!("{}", Color::Hex(0, 0, 0)), "#000000");
        assert_eq!(format!("{}", Color::Hex(0xFF, 0xD7, 0x00)), "#FFD700");
    }

    #[test]
    fn color_named_display() {
        assert_eq!(format!("{}", Color::Named("gold".into())), "gold");
    }

    #[test]
    fn position_display_variants() {
        assert_eq!(format!("{}", PositionDecl::Center), "center");
        assert_eq!(
            format!("{}", PositionDecl::Below("text".into())),
            "below text"
        );
        assert_eq!(format!("{}", PositionDecl::Custom(1.0, 2.0)), "1 2");
    }

    #[test]
    fn ast_structs_have_debug() {
        let m = ModuleDecl {
            name: "Hello".into(),
            scene: Some(SceneDecl {
                background: Some(Color::Hex(0, 0, 0)),
                nodes: vec![NodeDecl::Text(TextNode {
                    content: "Hi".into(),
                    color: Some(Color::Named("gold".into())),
                    font_size: Some(64.0),
                    rotation: Some(RotationDecl {
                        axis: "y-axis".into(),
                        speed: 0.5,
                    }),
                    position: Some(PositionDecl::Center),
                    attributes: vec![Attribute::new("monotone", 1, 1)],
                    line: 1,
                    col: 1,
                })],
                attributes: Vec::new(),
                line: 1,
                col: 1,
            }),
            attributes: vec![Attribute::new("deny(monotonicity)", 1, 1)],
            items: Vec::new(),
            line: 1,
            col: 1,
        };
        assert!(format!("{:?}", m).contains("Hello"));
        assert!(format!("{:?}", m).contains("monotone"));
        assert!(format!("{:?}", m).contains("deny(monotonicity)"));
    }

    #[test]
    fn attribute_display_smoke() {
        let a = Attribute::new("monotone", 1, 1);
        assert_eq!(format!("{}", a), "@monotone");
    }

    #[test]
    fn node_decl_attributes_accessor() {
        let t = NodeDecl::Text(TextNode {
            content: "x".into(),
            color: None,
            font_size: None,
            rotation: None,
            position: None,
            attributes: vec![Attribute::new("monotone", 2, 3)],
            line: 1,
            col: 1,
        });
        assert_eq!(t.attributes().len(), 1);
        assert_eq!(t.attributes()[0].name, "monotone");

        let f = NodeDecl::InputField(InputFieldNode {
            placeholder: None,
            position: None,
            attributes: vec![
                Attribute::new("antitone", 4, 5),
                Attribute::new("monotone", 5, 5),
            ],
            line: 1,
            col: 1,
        });
        assert_eq!(f.attributes().len(), 2);
        assert_eq!(f.attributes()[1].name, "monotone");
    }
}
