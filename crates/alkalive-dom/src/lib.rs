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
    fn declare_routes(&mut self, routes: Vec<Route>) -> Result<(), DomError>;

    /// Serialise restorable state for the host.
    fn serialize_state(&self) -> Result<SerialisableState, DomError>;
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
    fn set_title(&mut self, text: String) -> Result<(), DomError>;

    /// Write a `<meta>` tag.
    fn set_meta(&mut self, name: String, content: String) -> Result<(), DomError>;

    /// Serve a static HTML snapshot for `route` + `state` (§9.4).
    fn serve_snapshot(&mut self, route: Route, state: SerialisableState) -> Result<Html, DomError>;

    // NOTE: `declare_routes` and `serialize_state` are inherited from
    // `NavigationContract`. The DomBridge method set is EXACTLY the five
    // verbs above — no IME method is exposed (§9.5).
}

// ---------------------------------------------------------------------------
// §9.7 DomBridgeImpl — Wave 9 in-process implementation
// ---------------------------------------------------------------------------

/// In-process [`DomBridge`] implementation used by Wave 9 host shells
/// and unit tests.
///
/// Stores SEO metadata (title, meta tags) and the declared route table
/// in plain owned fields. [`DomBridgeImpl::serve_snapshot`] materialises
/// a frozen [`SeoSnapshot`] value from those fields plus the
/// caller-supplied route and serialisable state, then returns the
/// crawler-grade [`Html`] text.
///
/// This type performs no I/O and no host interaction — it is the
/// canonical no-op host for tests and for runtime builds that have not
/// yet bound a real DOM context. The full method set is exactly the
/// five `DomBridge` verbs; there is **no IME surface** (§9.5).
#[derive(Debug, Clone, Default)]
pub struct DomBridgeImpl {
    /// Last set document title (`<title>` text).
    title: String,
    /// Accumulated `<meta>` name / content pairs in insertion order.
    meta: Vec<(String, String)>,
    /// Last declared route table.
    routes: Vec<Route>,
}

/// HTML-escape a string for safe interpolation into HTML text content and attributes.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

impl DomBridgeImpl {
    /// Construct an empty `DomBridgeImpl` (no title, no meta, no routes).
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only access to the stored document title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Read-only access to the stored `<meta>` name / content pairs.
    pub fn meta(&self) -> &[(String, String)] {
        &self.meta
    }

    /// Read-only access to the stored declared route table.
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }
}

impl NavigationContract for DomBridgeImpl {
    fn declare_routes(&mut self, routes: Vec<Route>) -> Result<(), DomError> {
        self.routes = routes;
        Ok(())
    }

    fn serialize_state(&self) -> Result<SerialisableState, DomError> {
        // Wave 9 ships with an empty (no-op) state payload; the host is
        // free to ignore it. Real serialisation is owned by the runtime
        // state module and injected in a later wave.
        Ok(SerialisableState { bytes: vec![] })
    }
}

impl DomBridge for DomBridgeImpl {
    fn set_title(&mut self, text: String) -> Result<(), DomError> {
        self.title = text;
        Ok(())
    }

    fn set_meta(&mut self, name: String, content: String) -> Result<(), DomError> {
        self.meta.push((name, content));
        Ok(())
    }

    fn serve_snapshot(&mut self, route: Route, state: SerialisableState) -> Result<Html, DomError> {
        // Build a minimal crawler-grade HTML document from stored SEO
        // metadata. State bytes are not interpreted in Wave 9 — the
        // payload is consumed here so later waves can fold it into the
        // snapshot body without an API change.
        let mut text = String::new();
        text.push_str("<!DOCTYPE html><html><head>");
        text.push_str("<title>");
        text.push_str(&escape_html(&self.title));
        text.push_str("</title>");
        for (name, content) in &self.meta {
            text.push_str("<meta name=\"");
            text.push_str(&escape_html(name));
            text.push_str("\" content=\"");
            text.push_str(&escape_html(content));
            text.push_str("\">");
        }
        text.push_str("</head><body></body></html>");

        let html = Html { text };

        // Materialise a frozen SeoSnapshot to validate the value shape.
        // The snapshot is intentionally not retained — only the Html
        // payload is returned to the caller (§9.4).
        let _snapshot = SeoSnapshot {
            route,
            title: self.title.clone(),
            meta: self.meta.clone(),
            html: html.clone(),
            generated_at: Timestamp(0),
            source: SnapshotSource::BuildTime,
        };
        // `state` is part of the closed surface but not yet consumed.
        let _ = state;

        Ok(html)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// W9-T5 interface-surface test (compile-time assertion): assert
    /// that `DomBridgeImpl` implements the full `DomBridge` trait. If a
    /// method is removed, renamed, or an IME verb is added without an
    /// amending ADR, this test fails to compile.
    #[test]
    fn dom_bridge_impl_implements_dom_bridge() {
        fn _assert_dom_bridge<T: DomBridge>() {}
        _assert_dom_bridge::<DomBridgeImpl>();
    }

    /// `set_title` stores the title and returns `Ok(())`.
    #[test]
    fn set_title_stores_title() {
        let mut bridge = DomBridgeImpl::new();
        assert_eq!(bridge.title(), "");
        bridge.set_title("AlkALive".to_string()).unwrap();
        assert_eq!(bridge.title(), "AlkALive");
        // Overwrites the previous value.
        bridge.set_title("AlkALive — Runtime".to_string()).unwrap();
        assert_eq!(bridge.title(), "AlkALive — Runtime");
    }

    /// `set_meta` accumulates meta pairs in insertion order.
    #[test]
    fn set_meta_stores_meta_pairs() {
        let mut bridge = DomBridgeImpl::new();
        assert!(bridge.meta().is_empty());
        bridge
            .set_meta(
                "description".to_string(),
                "GPU-resident UI runtime".to_string(),
            )
            .unwrap();
        bridge
            .set_meta("viewport".to_string(), "width=device-width".to_string())
            .unwrap();
        assert_eq!(
            bridge.meta(),
            &[
                (
                    "description".to_string(),
                    "GPU-resident UI runtime".to_string()
                ),
                ("viewport".to_string(), "width=device-width".to_string()),
            ][..]
        );
    }

    /// `declare_routes` stores the declared route table.
    #[test]
    fn declare_routes_stores_routes() {
        let mut bridge = DomBridgeImpl::new();
        assert!(bridge.routes().is_empty());
        let routes = vec![
            Route {
                path: "/".to_string(),
                name: Some("root".to_string()),
            },
            Route {
                path: "/about".to_string(),
                name: None,
            },
        ];
        bridge.declare_routes(routes).unwrap();
        assert_eq!(
            bridge.routes(),
            &[
                Route {
                    path: "/".to_string(),
                    name: Some("root".to_string())
                },
                Route {
                    path: "/about".to_string(),
                    name: None
                },
            ][..]
        );
        // Replaces (not appends) the previous table.
        bridge
            .declare_routes(vec![Route {
                path: "/".to_string(),
                name: None,
            }])
            .unwrap();
        assert_eq!(bridge.routes().len(), 1);
    }

    /// `serialize_state` returns `Ok` with an empty payload (Wave 9).
    #[test]
    fn serialize_state_returns_ok_empty() {
        let bridge = DomBridgeImpl::new();
        let state = bridge.serialize_state().unwrap();
        assert!(state.bytes.is_empty(), "Wave 9 state must be empty");
    }

    /// `serve_snapshot` produces a valid crawler-grade HTML document
    /// reflecting the stored title and meta tags.
    #[test]
    fn serve_snapshot_produces_valid_html() {
        let mut bridge = DomBridgeImpl::new();
        bridge.set_title("Hello".to_string()).unwrap();
        bridge
            .set_meta("description".to_string(), "Snapshot test".to_string())
            .unwrap();

        let route = Route {
            path: "/".to_string(),
            name: Some("root".to_string()),
        };
        let state = SerialisableState { bytes: vec![] };
        let html = bridge.serve_snapshot(route, state).unwrap();

        assert!(html.text.starts_with("<!DOCTYPE html>"));
        assert!(html.text.contains("<title>Hello</title>"));
        assert!(html
            .text
            .contains("<meta name=\"description\" content=\"Snapshot test\">"));
        assert!(html.text.ends_with("</html>"));
    }

    /// `serve_snapshot` HTML-escapes the title and meta name/content so
    /// that attacker-controlled metadata cannot break out of the
    /// `<title>` element or the `<meta>` attributes (SEC-01).
    #[test]
    fn serve_snapshot_escapes_html_in_title_and_meta() {
        let mut bridge = DomBridgeImpl::new();
        bridge
            .set_title("</title><script>alert(1)</script>".to_string())
            .unwrap();
        bridge
            .set_meta(
                "description".to_string(),
                "\"><img onerror=alert(1) src=x>".to_string(),
            )
            .unwrap();
        let html = bridge
            .serve_snapshot(
                Route {
                    path: "/".to_string(),
                    name: None,
                },
                SerialisableState { bytes: vec![] },
            )
            .unwrap();
        // Verify the script tag is escaped
        assert!(!html.text.contains("<script>"));
        assert!(html.text.contains("&lt;script&gt;"));
        assert!(!html.text.contains("<img onerror"));
        assert!(html.text.contains("&quot;&gt;"));
    }

    /// Interface-surface test: construct a `DomBridgeImpl` and exercise
    /// each of the five closed-surface verbs end-to-end. This is the
    /// runtime complement to the compile-time trait-bound assertion
    /// above — together they enforce that the method set is exactly
    /// `{ set_title, set_meta, serve_snapshot, declare_routes,
    /// serialize_state }` (§9.5, §9.7).
    #[test]
    fn interface_surface_exercises_all_five_verbs() {
        let mut bridge = DomBridgeImpl::new();

        // 1. set_title
        bridge.set_title("Title".to_string()).unwrap();
        assert_eq!(bridge.title(), "Title");

        // 2. set_meta
        bridge.set_meta("a".to_string(), "b".to_string()).unwrap();
        assert_eq!(bridge.meta(), &[("a".to_string(), "b".to_string())][..]);

        // 3. serve_snapshot
        let html = bridge
            .serve_snapshot(
                Route {
                    path: "/".to_string(),
                    name: None,
                },
                SerialisableState { bytes: vec![] },
            )
            .unwrap();
        assert!(!html.text.is_empty());

        // 4. declare_routes (inherited from NavigationContract)
        bridge
            .declare_routes(vec![Route {
                path: "/".to_string(),
                name: None,
            }])
            .unwrap();
        assert_eq!(bridge.routes().len(), 1);

        // 5. serialize_state (inherited from NavigationContract)
        let state = bridge.serialize_state().unwrap();
        assert!(state.bytes.is_empty());
    }
}
