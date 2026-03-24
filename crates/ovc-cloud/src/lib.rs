//! `ovc-cloud` — Cloud sync layer for OVC (Olib Version Control).
//!
//! Provides storage backends and a sync engine for uploading and downloading
//! `.ovc` repository files to/from cloud storage. Uses content-defined
//! chunking (FastCDC-style gear hash) to minimize data transfer by
//! uploading only changed chunks.
//!
//! # Backends
//!
//! - [`local::LocalBackend`] — Filesystem-based backend for testing and local mirrors.
//! - [`gcs::GcsBackend`] — Google Cloud Storage backend using the JSON API.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────┐    chunks     ┌──────────────────┐
//! │ .ovc file│──────────────>│  StorageBackend   │
//! └──────────┘  (FastCDC)    │ (local/GCS/S3/..) │
//!       ▲                    └──────────────────┘
//!       │                           ▲
//!       │ reassemble                │ put/get/list
//!       │                           │
//! ┌─────┴──────┐             ┌──────┴──────┐
//! │ SyncEngine │────────────>│  Manifest   │
//! └────────────┘             └─────────────┘
//! ```

pub mod backend;
pub mod chunker;
pub mod error;
pub mod gcs;
pub mod local;
pub mod manifest;
pub mod sync;

pub use backend::StorageBackend;
pub use error::{CloudError, CloudResult};
pub use local::LocalBackend;
pub use sync::{PullResult, PushResult, SyncEngine, SyncStatus};
