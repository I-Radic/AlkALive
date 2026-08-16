//! Render-Graph IR — the data structure that drives GPU rendering.
//!
//! This module implements the Wave 11 (Gap 6) render-graph IR: a real,
//! data-driven [`RenderGraph`] consumed by the GPU backend. The existing
//! `compile()` compiler at the crate root and its associated IR types
//! (`crate::RenderGraph`, `crate::RenderPass`, …) remain in place for the
//! Wave 5 compiler tests; this module adds the **practical** IR that the
//! `WgpuRenderer::render_graph` method consumes at frame time.
//!
//! # Design
//!
//! The graph is a flat list of [`RenderPass`]es plus a flat list of
//! [`Attachment`]s. Each pass owns its [`DrawCall`]s directly (no side
//! table) and references its input/output attachments by index into
//! `RenderGraph::attachments`. `pass_order` is an explicit list of indices
//! into `RenderGraph::passes` — the renderer iterates `pass_order` and
//! dispatches each pass's draw calls in order.
//!
//! The [`build_render_graph`] function lowers a [`TextSceneData`] +
//! canvas-size + input-field-bounds triple into the 5-pass Hello-World
//! graph (Clear → InputFieldBackground → InputFieldBorder → TitleText →
//! InputText), matching the previously hardcoded sequence in
//! `WgpuRenderer::render_frame_internal`.
//!
//! # Why a separate module?
//!
//! The crate root already defines a richer IR (`crate::RenderGraph`,
//! `crate::RenderPass`, `crate::Attachment`, `crate::AttachmentFormat`,
//! `crate::DrawCall`) used by the Wave 5 compiler (`crate::compile`) and
//! its tests. This module's IR is intentionally simpler ( Vec`-backed
//! fields, `pass_order: Vec<usize>`, draw calls *inside* passes) so the
//! GPU backend can consume it without the Box<[T]> ceremony. The two IRs
//! coexist; the long-term plan (per the rendering spec §1.2) is for them
//! to converge.

use alkalive_scene_data::TextSceneData;

// ===========================================================================
// Core IR types
// ===========================================================================

/// A complete render graph: passes, attachments, and an explicit execution
/// order over the passes.
///
/// Produced by [`build_render_graph`] (or constructed by hand in tests) and
/// consumed by `WgpuRenderer::render_graph`. The graph is immutable for the
/// lifetime of a frame — the renderer reads it but does not mutate it.
#[derive(Debug, Clone, Default)]
pub struct RenderGraph {
    /// All passes in the graph. Indexed by `usize`; `pass_order` selects
    /// the execution order.
    pub passes: Vec<RenderPass>,
    /// All attachments in the graph. Indexed by `usize`; each pass's
    /// `inputs` and `outputs` are indices into this list.
    pub attachments: Vec<Attachment>,
    /// Explicit execution order — indices into `passes`. The renderer
    /// iterates this list in order. Today this is `0..passes.len()`, but
    /// advanced schedulers (e.g. the Wave 5 topological-sort compiler)
    /// may reorder passes here without touching `passes` itself.
    pub pass_order: Vec<usize>,
}

/// One render-graph pass: a named collection of draw calls plus the
/// attachments it reads from (`inputs`) and writes to (`outputs`).
#[derive(Debug, Clone, Default)]
pub struct RenderPass {
    /// Human-readable pass name (e.g. `"clear"`, `"title-text"`).
    pub name: String,
    /// Draw calls recorded in this pass, in execution order.
    pub draw_calls: Vec<DrawCall>,
    /// Attachment indices this pass reads from (into
    /// [`RenderGraph::attachments`]).
    pub inputs: Vec<usize>,
    /// Attachment indices this pass writes to (into
    /// [`RenderGraph::attachments`]).
    pub outputs: Vec<usize>,
}

/// A render-target attachment.
#[derive(Debug, Clone, Default)]
pub struct Attachment {
    /// Human-readable attachment name (e.g. `"canvas"`).
    pub name: String,
    /// Pixel format.
    pub format: AttachmentFormat,
    /// Optional clear value. `None` means "load previous contents"; a
    /// four-tuple `(r, g, b, a)` means "clear to this color before the
    /// first pass writes".
    pub clear_value: Option<(f32, f32, f32, f32)>,
}

/// Pixel format of an [`Attachment`].
///
/// This is a small, practical enum (RGBA8 + R8) matching the formats the
/// current WebGL2 backend actually allocates. The richer
/// [`crate::AttachmentFormat`] enum (Bgra8Unorm, Rgba16Float, BCn, ASTC,
/// …) remains at the crate root for the Wave 5 compiler IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AttachmentFormat {
    /// 8-bit RGBA, unorm (the canvas swapchain texture).
    #[default]
    Rgba8,
    /// 8-bit single-channel (the glyph-atlas texture).
    R8,
}

/// One draw call inside a [`RenderPass`].
#[derive(Debug, Clone)]
pub struct DrawCall {
    /// Draw-call identifier. Stable within a frame; the renderer uses it
    /// for diagnostics and (future) dirty-tracking.
    pub id: usize,
    /// The high-level kind of draw call — clear, filled rect, outlined
    /// rect, or shaped text.
    pub kind: DrawCallKind,
}

/// High-level draw-call descriptor. Lowered by the renderer to the
/// concrete GPU operation (clear, draw-rect, draw-text-quad).
#[derive(Debug, Clone)]
pub enum DrawCallKind {
    /// Clear the entire attachment to a solid color.
    Clear {
        /// RGBA clear color, normalized 0.0–1.0.
        color: (f32, f32, f32, f32),
    },
    /// Draw a solid-color filled rectangle.
    DrawRect {
        /// Top-left X (pixels, Y-down).
        x: f32,
        /// Top-left Y (pixels, Y-down).
        y: f32,
        /// Width (pixels).
        w: f32,
        /// Height (pixels).
        h: f32,
        /// RGBA fill color, normalized 0.0–1.0.
        color: (f32, f32, f32, f32),
    },
    /// Draw a rectangle outline (4 edges) with the given line width.
    DrawRectOutline {
        /// Top-left X (pixels, Y-down).
        x: f32,
        /// Top-left Y (pixels, Y-down).
        y: f32,
        /// Width (pixels).
        w: f32,
        /// Height (pixels).
        h: f32,
        /// RGBA outline color, normalized 0.0–1.0.
        color: (f32, f32, f32, f32),
        /// Line width in pixels.
        line_width: f32,
    },
    /// Draw a shaped text run.
    ///
    /// `text_ptr` / `text_len` model a borrowed slice of UTF-8 text in
    /// *WASM linear memory*: the renderer does not dereference these on
    /// the GPU side — they are passed to the host text-shaping path
    /// (`alkalive-text`) which copies the bytes into its own buffer. The
    /// pointer is `i32` (not `usize`) so the IR is portable across 32-
    /// and 64-bit targets.
    DrawText {
        /// Pointer to the UTF-8 text bytes in linear memory (WASM ABI).
        ///
        /// On native test builds this is `0` — the renderer re-reads the
        /// text from its own cached scene state. The pointer is carried
        /// for future SAB/IPC transport (Gap 8) and for headless test
        /// parity.
        text_ptr: i32,
        /// Length of the UTF-8 text bytes in linear memory.
        text_len: i32,
        /// Font size in pixels.
        font_size: f32,
        /// RGBA text color, normalized 0.0–1.0.
        color: (f32, f32, f32, f32),
        /// Y-axis rotation angle (radians). `0.0` for non-rotated text.
        rotation: f32,
        /// Top-left anchor position `(x, y)` in pixel space (Y-down).
        ///
        /// Today this is unused by the WebGL2 backend (it positions text
        /// from the cached shaped-run metrics); it is plumbed through for
        /// the future layout-driven path (ADR-004).
        position: (f32, f32),
    },
}

// ===========================================================================
// Graph construction
// ===========================================================================

/// Build the 5-pass Hello-World render graph from the per-frame scene data.
///
/// The graph matches the previously hardcoded sequence in
/// `WgpuRenderer::render_frame_internal`:
///
/// | Pass # | Name               | Draw call kind                          |
/// |--------|--------------------|-----------------------------------------|
/// | 0      | `clear`            | `Clear { background }`                  |
/// | 1      | `input-field-bg`   | `DrawRect { input_field_bounds, … }`    |
/// | 2      | `input-field-border` | `DrawRectOutline { input_field_bounds, … }` |
/// | 3      | `title-text`       | `DrawText { text, rotation, … }`        |
/// | 4      | `input-text`       | `DrawText { input_text, no rotation, … }` |
///
/// # Inputs
///
/// - `scene` — the per-frame scene description (text, colors, rotation
///   speed).
/// - `canvas_size` — physical pixel dimensions of the canvas attachment.
/// - `input_field_bounds` — pixel-space `(x, y, w, h)` of the input field
///   rectangle (computed by the renderer's `upload_text_atlas` path and
///   passed through here so the graph is self-contained).
///
/// # Output
///
/// A [`RenderGraph`] with:
/// - 1 attachment (the canvas, format `Rgba8`, clear value = background).
/// - 5 passes, each with exactly one draw call.
/// - `pass_order = [0, 1, 2, 3, 4]` (linear chain today; the Wave 5
///   topological-sort compiler may reorder passes here in the future).
///
/// # Validation
///
/// The returned graph is structurally valid (see [`RenderGraph::validate`]):
/// all `inputs`/`outputs` indices are in range, all `pass_order` entries
/// are in range, and the pass-dependency graph (induced by attachment
/// reads/writes) is acyclic.
pub fn build_render_graph(
    scene: &TextSceneData,
    canvas_size: (u32, u32),
    input_field_bounds: (f32, f32, f32, f32),
) -> RenderGraph {
    // 1. One attachment: the canvas swapchain texture.
    let (br, bg, bb) = scene.background_normalized();
    let canvas_attachment = Attachment {
        name: "canvas".to_string(),
        format: AttachmentFormat::Rgba8,
        clear_value: Some((br, bg, bb, 1.0)),
    };

    // 2. Compute per-pass draw-call kinds.
    let clear_kind = DrawCallKind::Clear {
        color: (br, bg, bb, 1.0),
    };

    let (fx, fy, fw, fh) = input_field_bounds;
    let input_bg_kind = DrawCallKind::DrawRect {
        x: fx,
        y: fy,
        w: fw,
        h: fh,
        color: (0.05, 0.05, 0.08, 0.9),
    };
    let input_border_kind = DrawCallKind::DrawRectOutline {
        x: fx,
        y: fy,
        w: fw,
        h: fh,
        color: (0.8, 0.65, 0.0, 0.8),
        line_width: 2.0,
    };

    // Title text: drawn with rotation (rotation_speed * time, applied by
    // the renderer). The pointer/length are 0/0 today — the renderer
    // reads the text from its cached scene state. They are plumbed
    // through for the future SAB/IPC transport path (Gap 8).
    let title_kind = draw_text_kind(
        scene.text.as_str(),
        scene.font_size,
        scene.text_color,
        scene.rotation_speed, // multiplied by time in the renderer
        (0.0, 0.0),
    );
    let input_kind = draw_text_kind(
        if scene.input_text.is_empty() {
            scene.input_placeholder.as_str()
        } else {
            scene.input_text.as_str()
        },
        scene.font_size * 0.5,
        if scene.input_text.is_empty() {
            (0.35, 0.35, 0.4, 1.0)
        } else {
            (0.9, 0.9, 0.95, 1.0)
        },
        0.0, // no rotation
        (0.0, 0.0),
    );

    // 3. Build passes. Only the Clear pass declares the canvas as an output
    //    (it is the producer: it clears the canvas to the background color).
    //    Subsequent passes declare the canvas as an input only — they
    //    composite onto the existing canvas via alpha blending, which the
    //    GPU model treats as an in-place modification (no new attachment
    //    version, no RAW dependency edge between them).
    let passes = vec![
        RenderPass {
            name: "clear".to_string(),
            draw_calls: vec![DrawCall {
                id: 0,
                kind: clear_kind,
            }],
            inputs: vec![],
            outputs: vec![0],
        },
        RenderPass {
            name: "input-field-bg".to_string(),
            draw_calls: vec![DrawCall {
                id: 1,
                kind: input_bg_kind,
            }],
            inputs: vec![0],
            outputs: vec![],
        },
        RenderPass {
            name: "input-field-border".to_string(),
            draw_calls: vec![DrawCall {
                id: 2,
                kind: input_border_kind,
            }],
            inputs: vec![0],
            outputs: vec![],
        },
        RenderPass {
            name: "title-text".to_string(),
            draw_calls: vec![DrawCall {
                id: 3,
                kind: title_kind,
            }],
            inputs: vec![0],
            outputs: vec![],
        },
        RenderPass {
            name: "input-text".to_string(),
            draw_calls: vec![DrawCall {
                id: 4,
                kind: input_kind,
            }],
            inputs: vec![0],
            outputs: vec![],
        },
    ];

    let pass_order: Vec<usize> = (0..passes.len()).collect();

    let _ = canvas_size; // currently unused — the canvas attachment has no
                         // explicit extent field yet (the renderer knows
                         // its own canvas size); plumbed through for the
                         // future per-pass render-target path.

    RenderGraph {
        passes,
        attachments: vec![canvas_attachment],
        pass_order,
    }
}

/// Helper: construct a [`DrawCallKind::DrawText`] from a text slice,
/// copying the bytes into the IR's `text_ptr`/`text_len` slots.
//
// On native + WASM the renderer does not dereference `text_ptr` — it
// re-reads the text from its cached scene state. The pointer/length are
// plumbed for the future SAB/IPC transport path. For now they are 0/0
// (signalling "use the cached text"); the bytes are not leaked because
// nothing is allocated.
fn draw_text_kind(
    _text: &str,
    font_size: f32,
    color: (f32, f32, f32, f32),
    rotation: f32,
    position: (f32, f32),
) -> DrawCallKind {
    DrawCallKind::DrawText {
        text_ptr: 0,
        text_len: 0,
        font_size,
        color,
        rotation,
        position,
    }
}

// ===========================================================================
// Validation
// ===========================================================================

/// Error returned by [`RenderGraph::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphValidationError {
    /// An attachment index in a pass's `inputs` or `outputs` is out of
    /// range.
    AttachmentOutOfRange {
        /// Pass index in `passes`.
        pass_idx: usize,
        /// The offending attachment index.
        attachment_idx: usize,
    },
    /// A `pass_order` entry is out of range.
    PassOrderOutOfRange {
        /// The offending pass-order entry.
        pass_order_entry: usize,
    },
    /// A duplicate pass index in `pass_order`.
    DuplicatePassOrder {
        /// The duplicated pass index.
        pass_idx: usize,
    },
    /// A cycle was detected in the pass-dependency graph induced by
    /// attachment writes/reads.
    Cycle,
}

impl RenderGraph {
    /// Validate the graph's structural integrity.
    ///
    /// Checks:
    /// 1. Every `inputs[i]` and `outputs[i]` on every pass is `< attachments.len()`.
    /// 2. Every entry in `pass_order` is `< passes.len()`.
    /// 3. No duplicate entries in `pass_order`.
    /// 4. The pass-dependency graph (pass A → pass B if B reads an
    ///    attachment A writes) is acyclic.
    ///
    /// Returns `Ok(())` if all checks pass, `Err(GraphValidationError)`
    /// otherwise.
    pub fn validate(&self) -> Result<(), GraphValidationError> {
        // 1. Attachment-index range checks.
        for (pass_idx, pass) in self.passes.iter().enumerate() {
            for &att_idx in pass.inputs.iter().chain(pass.outputs.iter()) {
                if att_idx >= self.attachments.len() {
                    return Err(GraphValidationError::AttachmentOutOfRange {
                        pass_idx,
                        attachment_idx: att_idx,
                    });
                }
            }
        }

        // 2. pass_order range checks.
        for &entry in &self.pass_order {
            if entry >= self.passes.len() {
                return Err(GraphValidationError::PassOrderOutOfRange {
                    pass_order_entry: entry,
                });
            }
        }

        // 3. Duplicate pass_order entries.
        let mut seen = std::collections::HashSet::with_capacity(self.pass_order.len());
        for &entry in &self.pass_order {
            if !seen.insert(entry) {
                return Err(GraphValidationError::DuplicatePassOrder { pass_idx: entry });
            }
        }

        // 4. Cycle detection on the pass-dependency graph.
        //
        // Edge: pass A → pass B if there exists an attachment `att` such
        // that A writes `att` and B reads `att`. (We don't enforce an
        // ordering between two passes that both write the same
        // attachment — that's a higher-level scheduling concern.)
        //
        // Topological sort via Kahn's algorithm: if the sort visits all
        // passes, no cycle; otherwise a cycle exists.
        let n = self.passes.len();
        let mut writes: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, pass) in self.passes.iter().enumerate() {
            for &att_idx in &pass.outputs {
                writes.entry(att_idx).or_default().push(i);
            }
        }

        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut in_degree: Vec<usize> = vec![0; n];
        for (b, pass) in self.passes.iter().enumerate() {
            for &att_idx in &pass.inputs {
                if let Some(producers) = writes.get(&att_idx) {
                    for &a in producers {
                        if a != b {
                            adj[a].push(b);
                            in_degree[b] += 1;
                        }
                    }
                }
            }
        }

        let mut queue: std::collections::VecDeque<usize> =
            (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut visited = 0usize;
        while let Some(i) = queue.pop_front() {
            visited += 1;
            for &b in &adj[i] {
                in_degree[b] -= 1;
                if in_degree[b] == 0 {
                    queue.push_back(b);
                }
            }
        }
        if visited != n {
            return Err(GraphValidationError::Cycle);
        }

        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_scene() -> TextSceneData {
        TextSceneData::default()
    }

    // --- build_render_graph: structure -----------------------------------------

    #[test]
    fn build_render_graph_produces_five_passes() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (100.0, 200.0, 400.0, 40.0));
        assert_eq!(graph.passes.len(), 5, "expected 5 passes");
        assert_eq!(
            graph.pass_order,
            vec![0, 1, 2, 3, 4],
            "pass_order must be 0..5"
        );
    }

    #[test]
    fn build_render_graph_pass_names_match_canonical_sequence() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (100.0, 200.0, 400.0, 40.0));
        let names: Vec<&str> = graph.passes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "clear",
                "input-field-bg",
                "input-field-border",
                "title-text",
                "input-text"
            ],
            "pass names must match the canonical Hello-World sequence"
        );
    }

    #[test]
    fn build_render_graph_each_pass_has_exactly_one_draw_call() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (100.0, 200.0, 400.0, 40.0));
        for (i, pass) in graph.passes.iter().enumerate() {
            assert_eq!(
                pass.draw_calls.len(),
                1,
                "pass {} ({}) must have exactly one draw call, got {}",
                i,
                pass.name,
                pass.draw_calls.len()
            );
        }
    }

    #[test]
    fn build_render_graph_draw_call_kinds_match_pass_names() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (100.0, 200.0, 400.0, 40.0));

        assert!(
            matches!(
                graph.passes[0].draw_calls[0].kind,
                DrawCallKind::Clear { .. }
            ),
            "clear pass must emit a Clear draw call"
        );
        assert!(
            matches!(
                graph.passes[1].draw_calls[0].kind,
                DrawCallKind::DrawRect { .. }
            ),
            "input-field-bg pass must emit a DrawRect draw call"
        );
        assert!(
            matches!(
                graph.passes[2].draw_calls[0].kind,
                DrawCallKind::DrawRectOutline { .. }
            ),
            "input-field-border pass must emit a DrawRectOutline draw call"
        );
        assert!(
            matches!(
                graph.passes[3].draw_calls[0].kind,
                DrawCallKind::DrawText { .. }
            ),
            "title-text pass must emit a DrawText draw call"
        );
        assert!(
            matches!(
                graph.passes[4].draw_calls[0].kind,
                DrawCallKind::DrawText { .. }
            ),
            "input-text pass must emit a DrawText draw call"
        );
    }

    #[test]
    fn build_render_graph_clear_color_matches_scene_background() {
        let scene = TextSceneData {
            background: (10, 20, 30),
            ..Default::default()
        };
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        let att = &graph.attachments[0];
        let clear = att
            .clear_value
            .expect("canvas attachment must have a clear value");
        assert!((clear.0 - 10.0 / 255.0).abs() < 1e-6);
        assert!((clear.1 - 20.0 / 255.0).abs() < 1e-6);
        assert!((clear.2 - 30.0 / 255.0).abs() < 1e-6);
        assert!((clear.3 - 1.0).abs() < 1e-6);

        if let DrawCallKind::Clear { color } = &graph.passes[0].draw_calls[0].kind {
            assert!((color.0 - 10.0 / 255.0).abs() < 1e-6);
            assert!((color.1 - 20.0 / 255.0).abs() < 1e-6);
            assert!((color.2 - 30.0 / 255.0).abs() < 1e-6);
            assert!((color.3 - 1.0).abs() < 1e-6);
        } else {
            panic!("first draw call must be Clear");
        }
    }

    #[test]
    fn build_render_graph_input_field_bounds_propagate_to_draw_calls() {
        let scene = hello_scene();
        let bounds = (123.4, 456.7, 300.0, 50.0);
        let graph = build_render_graph(&scene, (800, 600), bounds);

        if let DrawCallKind::DrawRect { x, y, w, h, .. } = &graph.passes[1].draw_calls[0].kind {
            assert!((x - bounds.0).abs() < 1e-6);
            assert!((y - bounds.1).abs() < 1e-6);
            assert!((w - bounds.2).abs() < 1e-6);
            assert!((h - bounds.3).abs() < 1e-6);
        } else {
            panic!("input-field-bg draw call must be DrawRect");
        }

        if let DrawCallKind::DrawRectOutline {
            x,
            y,
            w,
            h,
            line_width,
            ..
        } = &graph.passes[2].draw_calls[0].kind
        {
            assert!((x - bounds.0).abs() < 1e-6);
            assert!((y - bounds.1).abs() < 1e-6);
            assert!((w - bounds.2).abs() < 1e-6);
            assert!((h - bounds.3).abs() < 1e-6);
            assert!((line_width - 2.0).abs() < 1e-6);
        } else {
            panic!("input-field-border draw call must be DrawRectOutline");
        }
    }

    #[test]
    fn build_render_graph_title_text_has_rotation_input_text_does_not() {
        let scene = TextSceneData {
            rotation_speed: 1.5,
            ..Default::default()
        };
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));

        if let DrawCallKind::DrawText { rotation, .. } = &graph.passes[3].draw_calls[0].kind {
            assert!(
                (rotation - 1.5).abs() < 1e-6,
                "title text rotation must equal scene.rotation_speed"
            );
        } else {
            panic!("title-text draw call must be DrawText");
        }

        if let DrawCallKind::DrawText { rotation, .. } = &graph.passes[4].draw_calls[0].kind {
            assert!(rotation.abs() < 1e-6, "input text rotation must be 0");
        } else {
            panic!("input-text draw call must be DrawText");
        }
    }

    #[test]
    fn build_render_graph_input_text_color_depends_on_placeholder_state() {
        // Empty input_text → placeholder color.
        let scene_empty = TextSceneData {
            input_text: String::new(),
            ..Default::default()
        };
        let graph_empty = build_render_graph(&scene_empty, (800, 600), (0.0, 0.0, 0.0, 0.0));
        if let DrawCallKind::DrawText { color, .. } = &graph_empty.passes[4].draw_calls[0].kind {
            assert!((color.0 - 0.35).abs() < 1e-6, "placeholder color R");
        } else {
            panic!("input-text draw call must be DrawText");
        }

        // Non-empty input_text → typed-text color.
        let scene_typed = TextSceneData {
            input_text: "hi".to_string(),
            ..Default::default()
        };
        let graph_typed = build_render_graph(&scene_typed, (800, 600), (0.0, 0.0, 0.0, 0.0));
        if let DrawCallKind::DrawText { color, .. } = &graph_typed.passes[4].draw_calls[0].kind {
            assert!((color.0 - 0.9).abs() < 1e-6, "typed-text color R");
        } else {
            panic!("input-text draw call must be DrawText");
        }
    }

    // --- validate: structural checks ------------------------------------------

    #[test]
    fn validate_accepts_built_graph() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        assert!(
            graph.validate().is_ok(),
            "graph produced by build_render_graph must validate"
        );
    }

    #[test]
    fn validate_rejects_attachment_out_of_range() {
        let mut graph = build_render_graph(&hello_scene(), (800, 600), (0.0, 0.0, 0.0, 0.0));
        // Inject an out-of-range attachment index on pass 0.
        graph.passes[0].inputs.push(99);
        assert!(
            matches!(
                graph.validate(),
                Err(GraphValidationError::AttachmentOutOfRange {
                    pass_idx: 0,
                    attachment_idx: 99
                })
            ),
            "out-of-range attachment index must be rejected"
        );
    }

    #[test]
    fn validate_rejects_pass_order_out_of_range() {
        let mut graph = build_render_graph(&hello_scene(), (800, 600), (0.0, 0.0, 0.0, 0.0));
        graph.pass_order.push(99);
        assert!(
            matches!(
                graph.validate(),
                Err(GraphValidationError::PassOrderOutOfRange {
                    pass_order_entry: 99
                })
            ),
            "out-of-range pass_order entry must be rejected"
        );
    }

    #[test]
    fn validate_rejects_duplicate_pass_order() {
        let mut graph = build_render_graph(&hello_scene(), (800, 600), (0.0, 0.0, 0.0, 0.0));
        graph.pass_order.push(0);
        assert!(
            matches!(
                graph.validate(),
                Err(GraphValidationError::DuplicatePassOrder { pass_idx: 0 })
            ),
            "duplicate pass_order entry must be rejected"
        );
    }

    #[test]
    fn validate_rejects_cycle() {
        // Construct a 2-pass graph where:
        //   pass 0 writes attachment 0, reads attachment 1
        //   pass 1 writes attachment 1, reads attachment 0
        // This forms a cycle 0 → 1 → 0.
        let graph = RenderGraph {
            passes: vec![
                RenderPass {
                    name: "a".to_string(),
                    draw_calls: vec![DrawCall {
                        id: 0,
                        kind: DrawCallKind::Clear {
                            color: (0.0, 0.0, 0.0, 1.0),
                        },
                    }],
                    inputs: vec![1],
                    outputs: vec![0],
                },
                RenderPass {
                    name: "b".to_string(),
                    draw_calls: vec![DrawCall {
                        id: 1,
                        kind: DrawCallKind::Clear {
                            color: (0.0, 0.0, 0.0, 1.0),
                        },
                    }],
                    inputs: vec![0],
                    outputs: vec![1],
                },
            ],
            attachments: vec![
                Attachment {
                    name: "att0".to_string(),
                    format: AttachmentFormat::Rgba8,
                    clear_value: None,
                },
                Attachment {
                    name: "att1".to_string(),
                    format: AttachmentFormat::Rgba8,
                    clear_value: None,
                },
            ],
            pass_order: vec![0, 1],
        };
        assert!(
            matches!(graph.validate(), Err(GraphValidationError::Cycle)),
            "cyclic dependency must be rejected"
        );
    }

    #[test]
    fn validate_accepts_acyclic_chain() {
        // pass 0 writes att 0; pass 1 reads att 0 and writes att 1; pass 2
        // reads att 1. Linear chain — no cycle.
        let graph = RenderGraph {
            passes: vec![
                RenderPass {
                    name: "a".to_string(),
                    draw_calls: vec![DrawCall {
                        id: 0,
                        kind: DrawCallKind::Clear {
                            color: (0.0, 0.0, 0.0, 1.0),
                        },
                    }],
                    inputs: vec![],
                    outputs: vec![0],
                },
                RenderPass {
                    name: "b".to_string(),
                    draw_calls: vec![DrawCall {
                        id: 1,
                        kind: DrawCallKind::Clear {
                            color: (0.0, 0.0, 0.0, 1.0),
                        },
                    }],
                    inputs: vec![0],
                    outputs: vec![1],
                },
                RenderPass {
                    name: "c".to_string(),
                    draw_calls: vec![DrawCall {
                        id: 2,
                        kind: DrawCallKind::Clear {
                            color: (0.0, 0.0, 0.0, 1.0),
                        },
                    }],
                    inputs: vec![1],
                    outputs: vec![],
                },
            ],
            attachments: vec![
                Attachment {
                    name: "att0".to_string(),
                    format: AttachmentFormat::Rgba8,
                    clear_value: None,
                },
                Attachment {
                    name: "att1".to_string(),
                    format: AttachmentFormat::Rgba8,
                    clear_value: None,
                },
            ],
            pass_order: vec![0, 1, 2],
        };
        assert!(graph.validate().is_ok(), "acyclic chain must validate");
    }

    // --- draw-call ID stability -----------------------------------------------

    #[test]
    fn draw_call_ids_are_zero_through_four() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        for (i, pass) in graph.passes.iter().enumerate() {
            assert_eq!(
                pass.draw_calls[0].id, i,
                "draw call id in pass {} must be {}",
                i, i
            );
        }
    }

    // --- attachment shape ------------------------------------------------------

    #[test]
    fn canvas_attachment_is_rgba8_with_clear_value() {
        let scene = hello_scene();
        let graph = build_render_graph(&scene, (800, 600), (0.0, 0.0, 0.0, 0.0));
        assert_eq!(graph.attachments.len(), 1, "exactly one attachment");
        let att = &graph.attachments[0];
        assert_eq!(att.name, "canvas");
        assert_eq!(att.format, AttachmentFormat::Rgba8);
        assert!(att.clear_value.is_some(), "canvas must have a clear value");
    }
}
