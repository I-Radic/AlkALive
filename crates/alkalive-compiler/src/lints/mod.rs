//! Lint passes for the AlkALive `.alk` compiler (ADR-027 Phase 1).
//!
//! Lints run *after* parsing but *before* codegen. They consume the
//! [`crate::ast::ModuleDecl`] and produce a [`LintSet`] of findings. Lints
//! never abort compilation on their own — instead, they record
//! [`LintReport`]s with a [`LintSeverity`], and the caller decides whether
//! to surface them as warnings or to escalate to errors via the
//! `#![deny(monotonicity)]` file-level attribute.
//!
//! # Available lints
//!
//! - [`monotonicity`]: enforces `@monotone` / `@antitone` attribute
//!   annotations on scene-level node declarations (Phase 1 scope:
//!   intra-function enforcement is deferred until the `.alk` grammar grows
//!   function bodies).
//!
//! # Adding a new lint
//!
//! 1. Create `lints/<name>.rs` with a `pub fn run(ast: &ModuleDecl, set: &mut LintSet)`.
//! 2. Add `pub mod <name>;` to this file.
//! 3. Call the lint from [`run_lints`].
//! 4. Add tests in `crates/alkalive-compiler/tests/lint_tests.rs`.

#![forbid(unsafe_code)]

pub mod monotonicity;

use core::fmt;

/// Re-export the AST module so lint submodules can reference it without
/// repeating the `crate::` prefix.
pub use crate::ast;

/// Severity of a lint finding.
///
/// `Warning` is the default. `Deny` is reserved for findings that have been
/// escalated by `#![deny(monotonicity)]` (or future file-level deny
/// attributes). The compiler surfaces `Deny` findings as hard errors via
/// [`LintSet::has_errors`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    /// A non-fatal finding. Surfaced to the user but does not block
    /// compilation.
    Warning,
    /// A fatal finding. Surfaced as a compile error by the caller.
    Deny,
}

impl fmt::Display for LintSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintSeverity::Warning => write!(f, "warning"),
            LintSeverity::Deny => write!(f, "error"),
        }
    }
}

/// A single lint finding.
///
/// Carries the severity, a human-readable message, and the 1-based source
/// position of the offending construct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintReport {
    /// Severity of this finding.
    pub severity: LintSeverity,
    /// Human-readable description of the finding, including the lint name
    /// in the form `"monotonicity: <description>"`.
    pub message: String,
    /// 1-based line where the offending construct begins.
    pub line: u32,
    /// 1-based column where the offending construct begins.
    pub col: u32,
}

impl LintReport {
    /// Convenience constructor.
    pub fn new(severity: LintSeverity, message: impl Into<String>, line: u32, col: u32) -> Self {
        Self {
            severity,
            message: message.into(),
            line,
            col,
        }
    }

    /// Render this report as a single human-readable line, prefixed by
    /// the severity and the source position.
    pub fn render(&self) -> String {
        format!("{}:{}: {}: {}", self.line, self.col, self.severity, self.message)
    }
}

impl fmt::Display for LintReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// A collection of lint findings.
///
/// The set tracks whether `#![deny(monotonicity)]` was set on the module
/// so that subsequent [`LintReport`]s added via [`LintSet::add`] can be
/// upgraded from [`LintSeverity::Warning`] to [`LintSeverity::Deny`]
/// automatically.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LintSet {
    /// The accumulated lint findings, in insertion order.
    pub reports: Vec<LintReport>,
    /// `true` if the module carried a `#![deny(monotonicity)]` attribute.
    /// When set, [`LintSet::add`] upgrades `Warning` findings to `Deny`.
    pub deny_monotonicity: bool,
}

impl LintSet {
    /// Construct an empty `LintSet`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a lint finding. If `deny_monotonicity` is set and the report
    /// is a [`LintSeverity::Warning`], it is upgraded to
    /// [`LintSeverity::Deny`] before being stored.
    pub fn add(&mut self, report: LintReport) {
        let report = if self.deny_monotonicity && report.severity == LintSeverity::Warning {
            LintReport {
                severity: LintSeverity::Deny,
                ..report
            }
        } else {
            report
        };
        self.reports.push(report);
    }

    /// Returns `true` if any recorded finding has severity
    /// [`LintSeverity::Deny`].
    pub fn has_errors(&self) -> bool {
        self.reports
            .iter()
            .any(|r| r.severity == LintSeverity::Deny)
    }

    /// Returns the number of recorded findings.
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// Returns `true` if there are no recorded findings.
    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    /// Returns an iterator over the recorded findings.
    pub fn iter(&self) -> core::slice::Iter<'_, LintReport> {
        self.reports.iter()
    }
}

impl fmt::Display for LintSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, r) in self.reports.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", r)?;
        }
        Ok(())
    }
}

/// Run all registered lints against the AST and return the collected
/// [`LintSet`].
///
/// The lint passes themselves live in submodules (e.g.
/// [`monotonicity::run`]). This function dispatches to each in turn,
/// accumulating findings into a single [`LintSet`].
///
/// # Phase 1 scope
///
/// Only the [`monotonicity`] lint is registered. As the `.alk` grammar
/// grows (function bodies, method calls, collections), additional lints
/// will be wired in here.
pub fn run_lints(ast: &ast::ModuleDecl) -> LintSet {
    let mut set = LintSet::new();

    // Pre-scan: detect `#![deny(monotonicity)]` on the module so that
    // subsequent warnings are upgraded to errors as they are added.
    for attr in &ast.attributes {
        if attr.name == "deny(monotonicity)" {
            set.deny_monotonicity = true;
        }
    }

    monotonicity::run(ast, &mut set);

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_set_default_is_empty() {
        let s = LintSet::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(!s.has_errors());
        assert!(!s.deny_monotonicity);
    }

    #[test]
    fn lint_set_add_warning() {
        let mut s = LintSet::new();
        s.add(LintReport::new(
            LintSeverity::Warning,
            "[test] something fishy",
            3,
            7,
        ));
        assert_eq!(s.len(), 1);
        assert!(!s.has_errors());
        assert_eq!(s.reports[0].severity, LintSeverity::Warning);
    }

    #[test]
    fn lint_set_add_deny() {
        let mut s = LintSet::new();
        s.add(LintReport::new(LintSeverity::Deny, "[test] nope", 1, 1));
        assert!(s.has_errors());
    }

    #[test]
    fn lint_set_deny_monotonicity_upgrades_warnings() {
        let mut s = LintSet::new();
        s.deny_monotonicity = true;
        s.add(LintReport::new(
            LintSeverity::Warning,
            "monotonicity: @monotone on non-collection",
            5,
            1,
        ));
        assert_eq!(s.len(), 1);
        assert_eq!(s.reports[0].severity, LintSeverity::Deny);
        assert!(s.has_errors());
    }

    #[test]
    fn lint_set_deny_monotonicity_does_not_downgrade() {
        // A Deny report stays Deny regardless of the flag.
        let mut s = LintSet::new();
        s.deny_monotonicity = true;
        s.add(LintReport::new(LintSeverity::Deny, "[x]", 1, 1));
        assert_eq!(s.reports[0].severity, LintSeverity::Deny);
    }

    #[test]
    fn lint_report_render() {
        let r = LintReport::new(LintSeverity::Warning, "monotonicity: hi", 10, 20);
        let s = r.render();
        assert!(s.contains("10:20"), "got: {}", s);
        assert!(s.contains("warning"), "got: {}", s);
        assert!(s.contains("monotonicity: hi"), "got: {}", s);
    }

    #[test]
    fn lint_severity_display() {
        assert_eq!(format!("{}", LintSeverity::Warning), "warning");
        assert_eq!(format!("{}", LintSeverity::Deny), "error");
    }

    #[test]
    fn lint_set_display_renders_each_report() {
        let mut s = LintSet::new();
        s.add(LintReport::new(LintSeverity::Warning, "[a] one", 1, 1));
        s.add(LintReport::new(LintSeverity::Deny, "[b] two", 2, 2));
        let out = format!("{}", s);
        assert!(out.contains("[a] one"), "got: {}", out);
        assert!(out.contains("[b] two"), "got: {}", out);
    }

    #[test]
    fn run_lints_on_empty_module_returns_empty_set() {
        let m = ast::ModuleDecl {
            name: "M".into(),
            scene: None,
            attributes: Vec::new(),
            line: 1,
            col: 1,
        };
        let set = run_lints(&m);
        assert!(set.is_empty());
        assert!(!set.deny_monotonicity);
    }

    #[test]
    fn run_lints_detects_deny_monotonicity_attribute() {
        let m = ast::ModuleDecl {
            name: "M".into(),
            scene: None,
            attributes: vec![ast::Attribute::new("deny(monotonicity)", 1, 1)],
            line: 1,
            col: 1,
        };
        let set = run_lints(&m);
        assert!(set.deny_monotonicity);
    }
}
