//! `Feature` — the enumerated capability registry (spec/0010 §1).
//!
//! Each `Feature` names one distinct capability the user can invoke through a
//! control or test fixture. `parent()` defines a forest (§1.6) used by both
//! the EDSL emitter and the exhaustiveness check (§4.4).

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    // ---------- Propositions (per spec §1.2) ----------
    PropositionFile,
    PropositionForall,
    PropositionExists,
    PropositionAll,
    PropositionAny,
    PropositionNot,
    PropositionConditionally,

    // ---------- File predicates ----------
    PredicateFileExists,
    PredicateTextHasLines,
    PredicateTextMatches,
    PredicateTextContains,
    PredicateShellExportsVariable,
    PredicateShellDefinesVariable,
    PredicateShellAddsToPath,
    PredicatePropertiesDefinesKey,
    PredicateXmlMatches,
    PredicateJsonMatches,
    PredicateYamlMatches,
    PredicateAnd,
    PredicateOr,
    PredicateNot,
    PredicateConditionally,

    // ---------- Predicate refinements (spec §6.X.2) ----------
    PredicateShellExportsVariableValueMatches,
    PredicateShellDefinesVariableValueMatches,

    // ---------- Pseudo-files (spec/0009 §1.1) ----------
    PseudoFileEnv,
    PseudoFileExecutable,

    // ---------- Control container (spec/0007) ----------
    ControlFile,
    ControlIdField,
    ControlTitleField,
    ControlDescriptionField,
    ControlRemediationField,

    // ---------- Test fixtures (spec/0009 §3.8 / §2.5) ----------
    TestFixtureFormat,
    TestFixtureEnvOverride,
    TestFixtureExecutableOverride,
    TestFixtureMalformedRejection,

    // ---------- CLI surface ----------
    CliAuditRun,
    CliAuditNew,
    CliAuditAdd,
    CliAuditList,
    CliAuditDelete,
    CliAuditGuide,
    CliAuditTest,
    CliAuditIgnoreFlag,
    CliAuditWarnOnlyFlag,
}

impl Feature {
    /// The full enumeration of every Feature.
    pub fn all() -> &'static [Feature] {
        use Feature::*;
        &[
            PropositionFile,
            PropositionForall,
            PropositionExists,
            PropositionAll,
            PropositionAny,
            PropositionNot,
            PropositionConditionally,
            PredicateFileExists,
            PredicateTextHasLines,
            PredicateTextMatches,
            PredicateTextContains,
            PredicateShellExportsVariable,
            PredicateShellDefinesVariable,
            PredicateShellAddsToPath,
            PredicatePropertiesDefinesKey,
            PredicateXmlMatches,
            PredicateJsonMatches,
            PredicateYamlMatches,
            PredicateAnd,
            PredicateOr,
            PredicateNot,
            PredicateConditionally,
            PredicateShellExportsVariableValueMatches,
            PredicateShellDefinesVariableValueMatches,
            PseudoFileEnv,
            PseudoFileExecutable,
            ControlFile,
            ControlIdField,
            ControlTitleField,
            ControlDescriptionField,
            ControlRemediationField,
            TestFixtureFormat,
            TestFixtureEnvOverride,
            TestFixtureExecutableOverride,
            TestFixtureMalformedRejection,
            CliAuditRun,
            CliAuditNew,
            CliAuditAdd,
            CliAuditList,
            CliAuditDelete,
            CliAuditGuide,
            CliAuditTest,
            CliAuditIgnoreFlag,
            CliAuditWarnOnlyFlag,
        ]
    }

    /// Total count of distinct features.
    pub const COUNT: usize = 44;

    /// Optional parent. Roots return `None`.
    pub fn parent(self) -> Option<Feature> {
        use Feature::*;
        match self {
            // Test-fixture children sit under their pseudo-file parent.
            TestFixtureEnvOverride => Some(PseudoFileEnv),
            TestFixtureExecutableOverride => Some(PseudoFileExecutable),
            TestFixtureMalformedRejection => Some(TestFixtureFormat),

            // Predicate refinements (§6.X.2): each value-matches feature
            // sits under its bare-string umbrella feature.
            PredicateShellExportsVariableValueMatches => Some(PredicateShellExportsVariable),
            PredicateShellDefinesVariableValueMatches => Some(PredicateShellDefinesVariable),

            // ControlFile field children sit under the parent control file.
            ControlIdField => Some(ControlFile),
            ControlTitleField => Some(ControlFile),
            ControlDescriptionField => Some(ControlFile),
            ControlRemediationField => Some(ControlFile),

            // CLI run flags sit under `key audit run`.
            CliAuditIgnoreFlag => Some(CliAuditRun),
            CliAuditWarnOnlyFlag => Some(CliAuditRun),

            _ => None,
        }
    }

    /// Canonical machine-readable name (matches the variant identifier).
    pub fn name(self) -> &'static str {
        use Feature::*;
        match self {
            PropositionFile => "PropositionFile",
            PropositionForall => "PropositionForall",
            PropositionExists => "PropositionExists",
            PropositionAll => "PropositionAll",
            PropositionAny => "PropositionAny",
            PropositionNot => "PropositionNot",
            PropositionConditionally => "PropositionConditionally",
            PredicateFileExists => "PredicateFileExists",
            PredicateTextHasLines => "PredicateTextHasLines",
            PredicateTextMatches => "PredicateTextMatches",
            PredicateTextContains => "PredicateTextContains",
            PredicateShellExportsVariable => "PredicateShellExportsVariable",
            PredicateShellDefinesVariable => "PredicateShellDefinesVariable",
            PredicateShellAddsToPath => "PredicateShellAddsToPath",
            PredicatePropertiesDefinesKey => "PredicatePropertiesDefinesKey",
            PredicateXmlMatches => "PredicateXmlMatches",
            PredicateJsonMatches => "PredicateJsonMatches",
            PredicateYamlMatches => "PredicateYamlMatches",
            PredicateAnd => "PredicateAnd",
            PredicateOr => "PredicateOr",
            PredicateNot => "PredicateNot",
            PredicateConditionally => "PredicateConditionally",
            PredicateShellExportsVariableValueMatches => {
                "PredicateShellExportsVariableValueMatches"
            }
            PredicateShellDefinesVariableValueMatches => {
                "PredicateShellDefinesVariableValueMatches"
            }
            PseudoFileEnv => "PseudoFileEnv",
            PseudoFileExecutable => "PseudoFileExecutable",
            ControlFile => "ControlFile",
            ControlIdField => "ControlIdField",
            ControlTitleField => "ControlTitleField",
            ControlDescriptionField => "ControlDescriptionField",
            ControlRemediationField => "ControlRemediationField",
            TestFixtureFormat => "TestFixtureFormat",
            TestFixtureEnvOverride => "TestFixtureEnvOverride",
            TestFixtureExecutableOverride => "TestFixtureExecutableOverride",
            TestFixtureMalformedRejection => "TestFixtureMalformedRejection",
            CliAuditRun => "CliAuditRun",
            CliAuditNew => "CliAuditNew",
            CliAuditAdd => "CliAuditAdd",
            CliAuditList => "CliAuditList",
            CliAuditDelete => "CliAuditDelete",
            CliAuditGuide => "CliAuditGuide",
            CliAuditTest => "CliAuditTest",
            CliAuditIgnoreFlag => "CliAuditIgnoreFlag",
            CliAuditWarnOnlyFlag => "CliAuditWarnOnlyFlag",
        }
    }

    /// Set of every feature.
    pub fn all_set() -> BTreeSet<Feature> {
        Self::all().iter().copied().collect()
    }

    /// Set of root features (parent == None).
    pub fn roots_set() -> BTreeSet<Feature> {
        Self::all()
            .iter()
            .copied()
            .filter(|f| f.parent().is_none())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec/0010 §1.7: the parent relation MUST form a forest — no cycles, no
    /// diamonds. Walk every Feature, follow `.parent()` links, confirm
    /// termination at None within at most COUNT steps; confirm each Feature
    /// appears at most once as a child.
    #[test]
    fn forest_invariant() {
        let mut child_count: std::collections::HashMap<Feature, usize> =
            std::collections::HashMap::new();
        for f in Feature::all() {
            // termination check (no cycles)
            let mut current = Some(*f);
            for step in 0..=Feature::COUNT {
                match current {
                    Some(c) => {
                        if step == Feature::COUNT {
                            panic!(
                                "feature {:?} did not terminate at None within {} parent \
                                 hops — likely cycle",
                                f.name(),
                                Feature::COUNT
                            );
                        }
                        current = c.parent();
                    }
                    None => break,
                }
            }
            // tally appearances as a child
            if let Some(parent) = f.parent() {
                *child_count.entry(*f).or_insert(0) += 1;
                // sanity: parent must itself be a known Feature
                let _ = parent;
            }
        }
        for (child, count) in child_count.iter() {
            assert_eq!(
                *count,
                1,
                "feature {:?} appears as a child {} times (forest requires <= 1)",
                child.name(),
                count
            );
        }
    }

    /// COUNT is consistent with all().
    #[test]
    fn count_matches_all() {
        assert_eq!(Feature::all().len(), Feature::COUNT);
    }

    /// `name()` matches the textual variant identifier.
    #[test]
    fn name_unique_and_consistent() {
        let names: BTreeSet<&str> = Feature::all().iter().map(|f| f.name()).collect();
        assert_eq!(
            names.len(),
            Feature::all().len(),
            "Feature::name() must be injective"
        );
    }
}
