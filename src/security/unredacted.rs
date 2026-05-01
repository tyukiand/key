//! `UnredactedMatcher` — the SOLE configuration surface for opting out of
//! redaction (spec/0017 §B.9, §C.1). Literal-only by design — no regex.
//!
//! A matcher is either:
//!   - `Value(literal)`: the value redacts iff it is NOT byte-for-byte equal
//!     to `literal`.
//!   - `Prefix(literal)`: the value redacts iff it does NOT start with
//!     `literal`.
//!
//! Empty / whitespace-only literals are rejected at construction time
//! (`UnredactedMatcher::value` / `UnredactedMatcher::prefix`).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnredactedMatcher {
    Value(String),
    Prefix(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnredactedMatcherError {
    Empty,
    WhitespaceOnly,
}

impl fmt::Display for UnredactedMatcherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnredactedMatcherError::Empty => {
                f.write_str("unredacted matcher literal must not be empty")
            }
            UnredactedMatcherError::WhitespaceOnly => f.write_str(
                "unredacted matcher literal must not be whitespace-only \
                 (would suppress every legitimate value at the boundary)",
            ),
        }
    }
}

impl std::error::Error for UnredactedMatcherError {}

impl UnredactedMatcher {
    /// Construct a `Value` matcher. Sanitizing constructor: empty / whitespace-
    /// only literals are rejected (spec/0017 §B.9).
    pub fn value(s: impl Into<String>) -> Result<Self, UnredactedMatcherError> {
        let s = s.into();
        sanitize(&s)?;
        Ok(UnredactedMatcher::Value(s))
    }

    /// Construct a `Prefix` matcher. Sanitizing constructor: empty / whitespace-
    /// only literals are rejected (spec/0017 §B.9).
    pub fn prefix(s: impl Into<String>) -> Result<Self, UnredactedMatcherError> {
        let s = s.into();
        sanitize(&s)?;
        Ok(UnredactedMatcher::Prefix(s))
    }

    /// True iff `value` should NOT be redacted under this matcher.
    pub fn matches(&self, value: &str) -> bool {
        match self {
            UnredactedMatcher::Value(v) => value == v,
            UnredactedMatcher::Prefix(p) => value.starts_with(p.as_str()),
        }
    }

    /// The matcher's literal payload (for diagnostics / serialization).
    pub fn literal(&self) -> &str {
        match self {
            UnredactedMatcher::Value(v) => v,
            UnredactedMatcher::Prefix(p) => p,
        }
    }

    /// The matcher's kind tag (`"value"` or `"prefix"`).
    pub fn kind(&self) -> &'static str {
        match self {
            UnredactedMatcher::Value(_) => "value",
            UnredactedMatcher::Prefix(_) => "prefix",
        }
    }
}

fn sanitize(s: &str) -> Result<(), UnredactedMatcherError> {
    if s.is_empty() {
        return Err(UnredactedMatcherError::Empty);
    }
    if s.chars().all(char::is_whitespace) {
        return Err(UnredactedMatcherError::WhitespaceOnly);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_constructor_accepts_literal() {
        assert_eq!(
            UnredactedMatcher::value("xyz"),
            Ok(UnredactedMatcher::Value("xyz".into()))
        );
    }

    #[test]
    fn prefix_constructor_accepts_literal() {
        assert_eq!(
            UnredactedMatcher::prefix("sha256:"),
            Ok(UnredactedMatcher::Prefix("sha256:".into()))
        );
    }

    #[test]
    fn empty_value_rejected() {
        assert_eq!(
            UnredactedMatcher::value(""),
            Err(UnredactedMatcherError::Empty)
        );
    }

    #[test]
    fn whitespace_only_value_rejected() {
        assert_eq!(
            UnredactedMatcher::value("   "),
            Err(UnredactedMatcherError::WhitespaceOnly)
        );
        assert_eq!(
            UnredactedMatcher::value("\t\n"),
            Err(UnredactedMatcherError::WhitespaceOnly)
        );
    }

    #[test]
    fn empty_prefix_rejected() {
        assert_eq!(
            UnredactedMatcher::prefix(""),
            Err(UnredactedMatcherError::Empty)
        );
    }

    #[test]
    fn value_match_is_byte_for_byte() {
        let m = UnredactedMatcher::value("xyz").unwrap();
        assert!(m.matches("xyz"));
        assert!(!m.matches("xy"));
        assert!(!m.matches("xyzz"));
        assert!(!m.matches("XYZ"));
    }

    #[test]
    fn prefix_match_is_starts_with() {
        let m = UnredactedMatcher::prefix("img_id_").unwrap();
        assert!(m.matches("img_id_"));
        assert!(m.matches("img_id_abc"));
        assert!(!m.matches("foo"));
        assert!(!m.matches("img_i"));
    }
}
