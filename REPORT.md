# Usability Report: `key audit` DX Evaluation

## Project

Built `mycompany-audit` — an audit project with 4 controls checking corporate Artifactory configuration for npm, Gradle, and pip, plus a `FOOBAR` export check in `~/.bashrc`.

## How easy was it to discover the YAML syntax?

`key audit guide` is excellent. It prints a comprehensive reference with working examples for every predicate and proposition type, including `file-exists`, `text-matches`, `shell-exports`, `exists` (existential quantifier), `all`/`any` combinators, `not`, and `conditionally`. The guide is organized by feature with runnable before/after scenarios, making it easy to find the right construct.

The quick-reference section at the bottom is particularly useful as a cheat sheet once you've read the full examples.

## How well did predicates map to real-world config checks?

Very well for most cases:

- **`text-matches`** handled npm's `.npmrc` (flat ini-style) and Gradle's groovy init script with no friction. Regex is the right tool for these.
- **`shell-exports`** was perfect for the FOOBAR check — zero-config sugar that does exactly what you'd expect.
- **`exists` (existential quantifier)** mapped cleanly to pip's two possible config locations (`~/.config/pip/pip.conf` and `~/.pip/pip.conf`). The semantics — "at least one of these files must satisfy the check" — are exactly right for fallback-path scenarios.

## Pain points / gaps encountered

1. **`exists` failure count is not obvious.** When using `exists` with two file paths and the invalid fixture only contains one of them, both paths generate a failure (one "no line matches regex", one "does not exist"). The test expected `count: 1` but got `count: 2`. This is logically correct (both alternatives failed), but the mental model takes a moment to click. A note in the guide about this would help.

2. **I missed `properties-defines-key` on my first read of the guide.** The guide documents it clearly, but I skimmed past it and suggested an "ini-key" predicate that essentially already exists. This is a discoverability data point — when scanning a long guide for a specific use case, it's easy to miss predicates with Java-centric naming (`properties-defines-key`) when thinking in terms of ini-style config files.

3. **`properties-defines-key` checks key existence, not value.** When I tried switching CTRL-0001 to `properties-defines-key: registry`, the invalid fixture (`registry=https://registry.npmjs.org/`) also passed — false negative. For audit controls that care about *which* registry, not just that a registry key exists, `text-matches` is still the right predicate. A hypothetical `properties-key-matches: {key: registry, value-matches: ...}` would bridge this gap.

4. **No built-in "file contains exact string" predicate.** `text-matches` with a regex works, but for cases like checking that a Gradle file mentions `artifactory.mycompany.com`, a simple `text-contains: artifactory.mycompany.com` would be slightly more ergonomic than remembering to escape dots in the regex. Minor.

## What worked well

- **`key audit project new`** scaffolding is clean and fast — gives you exactly the right directory structure with no boilerplate to delete.
- **`key audit project test`** output is clear: suite names, pass/fail per test, and actionable error messages when expectations don't match.
- **`key audit project build`** does parse-check + test + copy in one step. Good workflow.
- **`key audit list`** provides a nice human-readable summary of controls with remediation text.
- **The YAML schema is minimal and readable.** Controls are self-documenting with `id`, `title`, `description`, `remediation`, and `check`. No unnecessary ceremony.
- **The `~/.` path convention** that resolves to `$HOME` in run mode and to the fixture directory in test mode is elegant — write once, test anywhere.
- **Error messages** are specific: "no line matches regex", "does not exist" — easy to assert against in tests.

## Suggestions for improvement

1. **Document `exists` failure semantics** in the guide — specifically that when all alternatives fail, each failed alternative contributes to the failure count.
2. **Consider a `properties-key-matches` predicate** that checks both key existence and value pattern, e.g. `{key: registry, value-matches: artifactory\.mycompany\.com}`. The existing `properties-defines-key` only checks existence.
3. **Consider a `text-contains` convenience predicate** that does a literal substring match without regex escaping concerns.
4. **Add a `--verbose` flag to `key audit project test`** that shows the actual failure messages alongside expectations, to speed up debugging when counts don't match.
5. **Consider allowing `expect: fail` without `count`** to mean "at least one failure" (it may already work this way — `expect: fail` without the nested object seemed to work, which is good).

## Process note

All controls were written by hand — I read `key audit guide`, understood the schema, and wrote the full YAML directly. I never tried `key audit add` (the interactive flow). The guide was good enough that hand-authoring felt faster than stepping through an interactive wizard. This is itself a DX data point: the guide quality made the interactive tool feel unnecessary for someone comfortable with YAML.


-------
/ end of ai-generated report

-------

Human Dev's Comments:

  - Let's keep the properties, but add to the guide greppable "ini"-markers, if some agents expect it to be called this way
  - If the `exists` explains for every file why the predicate failed, it's fine, but we should  document this behavior
  - text-contains (or similar) for just checking for string verbatim, without haveing to regex-escape, would indeed be advantageous (also don't forget it in the guide)
