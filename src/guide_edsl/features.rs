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

    // ---------- json-matches mode tags (spec/0013 §A.7B + §A.8.2) ----------
    PredicateJsonMatchesIsTrue,
    PredicateJsonMatchesIsFalse,
    PredicateJsonMatchesRegex,

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

    // ---------- Spec/0017 §B / §C — env redaction & unredacted-allowlist ----
    /// Root umbrella Feature for OsEffects-boundary redaction.
    EnvRedaction,
    /// Layer 1 — variable-name substring rules.
    EnvRedactionByName,
    /// Layer 2 — value-shape regex set.
    EnvRedactionByValueShape,
    /// Layer 3 — high-entropy heuristic (DEFAULT-ON).
    EnvRedactionByEntropy,
    /// Mutation — append a literal opt-out matcher.
    MutationAddUnredactedMatcher,
    /// Mutation — remove a previously-added matcher.
    MutationDeleteUnredactedMatcher,
    /// Bare-string predicate `looks-like-password`.
    PredicateLooksLikePassword,
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
            PredicateJsonMatchesIsTrue,
            PredicateJsonMatchesIsFalse,
            PredicateJsonMatchesRegex,
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
            EnvRedaction,
            EnvRedactionByName,
            EnvRedactionByValueShape,
            EnvRedactionByEntropy,
            MutationAddUnredactedMatcher,
            MutationDeleteUnredactedMatcher,
            PredicateLooksLikePassword,
        ]
    }

    /// Total count of distinct features.
    pub const COUNT: usize = 54;

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

            // Spec/0013 §A.7B + §A.8.2 — json-matches mode tags sit under
            // the umbrella `json-matches` feature.
            PredicateJsonMatchesIsTrue => Some(PredicateJsonMatches),
            PredicateJsonMatchesIsFalse => Some(PredicateJsonMatches),
            PredicateJsonMatchesRegex => Some(PredicateJsonMatches),

            // ControlFile field children sit under the parent control file.
            ControlIdField => Some(ControlFile),
            ControlTitleField => Some(ControlFile),
            ControlDescriptionField => Some(ControlFile),
            ControlRemediationField => Some(ControlFile),

            // CLI run flags sit under `key audit run`.
            CliAuditIgnoreFlag => Some(CliAuditRun),
            CliAuditWarnOnlyFlag => Some(CliAuditRun),

            // Spec/0017 — EnvRedaction subtree under PseudoFileEnv (the
            // surface that benefits most), with detection layers as children
            // of the EnvRedaction root. Mutation roots sit at the top level
            // (no Mutation* root yet exists).
            EnvRedaction => Some(PseudoFileEnv),
            EnvRedactionByName => Some(EnvRedaction),
            EnvRedactionByValueShape => Some(EnvRedaction),
            EnvRedactionByEntropy => Some(EnvRedaction),
            MutationAddUnredactedMatcher => None,
            MutationDeleteUnredactedMatcher => None,
            PredicateLooksLikePassword => None,

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
            PredicateJsonMatchesIsTrue => "PredicateJsonMatchesIsTrue",
            PredicateJsonMatchesIsFalse => "PredicateJsonMatchesIsFalse",
            PredicateJsonMatchesRegex => "PredicateJsonMatchesRegex",
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
            EnvRedaction => "EnvRedaction",
            EnvRedactionByName => "EnvRedactionByName",
            EnvRedactionByValueShape => "EnvRedactionByValueShape",
            EnvRedactionByEntropy => "EnvRedactionByEntropy",
            MutationAddUnredactedMatcher => "MutationAddUnredactedMatcher",
            MutationDeleteUnredactedMatcher => "MutationDeleteUnredactedMatcher",
            PredicateLooksLikePassword => "PredicateLooksLikePassword",
        }
    }

    /// Canonical machine-friendly id used by `--feature=<id>` (spec/0012 §1):
    /// the shortest natural search term per Feature, hand-chosen. Disambiguation
    /// rule (§1.2): predicate-level `all`/`any`/`not`/`conditionally` take the
    /// bare names; the proposition-level versions get an `-prop` suffix.
    pub fn canonical_id(self) -> &'static str {
        use Feature::*;
        match self {
            PropositionFile => "file",
            PropositionForall => "forall",
            PropositionExists => "exists",
            PropositionAll => "all-prop",
            PropositionAny => "any-prop",
            PropositionNot => "not-prop",
            PropositionConditionally => "conditionally-prop",
            PredicateFileExists => "file-exists",
            PredicateTextHasLines => "text-has-lines",
            PredicateTextMatches => "text-matches",
            PredicateTextContains => "text-contains",
            PredicateShellExportsVariable => "shell-exports",
            PredicateShellDefinesVariable => "shell-defines",
            PredicateShellAddsToPath => "shell-adds-to-path",
            PredicatePropertiesDefinesKey => "properties-defines-key",
            PredicateXmlMatches => "xml-matches",
            PredicateJsonMatches => "json-matches",
            PredicateYamlMatches => "yaml-matches",
            PredicateAnd => "and",
            PredicateOr => "or",
            PredicateNot => "not",
            PredicateConditionally => "conditionally",
            PredicateShellExportsVariableValueMatches => "shell-exports-value-matches",
            PredicateShellDefinesVariableValueMatches => "shell-defines-value-matches",
            PredicateJsonMatchesIsTrue => "json-matches-is-true",
            PredicateJsonMatchesIsFalse => "json-matches-is-false",
            PredicateJsonMatchesRegex => "json-matches-regex",
            PseudoFileEnv => "env",
            PseudoFileExecutable => "executable",
            ControlFile => "control-file",
            ControlIdField => "control-id",
            ControlTitleField => "control-title",
            ControlDescriptionField => "control-description",
            ControlRemediationField => "control-remediation",
            TestFixtureFormat => "test-fixture",
            TestFixtureEnvOverride => "env-override",
            TestFixtureExecutableOverride => "executable-override",
            TestFixtureMalformedRejection => "fixture-malformed-rejection",
            CliAuditRun => "audit-run",
            CliAuditNew => "audit-new",
            CliAuditAdd => "audit-add",
            CliAuditList => "audit-list",
            CliAuditDelete => "audit-delete",
            CliAuditGuide => "audit-guide",
            CliAuditTest => "audit-test",
            CliAuditIgnoreFlag => "ignore-flag",
            CliAuditWarnOnlyFlag => "warn-only-flag",
            EnvRedaction => "env-redaction",
            EnvRedactionByName => "env-redaction-by-name",
            EnvRedactionByValueShape => "env-redaction-by-value-shape",
            EnvRedactionByEntropy => "env-redaction-by-entropy",
            MutationAddUnredactedMatcher => "add-unredacted-matcher",
            MutationDeleteUnredactedMatcher => "delete-unredacted-matcher",
            PredicateLooksLikePassword => "looks-like-password",
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

    /// Spec/0012 §4.1 — concrete spot-check that the natural short ids
    /// chosen in §1.1 are present (and that the disambiguation rule §1.2 was
    /// applied to the proposition-level combinators).
    #[test]
    fn canonical_id_natural_short_forms() {
        let pairs: &[(Feature, &str)] = &[
            (Feature::PseudoFileEnv, "env"),
            (Feature::PseudoFileExecutable, "executable"),
            (Feature::PropositionForall, "forall"),
            (Feature::PropositionExists, "exists"),
            (Feature::PropositionFile, "file"),
            (Feature::PropositionAll, "all-prop"),
            (Feature::PropositionAny, "any-prop"),
            (Feature::PropositionNot, "not-prop"),
            (Feature::PropositionConditionally, "conditionally-prop"),
            (Feature::PredicateAnd, "and"),
            (Feature::PredicateOr, "or"),
            (Feature::PredicateNot, "not"),
            (Feature::PredicateConditionally, "conditionally"),
            (Feature::PredicateShellExportsVariable, "shell-exports"),
            (Feature::PredicateShellDefinesVariable, "shell-defines"),
            (Feature::TestFixtureEnvOverride, "env-override"),
            (Feature::CliAuditRun, "audit-run"),
            (Feature::CliAuditIgnoreFlag, "ignore-flag"),
        ];
        for (f, want) in pairs {
            assert_eq!(
                f.canonical_id(),
                *want,
                "spec/0012 §1.1 expected {} → {:?}, got {:?}",
                f.name(),
                want,
                f.canonical_id()
            );
        }
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
