//! Security kernel.
//!
//! All external-process invocation funnels through `exec::safe_exec` (per
//! spec/0014); all OS-touching reads/writes / env loading funnel through
//! `os_effects::OsEffects` (per spec/0016 §A and spec/0017 §A.0). Every
//! byte that crosses the OsEffects boundary is filtered through
//! `redact::redact_value` to enforce the no-secrets-leave-the-kernel
//! invariant (spec/0017 §B).
//!
//! The unredacted-allowlist (`unredacted::UnredactedMatcher`) is the SOLE
//! configuration surface for opting out of redaction (spec/0017 §B.9).

pub mod exec;
pub mod os_effects;
pub mod redact;
pub mod unredacted;
