//! `ovc-git` — Bidirectional conversion between Git and OVC repositories.
//!
//! This crate provides a lightweight git object parser/writer that reads and
//! writes `.git` directories directly, without requiring external git tooling
//! or heavy dependencies. It supports:
//!
//! - **[`import`]** — Converting a git repository into an encrypted OVC `.ovc` file
//! - **[`export`]** — Converting an OVC repository back into a standard git repository
//! - **[`git_objects`]** — Parsing and computing SHA1 hashes for git loose objects
//! - **[`git_refs`]** — Reading and writing git references (branches, tags, HEAD)
//! - **[`write_git`]** — Writing git loose objects (zlib-compressed)
//! - **[`oid_map`]** — Bidirectional mapping between git SHA1 and OVC `ObjectId`

pub mod error;
pub mod export;
pub mod git_objects;
pub mod git_refs;
pub mod import;
pub mod oid_map;
pub mod write_git;
