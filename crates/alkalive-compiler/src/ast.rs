//! Abstract Syntax Tree for the AlkALive `.alk` Hello-World subset.
//!
//! The AST is a faithful, lossless representation of the source: every
//! token consumed by the parser lands in some AST node, and no semantic
//! validation (defaults, type-checking) is performed here. That work is
//! deferred to [`crate::codegen`], which lowers the AST to a [`crate::ir::SceneIR`].
//!
//! Grammar (informal):
//! ```text
//! ModuleDecl    := 'module' Ident '{' SceneDecl? '}'
//! SceneDecl     := 'scene' '{' SceneItem* '}'
//! SceneItem     := BackgroundProp | TextNode | InputFieldNode
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

/// Top-level module declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    /// Module name as written in source (e.g. `"HelloWorld"`).
    pub name: String,
    /// Optional scene block. `None` if the module declares no `scene { ... }`.
    pub scene: Option<SceneDecl>,
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
                    line: 1,
                    col: 1,
                })],
                line: 1,
                col: 1,
            }),
            line: 1,
            col: 1,
        };
        assert!(format!("{:?}", m).contains("Hello"));
    }
}
