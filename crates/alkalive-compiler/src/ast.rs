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
    /// 1-based line where the `module` keyword appears.
    pub line: u32,
    /// 1-based column where the `module` keyword appears.
    pub col: u32,
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
        assert_eq!(
            format!("{}", PositionDecl::Custom(1.0, 2.0)),
            "1 2"
        );
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
