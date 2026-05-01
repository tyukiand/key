//! Redaction filter — see `spec/0017-env-redaction-and-os-effects.txt` §B.
//!
//! Pure, no I/O. Applied at the OsEffects boundary so no sensitive value can
//! leave the kernel unredacted. Three detection layers:
//!
//!   * Layer 1 (§B.2) — variable-name substring match (case-insensitive).
//!     Only fires when `name_hint` is `Some(_)`, i.e. for env values.
//!     Whitelist (`PATH`, `*_PROXY`) is never redacted by name.
//!   * Layer 2 (§B.3) — value-shape regex set (GitHub PAT / OAuth, Slack,
//!     AWS access key, JWT, PEM private-key marker, long-hex 40+).
//!   * Layer 3 (§B.4) — high-entropy heuristic (length ≥ 20, contains
//!     digit AND letter, Shannon entropy ≥ 4.5 bits/char, not a path).
//!     **DEFAULT-ON**; suppression is via the `unredacted:` allowlist.
//!
//! Redaction strategy is the SOLE one allowed by the spec: literal pattern
//! `REDACTED42` looped, truncated to the original byte length. Alphabet is
//! a subset of base64 ∩ base64url so downstream regex/parser predicates do
//! not choke on the redacted payload, and length is preserved so length-
//! based signals (e.g. "this looks like a 40-char hex") do not flip after
//! redaction.

use std::sync::OnceLock;

use regex::Regex;

use crate::security::unredacted::UnredactedMatcher;

/// Literal redaction pattern. Letters + digits only — both characters live
/// in the base64 and base64url alphabets (spec §B.1).
const REDACTION_PATTERN: &str = "REDACTED42";

/// Variable-name substring patterns (Layer 1). Case-insensitive substring
/// match against the env variable's name (spec §B.2).
const NAME_NEEDLES: &[&str] = &[
    "token",
    "password",
    "passwd",
    "secret",
    "apikey",
    "api_key",
    "private_key",
    "privatekey",
    "pgp",
    "gpg",
    "ssh_key",
    "auth",
    "credential",
    "session",
    "cookie",
    "oauth",
    "bearer",
    "access_key",
    "refresh_key",
    "signing_key",
    "encryption_key",
];

/// Variable names exempt from Layer-1 name-based redaction even on substring
/// match (spec §B.2). The values may still be redacted by Layer 2 or 3.
const NAME_WHITELIST: &[&str] = &["path", "http_proxy", "https_proxy", "no_proxy"];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Carries the unredacted-allowlist and per-call hints into `redact_value`.
/// Held immutably by `RealOsEffects` for its lifetime (spec §C.5).
#[derive(Debug, Clone, Default)]
pub struct RedactionCtx {
    pub allowlist: Vec<UnredactedMatcher>,
}

impl RedactionCtx {
    pub fn new(allowlist: Vec<UnredactedMatcher>) -> Self {
        Self { allowlist }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    fn allowed(&self, value: &str) -> bool {
        self.allowlist.iter().any(|m| m.matches(value))
    }
}

/// Result of `redact_value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedValue {
    text: String,
    reason: Option<&'static str>,
}

impl RedactedValue {
    pub fn into_string(self) -> String {
        self.text
    }

    #[allow(dead_code)] // exposed to library consumers / future debug surfaces
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Short tag identifying which rule triggered redaction, or `None` if
    /// the value was returned verbatim.
    #[allow(dead_code)] // exposed to library consumers / future debug surfaces
    pub fn reason(&self) -> Option<&'static str> {
        self.reason
    }

    /// True iff the value was rewritten (i.e. matched at least one rule
    /// and was not suppressed by the allowlist).
    pub fn was_redacted(&self) -> bool {
        self.reason.is_some()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Pure redaction (spec §B.5). Returns the original value verbatim when no
/// rule fires, or the value is suppressed by the unredacted-allowlist;
/// returns the length-preserved REDACTED42-loop otherwise.
pub fn redact_value(value: &str, ctx: &RedactionCtx, name_hint: Option<&str>) -> RedactedValue {
    if value.is_empty() {
        return RedactedValue {
            text: String::new(),
            reason: None,
        };
    }

    if ctx.allowed(value) {
        return RedactedValue {
            text: value.to_string(),
            reason: None,
        };
    }

    if let Some(reason) = detect(value, name_hint) {
        return RedactedValue {
            text: redact_to_length(value.len()),
            reason: Some(reason),
        };
    }

    RedactedValue {
        text: value.to_string(),
        reason: None,
    }
}

/// Convenience: redact a multi-line file body line-by-line (spec §B.7).
/// Preserves the trailing newline (or absence thereof) of the original.
/// No `name_hint` — file content is not associated with a variable name.
pub fn redact_file_content(content: &str, ctx: &RedactionCtx) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(content.len());
    let mut lines = content.split_inclusive('\n');
    for line in lines.by_ref() {
        let (body, sep) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let red = redact_line_token_aware(body, ctx);
        out.push_str(&red);
        out.push_str(sep);
    }
    out
}

/// Inspect a value (or any free-floating text fragment) and report whether
/// it would be redacted (spec §B.8 — `looks-like-password`). Uses the same
/// allowlist suppression as `redact_value` so meta-controls remain
/// consistent with the actual redactor.
pub fn looks_like_password(value: &str, ctx: &RedactionCtx, name_hint: Option<&str>) -> bool {
    redact_value(value, ctx, name_hint).was_redacted()
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn redact_line_token_aware(line: &str, ctx: &RedactionCtx) -> String {
    if line.is_empty() {
        return String::new();
    }
    // Whole-line shape match (PEM markers, JWT-on-its-own-line, 40-char
    // hex line, etc.). Layer-3 entropy is intentionally NOT consulted here:
    // an `export GITHUB_TOKEN=ghp_…` line is itself high-entropy and we
    // would otherwise over-redact the surrounding `export NAME=` framing.
    if layer2_value_shape(line).is_some() && !ctx.allowed(line) {
        return redact_to_length(line.len());
    }
    // Otherwise scan for whitespace-/punctuation-delimited tokens and
    // redact each suspicious one in place. This catches `password = ghp_…`
    // style assignment lines without rewriting the whole structure.
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Pass through leading separator chars verbatim.
        let mut j = i;
        while j < bytes.len() && is_token_separator(bytes[j] as char) {
            j += 1;
        }
        if j > i {
            out.push_str(&line[i..j]);
            i = j;
            continue;
        }
        // Read one token up to the next separator.
        let mut k = i;
        while k < bytes.len() && !is_token_separator(bytes[k] as char) {
            k += 1;
        }
        let token = &line[i..k];
        let (lpad, payload, rpad) = strip_paired_quotes(token);
        if !payload.is_empty() && detect(payload, None).is_some() && !ctx.allowed(payload) {
            out.push_str(lpad);
            out.push_str(&redact_to_length(payload.len()));
            out.push_str(rpad);
        } else {
            out.push_str(token);
        }
        i = k;
    }
    out
}

fn is_token_separator(c: char) -> bool {
    c.is_whitespace() || c == '=' || c == ',' || c == ';'
}

fn strip_paired_quotes(s: &str) -> (&str, &str, &str) {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return (&s[..1], &s[1..s.len() - 1], &s[s.len() - 1..]);
        }
    }
    ("", s, "")
}

/// Compose the length-preserving redaction text. Bytes-only — the redaction
/// alphabet is ASCII so byte-length and char-length coincide.
fn redact_to_length(n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let pat = REDACTION_PATTERN.as_bytes();
    let mut out = String::with_capacity(n);
    for i in 0..n {
        out.push(pat[i % pat.len()] as char);
    }
    out
}

/// Detection — returns `Some(reason)` if any layer matches.
fn detect(value: &str, name_hint: Option<&str>) -> Option<&'static str> {
    if let Some(name) = name_hint {
        if let Some(r) = layer1_name(name) {
            return Some(r);
        }
    }
    if let Some(r) = layer2_value_shape(value) {
        return Some(r);
    }
    if let Some(r) = layer3_high_entropy(value) {
        return Some(r);
    }
    None
}

// ---- Layer 1 ---------------------------------------------------------------

fn layer1_name(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    for w in NAME_WHITELIST {
        if &lower == w {
            return None;
        }
    }
    for needle in NAME_NEEDLES {
        if lower.contains(needle) {
            return Some("name");
        }
    }
    None
}

// ---- Layer 2 ---------------------------------------------------------------

struct ValueShape {
    name: &'static str,
    re: Regex,
}

fn value_shape_set() -> &'static [ValueShape] {
    static SET: OnceLock<Vec<ValueShape>> = OnceLock::new();
    SET.get_or_init(|| {
        vec![
            ValueShape {
                name: "github_pat",
                re: Regex::new(r"^ghp_[A-Za-z0-9]{36,}$").unwrap(),
            },
            ValueShape {
                name: "github_pat_fine",
                re: Regex::new(r"^github_pat_[A-Za-z0-9_]{82,}$").unwrap(),
            },
            ValueShape {
                name: "github_oauth",
                re: Regex::new(r"^gho_[A-Za-z0-9]{36,}$").unwrap(),
            },
            ValueShape {
                name: "slack_token",
                re: Regex::new(r"^xox[baprs]-[A-Za-z0-9-]{10,}$").unwrap(),
            },
            ValueShape {
                name: "aws_access_key",
                re: Regex::new(r"^AKIA[0-9A-Z]{16}$").unwrap(),
            },
            ValueShape {
                name: "aws_temp_access_key",
                re: Regex::new(r"^ASIA[0-9A-Z]{16}$").unwrap(),
            },
            ValueShape {
                name: "jwt",
                re: Regex::new(r"^eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}$")
                    .unwrap(),
            },
            ValueShape {
                name: "pem_private_key",
                re: Regex::new(r"^-----BEGIN .* PRIVATE KEY-----").unwrap(),
            },
            ValueShape {
                name: "long_hex",
                re: Regex::new(r"^[A-Fa-f0-9]{40,}$").unwrap(),
            },
        ]
    })
    .as_slice()
}

fn layer2_value_shape(value: &str) -> Option<&'static str> {
    for shape in value_shape_set() {
        if shape.re.is_match(value) {
            return Some(shape.name);
        }
    }
    None
}

// ---- Layer 3 ---------------------------------------------------------------

fn layer3_high_entropy(value: &str) -> Option<&'static str> {
    if value.len() < 20 {
        return None;
    }
    if value.contains('/') || value.contains('~') || value.contains('\\') {
        return None;
    }
    let mut has_letter = false;
    let mut has_digit = false;
    for c in value.chars() {
        if c.is_ascii_alphabetic() {
            has_letter = true;
        }
        if c.is_ascii_digit() {
            has_digit = true;
        }
        if has_letter && has_digit {
            break;
        }
    }
    if !(has_letter && has_digit) {
        return None;
    }
    if shannon_entropy(value) >= 4.5 {
        Some("high_entropy")
    } else {
        None
    }
}

fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let bytes = s.as_bytes();
    let mut counts = [0u32; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let n = bytes.len() as f64;
    let mut h = 0.0_f64;
    for &c in &counts {
        if c == 0 {
            continue;
        }
        let p = c as f64 / n;
        h -= p * p.log2();
    }
    h
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_empty() -> RedactionCtx {
        RedactionCtx::empty()
    }

    fn redact(name: Option<&str>, value: &str) -> RedactedValue {
        redact_value(value, &ctx_empty(), name)
    }

    // ---- length & alphabet invariants ------------------------------------

    #[test]
    fn redaction_preserves_length() {
        let r = redact(Some("MY_TOKEN"), "abcdefghijklmnopqrstuvwxyz");
        assert!(r.was_redacted());
        assert_eq!(r.as_str().len(), "abcdefghijklmnopqrstuvwxyz".len());
    }

    #[test]
    fn redaction_alphabet_is_base64_subset() {
        let r = redact(Some("API_TOKEN"), "abcdefghijklmnop1234");
        let s = r.into_string();
        for c in s.chars() {
            assert!(
                c.is_ascii_uppercase() || c.is_ascii_digit(),
                "redaction alphabet must be uppercase letters + digits, found {:?}",
                c
            );
        }
    }

    #[test]
    fn empty_value_is_no_op() {
        let r = redact(Some("MY_TOKEN"), "");
        assert!(!r.was_redacted());
        assert_eq!(r.as_str(), "");
    }

    #[test]
    fn redaction_pattern_is_redacted42_loop() {
        // Length 36 → pattern (length 10) tiles 3× = 30 chars + first 6 chars
        // ("REDACT") of the next cycle.
        let r = redact(Some("MY_TOKEN"), "abcdefghijklmnopqrstuvwxyz0123456789");
        let expected = "REDACTED42REDACTED42REDACTED42REDACT";
        assert_eq!(r.as_str().len(), 36);
        assert_eq!(r.as_str(), expected);
    }

    // ---- Layer 1 ----------------------------------------------------------

    #[test]
    fn layer1_token_redacts() {
        for n in &[
            "GITHUB_TOKEN",
            "MY_PASSWORD",
            "MY_PASSWD",
            "MY_SECRET",
            "MY_APIKEY",
            "MY_API_KEY",
            "MY_PRIVATE_KEY",
            "MY_PRIVATEKEY",
            "GPG_KEY",
            "PGP_KEY",
            "MY_SSH_KEY",
            "BASIC_AUTH",
            "MY_CREDENTIAL",
            "MY_SESSION",
            "MY_COOKIE",
            "MY_OAUTH",
            "BEARER_TOKEN",
            "MY_ACCESS_KEY",
            "MY_REFRESH_KEY",
            "MY_SIGNING_KEY",
            "MY_ENCRYPTION_KEY",
        ] {
            let r = redact(Some(n), "ordinary_value");
            assert!(r.was_redacted(), "{} should redact by name", n);
        }
    }

    #[test]
    fn layer1_whitelist_not_redacted_by_name() {
        for n in &["PATH", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"] {
            let r = redact(Some(n), "/usr/bin:/usr/local/bin");
            assert!(
                !r.was_redacted(),
                "{} must not be redacted by name (whitelist)",
                n
            );
        }
    }

    #[test]
    fn layer1_does_not_fire_without_name_hint() {
        let r = redact(None, "ordinary_value");
        assert!(!r.was_redacted());
    }

    #[test]
    fn layer1_case_insensitive() {
        let r = redact(Some("My_Token_Var"), "ordinary");
        assert!(r.was_redacted());
    }

    // ---- Layer 2 ----------------------------------------------------------

    #[test]
    fn layer2_github_pat() {
        let r = redact(None, "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB");
        assert_eq!(r.reason(), Some("github_pat"));
    }

    #[test]
    fn layer2_github_pat_fine_grained() {
        let v = format!("github_pat_{}", "A".repeat(82));
        let r = redact(None, &v);
        assert_eq!(r.reason(), Some("github_pat_fine"));
    }

    #[test]
    fn layer2_github_oauth() {
        let r = redact(None, "gho_abcdefghijklmnopqrstuvwxyz0123456789AB");
        assert_eq!(r.reason(), Some("github_oauth"));
    }

    #[test]
    fn layer2_slack_token() {
        let r = redact(None, "xoxb-1234567890-AB-cdefghij");
        assert_eq!(r.reason(), Some("slack_token"));
    }

    #[test]
    fn layer2_aws_access_key() {
        let r = redact(None, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(r.reason(), Some("aws_access_key"));
    }

    #[test]
    fn layer2_aws_temp_access_key() {
        let r = redact(None, "ASIAIOSFODNN7EXAMPLE");
        assert_eq!(r.reason(), Some("aws_temp_access_key"));
    }

    #[test]
    fn layer2_jwt() {
        let r = redact(
            None,
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        );
        assert_eq!(r.reason(), Some("jwt"));
    }

    #[test]
    fn layer2_pem_private_key() {
        let r = redact(None, "-----BEGIN OPENSSH PRIVATE KEY-----\nMIIB");
        assert_eq!(r.reason(), Some("pem_private_key"));
    }

    #[test]
    fn layer2_long_hex() {
        let v: String = std::iter::repeat('a').take(40).collect();
        let r = redact(None, &v);
        assert_eq!(r.reason(), Some("long_hex"));
    }

    #[test]
    fn layer2_short_hex_does_not_match() {
        let v: String = std::iter::repeat('a').take(20).collect();
        let r = redact(None, &v);
        // 20 chars of 'a' has entropy 0; not a 40-char hex; so not redacted.
        assert!(!r.was_redacted(), "got reason = {:?}", r.reason());
    }

    // ---- Layer 3 ----------------------------------------------------------

    #[test]
    fn layer3_high_entropy_random_redacts() {
        // 32-char Base64ish: entropy will be high, mixed alpha+digits.
        let r = redact(None, "Z9q2Ld0xYwR4nPv8KsM7tBfH3aJgUj1V");
        assert_eq!(r.reason(), Some("high_entropy"));
    }

    #[test]
    fn layer3_short_value_not_redacted() {
        let r = redact(None, "Z9q2Ld0x");
        assert!(!r.was_redacted());
    }

    #[test]
    fn layer3_path_not_redacted() {
        // /usr/local/share/foo with mixed alpha+digits, length ≥ 20, but contains
        // path separators — high-entropy is suppressed.
        let r = redact(None, "/usr/local/share/foo123");
        assert!(!r.was_redacted());
    }

    #[test]
    fn layer3_letters_only_not_redacted() {
        let r = redact(None, "abcdefghijklmnopqrstuvwxyzabc");
        // No digit → fails layer-3 guard; entropy alone isn't enough.
        assert!(!r.was_redacted());
    }

    #[test]
    fn layer3_digits_only_not_redacted() {
        let r = redact(None, "12345678901234567890123456");
        assert!(!r.was_redacted());
    }

    // ---- Allowlist suppression -------------------------------------------

    #[test]
    fn allowlist_value_suppresses_redaction() {
        let val = "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB";
        let ctx = RedactionCtx::new(vec![UnredactedMatcher::value(val).unwrap()]);
        let r = redact_value(val, &ctx, None);
        assert!(!r.was_redacted());
        assert_eq!(r.as_str(), val);
    }

    #[test]
    fn allowlist_prefix_suppresses_redaction() {
        let val = "sha256:ddd31a130427c27518df266943a5308ed92d4b226cc639f5a8f1002816174301";
        let ctx = RedactionCtx::new(vec![UnredactedMatcher::prefix("sha256:").unwrap()]);
        let r = redact_value(val, &ctx, None);
        assert!(!r.was_redacted());
    }

    #[test]
    fn allowlist_suppresses_layer1_too() {
        let ctx = RedactionCtx::new(vec![UnredactedMatcher::value("ordinary").unwrap()]);
        let r = redact_value("ordinary", &ctx, Some("MY_TOKEN"));
        assert!(!r.was_redacted());
    }

    // ---- File content ----------------------------------------------------

    #[test]
    fn file_content_redacts_inline_token() {
        let body = "api_key = ghp_abcdefghijklmnopqrstuvwxyz0123456789AB\n";
        let out = redact_file_content(body, &ctx_empty());
        // The whole-line redaction wraps `api_key = ghp_…`. Since the token
        // is the only suspicious substring, only that token is overwritten.
        assert!(out.starts_with("api_key = "), "got: {:?}", out);
        assert!(out.contains("REDACTED42"), "got: {:?}", out);
        // Length preserved.
        assert_eq!(out.len(), body.len());
    }

    #[test]
    fn file_content_passes_normal_lines() {
        let body = "hello = world\nfoo = bar\n";
        let out = redact_file_content(body, &ctx_empty());
        assert_eq!(out, body);
    }

    #[test]
    fn looks_like_password_round_trip() {
        let ctx = ctx_empty();
        assert!(looks_like_password(
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB",
            &ctx,
            None
        ));
        assert!(!looks_like_password("hello", &ctx, None));
        assert!(looks_like_password("ordinary", &ctx, Some("MY_TOKEN")));
    }
}
