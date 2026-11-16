//! Core library for Dynamis Power.
//!
//! Phase 1 is **read-only**: sensors and a hardware inventory. Nothing in this
//! crate changes any system setting. See `docs/ARCHITECTURE.md` for where this
//! fits and `docs/SAFETY.md` for the rules writing code must follow.
//!
//! Everything reads through [`fs::SysRoot`], so tests run against fixture
//! directories instead of the real `/sys` and `/proc`.

pub mod fs;
pub mod inventory;
pub mod logfmt;
pub mod sensors;

pub use fs::SysRoot;
