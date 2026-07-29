//! AlkALive alkalive-dom crate.
//!
//! DOM Interop Layer — see `docs/SPECIFICATION.md` §9 and ADRs 012 / 013 / 019 / 020.
//!
//! Metadata-only DOM bridge (ADR 020): the runtime exposes a thin DOM
//! surface for exactly three concerns — setting `<title>`, writing
//! `<meta>` tags, and serving a static HTML snapshot. There is **no
//! DOM-tree interaction for UI** — no layout, text, accessibility,
//! navigation-DOM, or input bridge exists. The bridge is non-hot-path
//! by construction: it exposes **no per-frame verbs**.
//!
//! Wave 3 trait-definition skeleton: signatures are locked against the
//! spec; every body is `todo!()`. No implementation ships this wave.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// §9.7 Supporting structs
// ---------------------------------------------------------------------------

/// A declared application route (ADR 012).
///
/// Navigation is a structured contract, not a DOM mutation: the app
/// declares its routes and serialises restorable state to the host; the
/// host owns URL, history, and back/forward semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Route {
    /// Route path pattern (host-interpreted).
    pub path: String,
    /// Optional human-readable route name.
    pub name: Option<String>,
}

/// Serialisable application state for host-side restoration (ADR 012).
///
/// Owned, opaque byte payload; the host is free to ignore it. The
/// runtime never mutates addressable document state directly — no
/// `pushState` / `replaceState` analogue is exposed through the bridge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SerialisableState {
    /// Serialised state bytes (e.g. a compact binary or JSON blob).
    pub bytes: Vec<u8>,
}

/// Frozen, crawler-grade HTML — never a live tree (ADR 020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Html {
    /// The HTML document text.
    pub text: String,
}

/// UTC millisecond timestamp of snapshot emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

/// Whether a snapshot was generated at build time or on demand (§9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotSource {
    /// Emitted at build time from declared routes + serialisable state.
    BuildTime,
    /// Emitted on demand to a detected crawler user-agent.
    OnDemand,
}

/// A frozen SEO snapshot value (§9.4).
///
/// Never a live tree; on-demand generation runs off the render thread
/// and never blocks the frame loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SeoSnapshot {
    /// Route this snapshot belongs to.
    pub route: Route,
    /// Document title.
    pub title: String,
    /// `<meta>` name / content pairs.
    pub meta: Vec<(String, String)>,
    /// Frozen crawler-grade HTML.
    pub html: Html,
    /// When the snapshot was generated.
    pub generated_at: Timestamp,
    /// Build-time or on-demand origin.
    pub source: SnapshotSource,
}

// ---------------------------------------------------------------------------
// §9.6 Error handling
// ---------------------------------------------------------------------------

/// DOM API failure (§9.6).
///
/// Degrades gracefully: the build-time snapshot continues to be served
/// to crawlers, and the GPU render loop is unaffected. DOM failure
/// **never** blocks the WASM render thread; the bridge is fire-and-forget
/// from the frame loop's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomError {
    /// No host DOM context bound.
    HostUnavailable,
    /// Host refused a meta write (policy / CSP).
    MetaRejected,
    /// Snapshot serialisation / emission failed.
    SnapshotWriteFailed,
    /// Host rejected a declared route.
    RouteDeclined,
    /// State contained a non-serialisable value.
    StateUnserialisable,
    /// Host did not acknowledge within budget.
    Timeout,
}

// ---------------------------------------------------------------------------
// §9.7 Navigation / URL contract (ADR 012)
// ---------------------------------------------------------------------------

/// Structured navigation contract (ADR 012).
///
/// The host retains URL / history / back-forward ownership; the runtime
/// never mutates addressable document state directly.
pub trait NavigationContract {
    /// Declare the app's routes to the host.
    fn declare_routes(&mut self, routes: Vec<Route>) -> Result<(), DomError> {
        todo!()
    }
    /// Serialise restorable state for the host.
    fn serialize_state(&self) -> Result<SerialisableState, DomError> {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// §9.7 DomBridge (ADR 020) — CLOSED interface
// ---------------------------------------------------------------------------

/// Thin metadata-only DOM bridge (ADR 020).
///
/// Composes the SEO verbs (`set_title` / `set_meta` / `serve_snapshot`)
/// with the [`NavigationContract`] verbs (`declare_routes` /
/// `serialize_state`). Both are non-hot-path, host-facing, and
/// structurally incapable of crossing into the render loop.
///
/// # CLOSED under ADRs 012 / 013 / 019 / 020
///
/// No methods for layout, draw, hit-test, text-measurement, a11y,
/// focus, input, or IME. **None may be added without an ADR amending
/// ADRs 013 / 019 / 020.** The method set of any `DomBridge` value is
/// exactly `{ set_title, set_meta, serve_snapshot, declare_routes,
/// serialize_state }` — enforced by the W9-T5 interface-surface test.
pub trait DomBridge: NavigationContract {
    /// Set the document `<title>`.
    fn set_title(&mut self, text: String) -> Result<(), DomError> {
        todo!()
    }
    /// Write a `<meta>` tag.
    fn set_meta(&mut self, name: String, content: String) -> Result<(), DomError> {
        todo!()
    }
    /// Serve a static HTML snapshot for `route` + `state` (§9.4).
    fn serve_snapshot(
        &mut self,
        route: Route,
        state: SerialisableState,
    ) -> Result<Html, DomError> {
        todo!()
    }
    // NOTE: `declare_routes` and `serialize_state` are inherited from
    // `NavigationContract`. The DomBridge method set is EXACTLY the five
    // verbs above — no IME method is exposed (§9.5).
}
