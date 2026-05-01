//! `FeatureBearing` — the AST → Feature mapping (spec/0010 §2.1).
//!
//! Every AST variant claims one or more `Feature`s. The uniqueness +
//! completeness invariant (§1.4 + §1.5) is enforced by a unit test that
//! aggregates the features across every variant and asserts the multiset
//! has no duplicates AND equals `Feature::all_set()`.

use super::features::Feature;
use crate::project::ProjectMutation;
use crate::rules::ast::{
    Control, ControlFile, DataSchema, FilePredicateAst, Proposition, PseudoFile, PseudoFileFixture,
};
use crate::security::redact::RedactionCtx;

/// Implement on every AST variant that participates in feature coverage.
pub trait FeatureBearing {
    /// Return the canonical `Feature`s claimed by this variant. Most variants
    /// return a single-element slice; container types (`Control`,
    /// `PseudoFileFixture`) bundle their field-features into one slice.
    fn features(&self) -> &'static [Feature];
}

// -----------------------------------------------------------------------------
// Proposition variants (7 features)
// -----------------------------------------------------------------------------

impl FeatureBearing for Proposition {
    fn features(&self) -> &'static [Feature] {
        match self {
            Proposition::FileSatisfies { .. } => &[Feature::PropositionFile],
            Proposition::Forall { .. } => &[Feature::PropositionForall],
            Proposition::Exists { .. } => &[Feature::PropositionExists],
            Proposition::All(_) => &[Feature::PropositionAll],
            Proposition::Any(_) => &[Feature::PropositionAny],
            Proposition::Not(_) => &[Feature::PropositionNot],
            Proposition::Conditionally { .. } => &[Feature::PropositionConditionally],
        }
    }
}

// -----------------------------------------------------------------------------
// FilePredicateAst variants (16 features incl. value-matches refinements)
// -----------------------------------------------------------------------------

impl FeatureBearing for FilePredicateAst {
    fn features(&self) -> &'static [Feature] {
        match self {
            FilePredicateAst::FileExists => &[Feature::PredicateFileExists],
            FilePredicateAst::TextHasLines { .. } => &[Feature::PredicateTextHasLines],
            FilePredicateAst::TextMatchesRegex(_) => &[Feature::PredicateTextMatches],
            FilePredicateAst::TextContains(_) => &[Feature::PredicateTextContains],
            FilePredicateAst::ShellExports(_) => &[Feature::PredicateShellExportsVariable],
            FilePredicateAst::ShellExportsValueMatches { .. } => {
                &[Feature::PredicateShellExportsVariableValueMatches]
            }
            FilePredicateAst::ShellDefinesVariable(_) => &[Feature::PredicateShellDefinesVariable],
            FilePredicateAst::ShellDefinesVariableValueMatches { .. } => {
                &[Feature::PredicateShellDefinesVariableValueMatches]
            }
            FilePredicateAst::ShellAddsToPath(_) => &[Feature::PredicateShellAddsToPath],
            FilePredicateAst::PropertiesDefinesKey(_) => &[Feature::PredicatePropertiesDefinesKey],
            FilePredicateAst::XmlMatchesPath(_) => &[Feature::PredicateXmlMatches],
            FilePredicateAst::JsonMatches(_) => &[Feature::PredicateJsonMatches],
            FilePredicateAst::YamlMatches(_) => &[Feature::PredicateYamlMatches],
            FilePredicateAst::LooksLikePassword => &[Feature::PredicateLooksLikePassword],
            FilePredicateAst::All(_) => &[Feature::PredicateAnd],
            FilePredicateAst::Any { .. } => &[Feature::PredicateOr],
            FilePredicateAst::Not(_) => &[Feature::PredicateNot],
            FilePredicateAst::Conditionally { .. } => &[Feature::PredicateConditionally],
        }
    }
}

// -----------------------------------------------------------------------------
// DataSchema mode-tag variants (spec/0013 §A.7B + §A.8.2)
//
// json-matches modes claim Features for documentation drill-down. Most
// schemas (is-string, is-object, ...) don't have their own Features in
// this iteration; only the ones spec/0013 calls out explicitly do.
// Returns &[] for modes without a dedicated Feature claim.
// -----------------------------------------------------------------------------

impl FeatureBearing for DataSchema {
    fn features(&self) -> &'static [Feature] {
        match self {
            DataSchema::IsTrue => &[Feature::PredicateJsonMatchesIsTrue],
            DataSchema::IsFalse => &[Feature::PredicateJsonMatchesIsFalse],
            DataSchema::IsStringMatching(_) => &[Feature::PredicateJsonMatchesRegex],
            _ => &[],
        }
    }
}

// -----------------------------------------------------------------------------
// PseudoFile variants (2 features)
// -----------------------------------------------------------------------------

impl FeatureBearing for PseudoFile {
    fn features(&self) -> &'static [Feature] {
        match self {
            PseudoFile::Env => &[Feature::PseudoFileEnv],
            PseudoFile::Executable(_) => &[Feature::PseudoFileExecutable],
        }
    }
}

// -----------------------------------------------------------------------------
// ControlFile + Control fields (5 features)
// -----------------------------------------------------------------------------

impl FeatureBearing for ControlFile {
    fn features(&self) -> &'static [Feature] {
        &[Feature::ControlFile]
    }
}

impl FeatureBearing for Control {
    fn features(&self) -> &'static [Feature] {
        &[
            Feature::ControlIdField,
            Feature::ControlTitleField,
            Feature::ControlDescriptionField,
            Feature::ControlRemediationField,
        ]
    }
}

// -----------------------------------------------------------------------------
// PseudoFileFixture (test-fixture features, 4)
// -----------------------------------------------------------------------------

impl FeatureBearing for PseudoFileFixture {
    fn features(&self) -> &'static [Feature] {
        &[
            Feature::TestFixtureFormat,
            Feature::TestFixtureEnvOverride,
            Feature::TestFixtureExecutableOverride,
            Feature::TestFixtureMalformedRejection,
        ]
    }
}

// -----------------------------------------------------------------------------
// Spec/0017 — env redaction kernel (4 features) and project-mutation surface
// for the unredacted-allowlist (2 features).
//
// `RedactionCtx` is the configuration handle that the OsEffects boundary
// holds. By construction it ALWAYS represents all three detection layers
// plus the umbrella, so it bundles the four EnvRedaction* features.
// `ProjectMutation` is the existing mutation-op alphabet; the two
// unredacted-allowlist arms claim the matching Mutation* features.
// -----------------------------------------------------------------------------

impl FeatureBearing for RedactionCtx {
    fn features(&self) -> &'static [Feature] {
        &[
            Feature::EnvRedaction,
            Feature::EnvRedactionByName,
            Feature::EnvRedactionByValueShape,
            Feature::EnvRedactionByEntropy,
        ]
    }
}

impl FeatureBearing for ProjectMutation {
    fn features(&self) -> &'static [Feature] {
        match self {
            ProjectMutation::AddUnredactedMatcher { .. } => {
                &[Feature::MutationAddUnredactedMatcher]
            }
            ProjectMutation::DeleteUnredactedMatcher { .. } => {
                &[Feature::MutationDeleteUnredactedMatcher]
            }
            // Other mutation arms predate spec/0017 and are not part of the
            // documented Feature surface; they return an empty slice.
            _ => &[],
        }
    }
}

// -----------------------------------------------------------------------------
// CLI surface (9 features) — represented as a tagged enum so the test below
// can walk every variant the way it does for AST variants.
// -----------------------------------------------------------------------------

/// CLI feature carriers — one variant per documented `key audit` subcommand.
/// `Run` claims its two flag-children (ignore, warn-only) too.
#[derive(Debug, Clone, Copy)]
pub enum CliCommandFeature {
    Run,
    New,
    Add,
    List,
    Delete,
    Guide,
    Test,
}

impl FeatureBearing for CliCommandFeature {
    fn features(&self) -> &'static [Feature] {
        match self {
            CliCommandFeature::Run => &[
                Feature::CliAuditRun,
                Feature::CliAuditIgnoreFlag,
                Feature::CliAuditWarnOnlyFlag,
            ],
            CliCommandFeature::New => &[Feature::CliAuditNew],
            CliCommandFeature::Add => &[Feature::CliAuditAdd],
            CliCommandFeature::List => &[Feature::CliAuditList],
            CliCommandFeature::Delete => &[Feature::CliAuditDelete],
            CliCommandFeature::Guide => &[Feature::CliAuditGuide],
            CliCommandFeature::Test => &[Feature::CliAuditTest],
        }
    }
}

/// Walk every CLI command variant.
pub fn all_cli_command_variants() -> Vec<CliCommandFeature> {
    vec![
        CliCommandFeature::Run,
        CliCommandFeature::New,
        CliCommandFeature::Add,
        CliCommandFeature::List,
        CliCommandFeature::Delete,
        CliCommandFeature::Guide,
        CliCommandFeature::Test,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::{
        all_data_schema_variants, all_predicate_variants, all_proposition_variants, Control,
        ControlFile, ExecutableSnapshot, FilePredicateAst, Proposition, PseudoFile,
        PseudoFileFixture, SimplePath,
    };

    fn one_control() -> Control {
        Control {
            id: "X".into(),
            title: "t".into(),
            description: "d".into(),
            remediation: "r".into(),
            check: Proposition::FileSatisfies {
                path: SimplePath::new("~/x").unwrap(),
                check: FilePredicateAst::FileExists,
            },
        }
    }

    /// Spec/0010 §1.4 + §1.5 — uniqueness AND completeness of the AST →
    /// Feature mapping. Walk every AST variant, aggregate features into a
    /// multiset, assert no duplicates AND that the resulting set equals
    /// `Feature::all_set()`.
    #[test]
    fn uniqueness_and_completeness() {
        let mut multiset: Vec<Feature> = Vec::new();

        // Propositions
        for p in all_proposition_variants() {
            multiset.extend_from_slice(p.features());
        }
        // Predicates
        for p in all_predicate_variants() {
            multiset.extend_from_slice(p.features());
        }
        // Spec/0013 §A.7B + §A.8.2 — DataSchema mode-tag variants claim
        // their own Features (is-true / is-false / is-string-matching for
        // PredicateJsonMatchesRegex).
        for s in all_data_schema_variants() {
            multiset.extend_from_slice(s.features());
        }
        // Pseudo-files (both variants)
        multiset.extend_from_slice(PseudoFile::Env.features());
        multiset.extend_from_slice(PseudoFile::Executable("docker".into()).features());
        // ControlFile + Control fields
        let cf = ControlFile {
            controls: vec![one_control()],
        };
        multiset.extend_from_slice(cf.features());
        multiset.extend_from_slice(one_control().features());
        // Test fixture (claims TestFixture features)
        let fixture = PseudoFileFixture {
            executable_override: Some(
                std::collections::BTreeMap::<String, ExecutableSnapshot>::new(),
            ),
        };
        multiset.extend_from_slice(fixture.features());
        // CLI surface
        for c in all_cli_command_variants() {
            multiset.extend_from_slice(c.features());
        }
        // Spec/0017 — redaction kernel (1 representative ctx claims all 4
        // EnvRedaction* features) + project-mutation arms for the
        // unredacted-allowlist.
        let ctx = RedactionCtx::empty();
        multiset.extend_from_slice(ctx.features());
        for op in &[
            ProjectMutation::AddUnredactedMatcher {
                matcher: crate::security::unredacted::UnredactedMatcher::value("x").unwrap(),
            },
            ProjectMutation::DeleteUnredactedMatcher {
                matcher: crate::security::unredacted::UnredactedMatcher::value("x").unwrap(),
            },
        ] {
            multiset.extend_from_slice(op.features());
        }

        // Uniqueness: no feature claimed twice.
        let mut seen: std::collections::BTreeMap<Feature, usize> =
            std::collections::BTreeMap::new();
        for f in &multiset {
            *seen.entry(*f).or_insert(0) += 1;
        }
        let dups: Vec<_> = seen.iter().filter(|(_, c)| **c > 1).collect();
        assert!(
            dups.is_empty(),
            "Features claimed by more than one AST node: {:?}",
            dups.iter().map(|(f, c)| (f.name(), c)).collect::<Vec<_>>()
        );

        // Completeness: every Feature has a claimant.
        let claimed: std::collections::BTreeSet<Feature> = multiset.iter().copied().collect();
        let all = Feature::all_set();
        let missing: Vec<&str> = all.difference(&claimed).map(|f| f.name()).collect();
        assert!(
            missing.is_empty(),
            "Features declared but not claimed by any AST node: {:?}",
            missing
        );
        let extra: Vec<&str> = claimed.difference(&all).map(|f| f.name()).collect();
        assert!(
            extra.is_empty(),
            "Features claimed but not declared: {:?}",
            extra
        );
    }
}
