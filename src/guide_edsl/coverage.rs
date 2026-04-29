//! Interpretation 3 — feature coverage (spec/0010 §4.3).
//!
//! `terse_coverage()`: the set of Features claimed by EDSL nodes that survive
//! the terse pass (i.e. NOT inside any `detail: true` subtree).
//!
//! `full_coverage()`: every Feature claimed anywhere in the EDSL.
//!
//! The exhaustiveness check (§4.4) compares these against
//! `Feature::all_set()` and `Feature::roots_set()`.

use std::collections::BTreeSet;

use super::features::Feature;
use super::nodes::GuideNode;

/// Every Feature claimed anywhere in the EDSL (terse + verbose).
pub fn full_coverage(root: &GuideNode) -> BTreeSet<Feature> {
    let mut out = BTreeSet::new();
    root.walk_features(&mut |f, _detail| {
        out.insert(f);
    });
    out
}

/// Features claimed by nodes that the terse interpreter would emit
/// (i.e. not inside any `detail: true` ancestor).
pub fn terse_coverage(root: &GuideNode) -> BTreeSet<Feature> {
    let mut out = BTreeSet::new();
    root.walk_features(&mut |f, detail| {
        if !detail {
            out.insert(f);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::super::tree::root;
    use super::*;

    /// Spec/0010 §4.4(a) — FULL coverage equals the full Feature set.
    /// Difference in either direction is a hard failure naming the offending
    /// Feature(s).
    #[test]
    fn full_coverage_equals_feature_all_set() {
        let r = root();
        let full = full_coverage(&r);
        let all = Feature::all_set();
        let missing_from_guide: Vec<&str> = all.difference(&full).map(|f| f.name()).collect();
        let missing_from_impl: Vec<&str> = full.difference(&all).map(|f| f.name()).collect();
        assert!(
            missing_from_guide.is_empty(),
            "Features in registry but not claimed by any EDSL node: {:?}",
            missing_from_guide
        );
        assert!(
            missing_from_impl.is_empty(),
            "Features claimed by EDSL but not in registry: {:?}",
            missing_from_impl
        );
    }

    /// Spec/0010 §4.4(b) — every ROOT Feature is covered by the terse pass.
    /// A root that lives only in a `detail: true` subtree is a hard failure:
    /// the user-facing capability list must be complete in the default pass.
    #[test]
    fn every_root_feature_in_terse_pass() {
        let r = root();
        let terse = terse_coverage(&r);
        let roots = Feature::roots_set();
        let missing: Vec<&str> = roots.difference(&terse).map(|f| f.name()).collect();
        assert!(
            missing.is_empty(),
            "Root features missing from terse pass (must be promoted out of \
             detail: true subtrees): {:?}",
            missing
        );
    }

    /// Spec/0010 §4.4(c) — non-root Features MAY appear only in detail; this
    /// is the expected pattern for refinement / test-mechanics children. Sanity
    /// check that some such features actually live in detail-only subtrees
    /// (otherwise the terse/verbose distinction is decorative).
    #[test]
    fn some_non_root_features_live_in_detail_only() {
        let r = root();
        let full = full_coverage(&r);
        let terse = terse_coverage(&r);
        let detail_only: BTreeSet<Feature> = full.difference(&terse).copied().collect();
        // Every detail-only feature must be a non-root.
        for f in &detail_only {
            assert!(
                f.parent().is_some(),
                "feature {:?} is a root but lives only in a detail subtree \
                 (forbidden by §4.4(b))",
                f.name()
            );
        }
        assert!(
            !detail_only.is_empty(),
            "expected at least one feature to live exclusively in detail \
             subtrees (the terse/verbose distinction must do real work)"
        );
    }
}
