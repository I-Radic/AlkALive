//! Per-frame draw-call planning — the GPU-API-free bridge between the
//! render-graph IR and any GPU encoder.
//!
//! [`collect_frame_plan`] walks a [`RenderGraph`] in `pass_order` exactly
//! once and produces:
//!
//! - the clear color extracted from the first `Clear` draw call,
//! - one uniform record per rect-kind / text-kind draw call
//!   ([`RectUniformsData`] / [`TextUniformsData`]), and
//! - a 1:1 [`PlannedDraw`] encode list aligned with graph iteration order.
//!
//! This module is target-agnostic and unit-tested on native; the wasm32
//! wgpu renderer consumes it in its encode loop.
//!
//! # Uniform layout contract
//!
//! The uniform structs are byte-level mirrors of the WGSL declarations in
//! [`crate::wgsl_shaders`]. Offsets are asserted against naga-parsed WGSL
//! layouts (`wgsl_shaders::tests::wgsl_uniform_struct_layout_contract`) and
//! against Rust-side offsets (`uniform_layout_parity_*` here).

/// Byte-exact mirror of the WGSL `TextUniforms` struct. Offset contract:
/// `rotation` @0, `canvas_size` @8, `time` @16, `text_color` @32, size 48.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextUniformsData {
    /// Y-axis rotation angle (radians).
    pub rotation: f32,
    /// Explicit padding so `canvas_size` lands at WGSL offset 8.
    pub _pad0: f32,
    /// Canvas size in physical pixels.
    pub canvas_size: [f32; 2],
    /// Elapsed time (seconds).
    pub time: f32,
    /// Explicit padding so `text_color` lands at WGSL offset 32.
    pub _pad1: [f32; 3],
    /// Straight-alpha text color.
    pub text_color: [f32; 4],
}

impl TextUniformsData {
    /// Struct size per the WGSL uniform-stride rules.
    pub const WGSL_SIZE: u64 = 48;
}

/// Byte-exact mirror of the WGSL `RectUniforms` struct. Offset contract:
/// `rect` @0, `color` @16, `canvas_size` @32, `line_width` @40, size 48.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RectUniformsData {
    /// Rect `(x, y, w, h)` in canvas pixel space (Y-down).
    pub rect: [f32; 4],
    /// Straight-alpha fill/border color.
    pub color: [f32; 4],
    /// Canvas size in physical pixels.
    pub canvas_size: [f32; 2],
    /// Border width in pixels; `0.0` renders a filled rect.
    pub line_width: f32,
    /// Explicit padding to 48 bytes.
    pub _pad1: f32,
}

impl RectUniformsData {
    /// Struct size per the WGSL uniform-stride rules.
    pub const WGSL_SIZE: u64 = 48;
}

/// One planned draw call, aligned 1:1 with the draw calls encountered when
/// iterating `graph.pass_order` → `pass.draw_calls`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedDraw {
    /// Handled by the first pass's load op; nothing to encode.
    Clear,
    /// Bind the rect pipeline with rect uniform slot `usize`.
    Rect(usize),
    /// Bind the text pipeline with text uniform slot `usize`; the bool
    /// selects the tessellated range: `false` = rotated title run,
    /// `true` = unrotated input-field run. The canonical graph emits exactly
    /// two text calls (title first); additional calls reuse the input range.
    Text(usize, bool),
}

/// The complete per-frame plan produced from a render graph.
#[derive(Debug, Clone, PartialEq)]
pub struct FramePlan {
    /// Clear color extracted from the first `Clear` draw call (RGBA 0–1).
    pub clear_color: [f32; 4],
    /// Planned draws, in encode order (same order as graph iteration).
    pub draws: Vec<PlannedDraw>,
    /// Per-text-call uniforms, indexed by the `Text(n, _)` slots.
    pub text_uniforms: Vec<TextUniformsData>,
    /// Per-rect-call uniforms, indexed by the `Rect(n)` slots.
    pub rect_uniforms: Vec<RectUniformsData>,
}

impl FramePlan {
    /// Number of rect-uniform slots this plan requires.
    pub fn rect_slot_count(&self) -> usize {
        self.rect_uniforms.len()
    }

    /// Number of text-uniform slots this plan requires.
    pub fn text_slot_count(&self) -> usize {
        self.text_uniforms.len()
    }
}

/// Walk the render graph in execution order and produce per-draw-call
/// uniform data plus the encode plan.
///
/// Rotation semantics: the graph's `DrawText.rotation` field carries the
/// *rotation speed* from the scene; the actual angle is `speed × time`,
/// computed here so the shader receives a finished angle.
pub fn collect_frame_plan(
    graph: &alkalive_render::graph::RenderGraph,
    width: f32,
    height: f32,
    time: f32,
) -> FramePlan {
    use alkalive_render::graph::DrawCallKind;

    let mut clear_color = [0.0, 0.0, 0.0, 1.0];
    let mut draws = Vec::new();
    let mut text_uniforms = Vec::new();
    let mut rect_uniforms = Vec::new();

    for &pass_idx in &graph.pass_order {
        let pass = &graph.passes[pass_idx];
        for dc in &pass.draw_calls {
            match &dc.kind {
                DrawCallKind::Clear { color } => {
                    if !draws.contains(&PlannedDraw::Clear) {
                        clear_color = [color.0, color.1, color.2, color.3];
                    }
                    draws.push(PlannedDraw::Clear);
                }
                DrawCallKind::DrawRect { x, y, w, h, color } => {
                    draws.push(PlannedDraw::Rect(rect_uniforms.len()));
                    rect_uniforms.push(RectUniformsData {
                        rect: [*x, *y, *w, *h],
                        color: [color.0, color.1, color.2, color.3],
                        canvas_size: [width, height],
                        line_width: 0.0,
                        _pad1: 0.0,
                    });
                }
                DrawCallKind::DrawRectOutline {
                    x,
                    y,
                    w,
                    h,
                    color,
                    line_width,
                } => {
                    draws.push(PlannedDraw::Rect(rect_uniforms.len()));
                    rect_uniforms.push(RectUniformsData {
                        rect: [*x, *y, *w, *h],
                        color: [color.0, color.1, color.2, color.3],
                        canvas_size: [width, height],
                        line_width: *line_width,
                        _pad1: 0.0,
                    });
                }
                DrawCallKind::DrawText {
                    color, rotation, ..
                } => {
                    let is_input = !text_uniforms.is_empty();
                    draws.push(PlannedDraw::Text(text_uniforms.len(), is_input));
                    text_uniforms.push(TextUniformsData {
                        rotation: rotation * time,
                        _pad0: 0.0,
                        canvas_size: [width, height],
                        time,
                        _pad1: [0.0; 3],
                        text_color: [color.0, color.1, color.2, color.3],
                    });
                }
            }
        }
    }

    FramePlan {
        clear_color,
        draws,
        text_uniforms,
        rect_uniforms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alkalive_render::graph::build_render_graph;
    use alkalive_scene_data::TextSceneData;

    fn hello_graph() -> alkalive_render::graph::RenderGraph {
        build_render_graph(
            &TextSceneData::default(),
            (800, 600),
            (200.0, 320.0, 400.0, 40.0),
        )
    }

    #[test]
    fn plan_has_five_draws_in_execution_order() {
        let plan = collect_frame_plan(&hello_graph(), 800.0, 600.0, 1.5);
        assert_eq!(plan.draws.len(), 5);
        assert_eq!(plan.draws[0], PlannedDraw::Clear);
        assert_eq!(plan.draws[1], PlannedDraw::Rect(0));
        assert_eq!(plan.draws[2], PlannedDraw::Rect(1));
        assert_eq!(plan.draws[3], PlannedDraw::Text(0, false));
        assert_eq!(plan.draws[4], PlannedDraw::Text(1, true));
        assert_eq!(plan.text_uniforms.len(), 2);
        assert_eq!(plan.rect_uniforms.len(), 2);
        assert_eq!(plan.rect_slot_count(), 2);
        assert_eq!(plan.text_slot_count(), 2);
    }

    #[test]
    fn plan_extracts_clear_color_from_graph() {
        let scene = TextSceneData {
            background: (10, 20, 30),
            ..Default::default()
        };
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        let plan = collect_frame_plan(&graph, 800.0, 600.0, 0.0);
        let expected = [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0];
        for (a, b) in plan.clear_color.iter().zip(expected) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn plan_applies_rotation_speed_times_time() {
        let scene = TextSceneData {
            rotation_speed: 0.5,
            ..Default::default()
        };
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        let plan = collect_frame_plan(&graph, 800.0, 600.0, 4.0);
        // Title is the first text draw.
        assert!((plan.text_uniforms[0].rotation - 2.0).abs() < 1e-6);
        // Input text never rotates.
        assert_eq!(plan.text_uniforms[1].rotation, 0.0);
    }

    #[test]
    fn plan_carries_text_color_from_draw_call() {
        let scene = TextSceneData {
            input_text: "abc".to_string(),
            ..Default::default()
        };
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        let plan = collect_frame_plan(&graph, 800.0, 600.0, 0.0);
        // Typed input text uses the bright color from the graph builder.
        assert_eq!(plan.text_uniforms[1].text_color, [0.9, 0.9, 0.95, 1.0]);
    }

    #[test]
    fn plan_carries_rect_geometry_and_outline_flag() {
        let plan = collect_frame_plan(&hello_graph(), 800.0, 600.0, 0.0);
        let bg = &plan.rect_uniforms[0];
        assert_eq!(bg.rect, [200.0, 320.0, 400.0, 40.0]);
        assert_eq!(bg.line_width, 0.0);
        let border = &plan.rect_uniforms[1];
        assert_eq!(border.line_width, 2.0);
        assert_eq!(border.canvas_size, [800.0, 600.0]);
    }

    #[test]
    fn plan_is_deterministic_across_calls() {
        let g = hello_graph();
        assert_eq!(
            collect_frame_plan(&g, 800.0, 600.0, 1.0),
            collect_frame_plan(&g, 800.0, 600.0, 1.0)
        );
    }

    #[test]
    fn empty_graph_yields_empty_plan_with_default_clear() {
        let graph = alkalive_render::graph::RenderGraph::default();
        let plan = collect_frame_plan(&graph, 800.0, 600.0, 0.0);
        assert!(plan.draws.is_empty());
        assert!(plan.text_uniforms.is_empty());
        assert!(plan.rect_uniforms.is_empty());
        assert_eq!(plan.clear_color, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn uniform_layout_parity_text() {
        // Offsets must match the WGSL contract documented in wgsl_shaders:
        // rotation @0, canvas_size @8, time @16, text_color @32, size 48.
        let u = TextUniformsData {
            rotation: 1.0,
            _pad0: 0.0,
            canvas_size: [2.0, 3.0],
            time: 4.0,
            _pad1: [0.0; 3],
            text_color: [5.0, 6.0, 7.0, 8.0],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&u);
        assert_eq!(bytes.len() as u64, TextUniformsData::WGSL_SIZE);
        assert_eq!(bytes.len(), 48);
        assert_eq!(std::mem::offset_of!(TextUniformsData, rotation), 0);
        assert_eq!(std::mem::offset_of!(TextUniformsData, canvas_size), 8);
        assert_eq!(std::mem::offset_of!(TextUniformsData, time), 16);
        assert_eq!(std::mem::offset_of!(TextUniformsData, text_color), 32);
    }

    #[test]
    fn uniform_layout_parity_rect() {
        // Offsets must match the WGSL contract documented in wgsl_shaders:
        // rect @0, color @16, canvas_size @32, line_width @40, size 48.
        let u = RectUniformsData {
            rect: [1.0, 2.0, 3.0, 4.0],
            color: [5.0, 6.0, 7.0, 8.0],
            canvas_size: [9.0, 10.0],
            line_width: 11.0,
            _pad1: 0.0,
        };
        let bytes: &[u8] = bytemuck::bytes_of(&u);
        assert_eq!(bytes.len() as u64, RectUniformsData::WGSL_SIZE);
        assert_eq!(bytes.len(), 48);
        assert_eq!(std::mem::offset_of!(RectUniformsData, rect), 0);
        assert_eq!(std::mem::offset_of!(RectUniformsData, color), 16);
        assert_eq!(std::mem::offset_of!(RectUniformsData, canvas_size), 32);
        assert_eq!(std::mem::offset_of!(RectUniformsData, line_width), 40);
    }
}
