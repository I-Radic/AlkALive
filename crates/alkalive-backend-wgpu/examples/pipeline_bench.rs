//! Minimal performance benchmark harness for the CPU-side production path.
//!
//! Measures the exact stages the WASM runtime executes every frame, using
//! the canonical `examples/hello.alk` source and the real compiler:
//!
//! ```text
//! compile_full  →  build_render_graph  →  collect_frame_plan  →  tessellate_scene
//! ```
//!
//! GPU-side submission is verified separately by
//! `tests/offscreen_wgpu.rs`; this harness quantifies the CPU cost that
//! must fit within a frame budget on low-end hardware.
//!
//! Run with:
//!
//! ```text
//! cargo run --release -p alkalive-backend-wgpu --example pipeline_bench
//! ```

#![forbid(unsafe_code)]

use std::time::Instant;

use alkalive_backend_wgpu::frame_plan::collect_frame_plan;
use alkalive_backend_wgpu::tessellate::tessellate_scene;
use alkalive_backend_wgpu::TextSceneData;
use alkalive_compiler::compile_full;
use alkalive_render::graph::build_render_graph;

const HELLO_ALK: &str = include_str!("../../../examples/hello.alk");
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const WARMUP: usize = 20;
const ITERATIONS: usize = 300;

/// Run `f` `WARMUP` times, then `ITERATIONS` times, returning per-iteration
/// durations in nanoseconds (in execution order).
fn measure<T>(mut f: impl FnMut() -> T) -> Vec<u128> {
    for _ in 0..WARMUP {
        f();
    }
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_nanos());
    }
    samples
}

fn stats(samples: &[u128]) -> (f64, f64, f64) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let min = sorted[0] as f64;
    let median = sorted[n / 2] as f64;
    let mean = sorted.iter().sum::<u128>() as f64 / n as f64;
    (min, median, mean)
}

fn report(name: &str, samples: &[u128]) {
    let (min, median, mean) = stats(samples);
    println!(
        "{:<22} {:>10.1} {:>10.1} {:>10.1}",
        name,
        min / 1000.0,
        median / 1000.0,
        mean / 1000.0
    );
}

fn main() {
    println!(
        "AlkALive pipeline bench — {} iterations ({} warmup), {}x{} canvas",
        ITERATIONS, WARMUP, WIDTH, HEIGHT
    );
    println!(
        "{:<22} {:>10} {:>10} {:>10}",
        "stage", "min µs", "median µs", "mean µs"
    );

    // Stage 1: full compiler chain (parse → typecheck → lower → schedule →
    // incremental analysis → e-graph optimization), i.e. ADR-024/025/026.
    // The runtime executes this once at startup.
    let compile_samples = measure(|| compile_full(HELLO_ALK).expect("hello.alk must compile"));
    report("compile_full", &compile_samples);

    // Build the shared scene/graph inputs once (runtime startup work).
    let (scheduled, _dep_graph) = compile_full(HELLO_ALK).expect("hello.alk must compile");
    let scene = TextSceneData::default(); // golden-on-black Hello World
    let tess = tessellate_scene(&scene, WIDTH as f32, HEIGHT as f32).expect("tessellate");
    let graph = build_render_graph(&scene, (WIDTH, HEIGHT), tess.input_field_bounds);

    // Per-frame stages below are executed every RAF tick by the runtime.
    let graph_samples =
        measure(|| build_render_graph(&scene, (WIDTH, HEIGHT), (0.0, 0.0, 0.0, 0.0)));
    report("build_render_graph", &graph_samples);

    let plan_samples = measure(|| collect_frame_plan(&graph, WIDTH as f32, HEIGHT as f32, 1.0));
    report("collect_frame_plan", &plan_samples);

    let tess_samples =
        measure(|| tessellate_scene(&scene, WIDTH as f32, HEIGHT as f32).expect("tessellate"));
    report("tessellate_scene", &tess_samples);

    // Combined per-frame CPU prep (graph → plan → tessellation): the budget
    // that must hold before GPU submission inside one frame.
    let frame_prep = measure(|| {
        let g = build_render_graph(&scene, (WIDTH, HEIGHT), (0.0, 0.0, 0.0, 0.0));
        let p = collect_frame_plan(&g, WIDTH as f32, HEIGHT as f32, 1.0);
        let _t = tessellate_scene(&scene, WIDTH as f32, HEIGHT as f32).expect("tessellate");
        p
    });
    report("frame_prep_total", &frame_prep);

    let (_, _, mean_frame) = stats(&frame_prep);
    let fps_budget = if mean_frame > 0.0 {
        1_000_000_000.0 / mean_frame
    } else {
        f64::INFINITY
    };
    println!();
    println!(
        "mean frame-prep {:.1} µs ⇒ ~{:.0} fps of CPU-side headroom at {}×{}",
        mean_frame / 1000.0,
        fps_budget,
        WIDTH,
        HEIGHT
    );

    // Keep the schedule alive so its cost is not optimized out of the
    // measurement loop above.
    let _ = scheduled.schedule.passes.len();
}
