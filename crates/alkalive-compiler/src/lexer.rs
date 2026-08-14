//! Lexer for the AlkALive `.alk` source language (Hello-World subset).
//!
//! The lexer converts a source string into a flat `Vec<Token>`. Each token
//! records its [`TokenKind`], its raw text, and the 1-based `line` / `col`
//! where it begins, so the parser can produce precise diagnostics.
//!
//! The supported subset mirrors the grammar in
//! `PURE_ALKALIVE_PIPELINE_PLAN.md` Wave 2:
//!
//! ```text
//! module HelloWorld {
//!   scene {
//!     background: #000000
//!     text "Hello World!" {
//!       color: gold
//!       font-size: 64
//!       rotation: y-axis 0.5
//!       position: center
//!     }
//!     input-field {
//!       placeholder: "Type here..."
//!       position: below text
//!     }
//!   }
//! }
//! ```
//!
//! Whitespace (spaces, tabs, carriage returns) is skipped. `//` line
//! comments run to the next `\n` and are dropped. Newlines are emitted as
//! their own [`TokenKind::Newline`] token so the parser may use them as
//! soft statement separators if desired.

#![forbid(unsafe_code)]

use core::fmt;

/// The set of token kinds produced by [`Lexer::tokenize`].
///
/// Keywords are their own variants so the parser can pattern-match on them
/// directly. Identifier-shaped strings that are *not* reserved keywords
/// (e.g. `HelloWorld`, `gold`, `center`, `below`, `y-axis`, or node
/// references) become [`TokenKind::Ident`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // ---- Keywords ----
    /// `module` — top-level module declaration.
    Module,
    /// `scene` — scene block inside a module.
    Scene,
    /// `text` — introduces a text node (`text "..." { ... }`) or references
    /// the text node (`position: below text`).
    Text,
    /// `input-field` — introduces an input-field node.
    InputField,
    /// `color` — property key inside a node block.
    Color,
    /// `font-size` — property key inside a node block.
    FontSize,
    /// `rotation` — property key inside a node block.
    Rotation,
    /// `position` — property key inside a node block.
    Position,
    /// `background` — property key inside a scene block.
    Background,
    /// `placeholder` — property key inside an input-field block.
    Placeholder,

    // ---- ADR-027 Phase 2: type-system keywords ----
    /// `fn` — introduces a function declaration.
    Fn,
    /// `let` — introduces a typed binding.
    Let,
    /// `monotone` — type qualifier (collection only grows).
    Monotone,
    /// `antitone` — type qualifier (collection only shrinks).
    Antitone,
    /// `i32` — primitive signed 32-bit integer type.
    I32,
    /// `f32` — primitive 32-bit float type.
    F32,
    /// `string` — primitive string type.
    Str,
    /// `bool` — primitive boolean type.
    Bool,
    /// `Vec` — built-in growable collection type.
    Vec,
    /// `true` — boolean literal.
    True,
    /// `false` — boolean literal.
    False,
    /// `return` — return statement.
    Return,
    /// `if` — conditional statement.
    If,
    /// `else` — conditional else clause.
    Else,
    /// `while` — loop statement.
    While,

    // ---- Literals ----
    /// An identifier that is not a reserved keyword. The text lives in
    /// [`Token::value`].
    Ident,
    /// A double-quoted string literal. `value` holds the *decoded* contents
    /// (escape sequences expanded).
    String,
    /// A numeric literal (integer or float). `value` holds the raw text;
    /// the parser interprets it as `f32` / `u32` as needed.
    Number,
    /// A hex color literal `#RRGGBB`. `value` holds the 6 hex digits (no `#`).
    HexColor,

    // ---- Punctuation ----
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `:`
    Colon,
    /// `.`
    Dot,
    /// `@` — introduces a leading attribute (e.g. `@monotone`). The
    /// attribute name is lexed as a separate [`TokenKind::Ident`] token
    /// immediately afterwards, so `@monotone` becomes `At` then
    /// `Ident("monotone")`.
    At,
    /// `#!` — file-level attribute introducer (e.g. `#![deny(monotonicity)]`).
    /// Must appear at the start of the file before the `module` keyword.
    Shebang,
    /// `[` — opening square bracket. Currently used only inside shebang
    /// file-level attribute payloads (e.g. `#![deny(monotonicity)]`).
    LBracket,
    /// `]` — closing square bracket.
    RBracket,
    /// `(` — opening parenthesis. Currently used only inside shebang
    /// attribute payloads (e.g. `#![deny(monotonicity)]`).
    LParen,
    /// `)` — closing parenthesis.
    RParen,
    /// `,` (ADR-027 Phase 2 — parameter / argument lists).
    Comma,
    /// `;` (ADR-027 Phase 2 — statement terminator).
    Semi,
    /// `=` (ADR-027 Phase 2 — `let` initialiser).
    Eq,
    /// `<` (ADR-027 Phase 2 — `Vec<T>` type parameter).
    Lt,
    /// `>` (ADR-027 Phase 2 — `Vec<T>` type parameter close).
    Gt,
    /// `->` (ADR-027 Phase 2 — function return type).
    Arrow,
    /// `::` (ADR-027 Phase 2 — path separator, e.g. `Vec::new()`).
    ColonColon,
    /// `!` — used inside shebang attribute payloads.
    Bang,
    /// `+` (binary addition operator).
    Plus,
    /// `-` (binary subtraction operator; note: `-` as negative-sign is
    /// already handled by the number lexer).
    Minus,
    /// `*` (binary multiplication operator).
    Star,
    /// `/` (binary division operator; note: `//` is a line comment).
    Slash,
    /// `%` (binary modulo operator).
    Percent,
    /// `==` (equality comparison).
    EqEq,
    /// `!=` (inequality comparison).
    BangEq,
    /// `<=` (less-than-or-equal).
    LtEq,
    /// `>=` (greater-than-or-equal).
    GtEq,
    /// `&&` (logical AND).
    AndAnd,
    /// `||` (logical OR).
    OrOr,

    // ---- Structural ----
    /// A newline (`\n`). Emitted so the parser may use it as a soft
    /// separator; the parser is not required to consume them.
    Newline,
    /// End of input. Always the last token in a stream returned by
    /// [`Lexer::tokenize`].
    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Module => write!(f, "keyword `module`"),
            TokenKind::Scene => write!(f, "keyword `scene`"),
            TokenKind::Text => write!(f, "keyword `text`"),
            TokenKind::InputField => write!(f, "keyword `input-field`"),
            TokenKind::Color => write!(f, "keyword `color`"),
            TokenKind::FontSize => write!(f, "keyword `font-size`"),
            TokenKind::Rotation => write!(f, "keyword `rotation`"),
            TokenKind::Position => write!(f, "keyword `position`"),
            TokenKind::Background => write!(f, "keyword `background`"),
            TokenKind::Placeholder => write!(f, "keyword `placeholder`"),
            TokenKind::Fn => write!(f, "keyword `fn`"),
            TokenKind::Let => write!(f, "keyword `let`"),
            TokenKind::Monotone => write!(f, "keyword `monotone`"),
            TokenKind::Antitone => write!(f, "keyword `antitone`"),
            TokenKind::I32 => write!(f, "keyword `i32`"),
            TokenKind::F32 => write!(f, "keyword `f32`"),
            TokenKind::Str => write!(f, "keyword `string`"),
            TokenKind::Bool => write!(f, "keyword `bool`"),
            TokenKind::Vec => write!(f, "keyword `Vec`"),
            TokenKind::True => write!(f, "keyword `true`"),
            TokenKind::False => write!(f, "keyword `false`"),
            TokenKind::Return => write!(f, "keyword `return`"),
            TokenKind::If => write!(f, "keyword `if`"),
            TokenKind::Else => write!(f, "keyword `else`"),
            TokenKind::While => write!(f, "keyword `while`"),
            TokenKind::Ident => write!(f, "identifier"),
            TokenKind::String => write!(f, "string literal"),
            TokenKind::Number => write!(f, "number"),
            TokenKind::HexColor => write!(f, "hex color"),
            TokenKind::LBrace => write!(f, "`{{`"),
            TokenKind::RBrace => write!(f, "`}}`"),
            TokenKind::Colon => write!(f, "`:`"),
            TokenKind::Dot => write!(f, "`.`"),
            TokenKind::At => write!(f, "`@`"),
            TokenKind::Shebang => write!(f, "`#!`"),
            TokenKind::LBracket => write!(f, "`[`"),
            TokenKind::RBracket => write!(f, "`]`"),
            TokenKind::LParen => write!(f, "`(`"),
            TokenKind::RParen => write!(f, "`)`"),
            TokenKind::Comma => write!(f, "`,`"),
            TokenKind::Semi => write!(f, "`;`"),
            TokenKind::Eq => write!(f, "`=`"),
            TokenKind::Lt => write!(f, "`<`"),
            TokenKind::Gt => write!(f, "`>`"),
            TokenKind::Arrow => write!(f, "`->`"),
            TokenKind::ColonColon => write!(f, "`::`"),
            TokenKind::Bang => write!(f, "`!`"),
            TokenKind::Plus => write!(f, "`+`"),
            TokenKind::Minus => write!(f, "`-`"),
            TokenKind::Star => write!(f, "`*`"),
            TokenKind::Slash => write!(f, "`/`"),
            TokenKind::Percent => write!(f, "`%`"),
            TokenKind::EqEq => write!(f, "`==`"),
            TokenKind::BangEq => write!(f, "`!=`"),
            TokenKind::LtEq => write!(f, "`<=`"),
            TokenKind::GtEq => write!(f, "`>=`"),
            TokenKind::AndAnd => write!(f, "`&&`"),
            TokenKind::OrOr => write!(f, "`||`"),
            TokenKind::Newline => write!(f, "newline"),
            TokenKind::Eof => write!(f, "end of input"),
        }
    }
}

/// A single token produced by [`Lexer::tokenize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// The raw source text the token spans. For [`TokenKind::String`] this
    /// is the *decoded* contents (quotes stripped, escapes expanded). For
    /// [`TokenKind::HexColor`] the leading `#` is stripped. For all other
    /// kinds it is the exact source substring.
    pub value: String,
    /// 1-based line number where the token begins.
    pub line: u32,
    /// 1-based column number where the token begins.
    pub col: u32,
}

impl Token {
    /// Convenience constructor for tests and external lexers.
    pub fn new(kind: TokenKind, value: impl Into<String>, line: u32, col: u32) -> Self {
        Self {
            kind,
            value: value.into(),
            line,
            col,
        }
    }

    /// Returns `true` if this token is a keyword variant.
    pub fn is_keyword(&self) -> bool {
        !matches!(
            self.kind,
            TokenKind::Ident
                | TokenKind::String
                | TokenKind::Number
                | TokenKind::HexColor
                | TokenKind::LBrace
                | TokenKind::RBrace
                | TokenKind::Colon
                | TokenKind::Dot
                | TokenKind::At
                | TokenKind::Shebang
                | TokenKind::LBracket
                | TokenKind::RBracket
                | TokenKind::LParen
                | TokenKind::RParen
                | TokenKind::Comma
                | TokenKind::Semi
                | TokenKind::Eq
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::Arrow
                | TokenKind::ColonColon
                | TokenKind::Bang
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::EqEq
                | TokenKind::BangEq
                | TokenKind::LtEq
                | TokenKind::GtEq
                | TokenKind::AndAnd
                | TokenKind::OrOr
                | TokenKind::Newline
                | TokenKind::Eof
        )
    }
}

/// A lexing error, carrying the 1-based line/column of the offending byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// Human-readable description.
    pub message: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub col: u32,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lex error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl core::error::Error for LexError {}

/// Classifies an identifier-shaped run of characters as a keyword
/// ([`Some(kind)`](TokenKind)) or a plain identifier (`None`).
fn classify_keyword(text: &str) -> Option<TokenKind> {
    Some(match text {
        "module" => TokenKind::Module,
        "scene" => TokenKind::Scene,
        "text" => TokenKind::Text,
        "input-field" => TokenKind::InputField,
        "color" => TokenKind::Color,
        "font-size" => TokenKind::FontSize,
        "rotation" => TokenKind::Rotation,
        "position" => TokenKind::Position,
        "background" => TokenKind::Background,
        "placeholder" => TokenKind::Placeholder,
        // ADR-027 Phase 2 type-system keywords.
        "fn" => TokenKind::Fn,
        "let" => TokenKind::Let,
        "monotone" => TokenKind::Monotone,
        "antitone" => TokenKind::Antitone,
        "i32" => TokenKind::I32,
        "f32" => TokenKind::F32,
        "string" => TokenKind::Str,
        "bool" => TokenKind::Bool,
        "Vec" => TokenKind::Vec,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "return" => TokenKind::Return,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        _ => return None,
    })
}

/// The lexer. Owns the source string and a byte cursor; produces tokens
/// on demand via [`Lexer::tokenize`].
pub struct Lexer<'src> {
    src: &'src str,
    bytes: &'src [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'src> Lexer<'src> {
    /// Construct a new lexer over `src`.
    pub fn new(src: &'src str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// Tokenize the entire source, returning the token vector (ending in a
    /// single [`TokenKind::Eof`] sentinel) or the first [`LexError`].
    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut out = Vec::new();
        loop {
            // Skip whitespace (excluding newlines, which are tokenised).
            self.skip_ws_and_comments();
            if self.pos >= self.bytes.len() {
                break;
            }
            let b = self.bytes[self.pos];
            if b == b'\n' {
                self.emit_newline(&mut out);
                continue;
            }
            let tok = self.next_token()?;
            out.push(tok);
        }
        out.push(Token::new(TokenKind::Eof, "", self.line, self.col));
        Ok(out)
    }

    /// Skip spaces, tabs, carriage returns, and `//` line comments.
    fn skip_ws_and_comments(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            match b {
                b' ' | b'\t' | b'\r' => self.advance_byte(),
                b'/' if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                    // Line comment: consume to end of line (not including \n).
                    while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                        self.advance_byte();
                    }
                }
                _ => break,
            }
        }
    }

    /// Emit a [`TokenKind::Newline`] token and consume the `\n`.
    fn emit_newline(&mut self, out: &mut Vec<Token>) {
        let line = self.line;
        let col = self.col;
        out.push(Token::new(TokenKind::Newline, "\n", line, col));
        // advance past \n: line+1, col=1
        self.pos += 1;
        self.line += 1;
        self.col = 1;
    }

    /// Advance one byte, updating line/col. Assumes the byte is NOT `\n`
    /// (newlines are handled by [`Self::emit_newline`]).
    fn advance_byte(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    /// Lex the next single token (after whitespace/comments have been
    /// skipped and a non-newline byte is at the cursor).
    fn next_token(&mut self) -> Result<Token, LexError> {
        let b = self.bytes[self.pos];
        let line = self.line;
        let col = self.col;
        match b {
            b'{' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::LBrace, "{", line, col))
            }
            b'}' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::RBrace, "}", line, col))
            }
            b':' => {
                // `::` vs `:`
                self.advance_byte();
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b':' {
                    self.advance_byte();
                    Ok(Token::new(TokenKind::ColonColon, "::", line, col))
                } else {
                    Ok(Token::new(TokenKind::Colon, ":", line, col))
                }
            }
            b'.' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::Dot, ".", line, col))
            }
            b',' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::Comma, ",", line, col))
            }
            b';' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::Semi, ";", line, col))
            }
            b'=' => {
                self.advance_byte();
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                    self.advance_byte();
                    Ok(Token::new(TokenKind::EqEq, "==", line, col))
                } else {
                    Ok(Token::new(TokenKind::Eq, "=", line, col))
                }
            }
            b'<' => {
                self.advance_byte();
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                    self.advance_byte();
                    Ok(Token::new(TokenKind::LtEq, "<=", line, col))
                } else {
                    Ok(Token::new(TokenKind::Lt, "<", line, col))
                }
            }
            b'>' => {
                self.advance_byte();
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                    self.advance_byte();
                    Ok(Token::new(TokenKind::GtEq, ">=", line, col))
                } else {
                    Ok(Token::new(TokenKind::Gt, ">", line, col))
                }
            }
            b'!' => {
                self.advance_byte();
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                    self.advance_byte();
                    Ok(Token::new(TokenKind::BangEq, "!=", line, col))
                } else {
                    Ok(Token::new(TokenKind::Bang, "!", line, col))
                }
            }
            b'+' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::Plus, "+", line, col))
            }
            b'*' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::Star, "*", line, col))
            }
            b'%' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::Percent, "%", line, col))
            }
            b'&' => {
                if self.bytes.get(self.pos + 1) == Some(&b'&') {
                    self.advance_byte();
                    self.advance_byte();
                    Ok(Token::new(TokenKind::AndAnd, "&&", line, col))
                } else {
                    Err(LexError {
                        message: "unexpected `&` (did you mean `&&`?)".into(),
                        line,
                        col,
                    })
                }
            }
            b'|' => {
                if self.bytes.get(self.pos + 1) == Some(&b'|') {
                    self.advance_byte();
                    self.advance_byte();
                    Ok(Token::new(TokenKind::OrOr, "||", line, col))
                } else {
                    Err(LexError {
                        message: "unexpected `|` (did you mean `||`?)".into(),
                        line,
                        col,
                    })
                }
            }
            b'-' => {
                // `->` (arrow) vs `-N` (negative number) vs `-` (binary minus)
                match self.bytes.get(self.pos + 1) {
                    Some(&b'>') => {
                        self.advance_byte();
                        self.advance_byte();
                        Ok(Token::new(TokenKind::Arrow, "->", line, col))
                    }
                    Some(c) if c.is_ascii_digit() => {
                        // Negative number: `-N` or `-N.N`
                        self.lex_number(line, col)
                    }
                    _ => {
                        // Binary minus operator
                        self.advance_byte();
                        Ok(Token::new(TokenKind::Minus, "-", line, col))
                    }
                }
            }
            b'@' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::At, "@", line, col))
            }
            b'[' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::LBracket, "[", line, col))
            }
            b']' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::RBracket, "]", line, col))
            }
            b'(' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::LParen, "(", line, col))
            }
            b')' => {
                self.advance_byte();
                Ok(Token::new(TokenKind::RParen, ")", line, col))
            }
            b'#' => {
                // `#!` is a file-level attribute introducer; otherwise `#`
                // begins a hex color literal.
                if self.bytes.get(self.pos + 1) == Some(&b'!') {
                    self.advance_byte(); // consume '#'
                    self.advance_byte(); // consume '!'
                    Ok(Token::new(TokenKind::Shebang, "#!", line, col))
                } else {
                    self.lex_hex_color(line, col)
                }
            }
            b'"' => self.lex_string(line, col),
            b'/' => {
                // Single `/` is division. (`//` comments are handled by the
                // whitespace skipper before we reach here.)
                self.advance_byte();
                Ok(Token::new(TokenKind::Slash, "/", line, col))
            }
            b'0'..=b'9' => self.lex_number(line, col),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident(line, col),
            other => Err(LexError {
                message: format!("unexpected byte {:#?}", other as char),
                line,
                col,
            }),
        }
    }

    /// Lex a `#RRGGBB` hex color. Expects exactly 6 hex digits after `#`.
    fn lex_hex_color(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        // Consume '#'.
        debug_assert_eq!(self.bytes[self.pos], b'#');
        self.advance_byte();
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_hexdigit() {
            self.advance_byte();
        }
        let digits = &self.src[start..self.pos];
        if digits.len() != 6 {
            return Err(LexError {
                message: format!(
                    "hex color must be exactly 6 hex digits (#RRGGBB), got {} digits: \"{}\"",
                    digits.len(),
                    digits
                ),
                line,
                col,
            });
        }
        Ok(Token::new(TokenKind::HexColor, digits, line, col))
    }

    /// Lex a double-quoted string with `\\`, `\"`, `\n`, `\t`, `\r` escapes.
    fn lex_string(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        debug_assert_eq!(self.bytes[self.pos], b'"');
        self.advance_byte(); // consume opening "
        let mut buf = String::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(LexError {
                    message: "unterminated string literal".into(),
                    line,
                    col,
                });
            }
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.advance_byte(); // consume closing "
                break;
            }
            if b == b'\\' {
                self.advance_byte();
                if self.pos >= self.bytes.len() {
                    return Err(LexError {
                        message: "unterminated escape in string literal".into(),
                        line,
                        col,
                    });
                }
                let esc = self.bytes[self.pos];
                let ch = match esc {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'n' => '\n',
                    b't' => '\t',
                    b'r' => '\r',
                    b'0' => '\0',
                    other => {
                        return Err(LexError {
                            message: format!("invalid escape \\{:#?}", other as char),
                            line: self.line,
                            col: self.col,
                        });
                    }
                };
                buf.push(ch);
                self.advance_byte();
                continue;
            }
            // For multi-byte UTF-8, consume the whole char.
            let ch_len = utf8_len(b);
            if ch_len == 0 {
                return Err(LexError {
                    message: format!("invalid UTF-8 byte {:#?}", b),
                    line: self.line,
                    col: self.col,
                });
            }
            if self.pos + ch_len > self.bytes.len() {
                return Err(LexError {
                    message: "truncated UTF-8 sequence in string".into(),
                    line: self.line,
                    col: self.col,
                });
            }
            let chunk = &self.src[self.pos..self.pos + ch_len];
            buf.push_str(chunk);
            for _ in 0..ch_len {
                self.advance_byte();
            }
        }
        Ok(Token::new(TokenKind::String, buf, line, col))
    }

    /// Lex a numeric literal: integer (`64`) or decimal float (`0.5`).
    /// A leading `-` or `+` is allowed only when followed by a digit.
    fn lex_number(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        let start = self.pos;
        let b = self.bytes[self.pos];
        if b == b'-' || b == b'+' {
            self.advance_byte();
            // Must be followed by a digit, else it's punctuation we don't handle.
            if self.pos >= self.bytes.len() || !self.bytes[self.pos].is_ascii_digit() {
                return Err(LexError {
                    message: format!("'{}' must be followed by a digit", b as char),
                    line,
                    col,
                });
            }
        }
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.advance_byte();
        }
        // Optional single fractional part.
        if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self
                .bytes
                .get(self.pos + 1)
                .map(|b| b.is_ascii_digit())
                .unwrap_or(false)
        {
            self.advance_byte(); // '.'
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.advance_byte();
            }
        }
        let text = &self.src[start..self.pos];
        Ok(Token::new(TokenKind::Number, text, line, col))
    }

    /// Lex an identifier or keyword. Identifiers start with `[A-Za-z_]` and
    /// continue with `[A-Za-z0-9_-]`. Hyphens are permitted *inside* an
    /// identifier (so `input-field`, `font-size`, `y-axis` are single
    /// tokens) but not at the start or end.
    fn lex_ident(&mut self, line: u32, col: u32) -> Result<Token, LexError> {
        let start = self.pos;
        debug_assert!(self.bytes[self.pos].is_ascii_alphabetic() || self.bytes[self.pos] == b'_');
        self.advance_byte();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance_byte();
            } else if b == b'-' {
                // Hyphen allowed only if followed by an alphanumeric.
                match self.bytes.get(self.pos + 1) {
                    Some(next) if next.is_ascii_alphanumeric() => {
                        self.advance_byte(); // consume '-'
                                             // advance_byte handles the next char on the next iteration
                    }
                    _ => break,
                }
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = classify_keyword(text).unwrap_or(TokenKind::Ident);
        Ok(Token::new(kind, text, line, col))
    }
}

/// Returns the expected UTF-8 byte length for a leading byte, or 0 if the
/// byte is not a valid UTF-8 leading byte.
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        0
    }
}

/// Convenience free function: tokenize `src` in one shot.
pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(src).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(toks: &[Token]) -> Vec<TokenKind> {
        toks.iter().map(|t| t.kind).collect()
    }

    /// Filter out newlines for assertions that don't care about them.
    fn kinds_no_nl(toks: &[Token]) -> Vec<TokenKind> {
        toks.iter()
            .filter(|t| t.kind != TokenKind::Newline)
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn lex_empty_input() {
        let toks = tokenize("").unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Eof);
        assert_eq!(toks[0].line, 1);
        assert_eq!(toks[0].col, 1);
    }

    #[test]
    fn lex_whitespace_only() {
        let toks = tokenize("   \t  \n  \n").unwrap();
        // newlines are tokens, trailing whitespace is skipped, then EOF.
        assert!(toks
            .iter()
            .all(|t| matches!(t.kind, TokenKind::Newline | TokenKind::Eof)));
        assert_eq!(
            toks.iter().filter(|t| t.kind == TokenKind::Newline).count(),
            2
        );
        assert_eq!(toks.last().unwrap().kind, TokenKind::Eof);
    }

    #[test]
    fn lex_line_comment_dropped() {
        let toks = tokenize("// this is a comment\nmodule").unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(filtered, vec![TokenKind::Module, TokenKind::Eof]);
    }

    #[test]
    fn lex_line_comment_at_eof_no_newline() {
        let toks = tokenize("module // trailing comment").unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(filtered, vec![TokenKind::Module, TokenKind::Eof]);
    }

    #[test]
    fn lex_keywords_classified() {
        let src = "module scene text input-field color font-size rotation position background placeholder";
        let toks = tokenize(src).unwrap();
        let expected = vec![
            TokenKind::Module,
            TokenKind::Scene,
            TokenKind::Text,
            TokenKind::InputField,
            TokenKind::Color,
            TokenKind::FontSize,
            TokenKind::Rotation,
            TokenKind::Position,
            TokenKind::Background,
            TokenKind::Placeholder,
            TokenKind::Eof,
        ];
        assert_eq!(kinds_no_nl(&toks), expected);
    }

    #[test]
    fn lex_hyphenated_identifiers() {
        let toks = tokenize("y-axis below center gold HelloWorld").unwrap();
        let kinds = kinds_no_nl(&toks);
        assert!(kinds
            .iter()
            .all(|k| *k == TokenKind::Ident || *k == TokenKind::Eof));
        assert_eq!(toks[0].value, "y-axis");
        assert_eq!(toks[1].value, "below");
        assert_eq!(toks[2].value, "center");
        assert_eq!(toks[3].value, "gold");
        assert_eq!(toks[4].value, "HelloWorld");
    }

    #[test]
    fn lex_trailing_hyphen_not_part_of_ident() {
        // `gold-` should lex as `gold` then `Minus` (binary minus operator).
        // (Previously `-` was an error, but Wave 7 added binary operators.)
        let toks = tokenize("gold-").unwrap();
        assert_eq!(toks[0].kind, TokenKind::Ident);
        assert_eq!(toks[0].value, "gold");
        assert_eq!(toks[1].kind, TokenKind::Minus);
    }

    #[test]
    fn lex_string_literal_simple() {
        let toks = tokenize("\"Hello World!\"").unwrap();
        assert_eq!(toks[0].kind, TokenKind::String);
        assert_eq!(toks[0].value, "Hello World!");
        assert_eq!(toks[0].line, 1);
        assert_eq!(toks[0].col, 1);
    }

    #[test]
    fn lex_string_literal_escapes() {
        let toks = tokenize("\"a\\\"b\\\\c\\nd\\te\"").unwrap();
        assert_eq!(toks[0].kind, TokenKind::String);
        assert_eq!(toks[0].value, "a\"b\\c\nd\te");
    }

    #[test]
    fn lex_string_literal_unterminated_errors() {
        let err = tokenize("\"unterminated").unwrap_err();
        assert!(err.message.contains("unterminated"));
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 1);
    }

    #[test]
    fn lex_string_with_ellipsis() {
        let toks = tokenize("\"Type here...\"").unwrap();
        assert_eq!(toks[0].kind, TokenKind::String);
        assert_eq!(toks[0].value, "Type here...");
    }

    #[test]
    fn lex_number_integer() {
        let toks = tokenize("64").unwrap();
        assert_eq!(toks[0].kind, TokenKind::Number);
        assert_eq!(toks[0].value, "64");
    }

    #[test]
    fn lex_number_float() {
        let toks = tokenize("0.5").unwrap();
        assert_eq!(toks[0].kind, TokenKind::Number);
        assert_eq!(toks[0].value, "0.5");
    }

    #[test]
    fn lex_number_negative() {
        let toks = tokenize("-12.5").unwrap();
        assert_eq!(toks[0].kind, TokenKind::Number);
        assert_eq!(toks[0].value, "-12.5");
    }

    #[test]
    fn lex_number_does_not_eat_trailing_dot() {
        // `64.` followed by `}` — the `.` should NOT be consumed because it's
        // not followed by a digit.
        let toks = tokenize("64.}").unwrap();
        assert_eq!(toks[0].kind, TokenKind::Number);
        assert_eq!(toks[0].value, "64");
        assert_eq!(toks[1].kind, TokenKind::Dot);
        assert_eq!(toks[2].kind, TokenKind::RBrace);
    }

    #[test]
    fn lex_hex_color_black() {
        let toks = tokenize("#000000").unwrap();
        assert_eq!(toks[0].kind, TokenKind::HexColor);
        assert_eq!(toks[0].value, "000000");
    }

    #[test]
    fn lex_hex_color_gold() {
        let toks = tokenize("#FFD700").unwrap();
        assert_eq!(toks[0].kind, TokenKind::HexColor);
        assert_eq!(toks[0].value, "FFD700");
    }

    #[test]
    fn lex_hex_color_too_short_errors() {
        let err = tokenize("#FFF").unwrap_err();
        assert!(err.message.contains("6 hex digits"), "got: {}", err.message);
    }

    #[test]
    fn lex_hex_color_too_long_errors() {
        let err = tokenize("#FFD7001").unwrap_err();
        assert!(err.message.contains("6 hex digits"), "got: {}", err.message);
    }

    #[test]
    fn lex_punctuation() {
        let toks = tokenize("{ } : .").unwrap();
        assert_eq!(
            kinds_no_nl(&toks),
            vec![
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Colon,
                TokenKind::Dot,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lex_newlines_tracked_with_positions() {
        let toks = tokenize("module\nscene").unwrap();
        // module, Newline, scene, Eof
        assert_eq!(toks[0].kind, TokenKind::Module);
        assert_eq!(toks[0].line, 1);
        assert_eq!(toks[0].col, 1);
        assert_eq!(toks[1].kind, TokenKind::Newline);
        assert_eq!(toks[1].line, 1);
        assert_eq!(toks[1].col, 7);
        assert_eq!(toks[2].kind, TokenKind::Scene);
        assert_eq!(toks[2].line, 2);
        assert_eq!(toks[2].col, 1);
    }

    #[test]
    fn lex_unexpected_byte_errors() {
        // `;` is now a valid token (Phase 2 statement terminator), so use
        // a genuinely illegal byte to exercise the error path.
        let err = tokenize("`").unwrap_err();
        assert!(
            err.message.contains("unexpected byte"),
            "got: {}",
            err.message
        );
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 1);
    }

    #[test]
    fn lex_at_sign_token() {
        // `@` is the attribute introducer; it is now its own token kind.
        let toks = tokenize("@").unwrap();
        assert_eq!(toks[0].kind, TokenKind::At);
        assert_eq!(toks[0].value, "@");
        assert_eq!(toks[0].line, 1);
        assert_eq!(toks[0].col, 1);
        assert_eq!(toks[1].kind, TokenKind::Eof);
    }

    #[test]
    fn lex_monotone_attribute() {
        // In Phase 2, `monotone` is a reserved keyword. `@monotone` lexes
        // as `At` followed by `Monotone` — the attribute name IS a keyword
        // token, but the parser still accepts it as an attribute name.
        let toks = tokenize("@monotone").unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(
            filtered,
            vec![TokenKind::At, TokenKind::Monotone, TokenKind::Eof]
        );
        assert_eq!(toks[0].kind, TokenKind::At);
        assert_eq!(toks[1].kind, TokenKind::Monotone);
        assert_eq!(toks[1].value, "monotone");
        assert!(toks[1].is_keyword(), "monotone IS a keyword in Phase 2");
    }

    #[test]
    fn lex_antitone_attribute() {
        let toks = tokenize("@antitone").unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(
            filtered,
            vec![TokenKind::At, TokenKind::Antitone, TokenKind::Eof]
        );
        assert_eq!(toks[1].value, "antitone");
    }

    #[test]
    fn lex_shebang_token() {
        // `#!` is the file-level attribute introducer.
        let toks = tokenize("#!").unwrap();
        assert_eq!(toks[0].kind, TokenKind::Shebang);
        assert_eq!(toks[0].value, "#!");
        assert_eq!(toks[1].kind, TokenKind::Eof);
    }

    #[test]
    fn lex_shebang_does_not_swallow_hex_color() {
        // A bare `#` followed by hex digits is still a hex color.
        let toks = tokenize("#FFD700").unwrap();
        assert_eq!(toks[0].kind, TokenKind::HexColor);
        assert_eq!(toks[0].value, "FFD700");
    }

    #[test]
    fn lex_brackets_and_parens() {
        // `[`, `]`, `(`, `)` are now their own punctuation tokens (used by
        // the shebang attribute payload `#![...]`).
        let toks = tokenize("[ ] ( )").unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(
            filtered,
            vec![
                TokenKind::LBracket,
                TokenKind::RBracket,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lex_shebang_deny_monotonicity_payload() {
        // `#![deny(monotonicity)]` lexes as Shebang + LBracket + Ident +
        // LParen + Ident + RParen + RBracket.
        let toks = tokenize("#![deny(monotonicity)]").unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(
            filtered,
            vec![
                TokenKind::Shebang,
                TokenKind::LBracket,
                TokenKind::Ident, // deny
                TokenKind::LParen,
                TokenKind::Ident, // monotonicity
                TokenKind::RParen,
                TokenKind::RBracket,
                TokenKind::Eof,
            ]
        );
        assert_eq!(toks[2].value, "deny");
        assert_eq!(toks[4].value, "monotonicity");
    }

    #[test]
    fn lex_full_hello_world_source() {
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
        let toks = tokenize(src).unwrap();
        let filtered = kinds_no_nl(&toks);
        assert_eq!(
            filtered,
            vec![
                TokenKind::Module,
                TokenKind::Ident, // HelloWorld
                TokenKind::LBrace,
                TokenKind::Scene,
                TokenKind::LBrace,
                TokenKind::Background,
                TokenKind::Colon,
                TokenKind::HexColor, // 000000
                TokenKind::Text,
                TokenKind::String, // Hello World!
                TokenKind::LBrace,
                TokenKind::Color,
                TokenKind::Colon,
                TokenKind::Ident, // gold
                TokenKind::FontSize,
                TokenKind::Colon,
                TokenKind::Number, // 64
                TokenKind::Rotation,
                TokenKind::Colon,
                TokenKind::Ident,  // y-axis
                TokenKind::Number, // 0.5
                TokenKind::Position,
                TokenKind::Colon,
                TokenKind::Ident, // center
                TokenKind::RBrace,
                TokenKind::InputField,
                TokenKind::LBrace,
                TokenKind::Placeholder,
                TokenKind::Colon,
                TokenKind::String, // Type here...
                TokenKind::Position,
                TokenKind::Colon,
                TokenKind::Ident, // below
                TokenKind::Text,  // text (keyword used as node ref)
                TokenKind::RBrace,
                TokenKind::RBrace,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn token_is_keyword_helper() {
        let toks = tokenize("module gold").unwrap();
        assert!(toks[0].is_keyword());
        assert!(!toks[1].is_keyword());
    }

    #[test]
    fn tokenkind_display_smoke() {
        // Touch every variant's Display impl so a typo would surface.
        let all_kinds = [
            TokenKind::Module,
            TokenKind::Scene,
            TokenKind::Text,
            TokenKind::InputField,
            TokenKind::Color,
            TokenKind::FontSize,
            TokenKind::Rotation,
            TokenKind::Position,
            TokenKind::Background,
            TokenKind::Placeholder,
            TokenKind::Ident,
            TokenKind::String,
            TokenKind::Number,
            TokenKind::HexColor,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Colon,
            TokenKind::Dot,
            TokenKind::At,
            TokenKind::Shebang,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::Newline,
            TokenKind::Eof,
        ];
        for k in all_kinds {
            let s = format!("{}", k);
            assert!(!s.is_empty());
        }
        // Also exercise the unused `kinds` helper to keep it warm.
        let _ = kinds(&toks_module_scene());
    }

    fn toks_module_scene() -> Vec<Token> {
        tokenize("module scene").unwrap()
    }
}
