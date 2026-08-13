//! Schedule IR — the rendering strategy for an [`AlgorithmIR`].
//!
//! Per ADR-024, the rendering strategy (pass order, shader selection,
//! batching) is *separated* from the scene description (algorithm). This
//! module defines [`ScheduleIR`] — a data-driven description of how to
//! render an [`AlgorithmIR`] — plus the [`schedule_lowering`] compiler pass
//! that builds a default schedule from an algorithm.
//!
//! # Data flow
//!
//! ```text
//! .alk source ──► [codegen] ──► AlgorithmIR
//!                                  │
//!                                  ▼
//!                              [schedule_lowering]
//!                                  │
//!                                  ▼
//!                              ScheduleIR
//!                                  │
//!                                  ▼
//!                              ScheduledScene { algorithm, schedule }
//! ```
//!
//! The runtime then reads [`ScheduleIR`] at frame time to determine pass
//! order, shader selection, and batching — replacing previously hardcoded
//! dispatch with data-driven dispatch.
//!
//! # Default schedule rules
//!
//! The [`schedule_lowering`] pass applies the following default rules:
//!
//! | Pass # | Kind                   | Shader       | Batching    | Rotation |
//! |--------|------------------------|--------------|-------------|----------|
//! | 0      | [`Clear`]              | SolidColor   | None        | false    |
//! | 1      | [`InputFieldBackground`]| SolidColor  | None        | false    |
//! | 2      | [`InputFieldBorder`]   | SolidColor   | None        | false    |
//! | 3      | [`TitleText`]          | TextQuad     | ByFontSize  | true     |
//! | 4      | [`InputText`]          | TextQuad     | None        | false    |
//!
//! Passes whose required nodes are absent are *omitted* from the schedule
//! (e.g., a scene with no input field produces only the Clear and TitleText
//! passes). Pass 0 (Clear) is always present.
//!
//! [`Clear`]: PassKind::Clear
//! [`InputFieldBackground`]: PassKind::InputFieldBackground
//! [`InputFieldBorder`]: PassKind::InputFieldBorder
//! [`TitleText`]: PassKind::TitleText
//! [`InputText`]: PassKind::InputText

#![forbid(unsafe_code)]

use crate::ir::{AlgorithmIR, NodeIR};

/// Rendering strategy for a single pass.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPass {
    /// Which algorithm nodes this pass renders (indices into
    /// [`AlgorithmIR::nodes`]).
    pub node_indices: Vec<usize>,
    /// Which shader program to use.
    pub shader: ShaderId,
    /// Batching strategy.
    pub batching: BatchingStrategy,
    /// Whether rotation applies to this pass.
    pub rotation: bool,
    /// Pass kind (text, solid color, etc.).
    pub kind: PassKind,
}

/// Shader identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderId {
    /// Text quad shader (vertex + fragment for glyph atlas).
    TextQuad,
    /// Solid color shader (for input field background/border).
    SolidColor,
}

/// Batching strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchingStrategy {
    /// No batching — one draw call.
    None,
    /// Batch by font size.
    ByFontSize,
}

/// Pass kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    /// Clear/background pass.
    Clear,
    /// Input field background (solid fill).
    InputFieldBackground,
    /// Input field border (solid outline).
    InputFieldBorder,
    /// Title text with rotation.
    TitleText,
    /// Input field text without rotation.
    InputText,
}

/// The rendering schedule — how to render the scene.
#[derive(Debug, Clone)]
pub struct ScheduleIR {
    /// Render passes in execution order.
    pub passes: Vec<RenderPass>,
    /// Pass execution order (indices into [`passes`](Self::passes)).
    ///
    /// By default this is `0..passes.len()`, but advanced callers can
    /// reorder passes (e.g. for GPU state-change minimisation) by
    /// mutating this `Vec`.
    pub pass_order: Vec<usize>,
}

/// The combined output of the compiler: algorithm + schedule.
///
/// This is the runtime's view of a compiled scene — the *what* (algorithm)
/// and the *how* (schedule) packaged together. Per ADR-024, the algorithm
/// and schedule are kept as distinct fields so that incremental
/// computation (ADR-025) can track dirty state at the pass level without
/// touching the scene description.
#[derive(Debug, Clone)]
pub struct ScheduledScene {
    /// The scene description (what to render).
    pub algorithm: AlgorithmIR,
    /// The rendering strategy (how to render).
    pub schedule: ScheduleIR,
}

/// Build a default schedule for the given algorithm.
///
/// Applies the default schedule rules described at the top of this module:
///
/// - Pass 0: Clear (background) — always present.
/// - Pass 1: Input field background (solid color) — only if the scene has
///   at least one [`NodeIR::InputField`].
/// - Pass 2: Input field border (solid color) — only if the scene has at
///   least one [`NodeIR::InputField`].
/// - Pass 3: Title text (text_quad shader, with rotation) — only if the
///   scene has at least one [`NodeIR::Text`].
/// - Pass 4: Input text (text_quad shader, no rotation) — only if the
///   scene has at least one [`NodeIR::InputField`].
///
/// `pass_order` is set to `0..passes.len()` (passes execute in their
/// declared order). Advanced schedulers can re-order this `Vec` after
/// construction.
pub fn schedule_lowering(algorithm: &AlgorithmIR) -> ScheduleIR {
    // Find text and input field node indices.
    let mut text_indices: Vec<usize> = Vec::new();
    let mut input_indices: Vec<usize> = Vec::new();
    for (i, node) in algorithm.nodes.iter().enumerate() {
        match node {
            NodeIR::Text { .. } => text_indices.push(i),
            NodeIR::InputField { .. } => input_indices.push(i),
        }
    }

    let mut passes: Vec<RenderPass> = Vec::new();

    // Pass 0: Clear.
    passes.push(RenderPass {
        node_indices: Vec::new(),
        shader: ShaderId::SolidColor,
        batching: BatchingStrategy::None,
        rotation: false,
        kind: PassKind::Clear,
    });

    // Pass 1: Input field background.
    if !input_indices.is_empty() {
        passes.push(RenderPass {
            node_indices: input_indices.clone(),
            shader: ShaderId::SolidColor,
            batching: BatchingStrategy::None,
            rotation: false,
            kind: PassKind::InputFieldBackground,
        });
    }

    // Pass 2: Input field border.
    if !input_indices.is_empty() {
        passes.push(RenderPass {
            node_indices: input_indices.clone(),
            shader: ShaderId::SolidColor,
            batching: BatchingStrategy::None,
            rotation: false,
            kind: PassKind::InputFieldBorder,
        });
    }

    // Pass 3: Title text.
    if !text_indices.is_empty() {
        passes.push(RenderPass {
            node_indices: text_indices.clone(),
            shader: ShaderId::TextQuad,
            batching: BatchingStrategy::ByFontSize,
            rotation: true,
            kind: PassKind::TitleText,
        });
    }

    // Pass 4: Input text (if input field exists, render its placeholder/text).
    if !input_indices.is_empty() {
        passes.push(RenderPass {
            node_indices: input_indices.clone(),
            shader: ShaderId::TextQuad,
            batching: BatchingStrategy::None,
            rotation: false,
            kind: PassKind::InputText,
        });
    }

    let pass_order: Vec<usize> = (0..passes.len()).collect();

    ScheduleIR { passes, pass_order }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{mint_module_id, ColorIR, PositionIR, SceneIR};

    /// Build an `AlgorithmIR` with the given nodes (helper for tests).
    fn algo_with_nodes(nodes: Vec<NodeIR>) -> AlgorithmIR {
        let mut ir = SceneIR::new(mint_module_id("Test"), "Test");
        ir.nodes = nodes;
        ir
    }

    fn sample_text_node() -> NodeIR {
        NodeIR::Text {
            content: "Hello".into(),
            color: ColorIR::Gold,
            font_size: 64.0,
            rotation_speed: 0.5,
            position: PositionIR::Center,
        }
    }

    fn sample_input_field() -> NodeIR {
        NodeIR::InputField {
            placeholder: "Type here...".into(),
            position: PositionIR::BelowText,
        }
    }

    #[test]
    fn empty_scene_produces_only_clear_pass() {
        let algo = algo_with_nodes(Vec::new());
        let sched = schedule_lowering(&algo);
        assert_eq!(sched.passes.len(), 1);
        assert_eq!(sched.passes[0].kind, PassKind::Clear);
        assert_eq!(sched.passes[0].shader, ShaderId::SolidColor);
        assert!(!sched.passes[0].rotation);
        assert!(sched.passes[0].node_indices.is_empty());
        // pass_order should be 0..1.
        assert_eq!(sched.pass_order, vec![0]);
    }

    #[test]
    fn text_only_scene_produces_clear_and_title_text() {
        let algo = algo_with_nodes(vec![sample_text_node()]);
        let sched = schedule_lowering(&algo);
        assert_eq!(sched.passes.len(), 2);
        assert_eq!(sched.passes[0].kind, PassKind::Clear);
        assert_eq!(sched.passes[1].kind, PassKind::TitleText);
        assert_eq!(sched.passes[1].shader, ShaderId::TextQuad);
        assert_eq!(sched.passes[1].batching, BatchingStrategy::ByFontSize);
        assert!(sched.passes[1].rotation);
        // Title-text pass references node 0 (the text node).
        assert_eq!(sched.passes[1].node_indices, vec![0]);
        assert_eq!(sched.pass_order, vec![0, 1]);
    }

    #[test]
    fn input_field_only_scene_produces_clear_bg_border_input_text() {
        // Note: input-field without a preceding text node is normally a
        // codegen error (`below text` requires a text node). For schedule
        // lowering we don't care — we just match on the node variant.
        let input_node = NodeIR::InputField {
            placeholder: "P".into(),
            position: PositionIR::Center,
        };
        let algo = algo_with_nodes(vec![input_node]);
        let sched = schedule_lowering(&algo);
        assert_eq!(sched.passes.len(), 4);
        assert_eq!(sched.passes[0].kind, PassKind::Clear);
        assert_eq!(sched.passes[1].kind, PassKind::InputFieldBackground);
        assert_eq!(sched.passes[2].kind, PassKind::InputFieldBorder);
        assert_eq!(sched.passes[3].kind, PassKind::InputText);
        // The background and border passes use the solid color shader.
        assert_eq!(sched.passes[1].shader, ShaderId::SolidColor);
        assert_eq!(sched.passes[2].shader, ShaderId::SolidColor);
        // The input-text pass uses the text quad shader.
        assert_eq!(sched.passes[3].shader, ShaderId::TextQuad);
        // No rotation on any of the input passes.
        for p in &sched.passes {
            assert!(!p.rotation, "pass {:?} should not have rotation", p.kind);
        }
        // pass_order is 0..4.
        assert_eq!(sched.pass_order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn full_hello_world_scene_produces_five_passes() {
        // text + input-field — the canonical Hello World scene.
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        assert_eq!(sched.passes.len(), 5);
        assert_eq!(sched.passes[0].kind, PassKind::Clear);
        assert_eq!(sched.passes[1].kind, PassKind::InputFieldBackground);
        assert_eq!(sched.passes[2].kind, PassKind::InputFieldBorder);
        assert_eq!(sched.passes[3].kind, PassKind::TitleText);
        assert_eq!(sched.passes[4].kind, PassKind::InputText);
        // Title text references node 0 (text).
        assert_eq!(sched.passes[3].node_indices, vec![0]);
        // Input passes reference node 1 (input-field).
        assert_eq!(sched.passes[1].node_indices, vec![1]);
        assert_eq!(sched.passes[2].node_indices, vec![1]);
        assert_eq!(sched.passes[4].node_indices, vec![1]);
        // pass_order is 0..5.
        assert_eq!(sched.pass_order, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn multiple_text_nodes_all_in_title_pass() {
        let algo = algo_with_nodes(vec![
            sample_text_node(),
            NodeIR::Text {
                content: "Second".into(),
                color: ColorIR::Solid(255, 0, 0),
                font_size: 32.0,
                rotation_speed: 0.0,
                position: PositionIR::Center,
            },
            sample_input_field(),
        ]);
        let sched = schedule_lowering(&algo);
        // Clear + InputFieldBackground + InputFieldBorder + TitleText + InputText
        assert_eq!(sched.passes.len(), 5);
        let title_pass = sched
            .passes
            .iter()
            .find(|p| p.kind == PassKind::TitleText)
            .expect("title text pass present");
        // Both text nodes (indices 0 and 1) should be in the title pass.
        assert_eq!(title_pass.node_indices, vec![0, 1]);
    }

    #[test]
    fn scheduled_scene_struct_holds_algorithm_and_schedule() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let schedule = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo.clone(),
            schedule: schedule.clone(),
        };
        assert_eq!(scheduled.algorithm, algo);
        assert_eq!(scheduled.schedule.passes.len(), schedule.passes.len());
    }

    #[test]
    fn pass_order_is_identity_by_default() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        // pass_order should be 0..N where N is the number of passes.
        assert_eq!(sched.pass_order.len(), sched.passes.len());
        for (i, &order) in sched.pass_order.iter().enumerate() {
            assert_eq!(order, i);
        }
    }

    #[test]
    fn shader_id_enum_equality() {
        assert_eq!(ShaderId::TextQuad, ShaderId::TextQuad);
        assert_ne!(ShaderId::TextQuad, ShaderId::SolidColor);
    }

    #[test]
    fn batching_strategy_enum_equality() {
        assert_eq!(BatchingStrategy::None, BatchingStrategy::None);
        assert_ne!(BatchingStrategy::None, BatchingStrategy::ByFontSize);
    }

    #[test]
    fn pass_kind_enum_equality() {
        assert_eq!(PassKind::Clear, PassKind::Clear);
        assert_ne!(PassKind::Clear, PassKind::TitleText);
    }

    #[test]
    fn render_pass_clone_and_debug() {
        let pass = RenderPass {
            node_indices: vec![0, 1],
            shader: ShaderId::TextQuad,
            batching: BatchingStrategy::ByFontSize,
            rotation: true,
            kind: PassKind::TitleText,
        };
        let cloned = pass.clone();
        assert_eq!(pass, cloned);
        // Debug formatting should include the kind.
        let s = format!("{:?}", pass);
        assert!(s.contains("TitleText"));
    }
}
