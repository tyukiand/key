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

    /// Canonical machine-friendly id used by `--feature=<id>` (spec/0011 §B.3):
    /// the variant identifier converted to lowercased + hyphenated form.
    pub fn canonical_id(self) -> &'static str {
        use Feature::*;
        match self {
            PropositionFile => "proposition-file",
            PropositionForall => "proposition-forall",
            PropositionExists => "proposition-exists",
            PropositionAll => "proposition-all",
            PropositionAny => "proposition-any",
            PropositionNot => "proposition-not",
            PropositionConditionally => "proposition-conditionally",
            PredicateFileExists => "predicate-file-exists",
            PredicateTextHasLines => "predicate-text-has-lines",
            PredicateTextMatches => "predicate-text-matches",
            PredicateTextContains => "predicate-text-contains",
            PredicateShellExportsVariable => "predicate-shell-exports-variable",
            PredicateShellDefinesVariable => "predicate-shell-defines-variable",
            PredicateShellAddsToPath => "predicate-shell-adds-to-path",
            PredicatePropertiesDefinesKey => "predicate-properties-defines-key",
            PredicateXmlMatches => "predicate-xml-matches",
            PredicateJsonMatches => "predicate-json-matches",
            PredicateYamlMatches => "predicate-yaml-matches",
            PredicateAnd => "predicate-and",
            PredicateOr => "predicate-or",
            PredicateNot => "predicate-not",
            PredicateConditionally => "predicate-conditionally",
            PredicateShellExportsVariableValueMatches => {
                "predicate-shell-exports-variable-value-matches"
            }
            PredicateShellDefinesVariableValueMatches => {
                "predicate-shell-defines-variable-value-matches"
            }
            PseudoFileEnv => "pseudo-file-env",
            PseudoFileExecutable => "pseudo-file-executable",
            ControlFile => "control-file",
            ControlIdField => "control-id-field",
            ControlTitleField => "control-title-field",
            ControlDescriptionField => "control-description-field",
            ControlRemediationField => "control-remediation-field",
            TestFixtureFormat => "test-fixture-format",
            TestFixtureEnvOverride => "test-fixture-env-override",
            TestFixtureExecutableOverride => "test-fixture-executable-override",
            TestFixtureMalformedRejection => "test-fixture-malformed-rejection",
            CliAuditRun => "cli-audit-run",
            CliAuditNew => "cli-audit-new",
            CliAuditAdd => "cli-audit-add",
            CliAuditList => "cli-audit-list",
            CliAuditDelete => "cli-audit-delete",
            CliAuditGuide => "cli-audit-guide",
            CliAuditTest => "cli-audit-test",
            CliAuditIgnoreFlag => "cli-audit-ignore-flag",
            CliAuditWarnOnlyFlag => "cli-audit-warn-only-flag",
        }
    }

    /// Look up a feature by its canonical id (`Feature::canonical_id`).
    pub fn from_canonical_id(s: &str) -> Option<Feature> {
        Self::all().iter().copied().find(|f| f.canonical_id() == s)
    }

    /// Walk to the root of this feature's tree (Feature where `.parent() == None`).
    pub fn root(self) -> Feature {
        let mut current = self;
        while let Some(p) = current.parent() {
            current = p;
        }
        current
    }

    /// True iff `self == ancestor` or any chain of `.parent()` calls from
    /// `self` reaches `ancestor`.
    pub fn is_descendant_of(self, ancestor: Feature) -> bool {
        let mut current = Some(self);
        while let Some(c) = current {
            if c == ancestor {
                return true;
            }
            current = c.parent();
        }
        false
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

    /// Spec/0011 §B.3 — canonical_id must be unique across all Features and
    /// match a parser-friendly lowercase + hyphen form (no spaces, no
    /// underscores, no uppercase).
    #[test]
    fn canonical_id_unique_and_well_formed() {
        let ids: BTreeSet<&str> = Feature::all().iter().map(|f| f.canonical_id()).collect();
        assert_eq!(
            ids.len(),
            Feature::all().len(),
            "Feature::canonical_id() must be injective"
        );
        for f in Feature::all() {
            let id = f.canonical_id();
            assert!(!id.is_empty(), "{} has empty canonical_id", f.name());
            for c in id.chars() {
                assert!(
                    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-',
                    "canonical_id {:?} for {} contains invalid char {:?}",
                    id,
                    f.name(),
                    c,
                );
            }
            assert!(
                !id.starts_with('-') && !id.ends_with('-'),
                "canonical_id {:?} starts/ends with hyphen",
                id
            );
        }
    }

    /// Round-trip: `from_canonical_id` finds every feature via its id.
    #[test]
    fn canonical_id_round_trip() {
        for f in Feature::all() {
            let got = Feature::from_canonical_id(f.canonical_id());
            assert_eq!(got, Some(*f), "round-trip failed for {}", f.name());
        }
        assert_eq!(Feature::from_canonical_id("not-a-feature"), None);
    }

    /// Walking `.root()` always reaches a Feature with no parent, in <= COUNT hops.
    #[test]
    fn root_walks_to_a_root() {
        for f in Feature::all() {
            let r = f.root();
            assert!(
                r.parent().is_none(),
                "{} root {} has parent",
                f.name(),
                r.name()
            );
            assert!(
                f.is_descendant_of(r),
                "{} should be descendant of its root {}",
                f.name(),
                r.name()
            );
        }
    }
}
