//! EDSL node types (spec/0010 §3).
//!
//! Each node is a piece of the guide. `detail: bool` controls whether the
//! terse interpretation (§4.1.a) emits the node verbatim or replaces it with
//! a "rerun -v" pointer line. `terse_summary` is required on every
//! `detail: true` node so the pointer line names what was elided.

use super::features::Feature;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExampleExpect {
    /// The control passes when evaluated against the embedded fixture.
    Pass,
    /// The control evaluates to a failure with at least one error message.
    Fail,
    /// The control YAML is malformed and fails to load (parser error).
    LoadError,
}

#[derive(Debug, Clone)]
pub enum GuideNode {
    /// A heading with a body of child nodes.
    Section {
        title: &'static str,
        body: Vec<GuideNode>,
        detail: bool,
        terse_summary: Option<&'static str>,
    },
    /// Free-form prose paragraph.
    Prose {
        text: &'static str,
        detail: bool,
        terse_summary: Option<&'static str>,
    },
    /// A worked control example. Carries the exact YAML used in the guide
    /// AND used to drive the materialized-project test (§4.2).
    ExampleControl {
        feature: Feature,
        yaml: &'static str,
        expect: ExampleExpect,
        detail: bool,
        terse_summary: Option<&'static str>,
    },
    /// A worked test-fixture example (e.g. `<env>` override YAML).
    ExampleFixture {
        feature: Feature,
        /// Stable name used as the materialized fixture filename.
        name: &'static str,
        yaml: &'static str,
        detail: bool,
        terse_summary: Option<&'static str>,
    },
    /// A bare reference to a Feature (e.g. "see also: shell-defines"). Used
    /// to claim a Feature for coverage without a worked example.
    FeatureRef {
        feature: Feature,
        blurb: &'static str,
        detail: bool,
        terse_summary: Option<&'static str>,
    },
}

impl GuideNode {
    pub fn detail(&self) -> bool {
        match self {
            GuideNode::Section { detail, .. }
            | GuideNode::Prose { detail, .. }
            | GuideNode::ExampleControl { detail, .. }
            | GuideNode::ExampleFixture { detail, .. }
            | GuideNode::FeatureRef { detail, .. } => *detail,
        }
    }

    /// Short label used in the "rerun -v" pointer line (§4.1.a). Required on
    /// detail nodes; for non-detail nodes, may be `None`.
    pub fn terse_summary(&self) -> Option<&'static str> {
        match self {
            GuideNode::Section { terse_summary, .. }
            | GuideNode::Prose { terse_summary, .. }
            | GuideNode::ExampleControl { terse_summary, .. }
            | GuideNode::ExampleFixture { terse_summary, .. }
            | GuideNode::FeatureRef { terse_summary, .. } => *terse_summary,
        }
    }

    /// Walk every Feature claimed within this subtree. Order is depth-first.
    pub fn walk_features(&self, visit: &mut dyn FnMut(Feature, bool /*detail*/)) {
        self.walk_features_inner(false, visit);
    }

    fn walk_features_inner(&self, ancestor_detail: bool, visit: &mut dyn FnMut(Feature, bool)) {
        let here_detail = ancestor_detail || self.detail();
        match self {
            GuideNode::Section { body, .. } => {
                for child in body {
                    child.walk_features_inner(here_detail, visit);
                }
            }
            GuideNode::ExampleControl { feature, .. }
            | GuideNode::ExampleFixture { feature, .. }
            | GuideNode::FeatureRef { feature, .. } => {
                visit(*feature, here_detail);
            }
            GuideNode::Prose { .. } => {}
        }
    }
}
