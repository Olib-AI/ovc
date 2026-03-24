//! High-level repository operations.
//!
//! [`Repository`] ties together the object store, encryption layer, and
//! `.ovc` file format to provide a complete interface for creating, opening,
//! reading, and writing OVC repositories.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::access::{AccessControl, AccessRole, BranchProtection, UserAccess};
use crate::compression::CompressionAlgorithm;
use crate::config::RepositoryConfig;
use crate::crypto::{self, CipherAlgorithm, KdfAlgorithm};
use crate::error::{CoreError, CoreResult};
use crate::format::{
    FORMAT_VERSION, FileHeader, FileTrailer, HEADER_SIZE, MIN_READER_VERSION, SegmentIndex,
    Superblock, TRAILER_SIZE,
};
use crate::gc::{self, GcResult};
use crate::id::ObjectId;
use crate::index::Index;
use crate::keys::{self, OvcKeyPair, OvcPublicKey, SealedKey};
use crate::object::{Commit, Identity, Object};
use crate::rebase::{self, RebaseError, RebaseResult};
use crate::refs::{RefStore, RefTarget};
use crate::stash::StashStore;
use crate::store::ObjectStore;
use crate::workdir::WorkDir;

/// Default Argon2 time cost (number of iterations).
const DEFAULT_TIME_COST: u32 = 3;
/// Default Argon2 memory cost in KiB (64 MiB).
const DEFAULT_MEMORY_COST_KIB: u32 = 65536;
/// Default Argon2 parallelism.
const DEFAULT_PARALLELISM: u8 = 1;

/// Maximum superblock size (256 MiB). Prevents malicious headers from
/// triggering unbounded allocation.
const MAX_SUPERBLOCK_SIZE: u64 = 256 * 1024 * 1024;

/// A handle to an open OVC repository backed by a single `.ovc` file.
pub struct Repository {
    /// Path to the `.ovc` file on disk.
    path: PathBuf,
    /// The derived master key (zeroized on drop).
    master_key: Zeroizing<[u8; 32]>,
    /// The file header (unencrypted, needed for re-serialization).
    header: FileHeader,
    /// The decrypted superblock.
    superblock: Superblock,
    /// The segment index (deserialized from the superblock).
    segment_index: SegmentIndex,
    /// In-memory object store for pending and cached objects.
    store: ObjectStore,
    /// Current file-write sequence number (incremented on each save).
    file_sequence: u64,
    /// Snapshot of file state at open time for conflict detection.
    file_snapshot: Option<crate::conflict::FileSnapshot>,
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repository")
            .field("path", &self.path)
            .field("header", &self.header)
            .field("segment_index_objects", &self.segment_index.objects.len())
            .field("store_count", &self.store.count())
            .field("file_sequence", &self.file_sequence)
            .field("file_snapshot", &self.file_snapshot)
            .finish_non_exhaustive()
    }
}

impl Repository {
    /// Creates a new OVC repository at `path`.
    ///
    /// The file must not already exist. A fresh `.ovc` file is written
    /// containing an empty object store and default configuration.
    pub fn init(path: &Path, password: &[u8]) -> CoreResult<Self> {
        if path.exists() {
            return Err(CoreError::AlreadyExists {
                path: path.display().to_string(),
            });
        }

        let salt = crypto::generate_salt();
        let master_key = crypto::derive_master_key(
            password,
            &salt,
            DEFAULT_TIME_COST,
            DEFAULT_MEMORY_COST_KIB,
            DEFAULT_PARALLELISM,
        )?;

        let header = FileHeader {
            format_version: FORMAT_VERSION,
            min_reader_version: MIN_READER_VERSION,
            kdf_algorithm: KdfAlgorithm::Argon2id,
            cipher_algorithm: CipherAlgorithm::XChaCha20Poly1305,
            compression_algorithm: CompressionAlgorithm::Zstd,
            argon2_time_cost: DEFAULT_TIME_COST,
            argon2_memory_cost_kib: DEFAULT_MEMORY_COST_KIB,
            argon2_parallelism: DEFAULT_PARALLELISM,
            kdf_salt: salt,
        };

        let segment_encryption_key = *crypto::generate_key();
        let hmac_key = *crypto::generate_key();
        let segment_index = SegmentIndex::new();
        let config = RepositoryConfig::default();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        let ref_store = RefStore::new(&config.default_branch);

        let superblock = Superblock {
            segment_encryption_key,
            index_offset: 0,
            index_length: 0,
            index_nonce: [0u8; 24],
            head_ref: format!("refs/heads/{}", config.default_branch),
            created_at: now,
            refs: BTreeMap::new(),
            config,
            hmac_key,
            stored_objects: BTreeMap::new(),
            ref_store,
            staging_index: Index::new(),
            stash_store: StashStore::new(),
            key_slots: Vec::new(),
            notes: BTreeMap::new(),
            submodules: BTreeMap::new(),
            access_control: AccessControl::default(),
            pull_request_store: crate::pulls::PullRequestStore::default(),
        };

        let store = ObjectStore::default();

        let mut repo = Self {
            path: path.to_path_buf(),
            master_key,
            header,
            superblock,
            segment_index,
            store,
            file_sequence: 0,
            file_snapshot: None,
        };

        repo.save()?;

        Ok(repo)
    }

    /// Opens an existing OVC repository at `path`.
    ///
    /// The password is used to derive the master key, which decrypts the
    /// superblock and segment index. Returns an error if the file does not
    /// exist, the magic bytes are wrong, or the password is incorrect.
    #[allow(clippy::too_many_lines, clippy::items_after_statements)]
    pub fn open(path: &Path, password: &[u8]) -> CoreResult<Self> {
        if !path.exists() {
            return Err(CoreError::NotInitialized {
                path: path.display().to_string(),
            });
        }

        let mut file = std::fs::File::open(path)?;

        // Read header.
        let mut header_bytes = [0u8; HEADER_SIZE];
        file.read_exact(&mut header_bytes)?;
        let header = FileHeader::deserialize(&header_bytes)?;

        // Validate KDF parameters before deriving master key to prevent
        // attacker-controlled headers from triggering excessive CPU/memory use.
        if header.argon2_time_cost < 1 || header.argon2_time_cost > 100 {
            return Err(CoreError::FormatError {
                reason: format!(
                    "argon2 time_cost {} is out of allowed range [1, 100]",
                    header.argon2_time_cost
                ),
            });
        }
        if header.argon2_memory_cost_kib < 1024 || header.argon2_memory_cost_kib > 1_048_576 {
            return Err(CoreError::FormatError {
                reason: format!(
                    "argon2 memory_cost_kib {} is out of allowed range [1024, 1048576]",
                    header.argon2_memory_cost_kib
                ),
            });
        }
        if header.argon2_parallelism < 1 || header.argon2_parallelism > 16 {
            return Err(CoreError::FormatError {
                reason: format!(
                    "argon2 parallelism {} is out of allowed range [1, 16]",
                    header.argon2_parallelism
                ),
            });
        }

        // Derive master key.
        let master_key = crypto::derive_master_key(
            password,
            &header.kdf_salt,
            header.argon2_time_cost,
            header.argon2_memory_cost_kib,
            header.argon2_parallelism,
        )?;

        // Read trailer (last TRAILER_SIZE bytes).
        file.seek(SeekFrom::End(
            -i64::try_from(TRAILER_SIZE).expect("TRAILER_SIZE fits in i64"),
        ))?;
        let mut trailer_bytes = [0u8; TRAILER_SIZE];
        file.read_exact(&mut trailer_bytes)?;
        let trailer = FileTrailer::deserialize(&trailer_bytes)?;

        // Validate superblock bounds against actual file size to prevent
        // a malicious superblock_length from triggering an OOM allocation.
        let file_size = file.seek(SeekFrom::End(0))?;
        if trailer.superblock_length > MAX_SUPERBLOCK_SIZE {
            return Err(CoreError::FormatError {
                reason: format!(
                    "superblock length {} exceeds maximum allowed size of {} bytes",
                    trailer.superblock_length, MAX_SUPERBLOCK_SIZE
                ),
            });
        }

        let trailer_region_start = file_size
            .saturating_sub(u64::try_from(TRAILER_SIZE).expect("TRAILER_SIZE fits in u64"));
        if trailer
            .superblock_offset
            .checked_add(trailer.superblock_length)
            .is_none_or(|end| end > trailer_region_start)
        {
            return Err(CoreError::FormatError {
                reason: format!(
                    "superblock region [{}, +{}] exceeds file bounds (file size: {file_size})",
                    trailer.superblock_offset, trailer.superblock_length
                ),
            });
        }

        // Read encrypted superblock.
        file.seek(SeekFrom::Start(trailer.superblock_offset))?;
        let sb_len =
            usize::try_from(trailer.superblock_length).map_err(|_| CoreError::FormatError {
                reason: "superblock length overflow".into(),
            })?;
        let mut encrypted_superblock = vec![0u8; sb_len];
        file.read_exact(&mut encrypted_superblock)?;

        // The encrypted superblock format: 24-byte nonce + ciphertext.
        if encrypted_superblock.len() < 24 {
            return Err(CoreError::FormatError {
                reason: "encrypted superblock too short".into(),
            });
        }
        let (nonce_bytes, ciphertext) = encrypted_superblock.split_at(24);
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(nonce_bytes);

        // Wrap the decrypted superblock JSON in `Zeroizing` because it
        // contains plaintext cryptographic key material (`segment_encryption_key`,
        // `hmac_key`). Without this, the freed Vec would leave key material
        // readable in process memory.
        let superblock_json = Zeroizing::new(crypto::decrypt_segment(
            &master_key,
            &nonce,
            ciphertext,
            b"ovc-superblock",
        )?);

        let superblock: Superblock =
            serde_json::from_slice(&superblock_json).map_err(|e| CoreError::Serialization {
                reason: format!("failed to deserialize superblock: {e}"),
            })?;

        // Bug fix #2: Verify trailer HMAC.
        let expected_hmac = compute_trailer_hmac_with_key(
            &superblock.hmac_key,
            trailer.superblock_offset,
            trailer.superblock_length,
            trailer.file_sequence,
        );
        if expected_hmac != trailer.trailer_hmac_truncated {
            return Err(CoreError::IntegrityError {
                reason: "trailer HMAC verification failed".into(),
            });
        }

        // Run WAL crash recovery (cleans up orphaned temp files if needed).
        let _recovery = crate::wal::WriteAheadLog::recover(path)?;

        // Capture file snapshot for conflict detection on save.
        let snapshot = crate::conflict::FileSnapshot::capture(path, trailer.file_sequence)?;

        // Bug fix #1: Restore the object store from persisted data in the superblock.
        let mut store = ObjectStore::new(superblock.config.compression_level);
        store.import(superblock.stored_objects.clone());

        let segment_index = SegmentIndex::new();

        Ok(Self {
            path: path.to_path_buf(),
            master_key,
            header,
            superblock,
            segment_index,
            store,
            file_sequence: trailer.file_sequence,
            file_snapshot: Some(snapshot),
        })
    }

    /// Writes all pending changes back to the `.ovc` file.
    ///
    /// This re-writes the entire file: header, segments, superblock, trailer.
    /// Acquires an advisory lock for the duration of the write, checks for
    /// external conflicts, and writes a WAL entry for crash recovery.
    #[allow(clippy::too_many_lines)]
    pub fn save(&mut self) -> CoreResult<()> {
        // Check for external modifications before writing.
        if let Some(ref snapshot) = self.file_snapshot {
            snapshot.check_for_conflict(&self.path)?;
        }

        // Acquire advisory lock for the duration of the write operation.
        // Uses a short timeout since the lock only covers the write, not
        // the entire session. If the file doesn't exist yet (init), skip locking.
        let _write_lock = if self.path.exists() {
            Some(crate::lock::RepoLock::acquire(&self.path, None)?)
        } else {
            None
        };

        self.file_sequence = self.file_sequence.saturating_add(1);

        // Begin WAL entry before the write.
        let wal = crate::wal::WriteAheadLog::new(&self.path);
        let _wal_entry = wal
            .begin(
                crate::wal::WalOperation::Save,
                self.file_sequence.saturating_sub(1),
            )
            .ok(); // WAL failure should not prevent save

        // Serialize all objects from the store into a single segment.
        let segment_data = self.serialize_store_segment()?;

        // Update the segment index with the new segment layout.
        self.segment_index = SegmentIndex::new();
        let mut segment_offset = u64::try_from(HEADER_SIZE).expect("HEADER_SIZE fits in u64");

        // Bug fix #3: Use position-bound AAD for segments.
        let segment_aad = segment_aad(0);

        // Encrypt segment data if non-empty.
        let encrypted_segment = if segment_data.is_empty() {
            Vec::new()
        } else {
            let encrypted = crypto::encrypt_segment(
                &self.superblock.segment_encryption_key,
                &segment_data,
                &segment_aad,
            )?;

            let seg_disk = Self::encode_encrypted_segment(&encrypted);
            let seg_disk_len = u64::try_from(seg_disk.len()).expect("segment length fits in u64");

            self.segment_index
                .segments
                .push(crate::format::SegmentDescriptor {
                    file_offset: segment_offset,
                    disk_length: seg_disk_len,
                });

            segment_offset += seg_disk_len;
            seg_disk
        };

        // Rebuild object locations in the segment index from the store.
        self.rebuild_segment_index_from_store();

        // Bug fix #1: Persist the object store data in the superblock.
        self.superblock.stored_objects = self.store.export();

        // Serialize superblock. Wrapped in `Zeroizing` because the plaintext
        // JSON contains cryptographic key material that must not linger in memory.
        let superblock_json =
            Zeroizing::new(serde_json::to_vec(&self.superblock).map_err(|e| {
                CoreError::Serialization {
                    reason: format!("failed to serialize superblock: {e}"),
                }
            })?);

        // Encrypt superblock.
        let encrypted_sb =
            crypto::encrypt_segment(&self.master_key, &superblock_json, b"ovc-superblock")?;

        let sb_on_disk = Self::encode_encrypted_segment(&encrypted_sb);
        let superblock_offset = segment_offset;
        let superblock_length = u64::try_from(sb_on_disk.len()).expect("sb length fits in u64");

        // Serialize key slot bootstrap data (stored between superblock and
        // trailer so key-based open can find sealed master keys without
        // needing to decrypt the superblock first).
        let key_slots_data = if self.superblock.key_slots.is_empty() {
            Vec::new()
        } else {
            serde_json::to_vec(&self.superblock.key_slots).map_err(|e| {
                CoreError::Serialization {
                    reason: format!("failed to serialize key slots: {e}"),
                }
            })?
        };

        // Build trailer.
        let trailer = FileTrailer {
            superblock_offset,
            superblock_length,
            file_sequence: self.file_sequence,
            trailer_hmac_truncated: compute_trailer_hmac_with_key(
                &self.superblock.hmac_key,
                superblock_offset,
                superblock_length,
                self.file_sequence,
            ),
        };

        // Write atomically: write to a temp file, flush + sync, then rename
        // over the target. This ensures the `.ovc` file is never left in a
        // partially-written state.
        //
        // The temp file name includes random bytes to prevent a local attacker
        // from pre-creating a symlink at a predictable path to redirect the
        // write to an arbitrary location.
        let random_suffix = {
            let mut buf = [0u8; 8];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut buf);
            let mut hex = String::with_capacity(16);
            for b in buf {
                use std::fmt::Write;
                let _ = write!(hex, "{b:02x}");
            }
            hex
        };
        let tmp_extension = format!("ovc.{random_suffix}.tmp");
        let tmp_path = self.path.with_extension(tmp_extension);

        let write_result = (|| -> CoreResult<()> {
            let mut file = std::fs::File::create(&tmp_path)?;
            file.write_all(&self.header.serialize())?;
            if !encrypted_segment.is_empty() {
                file.write_all(&encrypted_segment)?;
            }
            file.write_all(&sb_on_disk)?;
            if !key_slots_data.is_empty() {
                file.write_all(&key_slots_data)?;
            }
            file.write_all(&trailer.serialize())?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();

        if let Err(e) = write_result {
            // Clean up the temp file on write failure.
            let _ = std::fs::remove_file(&tmp_path);
            let _ = wal.mark_failed();
            return Err(e);
        }

        std::fs::rename(&tmp_path, &self.path).map_err(|e| {
            // Clean up the temp file on rename failure.
            let _ = std::fs::remove_file(&tmp_path);
            let _ = wal.mark_failed();
            CoreError::Io(e)
        })?;

        // Mark WAL as completed now that the atomic rename succeeded.
        let _ = wal.complete();

        // Update the file snapshot to reflect the new state.
        if let Ok(snapshot) = crate::conflict::FileSnapshot::capture(&self.path, self.file_sequence)
        {
            self.file_snapshot = Some(snapshot);
        }

        Ok(())
    }

    /// Saves and consumes the repository handle.
    pub fn close(mut self) -> CoreResult<()> {
        self.save()
    }

    /// Saves the repository, automatically merging with remote changes if the
    /// file was modified externally (e.g., by another user via iCloud sync).
    ///
    /// This is the safe save method for collaborative workflows where each
    /// user works on their own branch. If the underlying `.ovc` file was
    /// modified since we opened it, we:
    /// 1. Re-open the file to get the remote state
    /// 2. Import all remote objects into our local store
    /// 3. Merge remote refs (branches, tags) into our refs
    /// 4. Save the combined state
    ///
    /// `password` is required because re-opening the file requires decryption.
    pub fn save_with_merge(&mut self, password: &[u8]) -> CoreResult<()> {
        match self.save() {
            Ok(()) => Ok(()),
            Err(CoreError::ConflictDetected { .. }) => self.merge_and_save(password),
            Err(e) => Err(e),
        }
    }

    /// Re-reads the `.ovc` file from disk, merges remote objects and refs
    /// into the local state, then saves the combined result.
    fn merge_and_save(&mut self, password: &[u8]) -> CoreResult<()> {
        let remote = Self::open(&self.path, password)?;

        // Import all remote objects that we don't already have.
        for (oid, entry) in &remote.superblock.stored_objects {
            self.superblock
                .stored_objects
                .entry(*oid)
                .or_insert_with(|| entry.clone());
        }
        // Rebuild the live store from the merged stored_objects so the save
        // serialization is complete.
        self.store.import(self.superblock.stored_objects.clone());

        // Merge refs: remote branches that don't exist locally get added.
        // Remote branches that DO exist locally keep the local version
        // (the user's own branch takes priority).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        let sync_identity = Identity {
            name: String::from("ovc-sync"),
            email: String::new(),
            timestamp: now,
            tz_offset_minutes: 0,
        };

        let remote_branches: Vec<(String, ObjectId)> = remote
            .ref_store()
            .list_branches()
            .into_iter()
            .map(|(name, oid)| (name.to_owned(), *oid))
            .collect();

        for (name, oid) in &remote_branches {
            let full_ref = format!("refs/heads/{name}");
            if self.ref_store().resolve(&full_ref).is_err() {
                self.ref_store_mut().set_branch(
                    name,
                    *oid,
                    &sync_identity,
                    &format!("sync: imported branch {name}"),
                )?;
            }
        }

        // Merge tags: import remote tags that don't exist locally.
        let remote_tags: Vec<(String, ObjectId)> = remote
            .ref_store()
            .list_tags()
            .into_iter()
            .map(|(name, oid, _msg)| (name.to_owned(), *oid))
            .collect();

        for (name, oid) in &remote_tags {
            let full_ref = format!("refs/tags/{name}");
            if self.ref_store().resolve(&full_ref).is_err() {
                // Ignore AlreadyExists errors from concurrent tag creation.
                let _ = self.ref_store_mut().create_tag(name, *oid, None);
            }
        }

        // Merge notes: import remote notes for commits we don't have notes on.
        for (oid, note) in &remote.superblock.notes {
            self.superblock
                .notes
                .entry(*oid)
                .or_insert_with(|| note.clone());
        }

        // Merge submodules: import remote submodule configs we don't have.
        for (name, config) in &remote.superblock.submodules {
            self.superblock
                .submodules
                .entry(name.clone())
                .or_insert_with(|| config.clone());
        }

        // Merge access control: import remote users and branch protection.
        self.superblock
            .access_control
            .merge_from(&remote.superblock.access_control);

        // Merge pull requests: import remote PRs, take max counter.
        self.superblock
            .pull_request_store
            .merge_from(&remote.superblock.pull_request_store);

        // Merge key slots: import remote key slots for users we don't have.
        for remote_slot in &remote.superblock.key_slots {
            if !self
                .superblock
                .key_slots
                .iter()
                .any(|s| s.recipient_fingerprint == remote_slot.recipient_fingerprint)
            {
                self.superblock.key_slots.push(remote_slot.clone());
            }
        }

        // Update our file_snapshot to match the remote's state so the
        // next save doesn't conflict again.
        self.file_sequence = remote.file_sequence;
        self.file_snapshot = remote.file_snapshot;

        // Save the merged state.
        self.save()
    }

    /// Inserts an object into the repository's object store.
    pub fn insert_object(&mut self, obj: &Object) -> CoreResult<ObjectId> {
        self.store.insert(obj)
    }

    /// Retrieves an object by its id.
    pub fn get_object(&self, oid: &ObjectId) -> CoreResult<Option<Object>> {
        self.store.get(oid)
    }

    /// Returns `true` if the repository contains an object with the given id.
    #[must_use]
    pub fn contains_object(&self, oid: &ObjectId) -> bool {
        self.store.contains(oid)
    }

    /// Returns the number of objects in the repository.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.store.count()
    }

    /// Returns an immutable reference to the repository configuration.
    #[must_use]
    pub const fn config(&self) -> &RepositoryConfig {
        &self.superblock.config
    }

    /// Returns a mutable reference to the repository configuration.
    pub const fn config_mut(&mut self) -> &mut RepositoryConfig {
        &mut self.superblock.config
    }

    /// Returns the path to the `.ovc` file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the current HEAD reference string.
    #[must_use]
    pub fn head_ref(&self) -> &str {
        &self.superblock.head_ref
    }

    /// Sets the HEAD reference string (e.g. `"refs/heads/main"`).
    ///
    /// This updates the `head_ref` field in the superblock. Callers should also
    /// update the ref store's HEAD target via [`ref_store_mut().set_head()`] to
    /// keep the two in sync.
    pub fn set_head_ref(&mut self, head_ref: String) {
        self.superblock.head_ref = head_ref;
    }

    /// Returns the file sequence number (incremented on each save).
    #[must_use]
    pub const fn file_sequence(&self) -> u64 {
        self.file_sequence
    }

    /// Returns an immutable reference to the `RefStore`.
    #[must_use]
    pub const fn ref_store(&self) -> &RefStore {
        &self.superblock.ref_store
    }

    /// Returns a mutable reference to the `RefStore`.
    pub const fn ref_store_mut(&mut self) -> &mut RefStore {
        &mut self.superblock.ref_store
    }

    /// Returns an immutable reference to the staging `Index`.
    #[must_use]
    pub const fn index(&self) -> &Index {
        &self.superblock.staging_index
    }

    /// Returns a mutable reference to the staging `Index`.
    pub const fn index_mut(&mut self) -> &mut Index {
        &mut self.superblock.staging_index
    }

    /// Returns an immutable reference to the object store.
    #[must_use]
    pub const fn object_store(&self) -> &ObjectStore {
        &self.store
    }

    /// Returns a mutable reference to the object store.
    pub const fn object_store_mut(&mut self) -> &mut ObjectStore {
        &mut self.store
    }

    /// Returns mutable references to both the index and object store
    /// simultaneously, avoiding borrow-checker conflicts when staging files.
    pub const fn index_and_store_mut(&mut self) -> (&mut Index, &mut ObjectStore) {
        (&mut self.superblock.staging_index, &mut self.store)
    }

    /// Creates a commit from the current index, updating HEAD.
    ///
    /// Builds a tree from the staged index, creates a commit object pointing
    /// to it, and advances the current branch to the new commit.
    pub fn create_commit(&mut self, message: &str, author: &Identity) -> CoreResult<ObjectId> {
        // Build tree from index.
        let tree_oid = self.superblock.staging_index.write_tree(&mut self.store)?;

        // Determine parent commits.
        let parents = self
            .superblock
            .ref_store
            .resolve_head()
            .map_or_else(|_| Vec::new(), |head_oid| vec![head_oid]);

        // Determine sequence number.
        let sequence = parents
            .first()
            .and_then(|oid| self.store.get(oid).ok().flatten())
            .map_or(1, |obj| {
                if let Object::Commit(parent_commit) = obj {
                    parent_commit.sequence + 1
                } else {
                    1
                }
            });

        let commit = Commit {
            tree: tree_oid,
            parents,
            author: author.clone(),
            committer: author.clone(),
            message: message.to_owned(),
            signature: None,
            sequence,
        };

        let commit_oid = self.store.insert(&Object::Commit(commit))?;

        // Update the branch that HEAD points to.
        match self.superblock.ref_store.head().clone() {
            RefTarget::Symbolic(ref_name) => {
                let branch_name = ref_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_name)
                    .to_owned();
                self.superblock
                    .ref_store
                    .set_branch(&branch_name, commit_oid, author, message)?;
            }
            RefTarget::Direct(_) => {
                // Detached HEAD: just update HEAD directly.
                self.superblock
                    .ref_store
                    .set_head(RefTarget::Direct(commit_oid));
                self.superblock.head_ref = commit_oid.to_string();
            }
        }

        Ok(commit_oid)
    }

    /// Creates a new signed commit with an Ed25519 signature.
    ///
    /// Behaves identically to [`create_commit`](Self::create_commit) but signs the
    /// canonical serialized commit bytes with the provided key pair.
    pub fn create_commit_signed(
        &mut self,
        message: &str,
        author: &Identity,
        keypair: &keys::OvcKeyPair,
    ) -> CoreResult<ObjectId> {
        use crate::serialize;
        use ed25519_dalek::Signer;

        // Build tree from index.
        let tree_oid = self.superblock.staging_index.write_tree(&mut self.store)?;

        // Determine parent commits.
        let parents = self
            .superblock
            .ref_store
            .resolve_head()
            .map_or_else(|_| Vec::new(), |head_oid| vec![head_oid]);

        // Determine sequence number.
        let sequence = parents
            .first()
            .and_then(|oid| self.store.get(oid).ok().flatten())
            .map_or(1, |obj| {
                if let Object::Commit(parent_commit) = obj {
                    parent_commit.sequence + 1
                } else {
                    1
                }
            });

        let mut commit = Commit {
            tree: tree_oid,
            parents,
            author: author.clone(),
            committer: author.clone(),
            message: message.to_owned(),
            signature: None,
            sequence,
        };

        // Serialize without signature for signing.
        let serialized = serialize::serialize_object(&Object::Commit(commit.clone()))?;
        let sig = keypair.signing_key().sign(&serialized);
        commit.signature = Some(sig.to_bytes().to_vec());

        let commit_oid = self.store.insert(&Object::Commit(commit))?;

        // Update the branch that HEAD points to.
        match self.superblock.ref_store.head().clone() {
            RefTarget::Symbolic(ref_name) => {
                let branch_name = ref_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_name)
                    .to_owned();
                self.superblock
                    .ref_store
                    .set_branch(&branch_name, commit_oid, author, message)?;
            }
            RefTarget::Direct(_) => {
                self.superblock
                    .ref_store
                    .set_head(RefTarget::Direct(commit_oid));
                self.superblock.head_ref = commit_oid.to_string();
            }
        }

        Ok(commit_oid)
    }

    /// Signs an existing commit and re-stores it.
    ///
    /// Because the commit hash is computed from the signature-stripped serialization,
    /// the object ID does not change when a signature is added.
    pub fn sign_commit(
        &mut self,
        oid: &ObjectId,
        keypair: &keys::OvcKeyPair,
    ) -> CoreResult<ObjectId> {
        use crate::serialize;
        use ed25519_dalek::Signer;

        let obj = self
            .store
            .get(oid)?
            .ok_or(CoreError::ObjectNotFound(*oid))?;

        let Object::Commit(mut commit) = obj else {
            return Err(CoreError::CorruptObject {
                reason: format!("expected commit at {oid}"),
            });
        };

        // Serialize without signature for signing.
        commit.signature = None;
        let serialized = serialize::serialize_object(&Object::Commit(commit.clone()))?;
        let sig = keypair.signing_key().sign(&serialized);
        commit.signature = Some(sig.to_bytes().to_vec());

        // Re-insert (same OID, but now with signature in stored bytes).
        self.store.insert(&Object::Commit(commit))
    }

    /// Switches HEAD to a different branch, updating the index and working directory.
    ///
    /// Files present in the old index but absent from the target tree are deleted
    /// from the working directory and removed from the index.
    pub fn checkout_branch(&mut self, name: &str, workdir: &WorkDir) -> CoreResult<()> {
        let full_name = if name.starts_with("refs/heads/") {
            name.to_owned()
        } else {
            format!("refs/heads/{name}")
        };

        // Resolve the target branch to get the commit.
        let commit_oid = self.superblock.ref_store.resolve(&full_name)?;

        let Some(Object::Commit(commit)) = self.store.get(&commit_oid)? else {
            return Err(CoreError::CorruptObject {
                reason: format!("expected commit at {commit_oid}"),
            });
        };

        // Snapshot the old index paths before overwriting.
        let old_paths: std::collections::BTreeSet<String> = self
            .superblock
            .staging_index
            .entries()
            .iter()
            .map(|e| e.path.clone())
            .collect();

        // Build new index from the target commit's tree.
        self.superblock
            .staging_index
            .read_tree(&commit.tree, &self.store)?;

        // Compute paths in the new tree for comparison.
        let new_paths: std::collections::BTreeSet<String> = self
            .superblock
            .staging_index
            .entries()
            .iter()
            .map(|e| e.path.clone())
            .collect();

        // Delete files that existed in the old tree but are absent in the new one.
        for stale_path in old_paths.difference(&new_paths) {
            workdir.delete_file(stale_path)?;
        }

        // Update HEAD to point to the branch.
        self.superblock
            .ref_store
            .set_head(RefTarget::Symbolic(full_name.clone()));
        self.superblock.head_ref = full_name;

        // Write files from the new tree to the working directory.
        for entry in self.superblock.staging_index.entries() {
            if let Some(Object::Blob(data)) = self.store.get(&entry.oid)? {
                workdir.write_file(&entry.path, &data, entry.mode)?;
            }
        }

        Ok(())
    }

    /// Creates a new branch at the current HEAD.
    pub fn create_branch(&mut self, name: &str) -> CoreResult<()> {
        let head_oid = self.superblock.ref_store.resolve_head()?;
        self.create_branch_at(name, head_oid)
    }

    /// Creates a new branch pointing to an explicit commit `ObjectId`.
    pub fn create_branch_at(&mut self, name: &str, oid: ObjectId) -> CoreResult<()> {
        let identity = Identity {
            name: self.superblock.config.user_name.clone(),
            email: self.superblock.config.user_email.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            tz_offset_minutes: 0,
        };
        self.superblock.ref_store.set_branch(
            name,
            oid,
            &identity,
            &format!("branch: created {name}"),
        )?;
        Ok(())
    }

    /// Renames a branch from `old_name` to `new_name`.
    ///
    /// Updates HEAD if it currently points to the old branch. Returns an error
    /// if the old branch does not exist or the new name is already in use.
    pub fn rename_branch(&mut self, old_name: &str, new_name: &str) -> CoreResult<()> {
        let identity = Identity {
            name: self.superblock.config.user_name.clone(),
            email: self.superblock.config.user_email.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            tz_offset_minutes: 0,
        };
        self.superblock
            .ref_store
            .rename_branch(old_name, new_name, &identity)
    }

    /// Deletes a branch.
    ///
    /// Returns an error if the branch is currently checked out (HEAD points to it).
    pub fn delete_branch(&mut self, name: &str) -> CoreResult<()> {
        let full_name = if name.starts_with("refs/heads/") {
            name.to_owned()
        } else {
            format!("refs/heads/{name}")
        };
        if let RefTarget::Symbolic(head_target) = self.superblock.ref_store.head()
            && *head_target == full_name
        {
            return Err(CoreError::Config {
                reason: "cannot delete the currently checked-out branch".into(),
            });
        }
        self.superblock.ref_store.delete_branch(name)
    }

    // ── Stash ───────────────────────────────────────────────────────────

    /// Returns an immutable reference to the stash store.
    #[must_use]
    pub const fn stash(&self) -> &StashStore {
        &self.superblock.stash_store
    }

    /// Returns a mutable reference to the stash store.
    pub const fn stash_mut(&mut self) -> &mut StashStore {
        &mut self.superblock.stash_store
    }

    /// Pushes the current index state onto the stash.
    ///
    /// Returns the stash index of the new entry (always 0).
    pub fn stash_push(&mut self, message: &str) -> CoreResult<usize> {
        let head_oid = self.superblock.ref_store.resolve_head()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let author = Identity {
            name: self.superblock.config.user_name.clone(),
            email: self.superblock.config.user_email.clone(),
            timestamp: now,
            tz_offset_minutes: 0,
        };
        let result = self.superblock.stash_store.push(
            message,
            &mut self.store,
            &self.superblock.staging_index,
            head_oid,
            &author,
        )?;

        // Reset the index to the HEAD tree so the working directory appears clean.
        let head_obj = self
            .store
            .get(&head_oid)?
            .ok_or(CoreError::ObjectNotFound(head_oid))?;
        if let Object::Commit(head_commit) = head_obj {
            self.superblock
                .staging_index
                .read_tree(&head_commit.tree, &self.store)?;
        }

        Ok(result)
    }

    /// Pops a stash entry, restoring it to the index.
    pub fn stash_pop(&mut self, idx: usize) -> CoreResult<()> {
        self.superblock
            .stash_store
            .pop(idx, &self.store, &mut self.superblock.staging_index)?;
        Ok(())
    }

    // ── Notes ───────────────────────────────────────────────────────────

    /// Returns an immutable reference to the notes map.
    #[must_use]
    pub const fn notes(&self) -> &std::collections::BTreeMap<ObjectId, String> {
        &self.superblock.notes
    }

    /// Returns a mutable reference to the notes map.
    pub const fn notes_mut(&mut self) -> &mut std::collections::BTreeMap<ObjectId, String> {
        &mut self.superblock.notes
    }

    // ── Submodules ──────────────────────────────────────────────────────

    /// Returns an immutable reference to the submodule configurations.
    #[must_use]
    pub const fn submodules(
        &self,
    ) -> &std::collections::BTreeMap<String, crate::submodule::SubmoduleConfig> {
        &self.superblock.submodules
    }

    /// Returns a mutable reference to the submodule configurations.
    pub const fn submodules_mut(
        &mut self,
    ) -> &mut std::collections::BTreeMap<String, crate::submodule::SubmoduleConfig> {
        &mut self.superblock.submodules
    }

    // ── Rebase ──────────────────────────────────────────────────────────

    /// Rebases the named branch onto the target branch.
    ///
    /// Resolves both branch names to commit ids, performs the rebase, and
    /// updates the source branch to point to the new tip.
    pub fn rebase_branch(&mut self, branch: &str, onto: &str) -> Result<RebaseResult, RebaseError> {
        let branch_ref = if branch.starts_with("refs/heads/") {
            branch.to_owned()
        } else {
            format!("refs/heads/{branch}")
        };
        let onto_ref = if onto.starts_with("refs/heads/") {
            onto.to_owned()
        } else {
            format!("refs/heads/{onto}")
        };

        let branch_tip = self
            .superblock
            .ref_store
            .resolve(&branch_ref)
            .map_err(RebaseError::Core)?;
        let onto_oid = self
            .superblock
            .ref_store
            .resolve(&onto_ref)
            .map_err(RebaseError::Core)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let committer = Identity {
            name: self.superblock.config.user_name.clone(),
            email: self.superblock.config.user_email.clone(),
            timestamp: now,
            tz_offset_minutes: 0,
        };

        let result = rebase::rebase(branch_tip, onto_oid, &mut self.store, &committer)?;

        // Update the branch to point to the new tip.
        let short_name = branch_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&branch_ref);
        self.superblock.ref_store.set_branch(
            short_name,
            result.new_tip,
            &committer,
            &format!("rebase: onto {onto}"),
        )?;

        Ok(result)
    }

    // ── Cherry-pick ─────────────────────────────────────────────────────

    /// Cherry-picks a commit onto the current HEAD.
    ///
    /// Creates a new commit applying the changes from `commit_id` and
    /// advances the current branch.
    pub fn cherry_pick_commit(&mut self, commit_id: &ObjectId) -> CoreResult<ObjectId> {
        let head_oid = self.superblock.ref_store.resolve_head()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let committer = Identity {
            name: self.superblock.config.user_name.clone(),
            email: self.superblock.config.user_email.clone(),
            timestamp: now,
            tz_offset_minutes: 0,
        };

        let new_oid =
            crate::cherry_pick::cherry_pick(*commit_id, head_oid, &mut self.store, &committer)?;

        // Advance the current branch.
        match self.superblock.ref_store.head().clone() {
            RefTarget::Symbolic(ref_name) => {
                let branch_name = ref_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_name)
                    .to_owned();
                let msg = format!("cherry-pick: {commit_id}");
                self.superblock
                    .ref_store
                    .set_branch(&branch_name, new_oid, &committer, &msg)?;
            }
            RefTarget::Direct(_) => {
                self.superblock
                    .ref_store
                    .set_head(RefTarget::Direct(new_oid));
                self.superblock.head_ref = new_oid.to_string();
            }
        }

        // Update the index to reflect the new tree.
        let obj = self.store.get(&new_oid)?;
        if let Some(Object::Commit(commit)) = obj {
            self.superblock
                .staging_index
                .read_tree(&commit.tree, &self.store)?;
        }

        Ok(new_oid)
    }

    // ── Revert ─────────────────────────────────────────────────────────

    /// Reverts a commit by creating a new commit that undoes its changes.
    ///
    /// Advances the current branch to point to the new revert commit and
    /// updates the staging index to match the reverted tree.
    pub fn revert_commit(&mut self, commit_id: &ObjectId) -> CoreResult<ObjectId> {
        let head_oid = self.superblock.ref_store.resolve_head()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let committer = Identity {
            name: self.superblock.config.user_name.clone(),
            email: self.superblock.config.user_email.clone(),
            timestamp: now,
            tz_offset_minutes: 0,
        };

        let new_oid = crate::revert::revert(*commit_id, head_oid, &mut self.store, &committer)?;

        // Advance the current branch.
        match self.superblock.ref_store.head().clone() {
            RefTarget::Symbolic(ref_name) => {
                let branch_name = ref_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_name)
                    .to_owned();
                let msg = format!("revert: {commit_id}");
                self.superblock
                    .ref_store
                    .set_branch(&branch_name, new_oid, &committer, &msg)?;
            }
            RefTarget::Direct(_) => {
                self.superblock
                    .ref_store
                    .set_head(RefTarget::Direct(new_oid));
                self.superblock.head_ref = new_oid.to_string();
            }
        }

        // Update the index to reflect the new tree.
        let obj = self.store.get(&new_oid)?;
        if let Some(Object::Commit(commit)) = obj {
            self.superblock
                .staging_index
                .read_tree(&commit.tree, &self.store)?;
        }

        Ok(new_oid)
    }

    // ── Garbage collection ──────────────────────────────────────────────

    /// Runs garbage collection, removing unreachable objects.
    ///
    /// After sweeping the object store, any notes whose associated commit OID
    /// no longer exists in the store are also removed.  Notes for deleted
    /// commits are permanently unreachable and would otherwise accumulate
    /// indefinitely in the superblock.
    pub fn gc(&mut self) -> CoreResult<GcResult> {
        let result = gc::garbage_collect(
            &mut self.store,
            &self.superblock.ref_store,
            &self.superblock.stash_store,
        )?;

        // Prune notes whose commit OID was swept from the store.
        self.superblock
            .notes
            .retain(|oid, _| self.store.contains(oid));

        Ok(result)
    }

    // ── SSH key-based init / open ───────────────────────────────────────

    /// Creates a new OVC repository encrypted with an SSH key pair.
    ///
    /// Instead of deriving a master key from a password, the segment
    /// encryption key is sealed to the given key pair using X25519 ECDH.
    /// The master key used for superblock encryption is derived from a
    /// dummy password with the salt; this keeps the on-disk format
    /// identical (password-based repos use KDF-derived keys, key-based
    /// repos use the same structure but the "password" is the unsealed
    /// segment key itself, ensuring the superblock is still encrypted).
    pub fn init_with_key(path: &Path, keypair: &OvcKeyPair) -> CoreResult<Self> {
        if path.exists() {
            return Err(CoreError::AlreadyExists {
                path: path.display().to_string(),
            });
        }

        let salt = crypto::generate_salt();
        // For key-based repos, the master key is a fresh random key
        // (not derived from a password). We store it sealed in key slots.
        let master_key = crypto::generate_key();

        let header = FileHeader {
            format_version: FORMAT_VERSION,
            min_reader_version: MIN_READER_VERSION,
            kdf_algorithm: KdfAlgorithm::Argon2id,
            cipher_algorithm: CipherAlgorithm::XChaCha20Poly1305,
            compression_algorithm: CompressionAlgorithm::Zstd,
            // For key-based repos these KDF params are not used during open,
            // but we store valid defaults so the header remains well-formed.
            argon2_time_cost: DEFAULT_TIME_COST,
            argon2_memory_cost_kib: DEFAULT_MEMORY_COST_KIB,
            argon2_parallelism: DEFAULT_PARALLELISM,
            kdf_salt: salt,
        };

        let segment_encryption_key = *crypto::generate_key();
        let hmac_key = *crypto::generate_key();
        let segment_index = SegmentIndex::new();
        let config = RepositoryConfig::default();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        let ref_store = RefStore::new(&config.default_branch);

        // Seal the master key for the initial key pair.
        let sealed_master = keys::seal_key(&master_key, &keypair.public_key())?;

        // Create initial access control with the creator as Owner.
        let now_iso = chrono::Utc::now().to_rfc3339();
        let creator_fingerprint = keypair.fingerprint().to_owned();
        let initial_access = AccessControl {
            users: vec![UserAccess {
                fingerprint: creator_fingerprint.clone(),
                role: AccessRole::Owner,
                identity: keypair.identity().cloned(),
                signing_public_key: Some(keypair.signing_public().to_bytes().to_vec()),
                added_at: now_iso,
                added_by: creator_fingerprint,
            }],
            branch_protection: BTreeMap::new(),
        };

        let superblock = Superblock {
            segment_encryption_key,
            index_offset: 0,
            index_length: 0,
            index_nonce: [0u8; 24],
            head_ref: format!("refs/heads/{}", config.default_branch),
            created_at: now,
            refs: BTreeMap::new(),
            config,
            hmac_key,
            stored_objects: BTreeMap::new(),
            ref_store,
            staging_index: Index::new(),
            stash_store: StashStore::new(),
            key_slots: vec![sealed_master],
            notes: BTreeMap::new(),
            submodules: BTreeMap::new(),
            access_control: initial_access,
            pull_request_store: crate::pulls::PullRequestStore::default(),
        };

        let store = ObjectStore::default();

        let mut repo = Self {
            path: path.to_path_buf(),
            master_key,
            header,
            superblock,
            segment_index,
            store,
            file_sequence: 0,
            file_snapshot: None,
        };

        repo.save()?;

        Ok(repo)
    }

    /// Opens an existing OVC repository using an SSH key pair.
    ///
    /// Reads the superblock's key slots, finds one matching the given
    /// key pair's fingerprint, unseals the master key, then proceeds
    /// identically to password-based open.
    #[allow(clippy::too_many_lines)]
    pub fn open_with_key(path: &Path, keypair: &OvcKeyPair) -> CoreResult<Self> {
        if !path.exists() {
            return Err(CoreError::NotInitialized {
                path: path.display().to_string(),
            });
        }

        let mut file = std::fs::File::open(path)?;

        // Read header.
        let mut header_bytes = [0u8; HEADER_SIZE];
        file.read_exact(&mut header_bytes)?;
        let header = FileHeader::deserialize(&header_bytes)?;

        // Read trailer (last TRAILER_SIZE bytes).
        file.seek(SeekFrom::End(
            -i64::try_from(TRAILER_SIZE).expect("TRAILER_SIZE fits in i64"),
        ))?;
        let mut trailer_bytes = [0u8; TRAILER_SIZE];
        file.read_exact(&mut trailer_bytes)?;
        let trailer = FileTrailer::deserialize(&trailer_bytes)?;

        // Validate superblock bounds.
        let file_size = file.seek(SeekFrom::End(0))?;
        if trailer.superblock_length > MAX_SUPERBLOCK_SIZE {
            return Err(CoreError::FormatError {
                reason: format!(
                    "superblock length {} exceeds maximum allowed size of {} bytes",
                    trailer.superblock_length, MAX_SUPERBLOCK_SIZE
                ),
            });
        }

        let trailer_region_start = file_size
            .saturating_sub(u64::try_from(TRAILER_SIZE).expect("TRAILER_SIZE fits in u64"));
        if trailer
            .superblock_offset
            .checked_add(trailer.superblock_length)
            .is_none_or(|end| end > trailer_region_start)
        {
            return Err(CoreError::FormatError {
                reason: format!(
                    "superblock region [{}, +{}] exceeds file bounds (file size: {file_size})",
                    trailer.superblock_offset, trailer.superblock_length
                ),
            });
        }

        // Read encrypted superblock.
        file.seek(SeekFrom::Start(trailer.superblock_offset))?;
        let sb_len =
            usize::try_from(trailer.superblock_length).map_err(|_| CoreError::FormatError {
                reason: "superblock length overflow".into(),
            })?;
        let mut encrypted_superblock = vec![0u8; sb_len];
        file.read_exact(&mut encrypted_superblock)?;

        if encrypted_superblock.len() < 24 {
            return Err(CoreError::FormatError {
                reason: "encrypted superblock too short".into(),
            });
        }

        // For key-based open, we need to try candidate master keys.
        // We first try decryption with a brute approach: attempt to
        // decrypt the superblock with each possible unsealed key from
        // the key slots. But we don't have access to the key slots yet
        // (they're inside the encrypted superblock). This is a chicken-
        // and-egg problem.
        //
        // Solution: key-based repos store the key slots inside the
        // superblock, and the superblock is encrypted with the master key.
        // The master key is sealed in the key slots. To break the cycle,
        // we store a *second* copy of the sealed key slots in the file
        // header's reserved area — but that's only 12 bytes.
        //
        // Alternative approach (what we implement): the master key IS
        // the sealed secret. During init_with_key, we generate a random
        // master key. During save, the superblock is encrypted with this
        // master key. The master key is sealed into key slots stored in
        // the superblock. But for open, we need the master key to decrypt
        // the superblock to get the key slots.
        //
        // The standard solution: try all possible master key derivation
        // methods. For password repos, derive from password. For key repos,
        // we need to store the sealed master key *outside* the encrypted
        // superblock. We'll store it as a small unencrypted blob between
        // the header and the first segment.
        //
        // Simpler approach used here: attempt password-derived decryption
        // first (will fail for key-based repos). Then try a "key slot
        // bootstrap" approach where the sealed key data is appended after
        // the trailer.
        //
        // ACTUAL approach: We use a two-pass strategy. Since the key slots
        // are JSON-serialized inside the superblock, and the superblock is
        // encrypted with the master key, we try to decrypt the superblock
        // with each candidate master key. For key-based repos, we store
        // the sealed master key slots as an additional unencrypted section
        // right after the file header (before segments), pointed to by
        // a flag in the header's reserved bytes.
        //
        // SIMPLEST correct approach: store sealed key slots in plaintext
        // JSON appended after the trailer, with a length prefix. The
        // trailer already has a fixed 32-byte format, so we append:
        //   [4-byte LE key_slots_len] [key_slots_json]
        // This data is after the trailer, so old readers ignore it.

        // Check if there's data after the trailer (key slot bootstrap data).
        // File layout for key-based repos:
        //   header | segments | superblock | key_slots_json | trailer
        // The key slots are stored between superblock end and trailer start.
        // The trailer's superblock_offset + superblock_length points to
        // the superblock, and the gap before the trailer holds key slot data.

        let key_slots_start = trailer
            .superblock_offset
            .saturating_add(trailer.superblock_length);
        let key_slots_end = file_size.saturating_sub(u64::try_from(TRAILER_SIZE).expect("fits"));

        let key_slots_len = key_slots_end.saturating_sub(key_slots_start);

        if key_slots_len > 0 {
            // There's key slot bootstrap data between the superblock and trailer.
            file.seek(SeekFrom::Start(key_slots_start))?;
            let ks_len = usize::try_from(key_slots_len).map_err(|_| CoreError::FormatError {
                reason: "key slots section overflow".into(),
            })?;
            let mut key_slots_data = vec![0u8; ks_len];
            file.read_exact(&mut key_slots_data)?;

            let bootstrap_slots: Vec<SealedKey> =
                serde_json::from_slice(&key_slots_data).map_err(|e| CoreError::FormatError {
                    reason: format!("failed to parse key slot bootstrap data: {e}"),
                })?;

            // Try to unseal the master key using our keypair.
            // Attempt standard (SHA-512) key first, then fall back to
            // legacy (SHA-256) for repos created before the migration.
            let fingerprint = keypair.fingerprint();
            for slot in &bootstrap_slots {
                if slot.recipient_fingerprint == fingerprint {
                    let master_key_bytes = keys::unseal_key(slot, keypair)?;
                    let master_key = Zeroizing::new(master_key_bytes);

                    // Decrypt superblock with the unsealed master key.
                    let (nonce_bytes, ciphertext) = encrypted_superblock.split_at(24);
                    let mut nonce = [0u8; 24];
                    nonce.copy_from_slice(nonce_bytes);

                    let superblock_json = crypto::decrypt_segment(
                        &master_key,
                        &nonce,
                        ciphertext,
                        b"ovc-superblock",
                    )?;

                    let superblock: Superblock =
                        serde_json::from_slice(&superblock_json).map_err(|e| {
                            CoreError::Serialization {
                                reason: format!("failed to deserialize superblock: {e}"),
                            }
                        })?;

                    // Verify trailer HMAC.
                    let expected_hmac = compute_trailer_hmac_with_key(
                        &superblock.hmac_key,
                        trailer.superblock_offset,
                        trailer.superblock_length,
                        trailer.file_sequence,
                    );
                    if expected_hmac != trailer.trailer_hmac_truncated {
                        return Err(CoreError::IntegrityError {
                            reason: "trailer HMAC verification failed".into(),
                        });
                    }

                    // Run WAL crash recovery.
                    let _recovery = crate::wal::WriteAheadLog::recover(path)?;

                    // Capture file snapshot for conflict detection.
                    let snapshot =
                        crate::conflict::FileSnapshot::capture(path, trailer.file_sequence)?;

                    let mut store = ObjectStore::new(superblock.config.compression_level);
                    store.import(superblock.stored_objects.clone());

                    return Ok(Self {
                        path: path.to_path_buf(),
                        master_key,
                        header,
                        superblock,
                        segment_index: SegmentIndex::new(),
                        store,
                        file_sequence: trailer.file_sequence,
                        file_snapshot: Some(snapshot),
                    });
                }
            }

            return Err(CoreError::DecryptionFailed {
                reason: "no matching key slot found for this key pair".into(),
            });
        }

        // No key slot data found — this is a password-only repo.
        Err(CoreError::DecryptionFailed {
            reason: "this repository has no key slots; use a password to open it".into(),
        })
    }

    /// Adds a public key to the repository, granting its holder access.
    ///
    /// Seals a copy of the master key for the new recipient and stores it
    /// in the superblock's key slots.
    ///
    /// A maximum of 1024 key slots is enforced to prevent unbounded growth
    /// of the superblock from excessive key additions.
    pub fn add_key(&mut self, public_key: &OvcPublicKey) -> CoreResult<()> {
        /// Maximum number of key slots per repository.
        const MAX_KEY_SLOTS: usize = 1024;

        // Check if this key is already authorized.
        for slot in &self.superblock.key_slots {
            if slot.recipient_fingerprint == public_key.fingerprint {
                return Err(CoreError::Config {
                    reason: format!(
                        "key {} is already authorized for this repository",
                        public_key.fingerprint
                    ),
                });
            }
        }

        if self.superblock.key_slots.len() >= MAX_KEY_SLOTS {
            return Err(CoreError::Config {
                reason: format!(
                    "maximum number of key slots ({MAX_KEY_SLOTS}) reached; \
                     remove unused keys before adding new ones"
                ),
            });
        }

        // Seal the master key for the new recipient.
        let sealed = keys::seal_key(&self.master_key, public_key)?;
        self.superblock.key_slots.push(sealed);
        Ok(())
    }

    /// Removes a key from the repository by fingerprint.
    ///
    /// The removed key holder will no longer be able to open the repo.
    pub fn remove_key(&mut self, fingerprint: &str) -> CoreResult<()> {
        let initial_len = self.superblock.key_slots.len();
        self.superblock
            .key_slots
            .retain(|slot| slot.recipient_fingerprint != fingerprint);

        if self.superblock.key_slots.len() == initial_len {
            return Err(CoreError::Config {
                reason: format!("no key slot found with fingerprint: {fingerprint}"),
            });
        }

        Ok(())
    }

    /// Lists the fingerprints of all authorized keys.
    #[must_use]
    pub fn list_keys(&self) -> Vec<&str> {
        self.superblock
            .key_slots
            .iter()
            .map(|slot| slot.recipient_fingerprint.as_str())
            .collect()
    }

    /// Returns `true` if this repository has any key slots (key-based auth).
    #[must_use]
    pub const fn has_key_slots(&self) -> bool {
        !self.superblock.key_slots.is_empty()
    }

    // ── Access control ────────────────────────────────────────────────

    /// Returns a reference to the repository's access control list.
    #[must_use]
    pub const fn access_control(&self) -> &AccessControl {
        &self.superblock.access_control
    }

    /// Returns a mutable reference to the repository's access control list.
    pub const fn access_control_mut(&mut self) -> &mut AccessControl {
        &mut self.superblock.access_control
    }

    /// Reconstructs `OvcPublicKey` objects from the ACL's stored public key bytes.
    ///
    /// This allows the server to verify signatures without needing `.pub` files
    /// on disk. Users without stored public keys are skipped.
    #[must_use]
    pub fn authorized_public_keys(&self) -> Vec<OvcPublicKey> {
        self.superblock
            .access_control
            .users
            .iter()
            .filter_map(|user| {
                let key_bytes = user.signing_public_key.as_ref()?;
                let bytes: [u8; 32] = key_bytes.as_slice().try_into().ok()?;
                let signing_public = ed25519_dalek::VerifyingKey::from_bytes(&bytes).ok()?;
                // Derive the X25519 public key via the standard Edwards-to-Montgomery
                // point conversion. This is safe for signature-verification contexts
                // where the X25519 key is never used for ECDH sealing; using the
                // all-zero placeholder that was here previously was dangerous because
                // any code path that inadvertently called `seal_key` with one of
                // these keys would have encrypted data with a trivially known key.
                let encryption_public = keys::ed25519_verifying_to_x25519_public(&signing_public)?;
                Some(OvcPublicKey {
                    signing_public,
                    encryption_public,
                    fingerprint: user.fingerprint.clone(),
                    identity: user.identity.clone(),
                })
            })
            .collect()
    }

    /// Grants access to a user by sealing the master key for their public key
    /// and adding an ACL entry.
    ///
    /// The `grantor_fingerprint` identifies who is granting access (for audit).
    pub fn grant_access(
        &mut self,
        public_key: &OvcPublicKey,
        role: AccessRole,
        grantor_fingerprint: &str,
    ) -> CoreResult<()> {
        // Seal the master key first (add_key checks for duplicates and max slots).
        self.add_key(public_key)?;

        let now_iso = chrono::Utc::now().to_rfc3339();
        self.superblock.access_control.users.push(UserAccess {
            fingerprint: public_key.fingerprint.clone(),
            role,
            identity: public_key.identity.clone(),
            signing_public_key: Some(public_key.signing_public.to_bytes().to_vec()),
            added_at: now_iso,
            added_by: grantor_fingerprint.to_owned(),
        });

        Ok(())
    }

    /// Revokes a user's access by removing their key slot and ACL entry.
    ///
    /// Owners cannot be revoked unless they are not the last Owner.
    pub fn revoke_access(&mut self, fingerprint: &str) -> CoreResult<()> {
        // Prevent revoking the last Owner.
        let is_owner = self
            .superblock
            .access_control
            .role_for(fingerprint)
            .is_some_and(|r| r == AccessRole::Owner);
        if is_owner {
            let owner_count = self
                .superblock
                .access_control
                .users
                .iter()
                .filter(|u| u.role == AccessRole::Owner)
                .count();
            if owner_count <= 1 {
                return Err(CoreError::Config {
                    reason: "cannot revoke the last Owner — transfer ownership first".into(),
                });
            }
        }

        // Remove key slot.
        self.remove_key(fingerprint)?;

        // Remove ACL entry.
        self.superblock
            .access_control
            .users
            .retain(|u| u.fingerprint != fingerprint);

        Ok(())
    }

    /// Changes a user's role.
    pub fn set_role(&mut self, fingerprint: &str, role: AccessRole) -> CoreResult<()> {
        // Check the current role and owner count before mutating.
        let current_role = self
            .superblock
            .access_control
            .role_for(fingerprint)
            .ok_or_else(|| CoreError::Config {
                reason: format!("no user found with fingerprint: {fingerprint}"),
            })?;

        // Prevent downgrading the last Owner.
        if current_role == AccessRole::Owner && role != AccessRole::Owner {
            let owner_count = self
                .superblock
                .access_control
                .users
                .iter()
                .filter(|u| u.role == AccessRole::Owner)
                .count();
            if owner_count <= 1 {
                return Err(CoreError::Config {
                    reason: "cannot downgrade the last Owner — transfer ownership first".into(),
                });
            }
        }

        // Now mutate.
        let user = self
            .superblock
            .access_control
            .users
            .iter_mut()
            .find(|u| u.fingerprint == fingerprint)
            .expect("user existence verified above");
        user.role = role;
        Ok(())
    }

    /// Sets branch protection rules for a branch.
    pub fn set_branch_protection(
        &mut self,
        branch: &str,
        protection: BranchProtection,
    ) -> CoreResult<()> {
        self.superblock
            .access_control
            .branch_protection
            .insert(branch.to_owned(), protection);
        Ok(())
    }

    /// Removes branch protection rules for a branch.
    pub fn remove_branch_protection(&mut self, branch: &str) -> CoreResult<()> {
        self.superblock
            .access_control
            .branch_protection
            .remove(branch)
            .ok_or_else(|| CoreError::Config {
                reason: format!("no branch protection rules for: {branch}"),
            })?;
        Ok(())
    }

    // ── Pull requests ──────────────────────────────────────────────────

    /// Returns a reference to the pull request store.
    #[must_use]
    pub const fn pull_request_store(&self) -> &crate::pulls::PullRequestStore {
        &self.superblock.pull_request_store
    }

    /// Returns a mutable reference to the pull request store.
    pub const fn pull_request_store_mut(&mut self) -> &mut crate::pulls::PullRequestStore {
        &mut self.superblock.pull_request_store
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Serializes all objects in the store into a flat byte buffer.
    ///
    /// Each object is stored as: `[4-byte LE length] [type_byte] [compressed_payload]`.
    fn serialize_store_segment(&self) -> CoreResult<Vec<u8>> {
        let mut buf = Vec::new();
        for stored in self.store.raw_objects().values() {
            let type_byte = stored.object_type as u8;
            // Entry: length(4 LE) + type(1) + compressed data
            let entry_len = 1 + stored.compressed_data.len();
            let len_bytes = u32::try_from(entry_len).map_err(|_| CoreError::FormatError {
                reason: "object too large for segment encoding".into(),
            })?;
            buf.extend_from_slice(&len_bytes.to_le_bytes());
            buf.push(type_byte);
            buf.extend_from_slice(&stored.compressed_data);
        }
        Ok(buf)
    }

    /// Rebuilds the segment index object-location map from the current store contents.
    fn rebuild_segment_index_from_store(&mut self) {
        self.segment_index.objects.clear();
        let mut offset: u64 = 0;

        for (oid, stored) in self.store.raw_objects() {
            let compressed_len = stored.compressed_data.len() as u64;
            let entry_len = 1 + compressed_len;
            // Skip the 4-byte length prefix for the offset calculation.
            self.segment_index.objects.insert(
                *oid,
                crate::format::ObjectLocation {
                    segment_index: 0,
                    offset_in_segment: offset + 4, // past the length prefix
                    object_type: stored.object_type,
                    object_size: compressed_len,
                },
            );
            offset += 4 + entry_len;
        }
    }

    /// Encodes an `EncryptedSegment` as `nonce || ciphertext` bytes.
    fn encode_encrypted_segment(seg: &crypto::EncryptedSegment) -> Vec<u8> {
        let mut buf = Vec::with_capacity(24 + seg.ciphertext.len());
        buf.extend_from_slice(&seg.nonce);
        buf.extend_from_slice(&seg.ciphertext);
        buf
    }
}

/// Constructs position-bound AAD for a segment (Bug fix #3).
fn segment_aad(index: u32) -> Vec<u8> {
    format!("ovc-segment-{index}").into_bytes()
}

/// Computes a truncated HMAC for trailer fields using a given key.
fn compute_trailer_hmac_with_key(
    hmac_key: &[u8; 32],
    superblock_offset: u64,
    superblock_length: u64,
    file_sequence: u64,
) -> [u8; 8] {
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&superblock_offset.to_le_bytes());
    data.extend_from_slice(&superblock_length.to_le_bytes());
    data.extend_from_slice(&file_sequence.to_le_bytes());

    let hash = blake3::keyed_hash(hmac_key, &data);
    let mut truncated = [0u8; 8];
    truncated.copy_from_slice(&hash.as_bytes()[..8]);
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::FileMode;

    #[test]
    fn init_and_open_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("test.ovc");
        let password = b"test-password";

        // Init
        let mut repo = Repository::init(&ovc_path, password).unwrap();
        let blob = Object::Blob(b"hello world".to_vec());
        let oid = repo.insert_object(&blob).unwrap();
        {
            let identity = Identity {
                name: "Test".into(),
                email: "t@t.com".into(),
                timestamp: 0,
                tz_offset_minutes: 0,
            };
            repo.ref_store_mut()
                .set_branch("main", oid, &identity, "test ref")
                .unwrap();
        }
        repo.save().unwrap();

        // Verify file exists and is non-trivial.
        let metadata = std::fs::metadata(&ovc_path).unwrap();
        assert!(metadata.len() > (HEADER_SIZE + TRAILER_SIZE) as u64);

        // Open
        let repo2 = Repository::open(&ovc_path, password).unwrap();
        assert_eq!(repo2.head_ref(), "refs/heads/main");
        assert_eq!(repo2.file_sequence(), 2); // init save + explicit save

        // Bug fix #1 verification: objects persist across open.
        let retrieved = repo2.get_object(&oid).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), blob);
    }

    #[test]
    fn init_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("test.ovc");
        let _repo = Repository::init(&ovc_path, b"pw").unwrap();
        assert!(Repository::init(&ovc_path, b"pw").is_err());
    }

    #[test]
    fn open_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("nope.ovc");
        assert!(Repository::open(&ovc_path, b"pw").is_err());
    }

    #[test]
    fn wrong_password_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("test.ovc");
        let _repo = Repository::init(&ovc_path, b"correct").unwrap();
        assert!(Repository::open(&ovc_path, b"wrong").is_err());
    }

    #[test]
    fn close_saves() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("test.ovc");
        let repo = Repository::init(&ovc_path, b"pw").unwrap();
        repo.close().unwrap();

        // Re-open should succeed.
        let repo2 = Repository::open(&ovc_path, b"pw").unwrap();
        assert!(repo2.object_count() == 0);
    }

    #[test]
    fn object_persistence_multiple_objects() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("persist.ovc");
        let password = b"persist-test";

        let mut repo = Repository::init(&ovc_path, password).unwrap();
        let blob1 = Object::Blob(b"first blob".to_vec());
        let blob2 = Object::Blob(b"second blob".to_vec());
        let oid1 = repo.insert_object(&blob1).unwrap();
        let oid2 = repo.insert_object(&blob2).unwrap();
        repo.save().unwrap();

        let repo2 = Repository::open(&ovc_path, password).unwrap();
        assert_eq!(repo2.object_count(), 2);
        assert_eq!(repo2.get_object(&oid1).unwrap().unwrap(), blob1);
        assert_eq!(repo2.get_object(&oid2).unwrap().unwrap(), blob2);
    }

    #[test]
    fn trailer_hmac_integrity() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("hmac.ovc");
        let password = b"hmac-test";

        let _repo = Repository::init(&ovc_path, password).unwrap();

        // Tamper with the trailer.
        let mut data = std::fs::read(&ovc_path).unwrap();
        let len = data.len();
        // Flip a bit in the HMAC field (last 8 bytes of trailer, but
        // actually superblock_offset is first 8 bytes, so flip byte at len-1).
        data[len - 1] ^= 0xFF;
        std::fs::write(&ovc_path, &data).unwrap();

        let result = Repository::open(&ovc_path, password);
        assert!(result.is_err());
    }

    #[test]
    fn create_commit_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("commit.ovc");
        let password = b"commit-test";

        let mut repo = Repository::init(&ovc_path, password).unwrap();

        // Stage a file.
        let store = &mut repo.store;
        let index = &mut repo.superblock.staging_index;
        index
            .stage_file("hello.txt", b"hello world", FileMode::Regular, store)
            .unwrap();

        let author = Identity {
            name: "Test Author".into(),
            email: "test@example.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        };

        let commit_oid = repo.create_commit("initial commit", &author).unwrap();
        assert!(!commit_oid.is_zero());

        // HEAD should now resolve to the commit.
        let head_oid = repo.ref_store().resolve_head().unwrap();
        assert_eq!(head_oid, commit_oid);

        // Save and reopen.
        repo.save().unwrap();
        let repo2 = Repository::open(&ovc_path, password).unwrap();
        let head_oid2 = repo2.ref_store().resolve_head().unwrap();
        assert_eq!(head_oid2, commit_oid);

        // Verify the commit object persisted.
        let obj = repo2.get_object(&commit_oid).unwrap().unwrap();
        match obj {
            Object::Commit(c) => {
                assert_eq!(c.message, "initial commit");
            }
            _ => panic!("expected commit object"),
        }
    }

    #[test]
    fn ref_store_persists_across_save() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("refs.ovc");
        let password = b"refs-test";

        let mut repo = Repository::init(&ovc_path, password).unwrap();

        // Stage and commit to create a valid commit.
        let store = &mut repo.store;
        let index = &mut repo.superblock.staging_index;
        index
            .stage_file("f.txt", b"data", FileMode::Regular, store)
            .unwrap();

        let author = Identity {
            name: "Test".into(),
            email: "t@t.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        };
        let commit_oid = repo.create_commit("init", &author).unwrap();

        // Create a tag.
        repo.ref_store_mut()
            .create_tag("v1.0", commit_oid, None)
            .unwrap();

        repo.save().unwrap();

        // Reopen and verify.
        let repo2 = Repository::open(&ovc_path, password).unwrap();
        let tags = repo2.ref_store().list_tags();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].0, "v1.0");
        assert_eq!(*tags[0].1, commit_oid);
        assert_eq!(tags[0].2, None);
    }

    #[test]
    fn index_persists_across_save() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("index.ovc");
        let password = b"index-test";

        let mut repo = Repository::init(&ovc_path, password).unwrap();

        let store = &mut repo.store;
        let index = &mut repo.superblock.staging_index;
        index
            .stage_file("staged.txt", b"staged content", FileMode::Regular, store)
            .unwrap();

        repo.save().unwrap();

        let repo2 = Repository::open(&ovc_path, password).unwrap();
        assert_eq!(repo2.index().entries().len(), 1);
        assert_eq!(repo2.index().entries()[0].path, "staged.txt");
    }

    #[test]
    fn atomic_save_uses_temp_file_and_rename() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("atomic.ovc");
        let tmp_path = ovc_path.with_extension("ovc.tmp");
        let password = b"atomic-test";

        // Create and save a repo with objects.
        let mut repo = Repository::init(&ovc_path, password).unwrap();
        let blob = Object::Blob(b"important data".to_vec());
        let oid = repo.insert_object(&blob).unwrap();
        repo.save().unwrap();

        // After a successful save, the temp file must not linger.
        assert!(
            !tmp_path.exists(),
            "temp file should be cleaned up after save"
        );

        // The final file must contain the object.
        let repo2 = Repository::open(&ovc_path, password).unwrap();
        assert_eq!(repo2.get_object(&oid).unwrap().unwrap(), blob);
    }

    #[test]
    fn truncated_file_returns_meaningful_error() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("truncated.ovc");
        let password = b"truncate-test";

        // Create a valid repo file.
        let _repo = Repository::init(&ovc_path, password).unwrap();
        assert!(ovc_path.exists());

        // Truncate the file to a few bytes (corrupt it).
        std::fs::write(&ovc_path, b"OVCX").unwrap();

        // Attempting to open should produce an error, not a panic.
        let result = Repository::open(&ovc_path, password);
        assert!(
            result.is_err(),
            "opening a truncated .ovc file should return an error"
        );
    }

    // ── Key-based repository tests ────────────────────────────────────

    #[test]
    fn init_with_key_and_open_with_key_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("key-repo.ovc");

        let kp = crate::keys::OvcKeyPair::generate();

        let mut repo = Repository::init_with_key(&ovc_path, &kp).unwrap();
        let blob = Object::Blob(b"key-encrypted data".to_vec());
        let oid = repo.insert_object(&blob).unwrap();
        repo.save().unwrap();

        let repo2 = Repository::open_with_key(&ovc_path, &kp).unwrap();
        let retrieved = repo2.get_object(&oid).unwrap();
        assert_eq!(retrieved.unwrap(), blob);
    }

    #[test]
    fn open_with_wrong_key_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("wrong-key.ovc");

        let kp1 = crate::keys::OvcKeyPair::generate();
        let kp2 = crate::keys::OvcKeyPair::generate();

        let _repo = Repository::init_with_key(&ovc_path, &kp1).unwrap();

        let result = Repository::open_with_key(&ovc_path, &kp2);
        assert!(result.is_err());
    }

    #[test]
    fn add_second_key_and_open_with_either() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("multi-key.ovc");

        let kp1 = crate::keys::OvcKeyPair::generate();
        let kp2 = crate::keys::OvcKeyPair::generate();

        let mut repo = Repository::init_with_key(&ovc_path, &kp1).unwrap();
        let blob = Object::Blob(b"shared data".to_vec());
        let oid = repo.insert_object(&blob).unwrap();

        // Add second key.
        repo.add_key(&kp2.public_key()).unwrap();
        assert_eq!(repo.list_keys().len(), 2);
        repo.save().unwrap();

        // Open with first key.
        let repo2 = Repository::open_with_key(&ovc_path, &kp1).unwrap();
        assert_eq!(repo2.get_object(&oid).unwrap().unwrap(), blob);

        // Open with second key.
        let repo3 = Repository::open_with_key(&ovc_path, &kp2).unwrap();
        assert_eq!(repo3.get_object(&oid).unwrap().unwrap(), blob);
    }

    #[test]
    fn remove_key_prevents_access() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("remove-key.ovc");

        let kp1 = crate::keys::OvcKeyPair::generate();
        let kp2 = crate::keys::OvcKeyPair::generate();

        let mut repo = Repository::init_with_key(&ovc_path, &kp1).unwrap();
        repo.add_key(&kp2.public_key()).unwrap();
        repo.save().unwrap();

        // Remove kp2 and save.
        let mut repo = Repository::open_with_key(&ovc_path, &kp1).unwrap();
        repo.remove_key(kp2.fingerprint()).unwrap();
        assert_eq!(repo.list_keys().len(), 1);
        repo.save().unwrap();

        // kp2 should no longer work.
        let result = Repository::open_with_key(&ovc_path, &kp2);
        assert!(result.is_err());

        // kp1 should still work.
        let _repo = Repository::open_with_key(&ovc_path, &kp1).unwrap();
    }

    #[test]
    fn password_repos_still_work_after_key_feature() {
        // Verifies backward compatibility: password-only repos are unaffected.
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("pw-compat.ovc");
        let password = b"backward-compat";

        let mut repo = Repository::init(&ovc_path, password).unwrap();
        let blob = Object::Blob(b"compat data".to_vec());
        let oid = repo.insert_object(&blob).unwrap();
        repo.save().unwrap();

        let repo2 = Repository::open(&ovc_path, password).unwrap();
        assert_eq!(repo2.get_object(&oid).unwrap().unwrap(), blob);
        assert!(!repo2.has_key_slots());
    }
}
