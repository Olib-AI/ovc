//! `ovc-llm` — Local LLM integration for OVC.
//!
//! Provides an OpenAI-compatible API client for local LLM servers (Ollama,
//! LM Studio, etc.) with intelligent context building for version control
//! operations.
//!
//! # Features
//!
//! - **Commit message generation** from staged diffs
//! - **PR code review** with streaming feedback
//! - **Diff explanation** in plain English
//! - **PR description generation** from commits and diffs
//!
//! # Architecture
//!
//! The crate is structured in layers:
//!
//! - [`config`] — Configuration hierarchy (server defaults + per-repo overrides)
//! - [`client`] — HTTP client with streaming SSE support
//! - [`context`] — Token-budget-aware context building with diff filtering
//! - [`prompts`] — System prompt templates for each feature

pub mod client;
pub mod config;
pub mod context;
pub mod error;
pub mod prompts;

pub use client::{ChatMessage, LlmClient, StreamChunk};
pub use config::{LlmServerConfig, ResolvedLlmConfig, resolve_config};
pub use context::{ContextBuilder, DiffBatch, FileDiffEntry, PassPlan};
pub use error::LlmError;
