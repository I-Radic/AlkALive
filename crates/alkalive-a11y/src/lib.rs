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
//! Wave 3 trait stubs: committed signatures with stub (no-op) bodies.
//! Per ADR 019 no real a11y tree is built this phase; the default trait
//! bodies return minimal neutral values so the extension contract is
//! locked and exercised by tests without depending on `todo!()` panics.
//! The concrete [`A11yExtensionStub`] type inherits these defaults.

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
///
/// Per ADR 019 the default method bodies are **stub** implementations
/// (no derivation, empty labels, no focus state exposed) so the
/// extension contract is locked and exercised without `todo!()` panics.
/// The concrete [`A11yExtensionStub`] type inherits these defaults.
pub trait A11yExtensionPoint {
    /// Derive an a11y node from a render object's mandatory metadata.
    ///
    /// Stub: returns `None` — no derivation this phase (ADR 019).
    fn derive_a11y_node(&self, _from: &RenderObject) -> Option<A11yNode> {
        None
    }
    /// Expose a11y text from a shaped glyph run (ADR 022 hook).
    ///
    /// Stub: returns an empty [`TextLabel`] — no text exposed this phase.
    fn expose_a11y_text(&self, _run: &ShapedGlyphRun) -> TextLabel {
        TextLabel { text: String::new() }
    }
    /// Read focus state — future reader of the ADR 011 annotation layer.
    ///
    /// Stub: returns [`FocusState::None`] — no focus state exposed this phase.
    fn read_focus_state(&self) -> FocusState {
        FocusState::None
    }
}

/// Top-level a11y placeholder stub (§10.5). PLACEHOLDER — no-op this phase.
///
/// Per ADR 019 the default method bodies are **stub** implementations:
/// [`A11yPlaceholder::build_tree`] returns a minimal [`A11yNode`] with
/// [`SemanticRole::None`] and no children; [`A11yPlaceholder::bridge_to_platform`]
/// is a no-op. The concrete [`A11yExtensionStub`] type inherits these defaults.
pub trait A11yPlaceholder {
    /// Build a virtual a11y tree from the render-object graph.
    ///
    /// Stub: returns a minimal [`A11yNode`] (`role: SemanticRole::None`,
    /// no label, empty structured/interaction payloads, no children,
    /// [`FocusState::None`]) — no real tree is built this phase (ADR 019).
    fn build_tree(&self, _root: &RenderObject) -> A11yNode {
        A11yNode {
            role: SemanticRole::None,
            label: None,
            structured: StructuredData { bytes: Vec::new() },
            interaction: InteractionDescriptor { bytes: Vec::new() },
            children: Vec::new(),
            focus_state: FocusState::None,
        }
    }
    /// Bridge the tree to platform a11y APIs. No-op this phase.
    ///
    /// Stub: does nothing (ADR 019).
    fn bridge_to_platform(&self, _tree: &A11yNode) {
        // Intentional no-op: no AT bridge ships this phase (ADR 019).
    }
}

// ---------------------------------------------------------------------------
// §10.5 Concrete stub type
// ---------------------------------------------------------------------------

/// Concrete accessibility stub (§10.5, ADR 019).
///
/// A zero-sized placeholder implementing both [`A11yExtensionPoint`] and
/// [`A11yPlaceholder`] via their stub default bodies. No real a11y tree
/// is derived and no platform bridge is invoked this phase; the type
/// exists to lock the extension contract so un-deferral is additive,
/// not architectural.
///
/// Construct with [`A11yExtensionStub::new`] or `Default::default()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct A11yExtensionStub;

impl A11yExtensionStub {
    /// Create a new [`A11yExtensionStub`].
    pub const fn new() -> Self {
        Self
    }
}

impl A11yExtensionPoint for A11yExtensionStub {}

impl A11yPlaceholder for A11yExtensionStub {}

// ---------------------------------------------------------------------------
// §10.5 Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_a11y_node_returns_none() {
        let stub = A11yExtensionStub::new();
        let ro = RenderObject;
        assert_eq!(stub.derive_a11y_node(&ro), None);
    }

    #[test]
    fn expose_a11y_text_returns_empty_label() {
        let stub = A11yExtensionStub::new();
        let run = ShapedGlyphRun;
        let label = stub.expose_a11y_text(&run);
        assert_eq!(label.text, String::new());
        assert!(label.text.is_empty());
    }

    #[test]
    fn read_focus_state_returns_none() {
        let stub = A11yExtensionStub::new();
        assert_eq!(stub.read_focus_state(), FocusState::None);
    }

    #[test]
    fn build_tree_returns_role_none_node() {
        let stub = A11yExtensionStub::new();
        let ro = RenderObject;
        let node = stub.build_tree(&ro);
        assert_eq!(node.role, SemanticRole::None);
        assert!(node.children.is_empty());
        assert_eq!(node.focus_state, FocusState::None);
        assert_eq!(node.label, None);
        assert!(node.structured.bytes.is_empty());
        assert!(node.interaction.bytes.is_empty());
    }

    #[test]
    fn bridge_to_platform_does_not_panic() {
        let stub = A11yExtensionStub::new();
        let node = stub.build_tree(&RenderObject);
        // Pure no-op; reaching this line means it did not panic.
        stub.bridge_to_platform(&node);
    }
}
