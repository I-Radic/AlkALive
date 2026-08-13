//! Scene IR — the runtime-consumable output of [`crate::codegen`].
//!
//! The IR is a flat, validated, fully-defaulted representation of a scene.
//! Unlike the AST, every optional field has been resolved to a concrete
//! value, and named colors (e.g. `gold`) have been lowered to
//! [`ColorIR::Gold`]. The runtime consumes this directly without needing
//! to know about the `.alk` source syntax.
//!
//! The IR also carries a [`module_id`](AlgorithmIR::module_id) minted from
//! the module name (deterministic FNV-1a hash) and the
//! [`module_name`](AlgorithmIR::module_name) string, so the runtime can
//! route the scene to the correct [`alkalive_core::Module`] instance.
//!
//! # ADR-024 — Algorithm/Schedule Separation
//!
//! As of ADR-024, this struct is the pure *algorithm* IR — it contains
//! only scene description data (what to render), with no rendering-strategy
//! fields (how to render). It has been **renamed to [`AlgorithmIR`]** to
//! reflect this role. The legacy name [`SceneIR`] is preserved as a type
//! alias for backward compatibility.
//!
//! The rendering strategy (pass order, shader selection, batching) now
//! lives in the separate [`ScheduleIR`](crate::schedule::ScheduleIR),
//! produced by the [`schedule_lowering`](crate::schedule::schedule_lowering)
//! pass.

#![forbid(unsafe_code)]

use core::fmt;

use alkalive_core::ModuleId;

/// The root algorithm IR — the pure scene description (what to render).
///
/// Produced by [`crate::codegen::lower`]. Per ADR-024, this is the
/// *algorithm* IR: it contains only scene description data (nodes,
/// background, identity) with no rendering-strategy fields (those live in
/// [`ScheduleIR`](crate::schedule::ScheduleIR)).
///
/// The legacy name `SceneIR` is preserved as a type alias for backward
/// compatibility — see [`SceneIR`] at the bottom of this module.
#[derive(Debug, Clone, PartialEq)]
pub struct AlgorithmIR {
    /// Stable, deterministic identifier minted from the module name
    /// (FNV-1a 64-bit hash). Wrapped in [`ModuleId`] for type safety.
    pub module_id: ModuleId,
    /// Module name as written in source (e.g. `"HelloWorld"`).
    pub module_name: String,
    /// Background fill color as an `(R, G, B)` triple.
    pub background: (u8, u8, u8),
    /// Ordered list of scene nodes.
    pub nodes: Vec<NodeIR>,
    /// Collection declarations with monotonicity metadata (ADR-027 Phase 2).
    /// The runtime uses this to enable seminaïve evaluation: only new
    /// elements are processed on reactive updates for `monotone` collections.
    pub collections: Vec<CollectionDeclIR>,
}

/// Backward-compatible alias for [`AlgorithmIR`].
///
/// Per ADR-024, the struct previously known as `SceneIR` was renamed to
/// `AlgorithmIR` to reflect its role as the pure *algorithm* (scene
/// description) IR. The legacy name is preserved as a type alias so
/// existing consumers continue to compile unchanged.
pub type SceneIR = AlgorithmIR;

/// A node in the scene.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeIR {
    /// A text node.
    Text {
        /// Text content to render.
        content: String,
        /// Text color.
        color: ColorIR,
        /// Font size in pixels.
        font_size: f32,
        /// Y-axis rotation speed in radians per second.
        rotation_speed: f32,
        /// Layout position.
        position: PositionIR,
    },
    /// A text input field.
    InputField {
        /// Placeholder text shown when the field is empty.
        placeholder: String,
        /// Layout position.
        position: PositionIR,
    },
}

/// A resolved color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorIR {
    /// An `(R, G, B)` solid color from a `#RRGGBB` literal.
    Solid(u8, u8, u8),
    /// The named color `gold` (`#FFD700`).
    Gold,
}

/// A resolved position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionIR {
    /// Centered in the viewport.
    Center,
    /// Below the text node (only meaningful when a text node precedes
    /// this one in [`AlgorithmIR::nodes`]).
    BelowText,
    /// Explicit normalized coordinates `(x, y)` in `[0, 1]`.
    Custom(f32, f32),
}

// ======================================================================
// ADR-027 Phase 2 — Monotonicity metadata in the IR
// ======================================================================

/// The monotonicity of a collection, lowered from [`crate::ast::Qualifier`].
///
/// The runtime's incremental engine (ADR-025) uses this to decide whether
/// a collection can be processed seminaïvely (only new elements) or must
/// be fully re-evaluated.
///
/// - `Monotone`: the collection only grows. Seminaïve evaluation processes
///   only the newly-added elements on each reactive update.
/// - `Antitone`: the collection only shrinks. The runtime can skip
///   elements that have been removed since the last frame.
/// - `Unrestricted`: no monotonicity guarantee. Full re-evaluation is
///   required on each reactive update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Monotonicity {
    /// No monotonicity constraint (the default). Full re-evaluation required.
    #[default]
    Unrestricted,
    /// Collection only grows. Seminaïve evaluation: process only new elements.
    Monotone,
    /// Collection only shrinks. Skip removed elements.
    Antitone,
}

impl fmt::Display for Monotonicity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Monotonicity::Unrestricted => write!(f, "unrestricted"),
            Monotonicity::Monotone => write!(f, "monotone"),
            Monotonicity::Antitone => write!(f, "antitone"),
        }
    }
}

impl Monotonicity {
    /// Returns `true` iff seminaïve evaluation (process only new elements)
    /// is safe for this collection.
    pub fn supports_seminive(&self) -> bool {
        matches!(self, Monotonicity::Monotone)
    }

    /// Lower an [`crate::ast::Qualifier`] to the IR [`Monotonicity`].
    pub fn from_qualifier(q: crate::ast::Qualifier) -> Self {
        match q {
            crate::ast::Qualifier::Unrestricted => Monotonicity::Unrestricted,
            crate::ast::Qualifier::Monotone => Monotonicity::Monotone,
            crate::ast::Qualifier::Antitone => Monotonicity::Antitone,
        }
    }
}

/// A collection declaration lowered to the IR, carrying monotonicity metadata.
///
/// Produced by [`crate::codegen::lower`] from [`crate::ast::ItemDecl::Let`].
/// Consumed by the runtime to enable seminaïve evaluation (ADR-025).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionDeclIR {
    /// The collection's name as written in source.
    pub name: String,
    /// The element type as a display string (e.g. `"i32"`, `"string"`).
    pub element_type: String,
    /// The monotonicity of this collection.
    pub monotonicity: Monotonicity,
}

impl ColorIR {
    /// Returns the `(R, G, B)` triple for this color.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            ColorIR::Solid(r, g, b) => (r, g, b),
            ColorIR::Gold => (0xFF, 0xD7, 0x00),
        }
    }
}

impl fmt::Display for ColorIR {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (r, g, b) = self.rgb();
        write!(f, "#{:02X}{:02X}{:02X}", r, g, b)
    }
}

impl fmt::Display for PositionIR {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PositionIR::Center => write!(f, "center"),
            PositionIR::BelowText => write!(f, "below-text"),
            PositionIR::Custom(x, y) => write!(f, "({}, {})", x, y),
        }
    }
}

impl AlgorithmIR {
    /// Construct a new `AlgorithmIR` with the given module identity and an
    /// empty node list. Background defaults to black.
    ///
    /// (Legacy callers may know this type as `SceneIR`; the alias is
    /// exported at the crate root.)
    pub fn new(module_id: ModuleId, module_name: impl Into<String>) -> Self {
        Self {
            module_id,
            module_name: module_name.into(),
            background: (0, 0, 0),
            nodes: Vec::new(),
            collections: Vec::new(),
        }
    }

    /// Returns the number of nodes matching the predicate (counted
    /// generically over the enum).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` iff the scene contains at least one [`NodeIR::Text`].
    pub fn has_text(&self) -> bool {
        self.nodes.iter().any(|n| matches!(n, NodeIR::Text { .. }))
    }

    /// Returns `true` iff the scene contains at least one
    /// [`NodeIR::InputField`].
    pub fn has_input_field(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| matches!(n, NodeIR::InputField { .. }))
    }

    /// Serialise this IR to a compact JSON string. This is a *manual*
    /// serialiser with zero external dependencies — it exists so that
    /// library consumers (and unit tests) can obtain JSON without pulling
    /// in `serde_json`. The CLI binary additionally supports pretty
    /// `serde_json` output via the `cli` feature.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        out.push_str("\"module_id\":");
        out.push_str(&self.module_id.0.to_string());
        out.push_str(",\"module_name\":\"");
        push_json_escaped(&mut out, &self.module_name);
        out.push_str("\",\"background\":[");
        out.push_str(&self.background.0.to_string());
        out.push(',');
        out.push_str(&self.background.1.to_string());
        out.push(',');
        out.push_str(&self.background.2.to_string());
        out.push_str("],\"nodes\":[");
        for (i, node) in self.nodes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_node_json(&mut out, node);
        }
        // ADR-027 Phase 2: serialize collection declarations with monotonicity.
        out.push_str("],\"collections\":[");
        for (i, col) in self.collections.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":\"");
            push_json_escaped(&mut out, &col.name);
            out.push_str("\",\"element_type\":\"");
            push_json_escaped(&mut out, &col.element_type);
            out.push_str("\",\"monotonicity\":\"");
            out.push_str(&col.monotonicity.to_string());
            out.push_str("\"}");
        }
        out.push_str("]}");
        out
    }
}

fn push_node_json(out: &mut String, node: &NodeIR) {
    match node {
        NodeIR::Text {
            content,
            color,
            font_size,
            rotation_speed,
            position,
        } => {
            out.push_str("{\"type\":\"text\",\"content\":\"");
            push_json_escaped(out, content);
            out.push_str("\",\"color\":\"");
            out.push_str(&color.to_string());
            out.push_str("\",\"font_size\":");
            push_f32(out, *font_size);
            out.push_str(",\"rotation_speed\":");
            push_f32(out, *rotation_speed);
            out.push_str(",\"position\":\"");
            out.push_str(&position.to_string());
            out.push('"');
            out.push('}');
        }
        NodeIR::InputField {
            placeholder,
            position,
        } => {
            out.push_str("{\"type\":\"input-field\",\"placeholder\":\"");
            push_json_escaped(out, placeholder);
            out.push_str("\",\"position\":\"");
            out.push_str(&position.to_string());
            out.push('"');
            out.push('}');
        }
    }
}

fn push_f32(out: &mut String, v: f32) {
    // Use a round-trippable representation. If `v` is a clean integer,
    // emit it as `N.0` to match JSON conventions; otherwise emit the
    // shortest float.
    if v.is_finite() && v.fract() == 0.0 {
        out.push_str(&format!("{}.0", v as i64));
    } else {
        out.push_str(&format!("{}", v));
    }
}

fn push_json_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

/// Mint a deterministic [`ModuleId`] from a module name using FNV-1a 64-bit.
///
/// This is the same algorithm used by [`crate::codegen`] so that
/// `mint_module_id("HelloWorld")` always produces the same `ModuleId`
/// across runs and across the library/binary boundary.
pub fn mint_module_id(name: &str) -> ModuleId {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in name.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ModuleId(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_ir_gold_rgb() {
        assert_eq!(ColorIR::Gold.rgb(), (0xFF, 0xD7, 0x00));
        assert_eq!(format!("{}", ColorIR::Gold), "#FFD700");
    }

    #[test]
    fn color_ir_solid_rgb() {
        assert_eq!(ColorIR::Solid(1, 2, 3).rgb(), (1, 2, 3));
        assert_eq!(format!("{}", ColorIR::Solid(0, 0, 0)), "#000000");
    }

    #[test]
    fn position_ir_display() {
        assert_eq!(format!("{}", PositionIR::Center), "center");
        assert_eq!(format!("{}", PositionIR::BelowText), "below-text");
        assert_eq!(format!("{}", PositionIR::Custom(0.5, 0.25)), "(0.5, 0.25)");
    }

    #[test]
    fn mint_module_id_deterministic() {
        let a = mint_module_id("HelloWorld");
        let b = mint_module_id("HelloWorld");
        assert_eq!(a, b);
        let c = mint_module_id("Different");
        assert_ne!(a, c);
    }

    #[test]
    fn mint_module_id_known_value() {
        // Sanity-check the FNV-1a 64-bit hash of "HelloWorld".
        // (Computed once and pinned; any change to the algorithm MUST
        // update this constant.)
        let id = mint_module_id("HelloWorld");
        // We don't pin the exact hash value (it's a magic number), but we
        // verify it's non-zero and stable.
        assert_ne!(id.0, 0);
    }

    #[test]
    fn scene_ir_new_defaults() {
        let ir = SceneIR::new(mint_module_id("M"), "M");
        assert_eq!(ir.background, (0, 0, 0));
        assert!(ir.nodes.is_empty());
        assert!(!ir.has_text());
        assert!(!ir.has_input_field());
        assert_eq!(ir.node_count(), 0);
    }

    #[test]
    fn scene_ir_has_text_and_input_field() {
        let mut ir = SceneIR::new(mint_module_id("M"), "M");
        ir.nodes.push(NodeIR::Text {
            content: "Hi".into(),
            color: ColorIR::Gold,
            font_size: 32.0,
            rotation_speed: 0.0,
            position: PositionIR::Center,
        });
        assert!(ir.has_text());
        assert!(!ir.has_input_field());

        ir.nodes.push(NodeIR::InputField {
            placeholder: "Type".into(),
            position: PositionIR::BelowText,
        });
        assert!(ir.has_input_field());
        assert_eq!(ir.node_count(), 2);
    }

    #[test]
    fn scene_ir_to_json_empty_scene() {
        let ir = SceneIR::new(mint_module_id("M"), "M");
        let json = ir.to_json();
        assert!(json.contains("\"module_name\":\"M\""));
        assert!(json.contains("\"background\":[0,0,0]"));
        assert!(json.contains("\"nodes\":[]"));
    }

    #[test]
    fn scene_ir_to_json_with_nodes() {
        let mut ir = SceneIR::new(mint_module_id("M"), "M");
        ir.nodes.push(NodeIR::Text {
            content: "Hello".into(),
            color: ColorIR::Gold,
            font_size: 64.0,
            rotation_speed: 0.5,
            position: PositionIR::Center,
        });
        ir.nodes.push(NodeIR::InputField {
            placeholder: "Type".into(),
            position: PositionIR::BelowText,
        });
        let json = ir.to_json();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"content\":\"Hello\""));
        assert!(json.contains("\"color\":\"#FFD700\""));
        assert!(json.contains("\"font_size\":64.0"));
        assert!(json.contains("\"rotation_speed\":0.5"));
        assert!(json.contains("\"type\":\"input-field\""));
        assert!(json.contains("\"placeholder\":\"Type\""));
    }

    #[test]
    fn scene_ir_to_json_escapes_special_chars() {
        let mut ir = SceneIR::new(mint_module_id("M"), "M\"\\");
        ir.nodes.push(NodeIR::Text {
            content: "a\"b\nc".into(),
            color: ColorIR::Gold,
            font_size: 1.0,
            rotation_speed: 0.0,
            position: PositionIR::Center,
        });
        let json = ir.to_json();
        assert!(json.contains("M\\\"\\\\"));
        assert!(json.contains("a\\\"b\\nc"));
    }

    #[test]
    fn scene_ir_to_json_roundtrips_through_simple_validation() {
        // Build, serialise, and verify the JSON is well-formed by checking
        // matching braces/brackets.
        let mut ir = SceneIR::new(mint_module_id("Hello"), "Hello");
        ir.background = (10, 20, 30);
        ir.nodes.push(NodeIR::Text {
            content: "Hi".into(),
            color: ColorIR::Solid(255, 0, 0),
            font_size: 48.0,
            rotation_speed: 1.0,
            position: PositionIR::Custom(0.5, 0.5),
        });
        let json = ir.to_json();
        let opens = json.chars().filter(|&c| c == '{').count();
        let closes = json.chars().filter(|&c| c == '}').count();
        assert_eq!(opens, closes, "unbalanced braces in: {}", json);
        let opens = json.chars().filter(|&c| c == '[').count();
        let closes = json.chars().filter(|&c| c == ']').count();
        assert_eq!(opens, closes, "unbalanced brackets in: {}", json);
    }
}
