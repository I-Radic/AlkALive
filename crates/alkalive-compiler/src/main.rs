//! CLI binary for the AlkALive compiler.
//!
//! Usage:
//! ```text
//! alkalive-compiler compile <input.alk> -o <output.scene>
//! ```
//!
//! Reads the `.alk` source, lexes/parses/lowers it to a [`SceneIR`],
//! and serialises the IR to a pretty-printed JSON artifact at the output
//! path.
//!
//! This binary is gated behind the `cli` Cargo feature (enabled by
//! default). Disabling default features builds the library alone with
//! zero external dependencies.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alkalive_compiler::ir::{ColorIR, NodeIR, PositionIR, SceneIR};
use alkalive_compiler::{compile, compile_with_lints, CompileError};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("alkalive-compiler: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        print_usage();
        return Err("missing subcommand".into());
    }

    match args[1].as_str() {
        "compile" => run_compile(&args[2..]),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => {
            print_usage();
            Err(format!("unknown subcommand `{}`", other))
        }
    }
}

fn run_compile(args: &[String]) -> Result<(), String> {
    let mut input_path: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut lint = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("`{}` requires a path argument", arg));
                }
                output_path = Some(PathBuf::from(&args[i]));
            }
            "--lint" => {
                lint = true;
            }
            "-h" | "--help" => {
                println!("Usage: alkalive-compiler compile <input.alk> -o <output.scene> [--lint]");
                println!();
                println!("Flags:");
                println!("  --lint   Run lint passes and print findings to stderr (ADR-027 P1).");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag `{}`", other));
            }
            _ => {
                if input_path.is_some() {
                    return Err(format!("unexpected extra argument `{}`", arg));
                }
                input_path = Some(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    let input_path = input_path.ok_or_else(|| {
        "missing input file; usage: alkalive-compiler compile <input.alk> -o <output.scene>".to_string()
    })?;
    let output_path = output_path.ok_or_else(|| {
        "missing -o <output.scene>; usage: alkalive-compiler compile <input.alk> -o <output.scene>".to_string()
    })?;

    compile_file(&input_path, &output_path, lint)
}

fn compile_file(input: &Path, output: &Path, lint: bool) -> Result<(), String> {
    let src = fs::read_to_string(input)
        .map_err(|e| format!("failed to read `{}`: {}", input.display(), e))?;

    let ir = if lint {
        let (ir, lint_set) =
            compile_with_lints(&src).map_err(|e| format_compile_error(&e, input))?;
        // Surface lint findings to stderr.
        if !lint_set.is_empty() {
            eprintln!(
                "alkalive-compiler: {} lint finding(s) for `{}`:",
                lint_set.len(),
                input.display()
            );
            for report in lint_set.iter() {
                eprintln!("  {}", report.render());
            }
        }
        if lint_set.has_errors() {
            let deny_count = lint_set
                .iter()
                .filter(|r| r.severity == alkalive_compiler::LintSeverity::Deny)
                .count();
            return Err(format!(
                "{} lint pass reported {} error(s); aborting",
                input.display(),
                deny_count
            ));
        }
        ir
    } else {
        compile(&src).map_err(|e| format_compile_error(&e, input))?
    };

    let json = scene_ir_to_json(&ir);
    fs::write(output, json.as_bytes())
        .map_err(|e| format!("failed to write `{}`: {}", output.display(), e))?;

    eprintln!(
        "alkalive-compiler: compiled `{}` -> `{}` ({} nodes){}",
        input.display(),
        output.display(),
        ir.nodes.len(),
        if lint { " [+lint]" } else { "" }
    );

    Ok(())
}

/// Format a [`CompileError`] with source-file context.
fn format_compile_error(e: &CompileError, input: &Path) -> String {
    match e {
        CompileError::Parse(pe) => {
            format!("{}:{}: {}", input.display(), pe.line, pe.message)
        }
        CompileError::Codegen(ce) => {
            format!("{}:{}: {}", input.display(), ce.line, ce.message)
        }
    }
}

/// Build a pretty-printed JSON string from a [`SceneIR`] using `serde_json`.
///
/// We construct a [`serde_json::Value`] by hand (rather than deriving
/// `Serialize` on `SceneIR`) so that the *library* can stay free of
/// external dependencies — only the binary pulls in `serde_json`.
fn scene_ir_to_json(ir: &SceneIR) -> String {
    use serde_json::{json, Map, Number, Value};

    let mut nodes: Vec<Value> = Vec::with_capacity(ir.nodes.len());
    for node in &ir.nodes {
        nodes.push(node_to_json(node));
    }

    let mut root = Map::new();
    root.insert(
        "module_id".into(),
        Value::Number(Number::from(ir.module_id.0)),
    );
    root.insert("module_name".into(), Value::String(ir.module_name.clone()));
    root.insert(
        "background".into(),
        json!([ir.background.0, ir.background.1, ir.background.2]),
    );
    root.insert("nodes".into(), Value::Array(nodes));

    let value = Value::Object(root);
    serde_json::to_string_pretty(&value).expect("SceneIR is always JSON-serialisable")
}

fn node_to_json(node: &NodeIR) -> serde_json::Value {
    use serde_json::{json, Map, Value};
    match node {
        NodeIR::Text {
            content,
            color,
            font_size,
            rotation_speed,
            position,
        } => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("text".into()));
            m.insert("content".into(), Value::String(content.clone()));
            m.insert("color".into(), Value::String(color_to_string(*color)));
            m.insert("font_size".into(), json!(*font_size));
            m.insert("rotation_speed".into(), json!(*rotation_speed));
            m.insert("position".into(), Value::String(position_to_string(*position)));
            Value::Object(m)
        }
        NodeIR::InputField { placeholder, position } => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("input-field".into()));
            m.insert("placeholder".into(), Value::String(placeholder.clone()));
            m.insert("position".into(), Value::String(position_to_string(*position)));
            Value::Object(m)
        }
    }
}

fn color_to_string(c: ColorIR) -> String {
    let (r, g, b) = c.rgb();
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

fn position_to_string(p: PositionIR) -> String {
    match p {
        PositionIR::Center => "center".into(),
        PositionIR::BelowText => "below-text".into(),
        PositionIR::Custom(x, y) => format!("({}, {})", x, y),
    }
}

fn print_usage() {
    eprintln!("AlkALive compiler — lexes/parses .alk source and emits a SceneIR JSON artifact");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  alkalive-compiler compile <input.alk> -o <output.scene> [--lint]");
    eprintln!();
    eprintln!("ARGS:");
    eprintln!("  <input.alk>          Path to the .alk source file");
    eprintln!("  -o, --output <path>  Output path for the scene IR (JSON)");
    eprintln!();
    eprintln!("FLAGS:");
    eprintln!("  --lint               Run lint passes (ADR-027 Phase 1) and print");
    eprintln!("                       findings to stderr. Aborts on `Deny` findings.");
    eprintln!();
    eprintln!("EXAMPLE:");
    eprintln!("  alkalive-compiler compile examples/hello.alk -o /tmp/hello.scene --lint");
}

#[cfg(test)]
mod tests {
    use super::*;
    use alkalive_compiler::ir::mint_module_id;

    #[test]
    fn color_to_string_gold() {
        assert_eq!(color_to_string(ColorIR::Gold), "#FFD700");
    }

    #[test]
    fn color_to_string_solid() {
        assert_eq!(color_to_string(ColorIR::Solid(0, 0, 0)), "#000000");
        assert_eq!(color_to_string(ColorIR::Solid(0xFF, 0xD7, 0x00)), "#FFD700");
    }

    #[test]
    fn position_to_string_variants() {
        assert_eq!(position_to_string(PositionIR::Center), "center");
        assert_eq!(position_to_string(PositionIR::BelowText), "below-text");
        assert_eq!(
            position_to_string(PositionIR::Custom(0.5, 0.25)),
            "(0.5, 0.25)"
        );
    }

    #[test]
    fn scene_ir_to_json_minimal() {
        let ir = SceneIR::new(mint_module_id("M"), "M");
        let json = scene_ir_to_json(&ir);
        assert!(json.contains("\"module_name\""));
        assert!(json.contains("\"M\""));
        assert!(json.contains("\"background\""));
        assert!(json.contains("\"nodes\""));
    }

    #[test]
    fn scene_ir_to_json_with_text_node() {
        let mut ir = SceneIR::new(mint_module_id("M"), "M");
        ir.nodes.push(NodeIR::Text {
            content: "Hi".into(),
            color: ColorIR::Gold,
            font_size: 64.0,
            rotation_speed: 0.5,
            position: PositionIR::Center,
        });
        let json = scene_ir_to_json(&ir);
        assert!(json.contains("\"type\""));
        assert!(json.contains("\"text\""));
        assert!(json.contains("\"Hi\""));
        assert!(json.contains("\"#FFD700\""));
        assert!(json.contains("64.0"));
        assert!(json.contains("0.5"));
    }

    #[test]
    fn run_no_args_errors() {
        let err = run(&[]).unwrap_err();
        assert!(err.contains("missing subcommand"), "got: {}", err);
    }

    #[test]
    fn run_unknown_subcommand_errors() {
        let err = run(&["alkalive-compiler".into(), "frobnicate".into()]).unwrap_err();
        assert!(err.contains("unknown subcommand"), "got: {}", err);
    }

    #[test]
    fn run_compile_missing_input_errors() {
        let err = run(&["alkalive-compiler".into(), "compile".into()]).unwrap_err();
        assert!(err.contains("missing input file"), "got: {}", err);
    }

    #[test]
    fn run_compile_missing_output_errors() {
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "hello.alk".into(),
        ])
        .unwrap_err();
        assert!(err.contains("missing -o"), "got: {}", err);
    }

    #[test]
    fn run_compile_o_without_path_errors() {
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "hello.alk".into(),
            "-o".into(),
        ])
        .unwrap_err();
        assert!(err.contains("requires a path"), "got: {}", err);
    }

    #[test]
    fn run_compile_unknown_flag_errors() {
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "hello.alk".into(),
            "--bogus".into(),
        ])
        .unwrap_err();
        assert!(err.contains("unknown flag"), "got: {}", err);
    }

    #[test]
    fn run_compile_extra_positional_errors() {
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "hello.alk".into(),
            "extra.alk".into(),
            "-o".into(),
            "out.scene".into(),
        ])
        .unwrap_err();
        assert!(err.contains("extra argument"), "got: {}", err);
    }

    #[test]
    fn run_help_succeeds() {
        run(&["alkalive-compiler".into(), "help".into()]).expect("help should succeed");
    }

    #[test]
    fn run_compile_lint_flag_parses_without_error() {
        // `--lint` should be parsed without "unknown flag" error. The
        // command will then fail because no input file is provided.
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "--lint".into(),
        ])
        .unwrap_err();
        assert!(
            !err.contains("unknown flag"),
            "--lint should be accepted: got {}",
            err
        );
        assert!(err.contains("missing input file"), "got: {}", err);
    }

    #[test]
    fn run_compile_lint_help_succeeds() {
        run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "-h".into(),
        ])
        .expect("compile -h should succeed");
    }

    #[test]
    fn run_compile_lint_flag_position_independent() {
        // `--lint` may appear before or after `-o`. It must always be
        // accepted.
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "-o".into(),
            "out.scene".into(),
            "--lint".into(),
        ])
        .unwrap_err();
        assert!(
            !err.contains("unknown flag"),
            "got: {}",
            err
        );
        assert!(err.contains("missing input file"), "got: {}", err);
    }
}
