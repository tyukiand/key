//! The complete guide tree — single source of truth for what the audit
//! guide documents (spec/0010 §3.4).
//!
//! Every Feature is claimed by exactly one node here. The exhaustiveness
//! test (§4.4) enforces both directions:
//!  - every Feature appears in some EDSL node (no orphan features),
//!  - every claimed Feature exists in the registry (no orphan EDSL nodes).

use super::features::Feature;
use super::nodes::{ExampleExpect, GuideNode};

/// Build the complete guide tree.
pub fn root() -> GuideNode {
    GuideNode::Section {
        title: "key audit \u{2014} Guide",
        detail: false,
        terse_summary: None,
        body: vec![
            intro_section(),
            cli_section(),
            propositions_section(),
            predicates_section(),
            pseudo_files_section(),
            test_fixtures_section(),
            control_file_section(),
        ],
    }
}

fn intro_section() -> GuideNode {
    GuideNode::Section {
        title: "Overview",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::Prose {
                text: "`key audit` evaluates user-defined YAML audit files \
                       against the filesystem (or against pseudo-files such \
                       as `<env>` and `<executable:NAME>`). Each audit file \
                       contains a list of **controls** \u{2014} named checks \
                       with an ID, title, description, remediation, and a \
                       check proposition.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::Prose {
                text: "All file paths in controls use `~/...` notation, which \
                       resolves to `$HOME` in `run` mode or to a fake home \
                       directory in `test` mode. Pseudo-file identifiers \
                       (`<...>`) are subjects that are NOT files on disk; see \
                       the pseudo-files section.",
                detail: false,
                terse_summary: None,
            },
        ],
    }
}

// -----------------------------------------------------------------------------
// CLI surface (claims 9 features)
// -----------------------------------------------------------------------------

fn cli_section() -> GuideNode {
    GuideNode::Section {
        title: "CLI subcommands",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::FeatureRef {
                feature: Feature::CliAuditRun,
                blurb: "`key audit run --file <audit.yaml>` \u{2014} evaluate every control \
                        against your real `$HOME`. Exits non-zero on any failure.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditIgnoreFlag,
                blurb: "`--ignore <ID>` (repeatable) \u{2014} skip the named control entirely. \
                        Useful for environments where a control is known to fail and \
                        you accept the risk.",
                detail: true,
                terse_summary: Some("--ignore flag"),
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditWarnOnlyFlag,
                blurb: "`--warn-only <ID>` (repeatable) \u{2014} run the control but \
                        downgrade failures to warnings (does not affect exit code).",
                detail: true,
                terse_summary: Some("--warn-only flag"),
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditNew,
                blurb: "`key audit new <path>` \u{2014} create a new empty audit file.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditAdd,
                blurb: "`key audit add <path>` \u{2014} interactively add a control to an \
                        existing audit file.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditList,
                blurb: "`key audit list <path>` \u{2014} list controls; `--short` for \
                        ID/title only.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditDelete,
                blurb: "`key audit delete --file <path> [--id <ID>]` \u{2014} remove a \
                        control. Interactive picker if `--id` is omitted.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditGuide,
                blurb: "`key audit guide` \u{2014} print this guide. Pass `-v` / \
                        `--verbose` for the full version with rationale and \
                        test-mechanics detail.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::CliAuditTest,
                blurb: "`key audit test <audit.yaml> <fake-home>` \u{2014} evaluate \
                        controls against a fixture directory. Supports \
                        `--expect-failures <N>` and `--expect-failure-message <msg>`.",
                detail: false,
                terse_summary: None,
            },
        ],
    }
}

// -----------------------------------------------------------------------------
// Propositions (claims 7 features)
// -----------------------------------------------------------------------------

fn propositions_section() -> GuideNode {
    GuideNode::Section {
        title: "Propositions",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::Prose {
                text: "A proposition selects a subject (a file, list of files, or a \
                       sub-proposition) and applies a check to it. A control's `check:` \
                       field is always a proposition.",
                detail: false,
                terse_summary: None,
            },
            // Canonical inline example (spec/0011 §A.1) — simplest form.
            GuideNode::ExampleControl {
                feature: Feature::PropositionFile,
                expect: ExampleExpect::Pass,
                detail: false,
                terse_summary: None,
                yaml: "controls:\n  - id: FILE-EXAMPLE\n    title: ssh config exists\n    description: file proposition example\n    remediation: create the file\n    check:\n      file:\n        path: ~/.ssh/config\n        check: file-exists\n",
            },
            // Other proposition variants — terse one-liner blurbs (root coverage),
            // full YAML examples are detail (verbose pass / --feature filter).
            GuideNode::FeatureRef {
                feature: Feature::PropositionForall,
                blurb: "`forall:` \u{2014} every file in `files:` must satisfy the inner check.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PropositionExists,
                blurb: "`exists:` \u{2014} at least one file in `files:` must satisfy the inner check.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PropositionAll,
                blurb: "`all:` \u{2014} every sub-proposition must hold (proposition-level conjunction).",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PropositionAny,
                blurb: "`any:` \u{2014} at least one sub-proposition must hold (proposition-level disjunction).",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PropositionNot,
                blurb: "`not:` \u{2014} negates the inner proposition.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PropositionConditionally,
                blurb: "`conditionally: { if:, then: }` \u{2014} proposition-level guarded check.",
                detail: false,
                terse_summary: None,
            },
            // Detail YAML examples (verbose pass / --feature drill-in).
            GuideNode::ExampleControl {
                feature: Feature::PropositionForall,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("forall worked example"),
                yaml: "controls:\n  - id: FORALL-EXAMPLE\n    title: every shell rc exports JAVA_HOME\n    description: forall proposition example\n    remediation: add the export to each rc file\n    check:\n      forall:\n        files:\n          - ~/.bashrc\n          - ~/.zshrc\n        check:\n          shell-exports: JAVA_HOME\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PropositionExists,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("exists worked example"),
                yaml: "controls:\n  - id: EXISTS-EXAMPLE\n    title: at least one rc file exports JAVA_HOME\n    description: exists proposition example\n    remediation: add the export to one rc file\n    check:\n      exists:\n        files:\n          - ~/.bash_profile\n          - ~/.profile\n          - ~/.zshrc\n        check:\n          shell-exports: JAVA_HOME\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PropositionAll,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("all-prop worked example"),
                yaml: "controls:\n  - id: ALL-PROP-EXAMPLE\n    title: bashrc and zshrc both exist\n    description: all-proposition example\n    remediation: create both files\n    check:\n      all:\n        - file:\n            path: ~/.bashrc\n            check: file-exists\n        - file:\n            path: ~/.zshrc\n            check: file-exists\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PropositionAny,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("any-prop worked example"),
                yaml: "controls:\n  - id: ANY-PROP-EXAMPLE\n    title: at least one ssh public key present\n    description: any-proposition example\n    remediation: generate at least one key\n    check:\n      any:\n        - file:\n            path: ~/.ssh/id_ed25519.pub\n            check: file-exists\n        - file:\n            path: ~/.ssh/id_rsa.pub\n            check: file-exists\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PropositionNot,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("not-prop worked example"),
                yaml: "controls:\n  - id: NOT-PROP-EXAMPLE\n    title: dsa key absent\n    description: negation example\n    remediation: remove the dsa key\n    check:\n      not:\n        file:\n          path: ~/.ssh/id_dsa.pub\n          check: file-exists\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PropositionConditionally,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("conditionally-prop worked example"),
                yaml: "controls:\n  - id: COND-PROP-EXAMPLE\n    title: if .npmrc exists, registry must be set\n    description: conditional proposition example\n    remediation: add the registry= line\n    check:\n      conditionally:\n        if:\n          file:\n            path: ~/.npmrc\n            check: file-exists\n        then:\n          file:\n            path: ~/.npmrc\n            check:\n              text-matches: \"^registry=\"\n",
            },
        ],
    }
}

// -----------------------------------------------------------------------------
// Predicates (claims 14 + 2 features)
// -----------------------------------------------------------------------------

fn predicates_section() -> GuideNode {
    GuideNode::Section {
        title: "Predicates",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::Prose {
                text: "Predicates appear inside `check:`. Most check the contents \
                       (or existence) of a single file or pseudo-file subject.",
                detail: false,
                terse_summary: None,
            },
            // Canonical inline example (spec/0011 §A.1) — simplest form.
            GuideNode::ExampleControl {
                feature: Feature::PredicateFileExists,
                expect: ExampleExpect::Pass,
                detail: false,
                terse_summary: None,
                yaml: "controls:\n  - id: FILE-EXISTS-EX\n    title: file-exists predicate\n    description: simplest predicate, can appear as a bare string\n    remediation: create the file\n    check:\n      file:\n        path: ~/.ssh/config\n        check: file-exists\n",
            },
            // Other predicate variants — terse one-liner blurbs (root coverage),
            // full YAML examples and refinement variants are detail.
            GuideNode::FeatureRef {
                feature: Feature::PredicateTextMatches,
                blurb: "`text-matches: <regex>` \u{2014} at least one line matches the regex.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateTextContains,
                blurb: "`text-contains: <substring>` \u{2014} literal substring (no regex escaping).",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateTextHasLines,
                blurb: "`text-has-lines: { min:, max: }` \u{2014} line-count bounds (both optional, inclusive).",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateShellExportsVariable,
                blurb: "`shell-exports: VAR` \u{2014} matches a line `export VAR=...`. \
                        Mapping form `{ name:, value-matches: }` adds an rhs regex.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateShellDefinesVariable,
                blurb: "`shell-defines: VAR` \u{2014} matches `VAR=...` with or without `export`. \
                        Mapping form `{ name:, value-matches: }` adds an rhs regex.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateShellAddsToPath,
                blurb: "`shell-adds-to-path: VAR` \u{2014} matches `export PATH=\"$VAR:$PATH\"`. \
                        On `<env>` this FAILs cleanly (env values are already fully expanded).",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicatePropertiesDefinesKey,
                blurb: "`properties-defines-key: KEY` \u{2014} line-starts-with `KEY=` in a \
                        properties / ini-style file.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateXmlMatches,
                blurb: "`xml-matches: <element/path>` \u{2014} slash-separated element path; \
                        no attributes or wildcards.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateJsonMatches,
                blurb: "`json-matches: <schema>` \u{2014} validate a JSON file against a typed schema.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateYamlMatches,
                blurb: "`yaml-matches: <schema>` \u{2014} same data schema as json-matches, applied to YAML files.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateAnd,
                blurb: "`all: [<sub-check>, ...]` \u{2014} predicate-level conjunction.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateOr,
                blurb: "`any: { hint:, checks: [...] }` \u{2014} predicate-level disjunction with optional hint.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateNot,
                blurb: "`not: <sub-check>` \u{2014} predicate-level negation.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::PredicateConditionally,
                blurb: "`conditionally: { if:, then: }` \u{2014} predicate-level guarded check.",
                detail: false,
                terse_summary: None,
            },
            // Detail YAML examples (verbose pass / --feature drill-in).
            // Adjacent same-root variants (e.g. shell-exports + shell-exports-value-matches)
            // collapse into a single rerun line per spec/0011 §C.
            GuideNode::ExampleControl {
                feature: Feature::PredicateTextMatches,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("text-matches worked example"),
                yaml: "controls:\n  - id: TEXT-MATCHES-EX\n    title: text-matches predicate\n    description: at least one line matches the regex\n    remediation: add a matching line\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          text-matches: \"^source ~/\\\\.env\"\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateTextContains,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("text-contains worked example"),
                yaml: "controls:\n  - id: TEXT-CONTAINS-EX\n    title: text-contains predicate\n    description: literal substring (no regex escaping needed)\n    remediation: add the substring\n    check:\n      file:\n        path: ~/.npmrc\n        check:\n          text-contains: \"artifactory.mycompany.com\"\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateTextHasLines,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("text-has-lines worked example"),
                yaml: "controls:\n  - id: TEXT-HAS-LINES-EX\n    title: text-has-lines predicate\n    description: line-count bounds (both optional, inclusive)\n    remediation: adjust the file length\n    check:\n      file:\n        path: ~/.ssh/authorized_keys\n        check:\n          text-has-lines:\n            min: 1\n            max: 50\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateShellExportsVariable,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("shell-exports bare-string worked example"),
                yaml: "controls:\n  - id: SHELL-EXPORTS-EX\n    title: shell-exports (bare-string form)\n    description: matches a line `export VAR=...`\n    remediation: add the export\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          shell-exports: JAVA_HOME\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateShellExportsVariableValueMatches,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("shell-exports value-matches refinement"),
                yaml: "controls:\n  - id: SHELL-EXPORTS-VM-EX\n    title: shell-exports (mapping form, with value-matches)\n    description: PASS iff the export exists AND the rhs matches the regex\n    remediation: adjust the rhs to match\n    check:\n      file:\n        path: \"<env>\"\n        check:\n          shell-exports:\n            name: PATH\n            value-matches: \"(^|:)/home/u/\\\\.cargo/bin(:|$)\"\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateShellDefinesVariable,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("shell-defines bare-string worked example"),
                yaml: "controls:\n  - id: SHELL-DEFINES-EX\n    title: shell-defines (bare-string form)\n    description: matches `VAR=...` with or without the `export` keyword\n    remediation: add the assignment\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          shell-defines: MY_VAR\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateShellDefinesVariableValueMatches,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("shell-defines value-matches refinement"),
                yaml: "controls:\n  - id: SHELL-DEFINES-VM-EX\n    title: shell-defines (mapping form, with value-matches)\n    description: PASS iff the assignment exists AND the rhs matches the regex\n    remediation: adjust the rhs to match\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          shell-defines:\n            name: MY_VAR\n            value-matches: \"^/opt/.*\"\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateShellAddsToPath,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("shell-adds-to-path worked example"),
                yaml: "controls:\n  - id: SHELL-ADDS-PATH-EX\n    title: shell-adds-to-path predicate\n    description: matches `export PATH=\"$VAR:$PATH\"`. On `<env>` this FAILs cleanly because env materializes fully-expanded values.\n    remediation: add the export PATH line in your rc file\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          shell-adds-to-path: JAVA_HOME_BIN\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicatePropertiesDefinesKey,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("properties-defines-key worked example"),
                yaml: "controls:\n  - id: PROPS-EX\n    title: properties-defines-key predicate\n    description: line-starts-with `key=` in a properties / ini-style file\n    remediation: add the property\n    check:\n      file:\n        path: ~/.gradle/gradle.properties\n        check:\n          properties-defines-key: signing.keyId\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateXmlMatches,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("xml-matches worked example"),
                yaml: "controls:\n  - id: XML-EX\n    title: xml-matches predicate\n    description: slash-separated element path; no attributes or wildcards\n    remediation: add the element\n    check:\n      file:\n        path: ~/.m2/settings.xml\n        check:\n          xml-matches: settings/servers/server/id\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateJsonMatches,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("json-matches worked example"),
                yaml: "controls:\n  - id: JSON-EX\n    title: json-matches predicate\n    description: validate a JSON file against a typed schema\n    remediation: align the file shape to the schema\n    check:\n      file:\n        path: ~/.config/app.json\n        check:\n          json-matches:\n            is-object:\n              settings:\n                is-object:\n                  theme: is-string\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateYamlMatches,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("yaml-matches worked example"),
                yaml: "controls:\n  - id: YAML-EX\n    title: yaml-matches predicate\n    description: same data schema as json-matches, applied to YAML files\n    remediation: align the file shape to the schema\n    check:\n      file:\n        path: ~/.config/models.yaml\n        check:\n          yaml-matches:\n            is-object:\n              models:\n                is-array:\n                  forall:\n                    is-object:\n                      name: is-string\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateAnd,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("and worked example"),
                yaml: "controls:\n  - id: PRED-ALL-EX\n    title: predicate-level all\n    description: every sub-check must hold\n    remediation: ensure both checks pass\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          all:\n            - file-exists\n            - shell-exports: JAVA_HOME\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateOr,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("or worked example"),
                yaml: "controls:\n  - id: PRED-ANY-EX\n    title: predicate-level any\n    description: at least one alternative must hold; hint shown on failure\n    remediation: configure one of the alternatives\n    check:\n      file:\n        path: ~/.bashrc\n        check:\n          any:\n            hint: configure Java (export or assign)\n            checks:\n              - shell-exports: JAVA_HOME\n              - shell-defines: JAVA_HOME\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateNot,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("not worked example"),
                yaml: "controls:\n  - id: PRED-NOT-EX\n    title: predicate-level not\n    description: passes when the inner check fails\n    remediation: ensure the prohibited line is absent\n    check:\n      file:\n        path: ~/.env.local\n        check:\n          not:\n            text-matches: \"(?i)password\"\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PredicateConditionally,
                expect: ExampleExpect::Pass,
                detail: true,
                terse_summary: Some("conditionally worked example"),
                yaml: "controls:\n  - id: PRED-COND-EX\n    title: predicate-level conditionally\n    description: if condition holds, then inner must hold\n    remediation: satisfy the conditional\n    check:\n      file:\n        path: ~/.npmrc\n        check:\n          conditionally:\n            if: file-exists\n            then:\n              text-matches: \"^registry=\"\n",
            },
        ],
    }
}

// -----------------------------------------------------------------------------
// Pseudo-files (claims 2 features)
// -----------------------------------------------------------------------------

fn pseudo_files_section() -> GuideNode {
    GuideNode::Section {
        title: "Pseudo-files",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::Prose {
                text: "Pseudo-files let predicates run against virtual subjects \
                       (the current shell environment, an introspected executable \
                       on PATH) instead of files on disk. Pseudo-file identifiers \
                       begin with `<` and end with `>`; they appear anywhere a \
                       concrete simple-path appears.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::ExampleControl {
                feature: Feature::PseudoFileEnv,
                expect: ExampleExpect::Pass,
                detail: false,
                terse_summary: None,
                yaml: "controls:\n  - id: ENV-EX\n    title: \"<env> pseudo-file\"\n    description: \"`<env>` materializes as `export NAME=VALUE` lines (sorted, newlines escaped).\"\n    remediation: export the variable in your shell\n    check:\n      file:\n        path: \"<env>\"\n        check:\n          shell-exports: RUSTUP_HOME\n",
            },
            GuideNode::ExampleControl {
                feature: Feature::PseudoFileExecutable,
                expect: ExampleExpect::Pass,
                detail: false,
                terse_summary: None,
                yaml: "controls:\n  - id: EXEC-EX\n    title: \"<executable:NAME> pseudo-file\"\n    description: \"snapshot of an executable resolved on PATH; use file-exists for presence or json-matches for shape.\"\n    remediation: install the executable\n    check:\n      file:\n        path: \"<executable:docker>\"\n        check: file-exists\n",
            },
            // Detail caveats — split per-root so the §C same-root invariant
            // can attribute each rerun line to a single `--feature=<id>`.
            GuideNode::FeatureRef {
                feature: Feature::PseudoFileEnv,
                blurb: "`<env>` is read-only and cached for the duration of a single \
                        `key audit run` invocation. Inapplicable predicates (e.g. \
                        `xml-matches` on `<env>`) fail explicitly with a message naming \
                        both the predicate and the pseudo-file.",
                detail: true,
                terse_summary: Some("<env> caching + inapplicable-predicate semantics"),
            },
            GuideNode::FeatureRef {
                feature: Feature::PseudoFileExecutable,
                blurb: "`<executable:NAME>` is read-only and cached for the duration of a \
                        single `key audit run` invocation. Inapplicable predicates (e.g. \
                        `shell-exports` on `<executable:NAME>`) fail explicitly with a \
                        message naming both the predicate and the pseudo-file.",
                detail: true,
                terse_summary: Some("<executable:NAME> caching + inapplicable-predicate semantics"),
            },
        ],
    }
}

// -----------------------------------------------------------------------------
// Test fixtures (claims 4 features) — closes the documentation hole that
// motivated this spec.
// -----------------------------------------------------------------------------

fn test_fixtures_section() -> GuideNode {
    GuideNode::Section {
        title: "Test-fixture format",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::FeatureRef {
                feature: Feature::TestFixtureFormat,
                blurb: "Fixtures (used by `key audit project test`) are directories \
                        under `src/test/resources/<NAME>/` containing files that mimic \
                        a `$HOME` layout. Pseudo-file fixtures (`<env>` / \
                        `<executable:NAME>`) live in YAML override files.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::ExampleFixture {
                feature: Feature::TestFixtureEnvOverride,
                name: "env-override",
                detail: true,
                terse_summary: Some("<env> fixture override YAML"),
                yaml: "env-override:\n  PATH: \"/usr/bin:/home/u/.cargo/bin\"\n  RUSTUP_HOME: \"/home/u/.rustup\"\n",
            },
            GuideNode::ExampleFixture {
                feature: Feature::TestFixtureExecutableOverride,
                name: "executable-override",
                detail: true,
                terse_summary: Some("<executable:NAME> fixture override YAML"),
                yaml: "executable-override:\n  docker:\n    name: docker\n    found: true\n    executable: true\n    path: /usr/bin/docker\n    command-full: docker --version\n    version-full: Docker version 20.10.7\n    version: \"20.10.7\"\n",
            },
            GuideNode::FeatureRef {
                feature: Feature::TestFixtureMalformedRejection,
                blurb: "Malformed fixture YAML (unknown top-level keys, wrong shape) \
                        is rejected at fixture-load time with a clear, line-numbered \
                        error \u{2014} so a typo in the fixture cannot silently change \
                        the meaning of the test.",
                detail: true,
                terse_summary: Some("malformed-fixture rejection rationale"),
            },
        ],
    }
}

// -----------------------------------------------------------------------------
// Control file (claims 5 features)
// -----------------------------------------------------------------------------

fn control_file_section() -> GuideNode {
    GuideNode::Section {
        title: "Control file format",
        detail: false,
        terse_summary: None,
        body: vec![
            GuideNode::FeatureRef {
                feature: Feature::ControlFile,
                blurb: "An audit file is a YAML mapping with a single top-level key, \
                        `controls:`, whose value is a list of controls.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::ControlIdField,
                blurb: "`id:` \u{2014} required string matching `[A-Z][A-Z0-9-]*`. \
                        Must be unique within the file.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::ControlTitleField,
                blurb: "`title:` \u{2014} required short human-readable label.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::ControlDescriptionField,
                blurb: "`description:` \u{2014} required prose explaining what the \
                        control checks and why.",
                detail: false,
                terse_summary: None,
            },
            GuideNode::FeatureRef {
                feature: Feature::ControlRemediationField,
                blurb: "`remediation:` \u{2014} required prose telling the user how to \
                        fix a failing control.",
                detail: false,
                terse_summary: None,
            },
        ],
    }
}
