//! Security kernel.
//!
//! All external-process invocation funnels through `exec::safe_exec`.
//! See `spec/0014-safe-exec-kernel.txt`.

pub mod exec;
