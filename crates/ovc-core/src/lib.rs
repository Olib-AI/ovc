//! `ovc-core` — Core library for OVC (Olib Version Control).
//!
//! OVC stores each repository as a single encrypted `.ovc` blob file.
//! This crate provides the foundational types and operations:
//!
//! - **[`id`]** — BLAKE3-based content-addressable object identifiers
//! - **[`object`]** — The object model (blobs, trees, commits, tags)
//! - **[`serialize`]** — Canonical binary serialization via postcard
//! - **[`crypto`]** — Argon2id key derivation + XChaCha20-Poly1305 encryption
//! - **[`compression`]** — Zstandard compression utilities
//! - **[`keys`]** — SSH-style key pair management (Ed25519 + X25519)
//! - **[`format`]** — `.ovc` binary file format structures
//! - **[`store`]** — In-memory content-addressable object store
//! - **[`refs`]** — Reference store (branches, tags, HEAD, reflog)
//! - **[`index`]** — Staging area for tracking files to commit
//! - **[`diff`]** — Myers diff algorithm for line-oriented comparison
//! - **[`merge`]** — Three-way merge for content and trees
//! - **[`ignore`]** — Ignore pattern matching (.ovcignore, .gitignore)
//! - **[`workdir`]** — Working directory operations and status
//! - **[`repository`]** — High-level repository operations
//! - **[`config`]** — Repository configuration
//! - **[`stash`]** — Stash store for saving/restoring index state
//! - **[`rebase`]** — Rebase operations (replay commits onto a new base)
//! - **[`cherry_pick`]** — Cherry-pick a single commit onto HEAD
//! - **[`bisect`]** — Binary search for regression-introducing commits
//! - **[`gc`]** — Garbage collection for unreachable objects
//! - **[`lock`]** — Cross-process file locking for `.ovc` repositories
//! - **[`conflict`]** — Conflict detection for concurrent modifications and iCloud sync
//! - **[`wal`]** — Write-ahead log for crash recovery
//! - **[`error`]** — Error types
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use ovc_core::repository::Repository;
//! use ovc_core::object::Object;
//!
//! let mut repo = Repository::init(Path::new("my-repo.ovc"), b"secret")?;
//! let oid = repo.insert_object(&Object::Blob(b"hello".to_vec()))?;
//! repo.save()?;
//! # Ok::<(), ovc_core::error::CoreError>(())
//! ```

pub mod access;
pub mod bisect;
pub mod blame;
pub mod cherry_pick;
pub mod compression;
pub mod config;
pub mod conflict;
pub mod crypto;
pub mod diff;
pub mod error;
pub mod format;
pub mod gc;
pub mod grep;
pub mod id;
pub mod ignore;
pub mod index;
pub mod keys;
pub mod lock;
pub mod merge;
pub mod notes;
pub mod object;
pub mod pulls;
pub mod rebase;
pub mod refs;
pub mod repository;
pub mod revert;
pub mod serialize;
pub mod stash;
pub mod store;
pub mod submodule;
pub mod wal;
pub mod workdir;
