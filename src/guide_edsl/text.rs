//! Interpretation 1 — markdown emission (spec/0010 §4.1).
//!
//! Two passes share the same emitter:
//!  - `Mode::Terse` (default `key audit guide`): elides `detail: true` subtrees,
//!    replacing each contiguous run of detail nodes with a single
//!    `> rerun \`key audit guide -v\` for more info on <summaries>` line.
//!  - `Mode::Verbose` (`key audit guide -v`): emits everything verbatim.
//!
//! Section headings of NON-detail content are byte-identical between the two
//! passes — that property is verified by the §6.4 consistency test.

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
                emit_pointer_line(std::slice::from_ref(node), out);
                return;
            }

            let hashes = "#".repeat(heading_level.min(6));
            out.push_str(&format!("{} {}\n\n", hashes, title));
            emit_children(body, mode, heading_level + 1, out);
        }
        GuideNode::Prose { text, detail, .. } => {
            if mode == Mode::Terse && *detail {
                emit_pointer_line(std::slice::from_ref(node), out);
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
                emit_pointer_line(std::slice::from_ref(node), out);
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
                emit_pointer_line(std::slice::from_ref(node), out);
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
                emit_pointer_line(std::slice::from_ref(node), out);
                return;
            }
            out.push_str("- ");
            out.push_str(blurb);
            out.push_str("\n\n");
        }
    }
}

/// Render a body, coalescing contiguous runs of detail-only nodes (in terse
/// mode) into a single pointer line that names every elided summary.
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
            emit_pointer_line(&body[start..i], out);
        } else {
            render_node(&body[i], mode, heading_level, out);
            i += 1;
        }
    }
}

fn emit_pointer_line(detail_nodes: &[GuideNode], out: &mut String) {
    let summaries: Vec<&str> = detail_nodes
        .iter()
        .filter_map(|n| n.terse_summary())
        .collect();
    let label = if summaries.is_empty() {
        "this section".to_string()
    } else {
        summaries.join(", ")
    };
    out.push_str(&format!(
        "> rerun `key audit guide -v` for more info on {}\n\n",
        label
    ));
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
            terse.contains("rerun `key audit guide -v`"),
            "terse pass should contain at least one pointer line; got:\n{}",
            terse
        );
    }

    #[test]
    fn verbose_does_not_emit_pointer_line() {
        let r = root();
        let verbose = render(&r, Mode::Verbose);
        assert!(
            !verbose.contains("rerun `key audit guide -v`"),
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
}
