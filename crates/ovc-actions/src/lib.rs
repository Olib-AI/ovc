//! `ovc-actions` — Actions engine for OVC.
//!
//! Provides lint, format, build, test, audit, and custom script execution
//! integrated into OVC's commit and push workflows.
//!
//! # Modules
//!
//! - [`config`] — YAML-based action configuration
//! - [`runner`] — Action execution engine
//! - [`builtin`] — Built-in actions (secret scan, whitespace, etc.)
//! - [`detect`] — Language and toolchain detection
//! - [`templates`] — Per-language action template generation
//! - [`history`] — Run history persistence
//! - [`hooks`] — Hook integration for commit/push workflows
//! - [`error`] — Error types

pub mod builtin;
pub mod config;
pub mod depcheck;
pub mod detect;
pub mod docker;
pub mod error;
pub mod history;
pub mod hooks;
pub mod runner;
pub mod secrets;
pub mod templates;

#[cfg(test)]
mod tests;
