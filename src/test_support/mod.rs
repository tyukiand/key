//! Test-support infrastructure (cfg(feature = "testing")).
//!
//! Mock backends + canned effects for unit and integration tests. Lives
//! OUTSIDE `src/security/` per spec/0017 §A.0: the security kernel is
//! production-only; test infrastructure does not dilute its audit surface.

#![cfg(feature = "testing")]

pub mod mock_os_effects;
