//! Interpretation 1 — markdown emission (spec/0010 §4.1).
//!
//! Two passes share the same emitter:
//!  - `Mode::Terse` (default `key audit guide`): elides `detail: true` subtrees,
//!    replacing each contiguous run of detail nodes with one
//!    `> rerun \`key audit guide -v --feature=<root-id>\` for more info on
//!    <summaries>` line PER ROOT (spec/0011 §C.3 same-root invariant).
//!  - `Mode::Verbose` (`key audit guide -v`): emits everything verbatim.
//!
//! Section headings of NON-detail content are byte-identical between the two
//! passes — that property is verified by the §6.4 consistency test.

use super::features::Feature;
use super::nodes::{ExampleExpect, GuideNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Terse,
    Verbose,
}

/// Render the entire guide tree as a markdown string.
pub fn render(root: &GuideNode, mode: Mode) -> String {
    let mut out = String::new();
    render_node(root, mode, 1, &mut out);
    out
}

/// Walk a subtree and collect the unique root features it claims (a feature's
/// "root" is found by walking `.parent()` to None). Used by the terse pass to
/// enforce the same-root invariant on contiguous detail-collapsed blocks
/// (spec/0011 §C.1).
fn collect_subtree_roots(node: &GuideNode, out: &mut Vec<Feature>) {
    node.walk_features(&mut |f, _detail| {
        let r = f.root();
        if !out.contains(&r) {
            out.push(r);
        }
    });
}

fn render_node(node: &GuideNode, mode: Mode, heading_level: usize, out: &mut String) {
    match node {
        GuideNode::Section {
            title,
            body,
            detail,
            ..
        } => {
            // Terse mode: detail sections are elided as a single pointer line.
            if mode == Mode::Terse && *detail {
                emit_pointer_lines_per_root(std::slice::from_ref(node), out);
                return;
            }

            let hashes = "#".repeat(heading_level.min(6));
            out.push_str(&format!("{} {}\n\n", hashes, title));
            emit_children(body, mode, heading_level + 1, out);
        }
        GuideNode::Prose { text, detail, .. } => {
            if mode == Mode::Terse && *detail {
                emit_pointer_lines_per_root(std::slice::from_ref(node), out);
                return;
            }
            out.push_str(text);
            out.push_str("\n\n");
        }
        GuideNode::ExampleControl {
            yaml,
            expect,
            detail,
            ..
        } => {
            if mode == Mode::Terse && *detail {
                emit_pointer_lines_per_root(std::slice::from_ref(node), out);
                return;
            }
            out.push_str(&format!(
                "Expected outcome: **{}**.\n\n",
                expect_label(*expect)
            ));
            out.push_str("```yaml\n");
            out.push_str(yaml);
            if !yaml.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        GuideNode::ExampleFixture {
            name, yaml, detail, ..
        } => {
            if mode == Mode::Terse && *detail {
                emit_pointer_lines_per_root(std::slice::from_ref(node), out);
                return;
            }
            out.push_str(&format!("Fixture YAML (`{}`):\n\n", name));
            out.push_str("```yaml\n");
            out.push_str(yaml);
            if !yaml.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        GuideNode::FeatureRef { blurb, detail, .. } => {
            if mode == Mode::Terse && *detail {
                emit_pointer_lines_per_root(std::slice::from_ref(node), out);
                return;
            }
            out.push_str("- ");
            out.push_str(blurb);
            out.push_str("\n\n");
        }
    }
}

/// Render a body, coalescing contiguous runs of detail-only nodes (in terse
/// mode) into pointer lines, one per root feature (spec/0011 §C.3).
fn emit_children(body: &[GuideNode], mode: Mode, heading_level: usize, out: &mut String) {
    if mode == Mode::Verbose {
        for child in body {
            render_node(child, mode, heading_level, out);
        }
        return;
    }

    let mut i = 0;
    while i < body.len() {
        if body[i].detail() {
            let start = i;
            while i < body.len() && body[i].detail() {
                i += 1;
            }
            emit_pointer_lines_per_root(&body[start..i], out);
        } else {
            render_node(&body[i], mode, heading_level, out);
            i += 1;
        }
    }
}

/// Emit one rerun line per root feature represented in the contiguous
/// detail-collapsed block. Spec/0011 §C.3: "If a collapsed block's
/// terse_summaries reference Features spanning multiple roots, the
/// implementation MUST split the block into one collapsed line per root
/// rather than emit a misleading shared-root rerun line." Each individual
/// detail subtree must internally carry features sharing a single root
/// (§C.1) — that's a hard panic here.
fn emit_pointer_lines_per_root(detail_nodes: &[GuideNode], out: &mut String) {
    // Per-root buckets, keyed by canonical root id. Preserve first-seen order
    // so the emitted lines mirror reading order.
    let mut order: Vec<Feature> = Vec::new();
    let mut summaries: std::collections::BTreeMap<Feature, Vec<&str>> =
        std::collections::BTreeMap::new();

    for node in detail_nodes {
        let mut roots = Vec::new();
        collect_subtree_roots(node, &mut roots);
        // Spec/0011 §C.1 — same-root invariant within a single detail subtree.
        assert!(
            roots.len() <= 1,
            "same-root invariant violated: detail subtree (terse_summary={:?}) \
             carries features spanning multiple roots: {:?}",
            node.terse_summary(),
            roots.iter().map(|f| f.canonical_id()).collect::<Vec<_>>(),
        );
        let root = match roots.first().copied() {
            Some(r) => r,
            // A detail subtree with zero feature claims (e.g. a bare detail
            // Prose) has no root anchor and therefore cannot produce a valid
            // `--feature=<id>` pointer line. The same-root invariant tests
            // (§C.4) require every rerun line to name a feature, so author
            // detail Prose under a feature-bearing parent (FeatureRef /
            // ExampleControl) instead.
            None => panic!(
                "detail subtree (terse_summary={:?}) has no Feature claim — \
                 cannot attribute a `--feature=<id>` rerun line. Wrap or \
                 replace it with a FeatureRef.",
                node.terse_summary()
            ),
        };
        if !order.contains(&root) {
            order.push(root);
        }
        if let Some(s) = node.terse_summary() {
            summaries.entry(root).or_default().push(s);
        } else {
            summaries.entry(root).or_default();
        }
    }

    for root in order {
        let label = match summaries.get(&root) {
            Some(s) if !s.is_empty() => s.join(", "),
            _ => "this section".to_string(),
        };
        out.push_str(&format!(
            "> rerun `key audit guide -v --feature={}` for more info on {}\n\n",
            root.canonical_id(),
            label
        ));
    }
}

fn expect_label(e: ExampleExpect) -> &'static str {
    match e {
        ExampleExpect::Pass => "PASS",
        ExampleExpect::Fail => "FAIL",
        ExampleExpect::LoadError => "load-error",
    }
}

/// The sequence of section headings produced by an interpretation. Used by
/// the §6.4 terse-vs-verbose consistency test.
pub fn section_headings(node: &GuideNode, mode: Mode) -> Vec<String> {
    let mut out = Vec::new();
    collect_headings(node, mode, 1, &mut out);
    out
}

fn collect_headings(node: &GuideNode, mode: Mode, level: usize, out: &mut Vec<String>) {
    if let GuideNode::Section {
        title,
        body,
        detail,
        ..
    } = node
    {
        if mode == Mode::Terse && *detail {
            return; // elided
        }
        out.push(format!("{}{}", "#".repeat(level.min(6)), title));
        for child in body {
            collect_headings(child, mode, level + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tree::root;
    use super::*;

    #[test]
    fn terse_emits_some_pointer_line() {
        let r = root();
        let terse = render(&r, Mode::Terse);
        assert!(
            terse.contains("rerun `key audit guide -v --feature="),
            "terse pass should contain at least one pointer line; got:\n{}",
            terse
        );
    }

    #[test]
    fn verbose_does_not_emit_pointer_line() {
        let r = root();
        let verbose = render(&r, Mode::Verbose);
        assert!(
            !verbose.contains("rerun `key audit guide -v"),
            "verbose pass must not contain pointer lines; got:\n{}",
            verbose
        );
    }

    /// Spec/0010 §6.4 (terse-vs-verbose consistency): the section-heading
    /// sequence in the terse pass MUST be a subsequence of the verbose pass.
    #[test]
    fn terse_section_headings_are_subsequence_of_verbose() {
        let r = root();
        let terse = section_headings(&r, Mode::Terse);
        let verbose = section_headings(&r, Mode::Verbose);
        let mut vi = 0;
        for t in &terse {
            while vi < verbose.len() && &verbose[vi] != t {
                vi += 1;
            }
            assert!(
                vi < verbose.len(),
                "terse heading {:?} not found in verbose sequence {:?}",
                t,
                verbose
            );
            vi += 1;
        }
    }

    #[test]
    fn terse_and_verbose_both_render_overview() {
        let r = root();
        for mode in [Mode::Terse, Mode::Verbose] {
            let s = render(&r, mode);
            assert!(
                s.contains("# key audit"),
                "missing top-level heading in {:?} mode",
                mode
            );
            assert!(
                s.contains("Overview"),
                "missing Overview heading in {:?} mode",
                mode
            );
        }
    }

    // ---------------------------------------------------------------
    // Spec/0011 §B.5 — `--feature=<id>` filter tests.
    // ---------------------------------------------------------------

    use crate::guide_edsl::filter::filter_tree;

    /// Spec/0011 §B.5 — `--feature=<root>` shows root + descendants in the
    /// chosen pass.
    #[test]
    fn filter_root_terse_shows_root_and_descendants() {
        let r = root();
        let filtered = filter_tree(&r, Feature::PseudoFileEnv).expect("non-empty");
        let terse = render(&filtered, Mode::Terse);
        // The root's canonical example survives.
        assert!(terse.contains("ENV-EX"), "missing root example: {}", terse);
        // The descendant (TestFixtureEnvOverride) is referenced (pointer line).
        assert!(
            terse.contains("--feature=pseudo-file-env"),
            "expected a rerun line drilled to pseudo-file-env in terse-of-filter; got:\n{}",
            terse
        );
        // The unrelated PseudoFileExecutable example is NOT present.
        assert!(
            !terse.contains("EXEC-EX"),
            "unrelated root content leaked into filter: {}",
            terse
        );
    }

    /// Spec/0011 §B.5 — `--feature=<descendant>` shows ONLY that node's
    /// content (no siblings, no parent's other children).
    #[test]
    fn filter_descendant_shows_only_that_node() {
        let r = root();
        let filtered = filter_tree(&r, Feature::TestFixtureEnvOverride).expect("non-empty");
        let verbose = render(&filtered, Mode::Verbose);
        // The descendant's fixture YAML survives.
        assert!(
            verbose.contains("env-override"),
            "missing fixture YAML: {}",
            verbose
        );
        // Sibling test fixture override (executable) is excluded.
        assert!(
            !verbose.contains("executable-override"),
            "sibling content leaked into descendant filter: {}",
            verbose
        );
        // Parent root content (the <env> ExampleControl) is NOT pulled in.
        assert!(
            !verbose.contains("ENV-EX"),
            "parent's example pulled in by descendant filter: {}",
            verbose
        );
    }

    /// Spec/0011 §B.5 — filter combines correctly with `-v` (root + descendants
    /// rendered without rerun-line elision).
    #[test]
    fn filter_root_verbose_no_rerun_lines() {
        let r = root();
        let filtered = filter_tree(&r, Feature::PseudoFileEnv).expect("non-empty");
        let verbose = render(&filtered, Mode::Verbose);
        assert!(
            !verbose.contains("rerun `key audit guide -v"),
            "verbose pass must not contain rerun lines; got:\n{}",
            verbose
        );
        // Both root example and descendant fixture are rendered.
        assert!(verbose.contains("ENV-EX"));
        assert!(verbose.contains("env-override"));
    }

    /// Spec/0011 §B.4 + §B.5 — unknown id resolves to `None` via
    /// `Feature::from_canonical_id`. The CLI layer turns that into a non-zero
    /// exit; here we assert the lookup, plus that the error-message surface
    /// (`Feature::all()` filtered to roots) is non-empty.
    #[test]
    fn unknown_canonical_id_returns_none_and_roots_are_listable() {
        assert!(Feature::from_canonical_id("definitely-not-a-feature").is_none());
        let roots: Vec<&str> = Feature::all()
            .iter()
            .filter(|f| f.parent().is_none())
            .map(|f| f.canonical_id())
            .collect();
        assert!(!roots.is_empty(), "must have at least one root feature");
        // Sanity: every well-known root id is in the list.
        for known in [
            "pseudo-file-env",
            "pseudo-file-executable",
            "control-file",
            "test-fixture-format",
            "cli-audit-run",
        ] {
            assert!(
                roots.contains(&known),
                "expected root {:?} in error-message surface",
                known
            );
        }
    }

    // ---------------------------------------------------------------
    // Spec/0011 §C.4 — same-root invariant for collapsed blocks.
    // ---------------------------------------------------------------

    fn detail_example(feature: Feature, summary: &'static str) -> GuideNode {
        GuideNode::ExampleControl {
            feature,
            yaml: "controls: []\n",
            expect: ExampleExpect::Pass,
            detail: true,
            terse_summary: Some(summary),
        }
    }

    /// Spec/0011 §C.4(a) — two consecutive detail subtrees declaring features
    /// under DIFFERENT roots → terse emits TWO rerun lines, one per root.
    #[test]
    fn collapsed_block_with_two_roots_emits_two_rerun_lines() {
        let tree = GuideNode::Section {
            title: "two-roots-fixture",
            detail: false,
            terse_summary: None,
            body: vec![
                detail_example(Feature::PropositionForall, "forall summary"),
                detail_example(Feature::PredicateTextMatches, "text-matches summary"),
            ],
        };
        let terse = render(&tree, Mode::Terse);
        let n_rerun = terse.matches("> rerun `key audit guide").count();
        assert_eq!(
            n_rerun, 2,
            "expected one rerun line per root (2); got {}: {}",
            n_rerun, terse
        );
        assert!(terse.contains("--feature=proposition-forall"));
        assert!(terse.contains("--feature=predicate-text-matches"));
    }

    /// Spec/0011 §C.4(b) — two consecutive detail subtrees sharing one root
    /// → ONE rerun line with the shared root id.
    #[test]
    fn collapsed_block_with_one_root_emits_one_rerun_line() {
        let tree = GuideNode::Section {
            title: "one-root-fixture",
            detail: false,
            terse_summary: None,
            body: vec![
                detail_example(
                    Feature::PredicateShellExportsVariable,
                    "shell-exports bare summary",
                ),
                detail_example(
                    Feature::PredicateShellExportsVariableValueMatches,
                    "shell-exports value-matches summary",
                ),
            ],
        };
        let terse = render(&tree, Mode::Terse);
        let n_rerun = terse.matches("> rerun `key audit guide").count();
        assert_eq!(
            n_rerun, 1,
            "expected one rerun line for shared root; got {}: {}",
            n_rerun, terse
        );
        // Both summaries appear on the single line.
        assert!(terse.contains("shell-exports bare summary"));
        assert!(terse.contains("shell-exports value-matches summary"));
        // Root id is the shared parent (PredicateShellExportsVariable).
        assert!(terse.contains("--feature=predicate-shell-exports-variable"));
        // The descendant id does NOT appear (the root's id is the canonical anchor).
        assert!(!terse.contains("--feature=predicate-shell-exports-variable-value-matches"));
    }

    /// Spec/0011 §C.4(c) — every rerun line in the production terse output
    /// names a valid `--feature=<id>` that, when used, ACTUALLY expands to
    /// the referenced content (round-trip).
    #[test]
    fn every_rerun_line_round_trips_via_feature_filter() {
        let r = root();
        let terse = render(&r, Mode::Terse);
        // Extract every `--feature=<id>` mentioned in a rerun line.
        let mut ids: Vec<&str> = Vec::new();
        for line in terse.lines() {
            if !line.starts_with("> rerun `key audit guide") {
                continue;
            }
            // Find `--feature=` and read the id up to the next backtick or space.
            let key = "--feature=";
            if let Some(pos) = line.find(key) {
                let after = &line[pos + key.len()..];
                let end = after
                    .find(|c: char| c == '`' || c.is_whitespace())
                    .unwrap_or(after.len());
                let id = &after[..end];
                ids.push(id);
            } else {
                panic!(
                    "rerun line missing `--feature=<id>` (spec/0011 §C.4(c)):\n  {}",
                    line
                );
            }
        }
        assert!(
            !ids.is_empty(),
            "expected at least one rerun line in terse output"
        );
        for id in ids {
            let feat = Feature::from_canonical_id(id)
                .unwrap_or_else(|| panic!("rerun line names unknown feature id {:?}", id));
            // Filter actually keeps something AND verbose pass renders content.
            let filtered = filter_tree(&r, feat).unwrap_or_else(|| {
                panic!(
                    "filter by --feature={} produced empty tree — round-trip broken",
                    id
                )
            });
            let verbose_filtered = render(&filtered, Mode::Verbose);
            assert!(
                verbose_filtered.len() > 50,
                "filter+verbose round-trip for {:?} produced near-empty output:\n{}",
                id,
                verbose_filtered
            );
        }
    }

    /// Spec/0011 §C.1 — within a single contiguous detail run, each subtree
    /// must internally carry features sharing one root. This test ensures
    /// the production tree honors that (tested implicitly: render() panics
    /// otherwise).
    #[test]
    fn production_terse_renders_without_panicking() {
        let r = root();
        let _ = render(&r, Mode::Terse);
        let _ = render(&r, Mode::Verbose);
    }
}
