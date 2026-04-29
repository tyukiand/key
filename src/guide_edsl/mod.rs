//! Guide-EDSL — interpretation-polymorphic source of truth for the audit
//! guide (spec/0010-guide-edsl.txt).
//!
//! The guide tree (`tree::root()`) is interpreted four ways:
//!  1. as terse markdown (default `key audit guide`),
//!  2. as verbose markdown (`key audit guide -v`),
//!  3. as a materialized on-disk project (controls + fixtures),
//!  4. as a feature-coverage set, used by the exhaustiveness check.
//!
//! Some interpretations (coverage, materialize, FeatureBearing) only run
//! inside `#[test]`s; that's by design — they exist to enforce invariants
//! against the same single source of truth that the runtime uses.

#![allow(dead_code)]

pub mod coverage;
pub mod feature_bearing;
pub mod features;
pub mod filter;
pub mod materialize;
pub mod nodes;
pub mod text;
pub mod tree;
