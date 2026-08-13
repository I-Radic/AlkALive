//! ADR-027 Phase 2 + ADR-025 — Seminaïve evaluation strategy.
//!
//! This module bridges the monotonicity metadata in [`crate::ir::AlgorithmIR`]
//! (ADR-027 Phase 2) with the incremental computation engine (ADR-025). It
//! decides, for each collection declaration, whether the runtime can use
//! **seminaïve evaluation** (process only new/removed elements) or must
//! fall back to **full re-evaluation**.
//!
//! # Strategy
//!
//! | Monotonicity | Strategy | Runtime behaviour |
//! |---|---|---|
//! | `Monotone` | `SeminiveNew` | Process only newly-added elements; existing elements are unchanged. |
//! | `Antitone` | `SeminiveRemoved` | Skip removed elements; remaining elements are unchanged. |
//! | `Unrestricted` | `Full` | Re-evaluate all elements on each reactive update. |
//!
//! The runtime calls [`collection_strategies`] to obtain the strategy for
//! every collection in a compiled [`AlgorithmIR`].

#![forbid(unsafe_code)]

use core::fmt;

use crate::ir::{AlgorithmIR, CollectionDeclIR, Monotonicity};

/// The evaluation strategy the runtime should use for a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvaluationStrategy {
    /// Full re-evaluation: process all elements on each reactive update.
    /// Used for `Unrestricted` collections.
    Full,
    /// Seminaïve (new only): process only newly-added elements.
    /// Used for `Monotone` collections (ADR-027 Phase 2).
    SeminiveNew,
    /// Seminaïve (removed skip): skip removed elements.
    /// Used for `Antitone` collections.
    SeminiveRemoved,
}

impl fmt::Display for EvaluationStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvaluationStrategy::Full => write!(f, "full"),
            EvaluationStrategy::SeminiveNew => write!(f, "seminaive-new"),
            EvaluationStrategy::SeminiveRemoved => write!(f, "seminaive-removed"),
        }
    }
}

/// Returns the evaluation strategy for a single collection based on its
/// monotonicity.
pub fn collection_strategy(col: &CollectionDeclIR) -> EvaluationStrategy {
    match col.monotonicity {
        Monotonicity::Monotone => EvaluationStrategy::SeminiveNew,
        Monotonicity::Antitone => EvaluationStrategy::SeminiveRemoved,
        Monotonicity::Unrestricted => EvaluationStrategy::Full,
    }
}

/// Returns a map from collection name to [`EvaluationStrategy`] for every
/// collection in `ir`. The runtime calls this once after compilation to
/// configure its incremental engine.
pub fn collection_strategies(ir: &AlgorithmIR) -> Vec<(String, EvaluationStrategy)> {
    ir.collections
        .iter()
        .map(|c| (c.name.clone(), collection_strategy(c)))
        .collect()
}

/// Returns the number of collections that support seminaïve evaluation
/// (either `SeminiveNew` or `SeminiveRemoved`).
pub fn seminive_eligible_count(ir: &AlgorithmIR) -> usize {
    ir.collections
        .iter()
        .filter(|c| c.monotonicity != Monotonicity::Unrestricted)
        .count()
}

/// Returns `true` iff at least one collection in `ir` supports seminaïve
/// evaluation. The runtime can use this as a fast check to decide whether
/// to activate the incremental engine at all.
pub fn has_seminive_collections(ir: &AlgorithmIR) -> bool {
    seminive_eligible_count(ir) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{mint_module_id, CollectionDeclIR, Monotonicity};

    fn make_ir(collections: Vec<CollectionDeclIR>) -> AlgorithmIR {
        let mut ir = AlgorithmIR::new(mint_module_id("M"), "M");
        ir.collections = collections;
        ir
    }

    fn col(name: &str, m: Monotonicity) -> CollectionDeclIR {
        CollectionDeclIR {
            name: name.into(),
            element_type: "i32".into(),
            monotonicity: m,
        }
    }

    #[test]
    fn monotone_gets_seminive_new() {
        assert_eq!(
            collection_strategy(&col("v", Monotonicity::Monotone)),
            EvaluationStrategy::SeminiveNew
        );
    }

    #[test]
    fn antitone_gets_seminive_removed() {
        assert_eq!(
            collection_strategy(&col("v", Monotonicity::Antitone)),
            EvaluationStrategy::SeminiveRemoved
        );
    }

    #[test]
    fn unrestricted_gets_full() {
        assert_eq!(
            collection_strategy(&col("v", Monotonicity::Unrestricted)),
            EvaluationStrategy::Full
        );
    }

    #[test]
    fn collection_strategies_map() {
        let ir = make_ir(vec![
            col("a", Monotonicity::Monotone),
            col("b", Monotonicity::Antitone),
            col("c", Monotonicity::Unrestricted),
        ]);
        let map = collection_strategies(&ir);
        assert_eq!(map.len(), 3);
        assert_eq!(map[0], ("a".into(), EvaluationStrategy::SeminiveNew));
        assert_eq!(map[1], ("b".into(), EvaluationStrategy::SeminiveRemoved));
        assert_eq!(map[2], ("c".into(), EvaluationStrategy::Full));
    }

    #[test]
    fn seminive_eligible_count_excludes_unrestricted() {
        let ir = make_ir(vec![
            col("a", Monotonicity::Monotone),
            col("b", Monotonicity::Antitone),
            col("c", Monotonicity::Unrestricted),
        ]);
        assert_eq!(seminive_eligible_count(&ir), 2);
    }

    #[test]
    fn has_seminive_collections_false_when_all_unrestricted() {
        let ir = make_ir(vec![col("a", Monotonicity::Unrestricted)]);
        assert!(!has_seminive_collections(&ir));
    }

    #[test]
    fn has_seminive_collections_true_when_any_monotone() {
        let ir = make_ir(vec![
            col("a", Monotonicity::Unrestricted),
            col("b", Monotonicity::Monotone),
        ]);
        assert!(has_seminive_collections(&ir));
    }

    #[test]
    fn empty_ir_has_no_seminive() {
        let ir = make_ir(vec![]);
        assert!(!has_seminive_collections(&ir));
        assert_eq!(seminive_eligible_count(&ir), 0);
        assert!(collection_strategies(&ir).is_empty());
    }

    #[test]
    fn evaluation_strategy_display() {
        assert_eq!(format!("{}", EvaluationStrategy::Full), "full");
        assert_eq!(
            format!("{}", EvaluationStrategy::SeminiveNew),
            "seminaive-new"
        );
        assert_eq!(
            format!("{}", EvaluationStrategy::SeminiveRemoved),
            "seminaive-removed"
        );
    }
}
