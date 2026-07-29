//! AlkALive alkalive-a11y crate.
//!
//! Accessibility Placeholder — see `docs/SPECIFICATION.md` §10 and ADR 019 / ADR 011.
//!
//! Accessibility is **deferred** for this phase by owner directive
//! (ADR 019), overriding the prior hybrid DOM-projection approach. No
//! DOM mirror, no DOM projection surface, and no assistive-technology
//! (AT) bridge ship in the initial release. This is a **deferral, not a
//! cancellation**: the extension surface below is committed so that
//! un-deferral is additive, not architectural.
//!
//! Wave 3 trait-definition skeleton: committed signatures, PLACEHOLDER
//! `todo!()` bodies. No implementation ships this phase.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// §10.5 Placeholder reference types
// ---------------------------------------------------------------------------

/// PLACEHOLDER render-object reference type.
///
/// Replaced by `alkalive-core::RenderObject` (W4-T1) when the a11y layer
/// is un-deferred. The render-object graph (ADR 007) already carries
/// `role`, `structured_data`, and `interaction` as **mandatory fields**
/// (ADR 011) — a future a11y tree is **derived** from this metadata,
/// never separately authored, never mirrored through a DOM sync boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderObject;

/// PLACEHOLDER shaped-glyph-run reference type.
///
/// Replaced by `alkalive-text::ShapedRun` / `ShapedGlyphRun` (ADR 022)
/// when the a11y layer is un-deferred. The text stack exposes a
/// placeholder `expose_a11y_text` interface so shaped glyph runs, BiDi
/// segments, selection, caret state, and labels can flow into the
/// future tree without re-engineering the shaper.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapedGlyphRun;

// ---------------------------------------------------------------------------
// §10.5 SemanticRole + FocusState
// ---------------------------------------------------------------------------

/// Extensible semantic role set aligned to future platform-a11y role sets.
///
/// Values MUST align to future platform-a11y role sets
/// (UIAutomation / NSAccessibility / AT-SPI / ARIA-equivalent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticRole {
    /// No role.
    None,
    /// Generic container.
    Generic,
    /// Button.
    Button,
    /// Hyperlink.
    Link,
    /// Heading.
    Heading,
    /// Text content.
    Text,
    /// Image.
    Image,
    /// List container.
    List,
    /// List item.
    ListItem,
    /// Editable text field.
    TextField,
    /// Checkbox.
    Checkbox,
    /// Slider.
    Slider,
    /// Dialog.
    Dialog,
    // Extensible; values MUST align to future platform-a11y role sets.
}

/// Focus-state mirror of the ADR 011 annotation layer (§10.2).
///
/// Read-only here: input dispatch is the sole writer (§8.5); the future
/// a11y reader consumes `FocusManager::current_focus` via
/// [`A11yExtensionPoint::read_focus_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusState {
    /// No focus.
    None,
    /// Focused.
    Focused,
    /// Focusable but not focused.
    Focusable,
}

// ---------------------------------------------------------------------------
// §10.5 Structured metadata
// ---------------------------------------------------------------------------

/// A localised text label (ADR 022 text stack → a11y).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextLabel {
    /// Label text.
    pub text: String,
}

/// Structured data carried by render objects (ADR 011 mandatory field).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructuredData {
    /// Opaque structured payload (e.g. heading level, list index).
    pub bytes: Vec<u8>,
}

/// Interaction descriptor carried by render objects (ADR 011 mandatory field).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractionDescriptor {
    /// Opaque interaction-affordance payload.
    pub bytes: Vec<u8>,
}

/// PLACEHOLDER a11y tree node — no implementation this phase (§10.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct A11yNode {
    /// Semantic role.
    pub role: SemanticRole,
    /// Optional accessible label.
    pub label: Option<TextLabel>,
    /// Structured data from the owning render object.
    pub structured: StructuredData,
    /// Interaction affordances.
    pub interaction: InteractionDescriptor,
    /// Child nodes.
    pub children: Vec<A11yNode>,
    /// Read-only mirror of ADR 011 focus state.
    pub focus_state: FocusState,
}

// ---------------------------------------------------------------------------
// §10.5 Extension points
// ---------------------------------------------------------------------------

/// Derived (not authored) a11y extension point (§10.5). PLACEHOLDER.
///
/// When un-deferred, a virtual accessibility tree is derived from the
/// render-object graph and bridged **directly to platform a11y APIs**
/// (UIAutomation / NSAccessibility / AT-SPI / ARIA-equivalent native
/// surfaces). No DOM is reintroduced.
///
/// [`A11yExtensionPoint::read_focus_state`] is the future reader entry
/// point that consumes `FocusManager::current_focus` (§8.5) — the shared
/// §8↔§10 boundary.
pub trait A11yExtensionPoint {
    /// Derive an a11y node from a render object's mandatory metadata.
    fn derive_a11y_node(&self, from: &RenderObject) -> Option<A11yNode> {
        todo!()
    }
    /// Expose a11y text from a shaped glyph run (ADR 022 hook).
    fn expose_a11y_text(&self, run: &ShapedGlyphRun) -> TextLabel {
        todo!()
    }
    /// Read focus state — future reader of the ADR 011 annotation layer.
    fn read_focus_state(&self) -> FocusState {
        todo!()
    }
}

/// Top-level a11y placeholder stub (§10.5). PLACEHOLDER — no-op this phase.
pub trait A11yPlaceholder {
    /// Build a virtual a11y tree from the render-object graph.
    fn build_tree(&self, root: &RenderObject) -> A11yNode {
        todo!()
    }
    /// Bridge the tree to platform a11y APIs. No-op this phase.
    fn bridge_to_platform(&self, tree: &A11yNode) {
        todo!()
    }
}
