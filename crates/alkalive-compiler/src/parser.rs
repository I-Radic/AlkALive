//! Recursive-descent parser for the AlkALive `.alk` Hello-World subset.
//!
//! Consumes a `Vec<Token>` from [`crate::lexer`] and produces a
//! [`crate::ast::ModuleDecl`]. Errors are reported with 1-based line/column
//! info via [`ParseError`].
//!
//! The parser is newline-tolerant: [`TokenKind::Newline`] tokens are
//! treated as soft separators and skipped between logical constructs, so
//! the grammar does not depend on newline placement for correctness.

#![forbid(unsafe_code)]

use core::fmt;

use crate::ast::{
    Color, InputFieldNode, ModuleDecl, NodeDecl, PositionDecl, RotationDecl, SceneDecl, TextNode,
};
use crate::lexer::{Token, TokenKind};

/// A parse error with location info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Human-readable message.
    pub message: String,
    /// 1-based line where the error was detected.
    pub line: u32,
    /// 1-based column where the error was detected.
    pub col: u32,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "parse error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl core::error::Error for ParseError {}

/// The recursive-descent parser.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Construct a parser over a token stream (as produced by
    /// [`crate::lexer::tokenize`]). The stream MUST end with an
    /// [`TokenKind::Eof`] sentinel; `tokenize` guarantees this.
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Parse the token stream into a [`ModuleDecl`].
    pub fn parse(&mut self) -> Result<ModuleDecl, ParseError> {
        self.skip_newlines();
        let mut module = self.parse_module()?;
        // Allow trailing newlines after the module body.
        self.skip_newlines();
        // If there's a non-Eof token after the module, that's an error.
        if !matches!(self.peek().kind, TokenKind::Eof) {
            return Err(self.unexpected("end of input"));
        }
        // Parse any scene declared inside the module.
        // `parse_module` already consumed the scene if present, but we
        // re-check here for a second scene (which is an error).
        let _ = &mut module; // borrow-check noop
        Ok(module)
    }

    // ----------------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len().saturating_sub(1));
        &self.tokens[idx]
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek().kind, TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<&Token, ParseError> {
        if self.peek().kind == kind {
            Ok(self.advance())
        } else {
            Err(self.unexpected_kind(kind))
        }
    }

    fn unexpected(&self, expected: &str) -> ParseError {
        let tok = self.peek();
        ParseError {
            message: format!(
                "expected {}, found {} {}",
                expected,
                tok.kind,
                debug_value(tok)
            ),
            line: tok.line,
            col: tok.col,
        }
    }

    fn unexpected_kind(&self, expected: TokenKind) -> ParseError {
        self.unexpected(&expected.to_string())
    }

    fn unexpected_msg(&self, msg: impl Into<String>) -> ParseError {
        let tok = self.peek();
        ParseError {
            message: msg.into(),
            line: tok.line,
            col: tok.col,
        }
    }

    // ----------------------------------------------------------------------
    // Grammar rules
    // ----------------------------------------------------------------------

    fn parse_module(&mut self) -> Result<ModuleDecl, ParseError> {
        let kw = self.expect(TokenKind::Module)?;
        let line = kw.line;
        let col = kw.col;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.value.clone();
        self.skip_newlines();
        self.expect(TokenKind::LBrace)?;
        self.skip_newlines();

        let mut scene: Option<SceneDecl> = None;
        // Parse module body: zero or one scene block.
        loop {
            match self.peek().kind {
                TokenKind::RBrace => {
                    self.advance();
                    break;
                }
                TokenKind::Scene => {
                    if scene.is_some() {
                        return Err(self.unexpected_msg(
                            "duplicate `scene` block; a module may declare at most one scene",
                        ));
                    }
                    scene = Some(self.parse_scene()?);
                    self.skip_newlines();
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("closing `}`"));
                }
                _ => {
                    return Err(self.unexpected("`scene` or closing `}`"));
                }
            }
        }

        Ok(ModuleDecl {
            name,
            scene,
            line,
            col,
        })
    }

    fn parse_scene(&mut self) -> Result<SceneDecl, ParseError> {
        let kw = self.expect(TokenKind::Scene)?;
        let line = kw.line;
        let col = kw.col;
        self.skip_newlines();
        self.expect(TokenKind::LBrace)?;
        self.skip_newlines();

        let mut background: Option<Color> = None;
        let mut nodes: Vec<NodeDecl> = Vec::new();

        loop {
            match self.peek().kind {
                TokenKind::RBrace => {
                    self.advance();
                    break;
                }
                TokenKind::Background => {
                    let color = self.parse_background_property()?;
                    background = Some(color);
                    self.skip_newlines();
                }
                TokenKind::Text => {
                    // Disambiguate: `text` as a node declaration is
                    // followed by a String literal. (If followed by
                    // something else, it's an error.)
                    if !matches!(self.peek_at(1).kind, TokenKind::String) {
                        return Err(self.unexpected_msg(
                            "`text` must be followed by a string literal",
                        ));
                    }
                    let node = self.parse_text_node()?;
                    nodes.push(NodeDecl::Text(node));
                    self.skip_newlines();
                }
                TokenKind::InputField => {
                    let node = self.parse_input_field_node()?;
                    nodes.push(NodeDecl::InputField(node));
                    self.skip_newlines();
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("closing `}`"));
                }
                _ => {
                    return Err(self.unexpected(
                        "`background`, `text`, `input-field`, or closing `}`",
                    ));
                }
            }
        }

        Ok(SceneDecl {
            background,
            nodes,
            line,
            col,
        })
    }

    /// `background: #RRGGBB`
    fn parse_background_property(&mut self) -> Result<Color, ParseError> {
        self.expect(TokenKind::Background)?;
        self.expect(TokenKind::Colon)?;
        self.parse_color_value()
    }

    /// `text "..." { props* }`
    fn parse_text_node(&mut self) -> Result<TextNode, ParseError> {
        let kw = self.expect(TokenKind::Text)?;
        let line = kw.line;
        let col = kw.col;
        let content_tok = self.expect(TokenKind::String)?;
        let content = content_tok.value.clone();
        self.skip_newlines();
        self.expect(TokenKind::LBrace)?;
        self.skip_newlines();

        let mut color: Option<Color> = None;
        let mut font_size: Option<f32> = None;
        let mut rotation: Option<RotationDecl> = None;
        let mut position: Option<PositionDecl> = None;

        loop {
            match self.peek().kind {
                TokenKind::RBrace => {
                    self.advance();
                    break;
                }
                TokenKind::Color => {
                    color = Some(self.parse_color_property()?);
                }
                TokenKind::FontSize => {
                    font_size = Some(self.parse_font_size_property()?);
                }
                TokenKind::Rotation => {
                    rotation = Some(self.parse_rotation_property()?);
                }
                TokenKind::Position => {
                    position = Some(self.parse_position_property()?);
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("closing `}`"));
                }
                _ => {
                    return Err(self.unexpected(
                        "`color`, `font-size`, `rotation`, `position`, or closing `}`",
                    ));
                }
            }
            self.skip_newlines();
        }

        Ok(TextNode {
            content,
            color,
            font_size,
            rotation,
            position,
            line,
            col,
        })
    }

    /// `input-field { props* }`
    fn parse_input_field_node(&mut self) -> Result<InputFieldNode, ParseError> {
        let kw = self.expect(TokenKind::InputField)?;
        let line = kw.line;
        let col = kw.col;
        self.skip_newlines();
        self.expect(TokenKind::LBrace)?;
        self.skip_newlines();

        let mut placeholder: Option<String> = None;
        let mut position: Option<PositionDecl> = None;

        loop {
            match self.peek().kind {
                TokenKind::RBrace => {
                    self.advance();
                    break;
                }
                TokenKind::Placeholder => {
                    placeholder = Some(self.parse_placeholder_property()?);
                }
                TokenKind::Position => {
                    position = Some(self.parse_position_property()?);
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("closing `}`"));
                }
                _ => {
                    return Err(self.unexpected(
                        "`placeholder`, `position`, or closing `}`",
                    ));
                }
            }
            self.skip_newlines();
        }

        Ok(InputFieldNode {
            placeholder,
            position,
            line,
            col,
        })
    }

    // ------------------------------------------------------------------
    // Property value parsers
    // ------------------------------------------------------------------

    /// `color: <color-value>`
    fn parse_color_property(&mut self) -> Result<Color, ParseError> {
        self.expect(TokenKind::Color)?;
        self.expect(TokenKind::Colon)?;
        self.parse_color_value()
    }

    /// `<color-value> := #RRGGBB | Ident`
    fn parse_color_value(&mut self) -> Result<Color, ParseError> {
        match self.peek().kind {
            TokenKind::HexColor => {
                let tok = self.advance();
                let (r, g, b) = decode_hex_color(&tok.value).ok_or_else(|| ParseError {
                    message: format!("invalid hex color: #{}", tok.value),
                    line: tok.line,
                    col: tok.col,
                })?;
                Ok(Color::Hex(r, g, b))
            }
            TokenKind::Ident => {
                let tok = self.advance();
                Ok(Color::Named(tok.value.clone()))
            }
            _ => Err(self.unexpected("hex color (`#RRGGBB`) or color name")),
        }
    }

    /// `font-size: <number>`
    fn parse_font_size_property(&mut self) -> Result<f32, ParseError> {
        self.expect(TokenKind::FontSize)?;
        self.expect(TokenKind::Colon)?;
        self.parse_number()
    }

    /// `rotation: y-axis <number>`
    fn parse_rotation_property(&mut self) -> Result<RotationDecl, ParseError> {
        self.expect(TokenKind::Rotation)?;
        self.expect(TokenKind::Colon)?;
        let axis_tok = self.expect(TokenKind::Ident)?;
        if axis_tok.value != "y-axis" {
            return Err(ParseError {
                message: format!(
                    "expected `y-axis`, found `{}` (only `y-axis` is supported in this subset)",
                    axis_tok.value
                ),
                line: axis_tok.line,
                col: axis_tok.col,
            });
        }
        // Clone the axis string before the next mutable borrow of `self`.
        let axis = axis_tok.value.clone();
        let speed = self.parse_number()?;
        Ok(RotationDecl { axis, speed })
    }

    /// `position: center | below <ref> | <number> <number>`
    fn parse_position_property(&mut self) -> Result<PositionDecl, ParseError> {
        self.expect(TokenKind::Position)?;
        self.expect(TokenKind::Colon)?;
        self.parse_position_value()
    }

    fn parse_position_value(&mut self) -> Result<PositionDecl, ParseError> {
        match self.peek().kind {
            TokenKind::Ident => {
                let tok = self.advance();
                match tok.value.as_str() {
                    "center" => Ok(PositionDecl::Center),
                    "below" => {
                        // Expect a node reference: either an Ident or the
                        // `text` keyword (which is the only node type
                        // referenceable from `below` in this subset).
                        let ref_tok = match self.peek().kind {
                            TokenKind::Ident | TokenKind::Text => self.advance(),
                            _ => return Err(self.unexpected("node name after `below`")),
                        };
                        Ok(PositionDecl::Below(ref_tok.value.clone()))
                    }
                    other => Err(ParseError {
                        message: format!(
                            "unknown position `{}`; expected `center`, `below <name>`, or two numbers",
                            other
                        ),
                        line: tok.line,
                        col: tok.col,
                    }),
                }
            }
            TokenKind::Number => {
                let x = self.parse_number()?;
                let y = self.parse_number()?;
                Ok(PositionDecl::Custom(x, y))
            }
            _ => Err(self.unexpected(
                "`center`, `below <name>`, or two numbers",
            )),
        }
    }

    /// `placeholder: <string>`
    fn parse_placeholder_property(&mut self) -> Result<String, ParseError> {
        self.expect(TokenKind::Placeholder)?;
        self.expect(TokenKind::Colon)?;
        let tok = self.expect(TokenKind::String)?;
        Ok(tok.value.clone())
    }

    /// Parse a number token and return its `f32` value.
    fn parse_number(&mut self) -> Result<f32, ParseError> {
        let tok = self.expect(TokenKind::Number)?;
        tok.value
            .parse::<f32>()
            .map_err(|_| ParseError {
                message: format!("invalid number literal: `{}`", tok.value),
                line: tok.line,
                col: tok.col,
            })
    }
}

fn debug_value(tok: &Token) -> String {
    if tok.value.is_empty() {
        String::new()
    } else {
        format!("`{}`", tok.value)
    }
}

/// Decode a 6-hex-digit string `RRGGBB` into `(r, g, b)`. Returns `None`
/// if the string is not exactly 6 hex digits.
fn decode_hex_color(digits: &str) -> Option<(u8, u8, u8)> {
    if digits.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&digits[0..2], 16).ok()?;
    let g = u8::from_str_radix(&digits[2..4], 16).ok()?;
    let b = u8::from_str_radix(&digits[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Convenience: tokenize + parse a source string in one call.
pub fn parse(src: &str) -> Result<ModuleDecl, ParseError> {
    let tokens = crate::lexer::tokenize(src)
        .map_err(|e| ParseError {
            message: format!("lex error: {}", e.message),
            line: e.line,
            col: e.col,
        })?;
    Parser::new(tokens).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> ModuleDecl {
        parse(src).unwrap_or_else(|e| panic!("parse failed: {}", e))
    }

    #[test]
    fn parse_minimal_module() {
        let m = parse_ok("module M { }");
        assert_eq!(m.name, "M");
        assert!(m.scene.is_none());
    }

    #[test]
    fn parse_module_with_empty_scene() {
        let m = parse_ok("module M { scene { } }");
        assert_eq!(m.name, "M");
        let s = m.scene.expect("scene");
        assert!(s.background.is_none());
        assert!(s.nodes.is_empty());
    }

    #[test]
    fn parse_module_missing_module_keyword_errors() {
        let err = parse("HelloWorld { }").unwrap_err();
        assert!(err.message.contains("expected"), "got: {}", err.message);
        assert_eq!(err.line, 1);
    }

    #[test]
    fn parse_module_missing_name_errors() {
        let err = parse("module { }").unwrap_err();
        assert!(err.message.contains("identifier"), "got: {}", err.message);
    }

    #[test]
    fn parse_module_unclosed_brace_errors() {
        let err = parse("module M {").unwrap_err();
        assert!(err.message.contains("closing"), "got: {}", err.message);
    }

    #[test]
    fn parse_duplicate_scene_errors() {
        let err = parse("module M { scene { } scene { } }").unwrap_err();
        assert!(err.message.contains("duplicate"), "got: {}", err.message);
    }

    #[test]
    fn parse_background_hex() {
        let m = parse_ok("module M { scene { background: #112233 } }");
        assert_eq!(m.scene.unwrap().background, Some(Color::Hex(0x11, 0x22, 0x33)));
    }

    #[test]
    fn parse_text_node_minimal() {
        let m = parse_ok(r#"module M { scene { text "Hi" { } } }"#);
        let s = m.scene.unwrap();
        assert_eq!(s.nodes.len(), 1);
        match &s.nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.content, "Hi");
                assert!(t.color.is_none());
                assert!(t.font_size.is_none());
                assert!(t.rotation.is_none());
                assert!(t.position.is_none());
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_text_node_full() {
        let m = parse_ok(r#"module M { scene { text "Hello!" { color: gold font-size: 64 rotation: y-axis 0.5 position: center } } }"#);
        let s = m.scene.unwrap();
        match &s.nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.content, "Hello!");
                assert_eq!(t.color, Some(Color::Named("gold".into())));
                assert_eq!(t.font_size, Some(64.0));
                assert_eq!(
                    t.rotation,
                    Some(RotationDecl {
                        axis: "y-axis".into(),
                        speed: 0.5,
                    })
                );
                assert_eq!(t.position, Some(PositionDecl::Center));
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_text_node_color_hex() {
        let m = parse_ok(r#"module M { scene { text "Hi" { color: #FFD700 } } }"#);
        match &m.scene.unwrap().nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.color, Some(Color::Hex(0xFF, 0xD7, 0x00)));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_text_node_missing_string_errors() {
        let err = parse(r#"module M { scene { text { } } }"#).unwrap_err();
        assert!(err.message.contains("string"), "got: {}", err.message);
    }

    #[test]
    fn parse_text_node_unknown_property_errors() {
        let err = parse(r#"module M { scene { text "Hi" { bogus: 1 } } }"#).unwrap_err();
        assert!(err.message.contains("`color`"), "got: {}", err.message);
    }

    #[test]
    fn parse_input_field_minimal() {
        let m = parse_ok(r#"module M { scene { input-field { } } }"#);
        match &m.scene.unwrap().nodes[0] {
            NodeDecl::InputField(f) => {
                assert!(f.placeholder.is_none());
                assert!(f.position.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_input_field_full() {
        let m = parse_ok(
            r#"module M { scene { input-field { placeholder: "Type" position: below text } } }"#,
        );
        match &m.scene.unwrap().nodes[0] {
            NodeDecl::InputField(f) => {
                assert_eq!(f.placeholder.as_deref(), Some("Type"));
                assert_eq!(f.position, Some(PositionDecl::Below("text".into())));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_position_custom_coords() {
        let m = parse_ok(r#"module M { scene { text "Hi" { position: 0.5 0.25 } } }"#);
        match &m.scene.unwrap().nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.position, Some(PositionDecl::Custom(0.5, 0.25)));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_position_below_unknown_ref_errors() {
        let err = parse(r#"module M { scene { input-field { position: below } } }"#).unwrap_err();
        assert!(err.message.contains("node name"), "got: {}", err.message);
    }

    #[test]
    fn parse_position_unknown_keyword_errors() {
        let err = parse(r#"module M { scene { text "Hi" { position: top-left } } }"#).unwrap_err();
        assert!(err.message.contains("unknown position"), "got: {}", err.message);
    }

    #[test]
    fn parse_rotation_wrong_axis_errors() {
        let err =
            parse(r#"module M { scene { text "Hi" { rotation: x-axis 1.0 } } }"#).unwrap_err();
        assert!(err.message.contains("y-axis"), "got: {}", err.message);
    }

    #[test]
    fn parse_font_size_non_number_errors() {
        let err = parse(r#"module M { scene { text "Hi" { font-size: big } } }"#).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn parse_placeholder_non_string_errors() {
        let err =
            parse(r#"module M { scene { input-field { placeholder: 42 } } }"#).unwrap_err();
        assert!(err.message.contains("string"), "got: {}", err.message);
    }

    #[test]
    fn parse_hello_world_full_source() {
        let src = r#"
module HelloWorld {
  scene {
    background: #000000
    text "Hello World!" {
      color: gold
      font-size: 64
      rotation: y-axis 0.5
      position: center
    }
    input-field {
      placeholder: "Type here..."
      position: below text
    }
  }
}
"#;
        let m = parse_ok(src);
        assert_eq!(m.name, "HelloWorld");
        let s = m.scene.expect("scene");
        assert_eq!(s.background, Some(Color::Hex(0, 0, 0)));
        assert_eq!(s.nodes.len(), 2);

        match &s.nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.content, "Hello World!");
                assert_eq!(t.color, Some(Color::Named("gold".into())));
                assert_eq!(t.font_size, Some(64.0));
                assert_eq!(
                    t.rotation,
                    Some(RotationDecl {
                        axis: "y-axis".into(),
                        speed: 0.5,
                    })
                );
                assert_eq!(t.position, Some(PositionDecl::Center));
            }
            other => panic!("expected Text, got {:?}", other),
        }
        match &s.nodes[1] {
            NodeDecl::InputField(f) => {
                assert_eq!(f.placeholder.as_deref(), Some("Type here..."));
                assert_eq!(f.position, Some(PositionDecl::Below("text".into())));
            }
            other => panic!("expected InputField, got {:?}", other),
        }
    }

    #[test]
    fn parse_trailing_garbage_errors() {
        let err = parse("module M { } extra").unwrap_err();
        assert!(err.message.contains("end of input"), "got: {}", err.message);
    }

    #[test]
    fn parse_comments_ignored() {
        let src = r#"
// This is a comment
module M { // inline comment
  scene { } // another
}
"#;
        let m = parse_ok(src);
        assert_eq!(m.name, "M");
    }

    #[test]
    fn decode_hex_color_valid() {
        assert_eq!(decode_hex_color("000000"), Some((0, 0, 0)));
        assert_eq!(decode_hex_color("FFFFFF"), Some((0xFF, 0xFF, 0xFF)));
        assert_eq!(decode_hex_color("FFD700"), Some((0xFF, 0xD7, 0x00)));
        assert_eq!(decode_hex_color("ffd700"), Some((0xFF, 0xD7, 0x00)));
    }

    #[test]
    fn decode_hex_color_invalid() {
        assert_eq!(decode_hex_color("FFF"), None);
        assert_eq!(decode_hex_color("FFFFFFF"), None);
        assert_eq!(decode_hex_color("GGGGGG"), None);
    }

    #[test]
    fn parse_error_display() {
        let err = parse("not a module").unwrap_err();
        let s = format!("{}", err);
        assert!(s.contains("parse error at 1:1"), "got: {}", s);
    }
}
