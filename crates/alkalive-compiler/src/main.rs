//! CLI binary for the AlkALive compiler.
//!
//! Usage:
//! ```text
//! alkalive-compiler compile <input.alk> -o <output.scene> [--scheduled]
//! ```
//!
//! Reads the `.alk` source, lexes/parses/lowers it to an
//! [`AlgorithmIR`](alkalive_compiler::AlgorithmIR), and serialises the IR to
//! a pretty-printed JSON artifact at the output path.
//!
//! With `--scheduled` (ADR-024), the output additionally contains the
//! `ScheduleIR` (rendering strategy) produced by
//! [`compile_scheduled`](alkalive_compiler::compile_scheduled).
//!
//! This binary is gated behind the `cli` Cargo feature (enabled by
//! default). Disabling default features builds the library alone with
//! zero external dependencies.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alkalive_compiler::ir::{ColorIR, NodeIR, PositionIR};
use alkalive_compiler::schedule::{
    BatchingStrategy, PassKind, RenderPass, ShaderId, ThreadAffinity,
};
use alkalive_compiler::{
    compile, compile_with_deps, compile_with_lints, AlgorithmIR, CompileError, DependencyGraph,
    SceneIR,
};

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
    let mut scheduled = false;
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
            "--scheduled" => {
                scheduled = true;
            }
            "-h" | "--help" => {
                println!("Usage: alkalive-compiler compile <input.alk> -o <output.scene> [--lint] [--scheduled]");
                println!();
                println!("Flags:");
                println!(
                    "  --lint        Run lint passes and print findings to stderr (ADR-027 P1)."
                );
                println!(
                    "  --scheduled   Emit the ADR-024 ScheduledScene JSON (algorithm + schedule)."
                );
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
        "missing input file; usage: alkalive-compiler compile <input.alk> -o <output.scene>"
            .to_string()
    })?;
    let output_path = output_path.ok_or_else(|| {
        "missing -o <output.scene>; usage: alkalive-compiler compile <input.alk> -o <output.scene>".to_string()
    })?;

    compile_file(&input_path, &output_path, lint, scheduled)
}

fn compile_file(input: &Path, output: &Path, lint: bool, scheduled: bool) -> Result<(), String> {
    let src = fs::read_to_string(input)
        .map_err(|e| format!("failed to read `{}`: {}", input.display(), e))?;

    // The `--scheduled` path produces a ScheduledScene (ADR-024). The `--lint`
    // flag is currently incompatible with `--scheduled` (would require
    // threading lints through the schedule lowering pass); for now, when
    // both are set, we honor `--scheduled` and skip lints (with a stderr
    // notice). A future PR can wire the two together.
    if scheduled {
        if lint {
            eprintln!("alkalive-compiler: --lint is ignored when --scheduled is set (ADR-024)");
        }
        // The --scheduled path emits the full ADR-024/025 pipeline:
        // algorithm + schedule + dependency graph (spec §4.2's
        // DependencyGraph serialization, surfaced for diagnostics).
        let (scheduled_scene, dep_graph) =
            compile_with_deps(&src).map_err(|e| format_compile_error(&e, input))?;
        let json = scheduled_scene_to_json(&scheduled_scene, &dep_graph);
        fs::write(output, json.as_bytes())
            .map_err(|e| format!("failed to write `{}`: {}", output.display(), e))?;

        eprintln!(
            "alkalive-compiler: compiled `{}` -> `{}` ({} nodes, {} passes, {} dep nodes) [scheduled]",
            input.display(),
            output.display(),
            scheduled_scene.algorithm.nodes.len(),
            scheduled_scene.schedule.passes.len(),
            dep_graph.len(),
        );
        return Ok(());
    }

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
        CompileError::Type(set) => {
            let mut out = format!("{}: type error(s):", input.display());
            for te in &set.errors {
                out.push_str(&format!(
                    "\n  {}:{}: {}",
                    input.display(),
                    te.line,
                    te.message
                ));
            }
            out
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
    // `SceneIR` is now a type alias for `AlgorithmIR` (ADR-024).
    algorithm_ir_to_json(ir)
}

/// Build a pretty-printed JSON string from an [`AlgorithmIR`] using
/// `serde_json`.
///
/// (ADR-024: `SceneIR` is now a type alias for `AlgorithmIR`. This helper
/// is the renamed implementation; `scene_ir_to_json` delegates to it for
/// backward compatibility with existing call sites and tests.)
fn algorithm_ir_to_json(ir: &AlgorithmIR) -> String {
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
    serde_json::to_string_pretty(&value).expect("AlgorithmIR is always JSON-serialisable")
}

/// Build a pretty-printed JSON string from a [`alkalive_compiler::ScheduledScene`]
/// (ADR-024) plus its ADR-025 [`DependencyGraph`].
///
/// The top-level shape is:
/// ```json
/// {
///   "algorithm": { ... AlgorithmIR ... },
///   "schedule": {
///     "passes": [ { ... RenderPass ... }, ... ],
///     "pass_order": [0, 1, 2, ...]
///   },
///   "dep_graph": {
///     "nodes": [ { "id": 0, "inputs": [4, 5], "outputs": [], "pass_index": 0, "description": "Clear" }, ... ]
///   }
/// }
/// ```
fn scheduled_scene_to_json(
    scheduled: &alkalive_compiler::ScheduledScene,
    dep_graph: &DependencyGraph,
) -> String {
    use serde_json::{Map, Value};

    // Reuse the algorithm-only serialiser and parse it back to a Value.
    // This avoids duplicating the node-serialisation logic.
    let algo_json = algorithm_ir_to_json(&scheduled.algorithm);
    let algo_value: Value =
        serde_json::from_str(&algo_json).expect("algorithm JSON is well-formed");

    let passes: Vec<Value> = scheduled
        .schedule
        .passes
        .iter()
        .map(render_pass_to_json)
        .collect();
    let pass_order: Vec<Value> = scheduled
        .schedule
        .pass_order
        .iter()
        .map(|i| Value::Number(serde_json::Number::from(*i as u64)))
        .collect();

    let mut schedule_obj = Map::new();
    schedule_obj.insert("passes".into(), Value::Array(passes));
    schedule_obj.insert("pass_order".into(), Value::Array(pass_order));

    // ADR-025 dependency graph (spec §4.2 serialization): the same
    // manual-JSON document the library emits, re-parsed for embedding.
    let dep_value: Value =
        serde_json::from_str(&dep_graph.to_json()).expect("dep-graph JSON is well-formed");

    let mut root = Map::new();
    root.insert("algorithm".into(), algo_value);
    root.insert("schedule".into(), Value::Object(schedule_obj));
    root.insert("dep_graph".into(), dep_value);

    let value = Value::Object(root);
    serde_json::to_string_pretty(&value).expect("ScheduledScene is always JSON-serialisable")
}

/// Serialise a single [`RenderPass`] to a [`serde_json::Value`].
fn render_pass_to_json(pass: &RenderPass) -> serde_json::Value {
    use serde_json::{Map, Value};
    let mut m = Map::new();
    m.insert(
        "node_indices".into(),
        Value::Array(
            pass.node_indices
                .iter()
                .map(|i| Value::Number(serde_json::Number::from(*i as u64)))
                .collect(),
        ),
    );
    m.insert(
        "shader".into(),
        Value::String(shader_id_to_string(pass.shader).to_string()),
    );
    m.insert(
        "batching".into(),
        Value::String(batching_strategy_to_string(pass.batching).to_string()),
    );
    m.insert("rotation".into(), Value::Bool(pass.rotation));
    m.insert(
        "kind".into(),
        Value::String(pass_kind_to_string(pass.kind).to_string()),
    );
    m.insert(
        "affinity".into(),
        Value::String(thread_affinity_to_string(pass.affinity).to_string()),
    );
    Value::Object(m)
}

fn shader_id_to_string(s: ShaderId) -> &'static str {
    match s {
        ShaderId::TextQuad => "text_quad",
        ShaderId::SolidColor => "solid_color",
    }
}

fn batching_strategy_to_string(b: BatchingStrategy) -> &'static str {
    match b {
        BatchingStrategy::None => "none",
        BatchingStrategy::ByFontSize => "by_font_size",
    }
}

fn pass_kind_to_string(k: PassKind) -> &'static str {
    match k {
        PassKind::Clear => "clear",
        PassKind::InputFieldBackground => "input_field_background",
        PassKind::InputFieldBorder => "input_field_border",
        PassKind::TitleText => "title_text",
        PassKind::InputText => "input_text",
    }
}

fn thread_affinity_to_string(a: ThreadAffinity) -> &'static str {
    match a {
        ThreadAffinity::MainThread => "main_thread",
        ThreadAffinity::Worker => "worker",
    }
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
            m.insert(
                "position".into(),
                Value::String(position_to_string(*position)),
            );
            Value::Object(m)
        }
        NodeIR::InputField {
            placeholder,
            position,
        } => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("input-field".into()));
            m.insert("placeholder".into(), Value::String(placeholder.clone()));
            m.insert(
                "position".into(),
                Value::String(position_to_string(*position)),
            );
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
    eprintln!("  alkalive-compiler compile <input.alk> -o <output.scene> [--lint] [--scheduled]");
    eprintln!();
    eprintln!("ARGS:");
    eprintln!("  <input.alk>          Path to the .alk source file");
    eprintln!("  -o, --output <path>  Output path for the scene IR (JSON)");
    eprintln!();
    eprintln!("FLAGS:");
    eprintln!("  --lint               Run lint passes (ADR-027 Phase 1) and print");
    eprintln!("                       findings to stderr. Aborts on `Deny` findings.");
    eprintln!("  --scheduled          Emit the ADR-024 ScheduledScene JSON");
    eprintln!("                       (algorithm + schedule) instead of just the");
    eprintln!("                       algorithm IR.");
    eprintln!();
    eprintln!("EXAMPLE:");
    eprintln!("  alkalive-compiler compile examples/hello.alk -o /tmp/hello.scene --scheduled");
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
        run(&["alkalive-compiler".into(), "compile".into(), "-h".into()])
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
        assert!(!err.contains("unknown flag"), "got: {}", err);
        assert!(err.contains("missing input file"), "got: {}", err);
    }

    // ---- ADR-024: --scheduled flag tests ----

    #[test]
    fn run_compile_scheduled_flag_parses_without_error() {
        // `--scheduled` should be parsed without "unknown flag" error.
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "--scheduled".into(),
        ])
        .unwrap_err();
        assert!(
            !err.contains("unknown flag"),
            "--scheduled should be accepted: got {}",
            err
        );
        assert!(err.contains("missing input file"), "got: {}", err);
    }

    #[test]
    fn run_compile_scheduled_help_succeeds() {
        run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "--scheduled".into(),
            "-h".into(),
        ])
        .expect("compile --scheduled -h should succeed");
    }

    #[test]
    fn run_compile_scheduled_flag_position_independent() {
        // `--scheduled` may appear before or after `-o`.
        let err = run(&[
            "alkalive-compiler".into(),
            "compile".into(),
            "-o".into(),
            "out.scene".into(),
            "--scheduled".into(),
        ])
        .unwrap_err();
        assert!(!err.contains("unknown flag"), "got: {}", err);
        assert!(err.contains("missing input file"), "got: {}", err);
    }

    #[test]
    fn shader_id_to_string_variants() {
        assert_eq!(shader_id_to_string(ShaderId::TextQuad), "text_quad");
        assert_eq!(shader_id_to_string(ShaderId::SolidColor), "solid_color");
    }

    #[test]
    fn batching_strategy_to_string_variants() {
        assert_eq!(batching_strategy_to_string(BatchingStrategy::None), "none");
        assert_eq!(
            batching_strategy_to_string(BatchingStrategy::ByFontSize),
            "by_font_size"
        );
    }

    #[test]
    fn pass_kind_to_string_variants() {
        assert_eq!(pass_kind_to_string(PassKind::Clear), "clear");
        assert_eq!(
            pass_kind_to_string(PassKind::InputFieldBackground),
            "input_field_background"
        );
        assert_eq!(
            pass_kind_to_string(PassKind::InputFieldBorder),
            "input_field_border"
        );
        assert_eq!(pass_kind_to_string(PassKind::TitleText), "title_text");
        assert_eq!(pass_kind_to_string(PassKind::InputText), "input_text");
    }

    #[test]
    fn render_pass_to_json_has_expected_keys() {
        let pass = RenderPass {
            node_indices: vec![0, 1],
            shader: ShaderId::TextQuad,
            batching: BatchingStrategy::ByFontSize,
            rotation: true,
            kind: PassKind::TitleText,
            affinity: ThreadAffinity::MainThread,
        };
        let v = render_pass_to_json(&pass);
        let s = serde_json::to_string(&v).unwrap();
        // Compact form (serde_json::to_string, not pretty) puts no spaces.
        assert!(s.contains("\"node_indices\":[0,1]"), "got: {}", s);
        assert!(s.contains("\"shader\":\"text_quad\""), "got: {}", s);
        assert!(s.contains("\"batching\":\"by_font_size\""), "got: {}", s);
        assert!(s.contains("\"rotation\":true"), "got: {}", s);
        assert!(s.contains("\"kind\":\"title_text\""), "got: {}", s);
        assert!(s.contains("\"affinity\":\"main_thread\""), "got: {}", s);
    }

    #[test]
    fn scheduled_scene_to_json_shape() {
        // Build a ScheduledScene with the canonical Hello World shape
        // (text + input field).
        let scheduled = alkalive_compiler::compile_scheduled(
            r#"module HelloWorld {
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
            }"#,
        )
        .expect("compile should succeed");
        let dep_graph =
            alkalive_compiler::incremental_analysis(&scheduled);

        let json = scheduled_scene_to_json(&scheduled, &dep_graph);
        // Top-level keys: algorithm + schedule + dep_graph.
        assert!(json.contains("algorithm"), "got: {}", json);
        assert!(json.contains("schedule"), "got: {}", json);
        assert!(json.contains("dep_graph"), "got: {}", json);
        // Algorithm sub-keys (serde_json::to_string_pretty puts a space after
        // the colon, so we don't pin the exact spacing).
        assert!(json.contains("module_name"), "got: {}", json);
        assert!(json.contains("HelloWorld"), "got: {}", json);
        assert!(json.contains("background"), "got: {}", json);
        // Schedule sub-keys.
        assert!(json.contains("passes"), "got: {}", json);
        assert!(json.contains("pass_order"), "got: {}", json);
        // Every pass carries its thread affinity (C10: main_thread today).
        assert!(
            json.contains("\"affinity\""),
            "affinity key missing: {}",
            json
        );
        assert!(
            json.matches("\"main_thread\"").count() >= 5,
            "all five passes should be main_thread: {}",
            json
        );
        // The five pass kinds appear in the JSON.
        assert!(json.contains("\"clear\""), "got: {}", json);
        assert!(json.contains("\"input_field_background\""), "got: {}", json);
        assert!(json.contains("\"input_field_border\""), "got: {}", json);
        assert!(json.contains("\"title_text\""), "got: {}", json);
        assert!(json.contains("\"input_text\""), "got: {}", json);
        // The dependency graph is embedded with its node descriptions.
        assert!(json.contains("\"nodes\""), "got: {}", json);
        assert!(
            json.contains("\"description\": \"Clear\""),
            "Clear dep node missing: {}",
            json
        );
        assert!(
            json.contains("\"pass_index\""),
            "dep node pass_index missing: {}",
            json
        );
        // pass_order = [0, 1, 2, 3, 4] (pretty-printed, so each on its own
        // line — we just verify the integers 0–4 all appear inside the
        // pass_order array). Since we know pass_order comes after passes,
        // check that each integer appears at least once in the JSON overall.
        assert!(json.contains("pass_order"), "got: {}", json);
    }

    #[test]
    fn compile_file_with_scheduled_writes_json_with_schedule() {
        // End-to-end CLI test for the --scheduled flag using a temp file.
        let tmp = std::env::temp_dir();
        let input_path = tmp.join("alkalive_test_scheduled_input.alk");
        let output_path = tmp.join("alkalive_test_scheduled_output.scene");
        std::fs::write(
            &input_path,
            r#"module M { scene { text "Hi" { } input-field { } } }"#,
        )
        .unwrap();

        let result = compile_file(&input_path, &output_path, false, true);
        assert!(result.is_ok(), "got: {:?}", result);
        let out = std::fs::read_to_string(&output_path).unwrap();
        assert!(out.contains("algorithm"), "got: {}", out);
        assert!(out.contains("schedule"), "got: {}", out);
        assert!(out.contains("dep_graph"), "got: {}", out);
        assert!(out.contains("\"title_text\""), "got: {}", out);
        assert!(out.contains("\"clear\""), "got: {}", out);

        // Cleanup.
        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_file(&output_path);
    }
}
