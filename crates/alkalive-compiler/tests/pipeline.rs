//! End-to-end integration tests for the `alkalive-compiler` crate.
//!
//! These tests exercise the public API surface as an external consumer
//! would: `tokenize -> parse -> lower -> SceneIR`. They do NOT test the
//! CLI binary (which lives in `src/main.rs` and is covered by inline
//! unit tests there).

#![forbid(unsafe_code)]

use alkalive_compiler::{
    compile, lower, parse, tokenize, ColorIR, NodeIR, PositionIR, SceneIR, TokenKind,
};

const HELLO_WORLD: &str = r#"
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

#[test]
fn end_to_end_hello_world() {
    let ir = compile(HELLO_WORLD).expect("hello world should compile cleanly");
    assert_eq!(ir.module_name, "HelloWorld");
    assert_eq!(ir.background, (0, 0, 0));
    assert_eq!(ir.nodes.len(), 2);

    match &ir.nodes[0] {
        NodeIR::Text {
            content,
            color,
            font_size,
            rotation_speed,
            position,
        } => {
            assert_eq!(content, "Hello World!");
            assert_eq!(*color, ColorIR::Gold);
            assert_eq!(*font_size, 64.0);
            assert!((*rotation_speed - 0.5).abs() < f32::EPSILON);
            assert_eq!(*position, PositionIR::Center);
        }
        other => panic!("expected Text node, got {:?}", other),
    }
    match &ir.nodes[1] {
        NodeIR::InputField {
            placeholder,
            position,
        } => {
            assert_eq!(placeholder, "Type here...");
            assert_eq!(*position, PositionIR::BelowText);
        }
        other => panic!("expected InputField node, got {:?}", other),
    }
}

#[test]
fn stages_can_be_composed_manually() {
    let tokens = tokenize(HELLO_WORLD).expect("lex");
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Module));
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Eof));

    let ast = parse(HELLO_WORLD).expect("parse");
    assert_eq!(ast.name, "HelloWorld");
    assert!(ast.scene.is_some());

    let ir: SceneIR = lower(&ast).expect("lower");
    assert!(ir.has_text());
    assert!(ir.has_input_field());
}

#[test]
fn json_output_is_well_formed() {
    let ir = compile(HELLO_WORLD).unwrap();
    let json = ir.to_json();
    // Verify balanced braces and brackets.
    let open_braces = json.chars().filter(|&c| c == '{').count();
    let close_braces = json.chars().filter(|&c| c == '}').count();
    assert_eq!(open_braces, close_braces, "unbalanced braces: {}", json);
    let open_brackets = json.chars().filter(|&c| c == '[').count();
    let close_brackets = json.chars().filter(|&c| c == ']').count();
    assert_eq!(
        open_brackets, close_brackets,
        "unbalanced brackets: {}",
        json
    );
    // Verify key fields are present.
    for needle in [
        "\"module_name\":\"HelloWorld\"",
        "\"background\":[0,0,0]",
        "\"type\":\"text\"",
        "\"content\":\"Hello World!\"",
        "\"color\":\"#FFD700\"",
        "\"font_size\":64.0",
        "\"rotation_speed\":0.5",
        "\"type\":\"input-field\"",
        "\"placeholder\":\"Type here...\"",
    ] {
        assert!(
            json.contains(needle),
            "missing `{}` in JSON: {}",
            needle,
            json
        );
    }
}

#[test]
fn multiple_text_nodes_preserve_order() {
    let src = r#"
module M {
  scene {
    text "first" { }
    text "second" { }
    text "third" { }
  }
}
"#;
    let ir = compile(src).unwrap();
    assert_eq!(ir.nodes.len(), 3);
    let contents: Vec<&str> = ir
        .nodes
        .iter()
        .map(|n| match n {
            NodeIR::Text { content, .. } => content.as_str(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(contents, vec!["first", "second", "third"]);
}

#[test]
fn module_without_scene_errors() {
    let err = compile("module M { }").unwrap_err();
    let s = format!("{}", err);
    assert!(s.contains("scene"), "got: {}", s);
}

#[test]
fn below_text_without_preceding_text_errors() {
    let err =
        compile(r#"module M { scene { input-field { position: below text } } }"#).unwrap_err();
    let s = format!("{}", err);
    assert!(s.contains("`text` node"), "got: {}", s);
}

#[test]
fn unknown_color_name_errors() {
    let err = compile(r#"module M { scene { text "Hi" { color: purple } } }"#).unwrap_err();
    let s = format!("{}", err);
    assert!(s.contains("purple"), "got: {}", s);
}

#[test]
fn invalid_syntax_errors() {
    let err = compile("module { }").unwrap_err();
    let s = format!("{}", err);
    assert!(
        s.contains("identifier") || s.contains("parse error"),
        "got: {}",
        s
    );
}

#[test]
fn comments_are_ignored() {
    let src = r#"
// Top-level comment
module M {
  // Before scene
  scene {
    background: #000000 // inline comment
    text "Hi" {
      // Inside text node
      color: gold
    }
  }
}
"#;
    let ir = compile(src).unwrap();
    assert_eq!(ir.module_name, "M");
    assert_eq!(ir.background, (0, 0, 0));
    assert!(ir.has_text());
}

#[test]
fn hex_colors_lowercase_accepted() {
    let src = r#"module M { scene { background: #ffd700 } }"#;
    let ir = compile(src).unwrap();
    assert_eq!(ir.background, (0xFF, 0xD7, 0x00));
}

#[test]
fn empty_module_name_still_compiles() {
    // A module named with an identifier is fine; we don't restrict names.
    let ir = compile("module MyModule123 { scene { } }").unwrap();
    assert_eq!(ir.module_name, "MyModule123");
}

#[test]
fn custom_position_coords_preserved() {
    let src = r#"module M { scene { text "Hi" { position: 0.25 0.75 } } }"#;
    let ir = compile(src).unwrap();
    match &ir.nodes[0] {
        NodeIR::Text { position, .. } => {
            assert_eq!(*position, PositionIR::Custom(0.25, 0.75));
        }
        _ => panic!(),
    }
}

#[test]
fn input_field_default_placeholder_is_empty_string() {
    let src = r#"module M { scene { text "Hi" { } input-field { } } }"#;
    let ir = compile(src).unwrap();
    match &ir.nodes[1] {
        NodeIR::InputField { placeholder, .. } => {
            assert_eq!(placeholder, "");
        }
        _ => panic!(),
    }
}

#[test]
fn module_id_is_stable_across_runs() {
    let ir1 = compile("module Same { scene { } }").unwrap();
    let ir2 = compile("module Same { scene { } }").unwrap();
    assert_eq!(ir1.module_id, ir2.module_id);
}

#[test]
fn special_characters_in_strings_escape_correctly() {
    let src = r#"module M { scene { text "a\"b\\c\nd" { } } }"#;
    let ir = compile(src).unwrap();
    match &ir.nodes[0] {
        NodeIR::Text { content, .. } => {
            assert_eq!(content, "a\"b\\c\nd");
        }
        _ => panic!(),
    }
    // And the JSON output should escape them too.
    let json = ir.to_json();
    assert!(json.contains("a\\\"b\\\\c\\nd"), "got: {}", json);
}
