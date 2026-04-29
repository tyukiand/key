//! `--feature=<id>` filter (spec/0011 §B).
//!
//! Filter the EDSL tree to subtrees whose declared Features (recursively)
//! include either the named Feature OR any descendant of it in the
//! Feature forest. Combinable with the terse / verbose pass.

use super::features::Feature;
use super::nodes::GuideNode;

/// Return a copy of the tree pruned so only nodes that contain at least one
/// Feature whose chain of `.parent()` calls reaches `target` (or that ARE
/// `target`) are retained. Sections are kept iff at least one descendant
/// matches; matching leaves keep their original detail flag so the chosen
/// pass (terse / verbose) renders them as it would in the unfiltered tree.
pub fn filter_tree(node: &GuideNode, target: Feature) -> Option<GuideNode> {
    match node {
        GuideNode::Section {
            title,
            body,
            detail,
            terse_summary,
        } => {
            let kept: Vec<GuideNode> = body.iter().filter_map(|c| filter_tree(c, target)).collect();
            if kept.is_empty() {
                None
            } else {
                Some(GuideNode::Section {
                    title,
                    body: kept,
                    detail: *detail,
                    terse_summary: *terse_summary,
                })
            }
        }
        GuideNode::ExampleControl { feature, .. }
        | GuideNode::ExampleFixture { feature, .. }
        | GuideNode::FeatureRef { feature, .. } => {
            if feature.is_descendant_of(target) {
                Some(node.clone())
            } else {
                None
            }
        }
        GuideNode::Prose { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tree::root;
    use super::*;
    use crate::guide_edsl::coverage::full_coverage;

    fn count_examples(node: &GuideNode) -> usize {
        let mut n = 0;
        count_examples_inner(node, &mut n);
        n
    }
    fn count_examples_inner(node: &GuideNode, n: &mut usize) {
        match node {
            GuideNode::Section { body, .. } => {
                for c in body {
                    count_examples_inner(c, n);
                }
            }
            GuideNode::ExampleControl { .. }
            | GuideNode::ExampleFixture { .. }
            | GuideNode::FeatureRef { .. } => *n += 1,
            GuideNode::Prose { .. } => {}
        }
    }

    /// Spec/0011 §B.5 — `--feature=<root>` keeps the root and its descendants.
    #[test]
    fn filter_root_keeps_root_and_descendants() {
        let r = root();
        let filtered = filter_tree(&r, Feature::PseudoFileEnv).expect("non-empty");
        let cov = full_coverage(&filtered);
        assert!(
            cov.contains(&Feature::PseudoFileEnv),
            "root must survive filter"
        );
        assert!(
            cov.contains(&Feature::TestFixtureEnvOverride),
            "descendant TestFixtureEnvOverride must survive filter"
        );
        // Sibling root (PseudoFileExecutable) is dropped.
        assert!(
            !cov.contains(&Feature::PseudoFileExecutable),
            "sibling root must be excluded from the filtered tree"
        );
    }

    /// Spec/0011 §B.5 — `--feature=<descendant>` keeps only that node.
    #[test]
    fn filter_descendant_keeps_only_that_node() {
        let r = root();
        let filtered = filter_tree(&r, Feature::TestFixtureEnvOverride).expect("non-empty");
        let cov = full_coverage(&filtered);
        // Only the descendant feature appears; the parent root is NOT pulled in.
        assert!(cov.contains(&Feature::TestFixtureEnvOverride));
        assert!(!cov.contains(&Feature::PseudoFileEnv));
        assert!(!cov.contains(&Feature::TestFixtureExecutableOverride));
    }

    /// Filtering by an unrelated root gives a tree containing none of the
    /// other roots.
    #[test]
    fn filter_disjoint_roots_are_disjoint() {
        let r = root();
        let env = filter_tree(&r, Feature::PseudoFileEnv).expect("non-empty");
        let exe = filter_tree(&r, Feature::PseudoFileExecutable).expect("non-empty");
        let env_cov = full_coverage(&env);
        let exe_cov = full_coverage(&exe);
        assert!(!env_cov.contains(&Feature::PseudoFileExecutable));
        assert!(!exe_cov.contains(&Feature::PseudoFileEnv));
        // Each filtered tree is strictly smaller than the unfiltered tree.
        assert!(count_examples(&env) < count_examples(&r));
        assert!(count_examples(&exe) < count_examples(&r));
    }
}
