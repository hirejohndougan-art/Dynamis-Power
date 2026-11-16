//! Core library for Dynamis Power.
//!
//! Sensors, a hardware inventory, setting plugins and the engine that changes
//! settings safely. See `docs/ARCHITECTURE.md` for where this fits and
//! `docs/SAFETY.md` for the rules writing code must follow.
//!
//! Everything reads through [`fs::SysRoot`], so tests run against fixture
//! directories instead of the real `/sys` and `/proc`. The only write path is
//! the private `writer` module, called only by [`engine::Engine`].

pub mod engine;
pub mod fs;
pub mod gauge;
pub mod inventory;
pub mod logfmt;
pub mod profiles;
pub mod report;
pub mod selfstat;
pub mod sensors;
pub mod settings;
mod writer;

pub use fs::SysRoot;
