//! End-to-end file-based module resolution through the full compile pipeline.
//!
//! These tests prove file-based module resolution at the executed level: an
//! `import { name } from "path";` in a compiled module resolves to a REAL
//! `.alk` file on disk, its `pub fn` signatures merge into the checking
//! context, and resolution failures are hard errors — all without depending
//! on the process working directory.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use alkalive_compiler::{compile_full_in, compile_src_to_wasm};

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Create a unique temporary project directory (self-cleaning base).
fn unique_project_dir(label: &str) -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir()
        .join("alkalive_module_e2e")
        .join(format!("{}_{}_{}", label, std::process::id(), n));
    fs::create_dir_all(&dir).expect("create project dir");
    dir
}

/// Write a `.alk` module file under `project_dir` ("mylib/utils" →
/// `<project_dir>/mylib/utils.alk`), creating parent directories.
fn write_module(project_dir: &Path, module_path: &str, content: &str) {
    let file = project_dir.join(format!("{}.alk", module_path));
    fs::create_dir_all(file.parent().unwrap()).expect("create module dir");
    fs::write(&file, content).expect("write module file");
}

const UTILS_SRC: &str = r#"module utils {
  pub fn double(x: i32) -> i32 {
    return x * 2;
  }

  fn internal_helper() -> i32 {
    return 7;
  }
}
"#;

const APP_USING_DOUBLE: &str = r#"module app {
  import { double } from "mylib/utils";

  scene {
    background: #000000
    text "Hello World!" {
      color: gold
      font-size: 64
      position: center
    }
  }

  fn call_it() -> i32 {
    return double(21);
  }
}
"#;

#[test]
fn imported_pub_fn_resolves_and_pipeline_compiles() {
    let dir = unique_project_dir("ok");
    write_module(&dir, "mylib/utils", UTILS_SRC);

    let result = compile_full_in(APP_USING_DOUBLE, &dir);
    assert!(
        result.is_ok(),
        "import of a real pub fn must compile: {:?}",
        result.err()
    );
    let (scheduled, dep_graph) = result.unwrap();
    assert_eq!(scheduled.algorithm.module_name, "app");
    // Hello-World-shaped scene: Clear + TitleText (+ InputField passes only
    // when an input-field exists — this scene has none).
    assert_eq!(dep_graph.nodes.len(), scheduled.schedule.passes.len());
    assert!(scheduled.schedule.passes.len() >= 2);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_non_std_import_is_a_hard_compile_error() {
    let dir = unique_project_dir("missing");
    // No mylib/utils.alk written on purpose.

    let err = compile_full_in(APP_USING_DOUBLE, &dir)
        .err()
        .expect("missing non-std import must fail compilation");
    let msg = format!("{}", err);
    assert!(
        msg.contains("module resolution error"),
        "error should be a module resolution error, got: {}",
        msg
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn unparseable_imported_module_is_a_hard_compile_error() {
    let dir = unique_project_dir("broken");
    write_module(&dir, "mylib/utils", "module utils { this is not .alk");

    let err = compile_full_in(APP_USING_DOUBLE, &dir)
        .err()
        .expect("unparseable imported module must fail compilation");
    let msg = format!("{}", err);
    assert!(
        msg.contains("module resolution error") && msg.contains("parse error"),
        "error should mention the parse failure in the imported file, got: {}",
        msg
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn private_fns_are_not_importable() {
    let dir = unique_project_dir("private");
    write_module(&dir, "mylib/utils", UTILS_SRC);

    let src = r#"module app {
  import { internal_helper } from "mylib/utils";

  scene {
    background: #000000
    text "T" { color: gold
      font-size: 32
      position: center }
  }
}
"#;
    let err = compile_full_in(src, &dir)
        .err()
        .expect("importing a private fn must fail");
    assert!(
        format!("{}", err).contains("does not export"),
        "error should say the module does not export the name, got: {}",
        err
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn aliased_import_resolves_under_local_name() {
    let dir = unique_project_dir("alias");
    write_module(&dir, "mylib/utils", UTILS_SRC);

    let src = r#"module app {
  import { double as twice } from "mylib/utils";

  scene {
    background: #000000
    text "T" { color: gold
      font-size: 32
      position: center }
  }

  fn call_it() -> i32 {
    return twice(10);
  }
}
"#;
    let result = compile_full_in(src, &dir);
    assert!(
        result.is_ok(),
        "aliased import must resolve under its local name: {:?}",
        result.err()
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn std_imports_remain_lenient_when_unresolved() {
    // `std/` modules are host-provided; a bare `std/canvas` import must not
    // block compilation even though no file exists.
    let dir = unique_project_dir("std");
    let src = r#"module app {
  import { draw } from "std/canvas";

  scene {
    background: #000000
    text "T" { color: gold
      font-size: 32
      position: center }
  }

  fn tick() {
    draw();
  }
}
"#;
    let result = compile_full_in(src, &dir);
    assert!(
        result.is_ok(),
        "std/ imports stay host-provided/lenient: {:?}",
        result.err()
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn wasm_output_is_byte_deterministic_across_compilations() {
    // Two independent compilations construct independent HashMaps (fresh
    // random seeds per map); if any section were emitted in HashMap
    // iteration order, these byte strings would differ.
    let src = r#"module M {
  scene { background: #000000
    text "Determinism!" { color: gold
      font-size: 48
      position: center }
  }

  fn f(x: i32) -> i32 { return x * 3 + 1; }
}
"#;
    let a = compile_src_to_wasm(src).expect("first compile");
    let b = compile_src_to_wasm(src).expect("second compile");
    assert_eq!(a.bytes, b.bytes, "WASM output must be deterministic");
}
