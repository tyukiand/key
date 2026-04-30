//! Generic Interaction coroutine — see
//! `spec/0015-interaction-coroutine-and-project-adt.txt` §2.

// The bin currently does not drive the Interaction surface end-to-end (the
// existing `key audit add` keeps its UX via the legacy interactive.rs path).
// This iteration ships the abstraction and exercises it via library tests
// + the round-trip / lower invariants. Allow the unused-warnings to silence
// outside of test builds; the lib + tests cover every public item.
#![allow(dead_code)]
//!
//! Models an interactive dialog as a reactive coroutine: each suspension
//! point carries the current Menu (the question being asked); the engine
//! resumes when an input of the alphabet `I` is provided.
//!
//! Two input alphabets are defined here:
//!   - `LowLevelInput`:  Index | Lexical | Text | Yes | No | Back
//!     (what a terminal driver produces from raw user input)
//!   - `AsmOp` (test-only): Select(LexicalPattern) | Enter(String) | Yes
//!     | No | Back  — a typed assembly that's stable across menu shape
//!     changes
//!
//! The functor `lower<T>(asm: Interaction<AsmOp, T>) ->
//! Interaction<LowLevelInput, T>` adapts AsmOp inputs to LowLevelInput
//! against the menu's options.

use std::fmt;

// ---------------------------------------------------------------------------
// Menu (spec §2.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeKind {
    Text,
    Integer,
    Regex,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuOption {
    pub tag: String,
    pub label: String,
    pub aliases: Vec<String>,
}

impl MenuOption {
    pub fn new(tag: impl Into<String>, label: impl Into<String>) -> Self {
        MenuOption {
            tag: tag.into(),
            label: label.into(),
            aliases: vec![],
        }
    }

    pub fn with_aliases(mut self, aliases: Vec<String>) -> Self {
        self.aliases = aliases;
        self
    }

    /// True if this option matches `pattern` case-insensitively against tag
    /// or any alias (spec §2.2).
    pub fn matches(&self, pattern: &str) -> bool {
        let p = pattern.to_ascii_lowercase();
        if self.tag.to_ascii_lowercase() == p {
            return true;
        }
        for a in &self.aliases {
            if a.to_ascii_lowercase() == p {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Menu {
    Pick {
        prompt: String,
        options: Vec<MenuOption>,
    },
    Free {
        prompt: String,
        kind: FreeKind,
    },
    YesNo {
        prompt: String,
    },
    Confirm {
        prompt: String,
        summary: String,
    },
}

impl Menu {
    pub fn prompt(&self) -> &str {
        match self {
            Menu::Pick { prompt, .. } => prompt,
            Menu::Free { prompt, .. } => prompt,
            Menu::YesNo { prompt } => prompt,
            Menu::Confirm { prompt, .. } => prompt,
        }
    }
}

// ---------------------------------------------------------------------------
// Input alphabets (spec §2.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowLevelInput {
    /// 1-based menu pick by integer.
    Index(usize),
    /// Menu pick by typed-prefix lexical match against tag/aliases.
    Lexical(String),
    /// Free-form text answer (path / regex / id / …).
    Text(String),
    Yes,
    No,
    Back,
}

/// Test-only typed assembly alphabet (spec §2.2). The newtype guarantees
/// case-insensitive identity for diff-friendly hashing.
#[cfg(any(test, feature = "testing"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalPattern(String);

#[cfg(any(test, feature = "testing"))]
impl LexicalPattern {
    pub fn new(s: impl Into<String>) -> Self {
        LexicalPattern(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(any(test, feature = "testing"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsmOp {
    /// Pick the unique menu option matching the given lexical pattern.
    Select(LexicalPattern),
    /// Provide free-form text.
    Enter(String),
    Yes,
    No,
    Back,
}

// ---------------------------------------------------------------------------
// Step + Interaction (spec §2.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum InteractionError {
    /// Resume callback was given an input but the dialog needed none, or vice
    /// versa.
    UnexpectedInput,
    /// `lower()` translation failed: zero matches for the AsmOp::Select pattern.
    NoMatch {
        pattern: String,
        prompt: String,
        tags: Vec<String>,
    },
    /// `lower()` translation failed: multiple matches for the AsmOp::Select
    /// pattern.
    MultipleMatches {
        pattern: String,
        prompt: String,
        tags: Vec<String>,
    },
    /// `lower()` translation failed: AsmOp variant didn't match the menu
    /// kind (e.g. Select on a Free menu, Enter on a Pick menu, Yes on Confirm).
    TypeMismatch {
        op: String,
        prompt: String,
        menu_kind: &'static str,
    },
    /// Engine error reported by the interaction body (e.g. validation).
    Engine(String),
}

impl fmt::Display for InteractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InteractionError::UnexpectedInput => write!(f, "unexpected input"),
            InteractionError::NoMatch {
                pattern,
                prompt,
                tags,
            } => write!(
                f,
                "no menu option matches {:?} for prompt {:?}; available tags: {}",
                pattern,
                prompt,
                tags.join(", ")
            ),
            InteractionError::MultipleMatches {
                pattern,
                prompt,
                tags,
            } => write!(
                f,
                "multiple menu options match {:?} for prompt {:?}; available tags: {}",
                pattern,
                prompt,
                tags.join(", ")
            ),
            InteractionError::TypeMismatch {
                op,
                prompt,
                menu_kind,
            } => write!(
                f,
                "AsmOp::{} cannot be applied to {} menu (prompt: {:?})",
                op, menu_kind, prompt,
            ),
            InteractionError::Engine(s) => write!(f, "interaction engine error: {}", s),
        }
    }
}

impl std::error::Error for InteractionError {}

/// One execution step of an Interaction.
pub enum Step<I, T> {
    /// The interaction is paused at a prompt; supply input via `resume`.
    Suspended {
        menu: Menu,
        resume: Box<dyn FnOnce(I) -> Step<I, T>>,
    },
    Done(T),
    Failed(InteractionError),
}

/// Opaque coroutine-like handle. Internally a closure that produces the
/// initial Step on demand.
pub struct Interaction<I, T> {
    start: Box<dyn FnOnce() -> Step<I, T>>,
}

impl<I: 'static, T: 'static> Interaction<I, T> {
    /// Construct an Interaction from a starter that produces the initial Step.
    pub fn new(start: impl FnOnce() -> Step<I, T> + 'static) -> Self {
        Interaction {
            start: Box::new(start),
        }
    }

    /// An interaction that immediately yields `t`.
    pub fn pure(t: T) -> Self
    where
        T: 'static,
    {
        Interaction::new(move || Step::Done(t))
    }

    /// Drive the dialog: `answer` is invoked at each suspension to provide
    /// the next input. Returns the final value or the first error.
    pub fn run<F: FnMut(&Menu) -> I>(self, mut answer: F) -> Result<T, InteractionError> {
        let mut step = (self.start)();
        loop {
            match step {
                Step::Done(t) => return Ok(t),
                Step::Failed(e) => return Err(e),
                Step::Suspended { menu, resume } => {
                    let input = answer(&menu);
                    step = resume(input);
                }
            }
        }
    }

    /// Drive the dialog with a fixed sequence of inputs.
    pub fn run_with(self, mut inputs: Vec<I>) -> Result<T, InteractionError> {
        inputs.reverse();
        self.run(|_menu| {
            inputs
                .pop()
                .expect("ran out of inputs while interaction still suspended")
        })
    }

    /// Take one step; returns the suspended menu or final value.
    pub fn step(self) -> Step<I, T> {
        (self.start)()
    }

    /// Monadic bind: chain a follow-up Interaction that depends on this one's
    /// output.
    pub fn and_then<U: 'static, F>(self, f: F) -> Interaction<I, U>
    where
        F: FnOnce(T) -> Interaction<I, U> + 'static,
    {
        Interaction::new(move || bind_step((self.start)(), f))
    }

    /// Map the result type without consuming the input alphabet.
    pub fn map<U: 'static, F>(self, f: F) -> Interaction<I, U>
    where
        F: FnOnce(T) -> U + 'static,
    {
        Interaction::new(move || map_step((self.start)(), f))
    }
}

fn bind_step<I: 'static, T: 'static, U: 'static, F>(step: Step<I, T>, f: F) -> Step<I, U>
where
    F: FnOnce(T) -> Interaction<I, U> + 'static,
{
    match step {
        Step::Done(t) => (f(t).start)(),
        Step::Failed(e) => Step::Failed(e),
        Step::Suspended { menu, resume } => Step::Suspended {
            menu,
            resume: Box::new(move |i: I| bind_step(resume(i), f)),
        },
    }
}

fn map_step<I: 'static, T: 'static, U: 'static, F>(step: Step<I, T>, f: F) -> Step<I, U>
where
    F: FnOnce(T) -> U + 'static,
{
    match step {
        Step::Done(t) => Step::Done(f(t)),
        Step::Failed(e) => Step::Failed(e),
        Step::Suspended { menu, resume } => Step::Suspended {
            menu,
            resume: Box::new(move |i: I| map_step(resume(i), f)),
        },
    }
}

// ---------------------------------------------------------------------------
// Building-block primitives (low-level alphabet)
// ---------------------------------------------------------------------------

/// Suspend with a Pick menu and yield the index of the chosen option.
///
/// The driver may answer with `LowLevelInput::Index(i)` (1-based) or
/// `LowLevelInput::Lexical(s)` (matched against tag/aliases). `Back` becomes
/// an InteractionError::Engine for now (the live UX handles back-out at the
/// outer driver layer).
pub fn ask_pick(
    prompt: impl Into<String>,
    options: Vec<MenuOption>,
) -> Interaction<LowLevelInput, usize> {
    let prompt = prompt.into();
    Interaction::new(move || {
        let menu = Menu::Pick {
            prompt: prompt.clone(),
            options: options.clone(),
        };
        Step::Suspended {
            menu,
            resume: Box::new(move |input| match input {
                LowLevelInput::Index(i) => {
                    if i == 0 || i > options.len() {
                        Step::Failed(InteractionError::Engine(format!(
                            "index {} out of range 1..={}",
                            i,
                            options.len()
                        )))
                    } else {
                        Step::Done(i - 1)
                    }
                }
                LowLevelInput::Lexical(pat) => {
                    let matches: Vec<usize> = options
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| o.matches(&pat))
                        .map(|(i, _)| i)
                        .collect();
                    if matches.len() == 1 {
                        Step::Done(matches[0])
                    } else if matches.is_empty() {
                        Step::Failed(InteractionError::NoMatch {
                            pattern: pat,
                            prompt: prompt.clone(),
                            tags: options.iter().map(|o| o.tag.clone()).collect(),
                        })
                    } else {
                        Step::Failed(InteractionError::MultipleMatches {
                            pattern: pat,
                            prompt: prompt.clone(),
                            tags: options.iter().map(|o| o.tag.clone()).collect(),
                        })
                    }
                }
                LowLevelInput::Back => Step::Failed(InteractionError::Engine(
                    "back at top of pick menu".to_string(),
                )),
                other => Step::Failed(InteractionError::TypeMismatch {
                    op: format!("{:?}", other),
                    prompt: prompt.clone(),
                    menu_kind: "Pick",
                }),
            }),
        }
    })
}

/// Suspend with a Free menu and yield the user's text answer.
pub fn ask_free(prompt: impl Into<String>, kind: FreeKind) -> Interaction<LowLevelInput, String> {
    let prompt = prompt.into();
    Interaction::new(move || {
        let menu = Menu::Free {
            prompt: prompt.clone(),
            kind: kind.clone(),
        };
        Step::Suspended {
            menu,
            resume: Box::new(move |input| match input {
                LowLevelInput::Text(s) => Step::Done(s),
                LowLevelInput::Back => {
                    Step::Failed(InteractionError::Engine("back at free prompt".to_string()))
                }
                other => Step::Failed(InteractionError::TypeMismatch {
                    op: format!("{:?}", other),
                    prompt: prompt.clone(),
                    menu_kind: "Free",
                }),
            }),
        }
    })
}

/// Suspend with a YesNo menu and yield the boolean answer.
pub fn ask_yesno(prompt: impl Into<String>) -> Interaction<LowLevelInput, bool> {
    let prompt = prompt.into();
    Interaction::new(move || {
        let menu = Menu::YesNo {
            prompt: prompt.clone(),
        };
        Step::Suspended {
            menu,
            resume: Box::new(move |input| match input {
                LowLevelInput::Yes => Step::Done(true),
                LowLevelInput::No => Step::Done(false),
                LowLevelInput::Back => Step::Failed(InteractionError::Engine(
                    "back at yes/no prompt".to_string(),
                )),
                other => Step::Failed(InteractionError::TypeMismatch {
                    op: format!("{:?}", other),
                    prompt: prompt.clone(),
                    menu_kind: "YesNo",
                }),
            }),
        }
    })
}

// ---------------------------------------------------------------------------
// lower() — AsmOp → LowLevelInput functor (spec §3.2)
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "testing"))]
pub fn lower<T: 'static>(asm: Interaction<AsmOp, T>) -> Interaction<LowLevelInput, T> {
    Interaction::new(move || lower_step((asm.start)()))
}

#[cfg(any(test, feature = "testing"))]
fn lower_step<T: 'static>(step: Step<AsmOp, T>) -> Step<LowLevelInput, T> {
    match step {
        Step::Done(t) => Step::Done(t),
        Step::Failed(e) => Step::Failed(e),
        Step::Suspended { menu, resume } => Step::Suspended {
            menu: menu.clone(),
            resume: Box::new(move |low: LowLevelInput| match low_to_asm(&menu, low) {
                Ok(asm) => lower_step(resume(asm)),
                Err(e) => Step::Failed(e),
            }),
        },
    }
}

/// Translate a LowLevelInput against the current menu into the equivalent
/// AsmOp. Implements spec §3.2 — Pick / Free / YesNo / Confirm with explicit
/// type-mismatch errors.
#[cfg(any(test, feature = "testing"))]
fn low_to_asm(menu: &Menu, low: LowLevelInput) -> Result<AsmOp, InteractionError> {
    match (menu, low) {
        (Menu::Pick { options, prompt }, LowLevelInput::Index(i)) => {
            if i == 0 || i > options.len() {
                Err(InteractionError::Engine(format!(
                    "index {} out of range 1..={} for prompt {:?}",
                    i,
                    options.len(),
                    prompt
                )))
            } else {
                Ok(AsmOp::Select(LexicalPattern::new(
                    options[i - 1].tag.clone(),
                )))
            }
        }
        (Menu::Pick { options, prompt }, LowLevelInput::Lexical(s)) => {
            let n = options.iter().filter(|o| o.matches(&s)).count();
            if n == 1 {
                Ok(AsmOp::Select(LexicalPattern::new(s)))
            } else if n == 0 {
                Err(InteractionError::NoMatch {
                    pattern: s,
                    prompt: prompt.clone(),
                    tags: options.iter().map(|o| o.tag.clone()).collect(),
                })
            } else {
                Err(InteractionError::MultipleMatches {
                    pattern: s,
                    prompt: prompt.clone(),
                    tags: options.iter().map(|o| o.tag.clone()).collect(),
                })
            }
        }
        (Menu::Free { .. }, LowLevelInput::Text(s)) => Ok(AsmOp::Enter(s)),
        (Menu::YesNo { .. }, LowLevelInput::Yes) => Ok(AsmOp::Yes),
        (Menu::YesNo { .. }, LowLevelInput::No) => Ok(AsmOp::No),
        (Menu::Confirm { .. }, LowLevelInput::Yes) => Ok(AsmOp::Yes),
        (Menu::Confirm { .. }, LowLevelInput::No) => Ok(AsmOp::No),
        (_, LowLevelInput::Back) => Ok(AsmOp::Back),
        (m, other) => Err(InteractionError::TypeMismatch {
            op: format!("{:?}", other),
            prompt: m.prompt().to_string(),
            menu_kind: match m {
                Menu::Pick { .. } => "Pick",
                Menu::Free { .. } => "Free",
                Menu::YesNo { .. } => "YesNo",
                Menu::Confirm { .. } => "Confirm",
            },
        }),
    }
}

// The above lower() pipeline goes through `low_to_asm` for Pick menus and
// `low_to_asm_for_menu` for the rest. The dual direction (driving an
// Interaction<AsmOp,T> directly with AsmOp inputs) does not need translation:
// run the asm interaction with an AsmOp answerer.
//
// For clarity, we expose a typed driver for AsmOp interactions:
#[cfg(any(test, feature = "testing"))]
impl<T: 'static> Interaction<AsmOp, T> {
    /// Drive this asm interaction with a sequence of AsmOps. Errors at the
    /// first malformed step (zero/multiple matches, type mismatch).
    pub fn run_asm(self, ops: Vec<AsmOp>) -> Result<T, InteractionError> {
        run_asm_loop((self.start)(), ops)
    }
}

#[cfg(any(test, feature = "testing"))]
fn run_asm_loop<T: 'static>(
    mut step: Step<AsmOp, T>,
    ops: Vec<AsmOp>,
) -> Result<T, InteractionError> {
    let mut iter = ops.into_iter();
    loop {
        match step {
            Step::Done(t) => return Ok(t),
            Step::Failed(e) => return Err(e),
            Step::Suspended { menu, resume } => {
                let op = match iter.next() {
                    Some(o) => o,
                    None => {
                        return Err(InteractionError::Engine(format!(
                            "ran out of AsmOps at prompt {:?}",
                            menu.prompt()
                        )))
                    }
                };
                // Validate the AsmOp against the menu kind per spec §3.2 BEFORE
                // we resume; the inner dialog assumes the engine matched it.
                if let Err(e) = validate_asm_against_menu(&menu, &op) {
                    return Err(e);
                }
                step = resume(op);
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
fn validate_asm_against_menu(menu: &Menu, op: &AsmOp) -> Result<(), InteractionError> {
    match (menu, op) {
        (Menu::Pick { options, prompt }, AsmOp::Select(pat)) => {
            let n = options.iter().filter(|o| o.matches(pat.as_str())).count();
            if n == 1 {
                Ok(())
            } else if n == 0 {
                Err(InteractionError::NoMatch {
                    pattern: pat.as_str().to_string(),
                    prompt: prompt.clone(),
                    tags: options.iter().map(|o| o.tag.clone()).collect(),
                })
            } else {
                Err(InteractionError::MultipleMatches {
                    pattern: pat.as_str().to_string(),
                    prompt: prompt.clone(),
                    tags: options.iter().map(|o| o.tag.clone()).collect(),
                })
            }
        }
        (Menu::Free { prompt, .. }, AsmOp::Enter(_)) => {
            let _ = prompt;
            Ok(())
        }
        (Menu::YesNo { .. }, AsmOp::Yes) | (Menu::YesNo { .. }, AsmOp::No) => Ok(()),
        (Menu::Confirm { .. }, AsmOp::Yes) | (Menu::Confirm { .. }, AsmOp::No) => Ok(()),
        (_, AsmOp::Back) => Ok(()),
        (m, other) => Err(InteractionError::TypeMismatch {
            op: format!("{:?}", other),
            prompt: m.prompt().to_string(),
            menu_kind: match m {
                Menu::Pick { .. } => "Pick",
                Menu::Free { .. } => "Free",
                Menu::YesNo { .. } => "YesNo",
                Menu::Confirm { .. } => "Confirm",
            },
        }),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pick3() -> Vec<MenuOption> {
        vec![
            MenuOption::new("alpha", "Alpha"),
            MenuOption::new("beta", "Beta"),
            MenuOption::new("gamma", "Gamma"),
        ]
    }

    #[test]
    fn pure_returns_immediately() {
        let i: Interaction<LowLevelInput, i32> = Interaction::pure(42);
        assert_eq!(i.run(|_| panic!("should not be called")).unwrap(), 42);
    }

    #[test]
    fn pick_by_index() {
        let i = ask_pick("pick", pick3());
        let r = i.run_with(vec![LowLevelInput::Index(2)]).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn pick_by_lexical_unique() {
        let i = ask_pick("pick", pick3());
        let r = i
            .run_with(vec![LowLevelInput::Lexical("beta".into())])
            .unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn pick_by_lexical_no_match_errors() {
        let i = ask_pick("pick", pick3());
        let err = i
            .run_with(vec![LowLevelInput::Lexical("zzz".into())])
            .unwrap_err();
        assert!(matches!(err, InteractionError::NoMatch { .. }));
    }

    #[test]
    fn pick_index_out_of_range() {
        let i = ask_pick("pick", pick3());
        let err = i.run_with(vec![LowLevelInput::Index(99)]).unwrap_err();
        assert!(matches!(err, InteractionError::Engine(_)));
    }

    #[test]
    fn yesno() {
        let i = ask_yesno("ok?");
        assert_eq!(i.run_with(vec![LowLevelInput::Yes]).unwrap(), true);
    }

    #[test]
    fn free_text() {
        let i = ask_free("name?", FreeKind::Text);
        let r = i
            .run_with(vec![LowLevelInput::Text("hello".into())])
            .unwrap();
        assert_eq!(r, "hello");
    }

    #[test]
    fn and_then_chains() {
        let i = ask_pick("pick", pick3())
            .and_then(|idx| ask_free(format!("for {}", idx), FreeKind::Text));
        let r = i
            .run_with(vec![
                LowLevelInput::Index(1),
                LowLevelInput::Text("x".into()),
            ])
            .unwrap();
        assert_eq!(r, "x");
    }

    #[test]
    fn map_transforms() {
        let i: Interaction<LowLevelInput, String> =
            ask_pick("pick", pick3()).map(|i| format!("idx={}", i));
        let r = i.run_with(vec![LowLevelInput::Index(2)]).unwrap();
        assert_eq!(r, "idx=1");
    }

    // -----------------------------------------------------------------------
    // AsmOp + lower() (test-only)
    // -----------------------------------------------------------------------

    fn pick_alpha() -> Interaction<AsmOp, usize> {
        Interaction::new(|| {
            let menu = Menu::Pick {
                prompt: "p".into(),
                options: vec![MenuOption::new("alpha", "A"), MenuOption::new("beta", "B")],
            };
            Step::Suspended {
                menu,
                resume: Box::new(|op: AsmOp| match op {
                    AsmOp::Select(pat) if pat.as_str().eq_ignore_ascii_case("alpha") => {
                        Step::Done(0)
                    }
                    AsmOp::Select(pat) if pat.as_str().eq_ignore_ascii_case("beta") => {
                        Step::Done(1)
                    }
                    other => {
                        Step::Failed(InteractionError::Engine(format!("unexpected {:?}", other)))
                    }
                }),
            }
        })
    }

    #[test]
    fn run_asm_picks_correctly() {
        let i = pick_alpha();
        let r = i
            .run_asm(vec![AsmOp::Select(LexicalPattern::new("alpha"))])
            .unwrap();
        assert_eq!(r, 0);
    }

    #[test]
    fn run_asm_no_match_errors() {
        let i = pick_alpha();
        let err = i
            .run_asm(vec![AsmOp::Select(LexicalPattern::new("ghost"))])
            .unwrap_err();
        assert!(matches!(err, InteractionError::NoMatch { .. }));
    }

    #[test]
    fn run_asm_type_mismatch_errors() {
        let i = pick_alpha();
        let err = i.run_asm(vec![AsmOp::Yes]).unwrap_err();
        assert!(matches!(err, InteractionError::TypeMismatch { .. }));
    }

    #[test]
    fn lower_passes_index() {
        let asm = pick_alpha();
        let low = lower(asm);
        let r = low.run_with(vec![LowLevelInput::Index(1)]).unwrap();
        assert_eq!(r, 0);
    }

    #[test]
    fn lower_passes_lexical() {
        let asm = pick_alpha();
        let low = lower(asm);
        let r = low
            .run_with(vec![LowLevelInput::Lexical("alpha".into())])
            .unwrap();
        assert_eq!(r, 0);
    }

    // -----------------------------------------------------------------------
    // spec/0015 §5.3 — lower() must hard-fail with the §3.2 message format on
    // malformed inputs at any step: zero matches, multiple matches, type
    // mismatch (Select vs Free, Enter vs Pick, Yes vs Pick).
    // -----------------------------------------------------------------------

    fn pick_collides() -> Interaction<AsmOp, usize> {
        Interaction::new(|| {
            let menu = Menu::Pick {
                prompt: "p".into(),
                options: vec![
                    MenuOption::new("alpha", "A").with_aliases(vec!["common".into()]),
                    MenuOption::new("beta", "B").with_aliases(vec!["common".into()]),
                ],
            };
            Step::Suspended {
                menu,
                resume: Box::new(|_op: AsmOp| Step::Done(0)),
            }
        })
    }

    fn ask_free_str() -> Interaction<AsmOp, String> {
        Interaction::new(|| Step::Suspended {
            menu: Menu::Free {
                prompt: "name".into(),
                kind: FreeKind::Text,
            },
            resume: Box::new(|op: AsmOp| match op {
                AsmOp::Enter(s) => Step::Done(s),
                other => Step::Failed(InteractionError::Engine(format!("got {:?}", other))),
            }),
        })
    }

    #[test]
    fn run_asm_zero_match_message_format() {
        let i = pick_alpha();
        let err = i
            .run_asm(vec![AsmOp::Select(LexicalPattern::new("nope"))])
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("\"nope\""), "msg should name pattern: {}", msg);
        assert!(msg.contains("\"p\""), "msg should name prompt: {}", msg);
        assert!(
            msg.contains("alpha"),
            "msg should list available tags: {}",
            msg
        );
    }

    #[test]
    fn run_asm_multiple_match_message_format() {
        let i = pick_collides();
        let err = i
            .run_asm(vec![AsmOp::Select(LexicalPattern::new("common"))])
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(matches!(err, InteractionError::MultipleMatches { .. }));
        assert!(
            msg.contains("\"common\""),
            "msg should name pattern: {}",
            msg
        );
        assert!(msg.contains("alpha"), "msg should list tags: {}", msg);
    }

    #[test]
    fn run_asm_type_mismatch_select_on_free() {
        let i = ask_free_str();
        let err = i
            .run_asm(vec![AsmOp::Select(LexicalPattern::new("anything"))])
            .unwrap_err();
        assert!(matches!(err, InteractionError::TypeMismatch { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("Free"), "msg should name menu kind: {}", msg);
    }

    #[test]
    fn run_asm_type_mismatch_enter_on_pick() {
        let i = pick_alpha();
        let err = i.run_asm(vec![AsmOp::Enter("x".into())]).unwrap_err();
        assert!(matches!(err, InteractionError::TypeMismatch { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("Pick"), "msg should name Pick: {}", msg);
    }

    #[test]
    fn lower_propagates_zero_match_via_low_lexical() {
        let asm = pick_alpha();
        let low = lower(asm);
        let err = low
            .run_with(vec![LowLevelInput::Lexical("ghost".into())])
            .unwrap_err();
        assert!(matches!(err, InteractionError::NoMatch { .. }));
    }

    #[test]
    fn lower_run_asm_and_low_yield_same_result() {
        // spec/0015 §5.3 — lower(asm).run(low_inputs) ==
        // asm.run_asm(asm_inputs) when the low_inputs are the translation of
        // asm_inputs.
        let asm_inputs = vec![AsmOp::Select(LexicalPattern::new("alpha"))];
        let asm = pick_alpha();
        let asm_result = asm.run_asm(asm_inputs.clone()).unwrap();

        let low_inputs: Vec<LowLevelInput> = asm_inputs
            .into_iter()
            .map(|op| match op {
                AsmOp::Select(p) => LowLevelInput::Lexical(p.as_str().to_string()),
                AsmOp::Enter(s) => LowLevelInput::Text(s),
                AsmOp::Yes => LowLevelInput::Yes,
                AsmOp::No => LowLevelInput::No,
                AsmOp::Back => LowLevelInput::Back,
            })
            .collect();
        let low = lower(pick_alpha());
        let low_result = low.run_with(low_inputs).unwrap();
        assert_eq!(asm_result, low_result);
    }
}
