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
    Attribute, BaseType, BinOp, Block, Color, Expr, FnDecl, InputFieldNode, ItemDecl, LetDecl, Lit,
    ModuleDecl, NodeDecl, Param, PositionDecl, Qualifier, RotationDecl, SceneDecl, Stmt, TextNode,
    Type,
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
        // File-level attributes (e.g. `#![deny(monotonicity)]`) may
        // appear before the `module` keyword and are parsed inside
        // `parse_module`.
        let mut module = self.parse_module()?;
        // Allow trailing newlines after the module body.
        self.skip_newlines();
        // If there's a non-Eof token after the module, that's an error.
        if !matches!(self.peek().kind, TokenKind::Eof) {
            return Err(self.unexpected("end of input"));
        }
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
        // File-level attributes (e.g. `#![deny(monotonicity)]`) may appear
        // before the `module` keyword.
        let attributes = self.parse_shebang_attributes()?;
        let kw = self.expect(TokenKind::Module)?;
        let line = kw.line;
        let col = kw.col;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.value.clone();
        self.skip_newlines();
        self.expect(TokenKind::LBrace)?;
        self.skip_newlines();

        let mut scene: Option<SceneDecl> = None;
        let mut items: Vec<ItemDecl> = Vec::new();
        // Parse module body: an optional `scene` block (possibly with
        // leading `@attributes`) followed by zero or more `fn` / `let`
        // top-level items (ADR-027 Phase 2). Items may also carry leading
        // `@attributes`.
        loop {
            // Collect any leading attributes.
            let attrs = self.parse_leading_attributes()?;
            self.skip_newlines();
            match self.peek().kind {
                TokenKind::RBrace => {
                    if !attrs.is_empty() {
                        return Err(self
                            .unexpected_msg("trailing attributes with no following declaration"));
                    }
                    self.advance();
                    break;
                }
                TokenKind::Scene => {
                    if !attrs.is_empty() {
                        return Err(self.unexpected_msg(
                            "attributes are not allowed on `scene` blocks in this position; \
                             place `@`-attributes immediately before `scene`",
                        ));
                    }
                    if scene.is_some() {
                        return Err(self.unexpected_msg(
                            "duplicate `scene` block; a module may declare at most one scene",
                        ));
                    }
                    scene = Some(self.parse_scene()?);
                    self.skip_newlines();
                }
                TokenKind::Fn => {
                    let f = self.parse_fn(attrs)?;
                    items.push(ItemDecl::Fn(f));
                    self.skip_newlines();
                }
                TokenKind::Let => {
                    let l = self.parse_let(attrs)?;
                    items.push(ItemDecl::Let(l));
                    self.skip_newlines();
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("closing `}`"));
                }
                _ => {
                    return Err(self.unexpected("`scene`, `fn`, `let`, or closing `}`"));
                }
            }
        }

        Ok(ModuleDecl {
            name,
            scene,
            attributes,
            items,
            line,
            col,
        })
    }

    // ------------------------------------------------------------------
    // ADR-027 Phase 2 — types, functions, let bindings, expressions
    // ------------------------------------------------------------------

    /// Accept any identifier-shaped token (keyword or plain `Ident`) as an
    /// attribute name, method name, or path member. This is necessary because
    /// `monotone` and `antitone` are keywords in Phase 2 but may also appear
    /// as attribute names in the Phase 1 `@monotone` form.
    fn expect_any_ident(&mut self) -> Result<Token, ParseError> {
        let tok = self.peek().clone();
        if matches!(
            tok.kind,
            TokenKind::Ident | TokenKind::Monotone | TokenKind::Antitone
        ) {
            self.advance();
            Ok(tok)
        } else {
            Err(self.unexpected("identifier"))
        }
    }

    /// Parse a type: `Qualifier? BaseType`.
    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let qualifier = match self.peek().kind {
            TokenKind::Monotone => {
                self.advance();
                Qualifier::Monotone
            }
            TokenKind::Antitone => {
                self.advance();
                Qualifier::Antitone
            }
            _ => Qualifier::Unrestricted,
        };
        let base = self.parse_base_type()?;
        Ok(Type { qualifier, base })
    }

    /// Parse a base type: `i32 | f32 | string | bool | Vec<T> | Ident`.
    fn parse_base_type(&mut self) -> Result<BaseType, ParseError> {
        let tok = self.peek().clone();
        let base = match tok.kind {
            TokenKind::I32 => {
                self.advance();
                BaseType::I32
            }
            TokenKind::F32 => {
                self.advance();
                BaseType::F32
            }
            TokenKind::Str => {
                self.advance();
                BaseType::Str
            }
            TokenKind::Bool => {
                self.advance();
                BaseType::Bool
            }
            TokenKind::Vec => {
                self.advance();
                self.expect(TokenKind::Lt)?;
                let elem = self.parse_type()?;
                self.expect(TokenKind::Gt)?;
                BaseType::Vec(Box::new(elem))
            }
            TokenKind::Ident => {
                self.advance();
                BaseType::Named(tok.value.clone())
            }
            _ => return Err(self.unexpected("type")),
        };
        Ok(base)
    }

    /// Parse `fn name(params) -> Type { body }`. `attrs` are the leading
    /// attributes already collected.
    fn parse_fn(&mut self, attrs: Vec<Attribute>) -> Result<FnDecl, ParseError> {
        let kw = self.expect(TokenKind::Fn)?;
        let line = kw.line;
        let col = kw.col;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.value.clone();
        self.expect(TokenKind::LParen)?;
        self.skip_newlines();
        let mut params = Vec::new();
        loop {
            if matches!(self.peek().kind, TokenKind::RParen) {
                self.advance();
                break;
            }
            let ptok = self.expect(TokenKind::Ident)?;
            let pname = ptok.value.clone();
            let pline = ptok.line;
            let pcol = ptok.col;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            params.push(Param {
                name: pname,
                ty,
                line: pline,
                col: pcol,
            });
            self.skip_newlines();
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                self.expect(TokenKind::RParen)?;
                break;
            }
        }
        self.skip_newlines();
        let return_type = if matches!(self.peek().kind, TokenKind::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.skip_newlines();
        let body = self.parse_block()?;
        Ok(FnDecl {
            name,
            params,
            return_type,
            body,
            attrs,
            line,
            col,
        })
    }

    /// Parse `let name: Type = init;`. `attrs` are the leading attributes
    /// already collected.
    fn parse_let(&mut self, attrs: Vec<Attribute>) -> Result<LetDecl, ParseError> {
        let kw = self.expect(TokenKind::Let)?;
        let line = kw.line;
        let col = kw.col;
        let name_tok = self.expect(TokenKind::Ident)?;
        let name = name_tok.value.clone();
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        self.expect(TokenKind::Eq)?;
        self.skip_newlines();
        let init = self.parse_expr()?;
        self.expect(TokenKind::Semi)?;
        Ok(LetDecl {
            name,
            ty,
            init,
            attrs,
            line,
            col,
        })
    }

    /// Parse `{ stmt* }`.
    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let lb = self.expect(TokenKind::LBrace)?;
        let line = lb.line;
        let col = lb.col;
        self.skip_newlines();
        let mut stmts = Vec::new();
        loop {
            if matches!(self.peek().kind, TokenKind::RBrace) {
                self.advance();
                break;
            }
            let stmt_attrs = self.parse_leading_attributes()?;
            if !stmt_attrs.is_empty() {
                if !matches!(self.peek().kind, TokenKind::Let) {
                    return Err(
                        self.unexpected_msg("attributes inside a body must be followed by `let`")
                    );
                }
                let l = self.parse_let(stmt_attrs)?;
                stmts.push(Stmt::Let(l));
            } else {
                let s = self.parse_stmt()?;
                stmts.push(s);
            }
            self.skip_newlines();
        }
        Ok(Block { stmts, line, col })
    }

    /// Parse a single statement.
    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind {
            TokenKind::Let => {
                let l = self.parse_let(Vec::new())?;
                Ok(Stmt::Let(l))
            }
            TokenKind::Return => {
                let kw = self.advance().clone();
                self.skip_newlines();
                if matches!(self.peek().kind, TokenKind::Semi) {
                    self.advance();
                    return Ok(Stmt::Return(None, kw.line, kw.col));
                }
                let e = self.parse_expr()?;
                self.expect(TokenKind::Semi)?;
                Ok(Stmt::Return(Some(e), kw.line, kw.col))
            }
            _ => {
                let e = self.parse_expr()?;
                self.expect(TokenKind::Semi)?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    /// Parse an expression: literal, variable, method call, or path call.
    /// Parse an expression with binary operator precedence (Pratt parsing).
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary_expr(1) // min precedence 1 = all operators
    }

    /// Parse a binary expression with minimum precedence `min_prec`.
    /// Uses Pratt parsing: parse a primary, then while the next token is a
    /// binary operator with precedence >= min_prec, parse the RHS at the
    /// operator's precedence level.
    fn parse_binary_expr(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_primary()?;

        loop {
            // Check if the next token is a binary operator.
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::BangEq => BinOp::Ne,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::LtEq => BinOp::Le,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::GtEq => BinOp::Ge,
                TokenKind::AndAnd => BinOp::And,
                TokenKind::OrOr => BinOp::Or,
                _ => break, // Not a binary operator
            };

            let prec = op.precedence();
            if prec < min_prec {
                break;
            }

            let op_tok = self.advance().clone();
            // Right-associative operators would use `prec - 1`; all our
            // operators are left-associative, so we use `prec + 1`.
            let rhs = self.parse_binary_expr(prec + 1)?;
            lhs = Expr::Binary {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
                line: op_tok.line,
                col: op_tok.col,
            };
        }

        Ok(lhs)
    }

    /// Parse a primary expression (literal, variable, call, path call) followed
    /// by postfix `.method(args)` chains.
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();
        let mut expr = match tok.kind {
            TokenKind::Number => {
                self.advance();
                let v = tok.value.clone();
                if let Ok(i) = v.parse::<i64>() {
                    Expr::Lit(Lit::Int(i), tok.line, tok.col)
                } else if let Ok(f) = v.parse::<f64>() {
                    Expr::Lit(Lit::Float(f), tok.line, tok.col)
                } else {
                    return Err(self.unexpected_msg(format!("unparseable number `{}`", v)));
                }
            }
            TokenKind::String => {
                self.advance();
                Expr::Lit(Lit::Str(tok.value.clone()), tok.line, tok.col)
            }
            TokenKind::True => {
                self.advance();
                Expr::Lit(Lit::Bool(true), tok.line, tok.col)
            }
            TokenKind::False => {
                self.advance();
                Expr::Lit(Lit::Bool(false), tok.line, tok.col)
            }
            TokenKind::LParen => {
                // Parenthesized expression: `( expr )`
                self.advance();
                self.skip_newlines();
                let inner = self.parse_expr()?;
                self.skip_newlines();
                self.expect(TokenKind::RParen)?;
                inner
            }
            TokenKind::Ident | TokenKind::Vec => {
                // Could be `Vec::member(...)` (path call), `foo(args)` (call),
                // or a plain variable.
                if matches!(self.peek_at(1).kind, TokenKind::ColonColon) {
                    let module = tok.value.clone();
                    self.advance(); // ident/Vec
                    self.advance(); // ::
                    let member_tok = self.expect_any_ident()?;
                    let member = member_tok.value.clone();
                    self.expect(TokenKind::LParen)?;
                    self.skip_newlines();
                    let args = self.parse_arg_list()?;
                    Expr::PathCall(module, member, args, tok.line, tok.col)
                } else if matches!(self.peek_at(1).kind, TokenKind::LParen) {
                    // Function call: `ident(args)`
                    let name = tok.value.clone();
                    self.advance(); // ident
                    self.expect(TokenKind::LParen)?;
                    self.skip_newlines();
                    let args = self.parse_arg_list()?;
                    Expr::Call {
                        callee: name,
                        args,
                        line: tok.line,
                        col: tok.col,
                    }
                } else {
                    self.advance();
                    Expr::Var(tok.value.clone(), tok.line, tok.col)
                }
            }
            _ => return Err(self.unexpected("expression")),
        };

        // Postfix: `.method(args)` chains.
        loop {
            if matches!(self.peek().kind, TokenKind::Dot) {
                let dot = self.advance().clone();
                let m_tok = self.expect_any_ident()?;
                let method = m_tok.value.clone();
                self.expect(TokenKind::LParen)?;
                self.skip_newlines();
                let args = self.parse_arg_list()?;
                expr = Expr::MethodCall {
                    receiver: Box::new(expr),
                    method,
                    args,
                    line: dot.line,
                    col: dot.col,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    /// Parse `(arg, arg, ...)` including the closing `)`. The `(` must be
    /// the current token. Returns an empty vec for `()`.
    fn parse_arg_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        loop {
            if matches!(self.peek().kind, TokenKind::RParen) {
                self.advance();
                break;
            }
            let e = self.parse_expr()?;
            args.push(e);
            self.skip_newlines();
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                self.expect(TokenKind::RParen)?;
                break;
            }
        }
        Ok(args)
    }

    fn parse_scene(&mut self) -> Result<SceneDecl, ParseError> {
        // The scene block itself may carry leading attributes (rare, but
        // supported by the grammar).
        let attributes = self.parse_leading_attributes()?;
        let kw = self.expect(TokenKind::Scene)?;
        let line = kw.line;
        let col = kw.col;
        self.skip_newlines();
        self.expect(TokenKind::LBrace)?;
        self.skip_newlines();

        let mut background: Option<Color> = None;
        let mut nodes: Vec<NodeDecl> = Vec::new();

        loop {
            // Leading `@ident` attributes attach to the *next* node
            // declaration. They cannot be applied to the `background`
            // property or the closing `}`.
            let attrs = self.parse_leading_attributes()?;
            match self.peek().kind {
                TokenKind::RBrace => {
                    if !attrs.is_empty() {
                        return Err(self.unexpected_msg(
                            "attributes must be followed by `text` or `input-field`; found `}`",
                        ));
                    }
                    self.advance();
                    break;
                }
                TokenKind::Background => {
                    if !attrs.is_empty() {
                        return Err(self.unexpected_msg(
                            "attributes cannot be applied to the `background` property; \
                             expected `text` or `input-field`",
                        ));
                    }
                    let color = self.parse_background_property()?;
                    background = Some(color);
                    self.skip_newlines();
                }
                TokenKind::Text => {
                    // Disambiguate: `text` as a node declaration is
                    // followed by a String literal. (If followed by
                    // something else, it's an error.)
                    if !matches!(self.peek_at(1).kind, TokenKind::String) {
                        return Err(
                            self.unexpected_msg("`text` must be followed by a string literal")
                        );
                    }
                    let node = self.parse_text_node(attrs)?;
                    nodes.push(NodeDecl::Text(node));
                    self.skip_newlines();
                }
                TokenKind::InputField => {
                    let node = self.parse_input_field_node(attrs)?;
                    nodes.push(NodeDecl::InputField(node));
                    self.skip_newlines();
                }
                TokenKind::Eof => {
                    return Err(self.unexpected("closing `}`"));
                }
                _ => {
                    return Err(
                        self.unexpected("`background`, `text`, `input-field`, or closing `}`")
                    );
                }
            }
        }

        Ok(SceneDecl {
            background,
            nodes,
            attributes,
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
    fn parse_text_node(&mut self, attributes: Vec<Attribute>) -> Result<TextNode, ParseError> {
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
            attributes,
            line,
            col,
        })
    }

    /// `input-field { props* }`
    fn parse_input_field_node(
        &mut self,
        attributes: Vec<Attribute>,
    ) -> Result<InputFieldNode, ParseError> {
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
                    return Err(self.unexpected("`placeholder`, `position`, or closing `}`"));
                }
            }
            self.skip_newlines();
        }

        Ok(InputFieldNode {
            placeholder,
            position,
            attributes,
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
            _ => Err(self.unexpected("`center`, `below <name>`, or two numbers")),
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
        tok.value.parse::<f32>().map_err(|_| ParseError {
            message: format!("invalid number literal: `{}`", tok.value),
            line: tok.line,
            col: tok.col,
        })
    }

    // ------------------------------------------------------------------
    // Attribute parsing (ADR-027 Phase 1)
    // ------------------------------------------------------------------

    /// Parse zero or more leading `@ident` attribute annotations.
    ///
    /// Each attribute is a single `@` followed by an identifier. The
    /// identifier is NOT a reserved keyword — `monotone` and `antitone`
    /// are ordinary identifiers so the lint pass can recognise them
    /// without growing the keyword set.
    ///
    /// Returns an empty `Vec` if no `@` is present at the cursor. This
    /// keeps the call site backward-compatible: existing `.alk` files
    /// without attributes produce an empty list and parse unchanged.
    fn parse_leading_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attrs = Vec::new();
        while matches!(self.peek().kind, TokenKind::At) {
            // Clone line/col before the next mutable borrow.
            let (at_line, at_col) = {
                let at_tok = self.advance(); // consume `@`
                (at_tok.line, at_tok.col)
            };
            // In Phase 2, `monotone` and `antitone` are keywords, but they
            // are still valid attribute names in the `@monotone` form.
            // Accept any identifier-shaped token.
            let name_tok = self.expect_any_ident()?;
            attrs.push(Attribute {
                name: name_tok.value.clone(),
                line: at_line,
                col: at_col,
            });
            self.skip_newlines();
        }
        Ok(attrs)
    }

    /// Parse zero or more file-level shebang attributes (`#![...]`).
    ///
    /// Syntax:
    /// ```text
    /// ShebangAttr := '#!' '[' Ident ( '(' Ident ')' )? ']'
    /// ```
    ///
    /// The resulting [`Attribute::name`] is `"ident"` if no parens are
    /// present, or `"ident(arg)"` if they are. For example,
    /// `#![deny(monotonicity)]` becomes
    /// `Attribute { name: "deny(monotonicity)", ... }`.
    ///
    /// Returns an empty `Vec` if no `#!` is present at the cursor.
    fn parse_shebang_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attrs = Vec::new();
        while matches!(self.peek().kind, TokenKind::Shebang) {
            // Clone line/col before the next mutable borrow.
            let (bang_line, bang_col) = {
                let bang_tok = self.advance(); // consume `#!`
                (bang_tok.line, bang_tok.col)
            };
            self.expect(TokenKind::LBracket)?;
            let name_tok = self.expect(TokenKind::Ident)?;
            let mut name = name_tok.value.clone();
            // Optional `( arg )` payload.
            if matches!(self.peek().kind, TokenKind::LParen) {
                self.advance(); // consume `(`
                let arg_tok = self.expect(TokenKind::Ident)?;
                let arg = arg_tok.value.clone();
                self.expect(TokenKind::RParen)?;
                name.push('(');
                name.push_str(&arg);
                name.push(')');
            }
            self.expect(TokenKind::RBracket)?;
            attrs.push(Attribute {
                name,
                line: bang_line,
                col: bang_col,
            });
            self.skip_newlines();
        }
        Ok(attrs)
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
    let tokens = crate::lexer::tokenize(src).map_err(|e| ParseError {
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
        assert_eq!(
            m.scene.unwrap().background,
            Some(Color::Hex(0x11, 0x22, 0x33))
        );
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
        let m = parse_ok(
            r#"module M { scene { text "Hello!" { color: gold font-size: 64 rotation: y-axis 0.5 position: center } } }"#,
        );
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
        assert!(
            err.message.contains("unknown position"),
            "got: {}",
            err.message
        );
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
        let err = parse(r#"module M { scene { input-field { placeholder: 42 } } }"#).unwrap_err();
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

    // ------------------------------------------------------------------
    // Attribute parsing (ADR-027 Phase 1)
    // ------------------------------------------------------------------

    #[test]
    fn parse_monotone_attribute_on_text_node() {
        let m = parse_ok(r#"module M { scene { @monotone text "Hi" { } } }"#);
        let s = m.scene.expect("scene");
        match &s.nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.content, "Hi");
                assert_eq!(t.attributes.len(), 1);
                assert_eq!(t.attributes[0].name, "monotone");
                assert_eq!(t.attributes[0].line, 1);
                assert_eq!(t.attributes[0].col, 20); // position of `@`
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_antitone_attribute_on_input_field() {
        let m = parse_ok(r#"module M { scene { @antitone input-field { } } }"#);
        let s = m.scene.expect("scene");
        match &s.nodes[0] {
            NodeDecl::InputField(f) => {
                assert_eq!(f.attributes.len(), 1);
                assert_eq!(f.attributes[0].name, "antitone");
            }
            other => panic!("expected InputField, got {:?}", other),
        }
    }

    #[test]
    fn parse_multiple_attributes_on_node() {
        let m = parse_ok(r#"module M { scene { @monotone @antitone text "Hi" { } } }"#);
        let s = m.scene.expect("scene");
        match &s.nodes[0] {
            NodeDecl::Text(t) => {
                assert_eq!(t.attributes.len(), 2);
                assert_eq!(t.attributes[0].name, "monotone");
                assert_eq!(t.attributes[1].name, "antitone");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_shebang_deny_monotonicity_attribute() {
        let src = "#![deny(monotonicity)]\nmodule M { scene { } }";
        let m = parse_ok(src);
        assert_eq!(m.attributes.len(), 1);
        assert_eq!(m.attributes[0].name, "deny(monotonicity)");
        assert_eq!(m.attributes[0].line, 1);
        assert_eq!(m.attributes[0].col, 1);
    }

    #[test]
    fn parse_shebang_attribute_without_parens() {
        // A bare `#![deny]` (no parens) is also accepted by the grammar.
        let src = "#![deny]\nmodule M { scene { } }";
        let m = parse_ok(src);
        assert_eq!(m.attributes.len(), 1);
        assert_eq!(m.attributes[0].name, "deny");
    }

    #[test]
    fn parse_shebang_attribute_after_comment() {
        // Shebang attributes may follow `//` line comments at the top of file.
        let src = "// top-level comment\n#![deny(monotonicity)]\nmodule M { scene { } }";
        let m = parse_ok(src);
        assert_eq!(m.attributes.len(), 1);
        assert_eq!(m.attributes[0].name, "deny(monotonicity)");
    }

    #[test]
    fn parse_shebang_attribute_missing_bracket_errors() {
        let err = parse("#![deny(monotonicity)\nmodule M { scene { } }").unwrap_err();
        assert!(
            err.message.contains("`]`") || err.message.contains("bracket"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn parse_attribute_on_background_errors() {
        let err = parse(r#"module M { scene { @monotone background: #000000 } }"#).unwrap_err();
        assert!(
            err.message
                .contains("cannot be applied to the `background`"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn parse_trailing_attribute_errors() {
        // An attribute must be followed by a node declaration, not `}`.
        let err = parse(r#"module M { scene { text "Hi" { } @monotone } }"#).unwrap_err();
        assert!(
            err.message.contains("attributes must be followed"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn parse_existing_alk_without_attributes_unchanged() {
        // The canonical Hello-World source must parse to the same AST as
        // before (all attribute Vecs empty).
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
        assert!(m.attributes.is_empty(), "module attributes should be empty");
        let s = m.scene.expect("scene");
        assert!(s.attributes.is_empty(), "scene attributes should be empty");
        for n in &s.nodes {
            assert!(
                n.attributes().is_empty(),
                "node {:?} should have no attributes",
                n
            );
        }
    }
}
