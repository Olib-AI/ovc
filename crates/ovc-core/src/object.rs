//! Object model for the OVC version control system.
//!
//! Every piece of data stored in an OVC repository is represented as an
//! [`Object`]: blobs hold file contents, trees describe directory structure,
//! commits record snapshots, and tags provide named references with optional
//! cryptographic signatures.

use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

/// The type discriminant for stored objects, used as a single byte prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ObjectType {
    /// Raw file content.
    Blob = 1,
    /// A directory listing.
    Tree = 2,
    /// A snapshot of the repository state.
    Commit = 3,
    /// A named, optionally signed reference.
    Tag = 4,
}

impl ObjectType {
    /// Creates an `ObjectType` from its `u8` discriminant.
    ///
    /// Returns `None` for unrecognized values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Blob),
            2 => Some(Self::Tree),
            3 => Some(Self::Commit),
            4 => Some(Self::Tag),
            _ => None,
        }
    }
}

/// Unix-style file mode for tree entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum FileMode {
    /// A regular (non-executable) file.
    Regular = 0,
    /// An executable file.
    Executable = 1,
    /// A symbolic link.
    Symlink = 2,
    /// A subdirectory (pointing to a tree object).
    Directory = 3,
    /// A nested repository reference.
    Subrepository = 4,
}

impl FileMode {
    /// Creates a `FileMode` from its `u8` discriminant.
    ///
    /// Returns `None` for unrecognized values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Regular),
            1 => Some(Self::Executable),
            2 => Some(Self::Symlink),
            3 => Some(Self::Directory),
            4 => Some(Self::Subrepository),
            _ => None,
        }
    }
}

/// A single entry in a [`Tree`], associating a name with a mode and object id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    /// The file mode (regular, executable, symlink, etc.).
    pub mode: FileMode,
    /// The entry name as raw bytes (typically UTF-8 but not required).
    pub name: Vec<u8>,
    /// The object id of the referenced blob or subtree.
    pub oid: ObjectId,
}

/// A directory listing containing zero or more [`TreeEntry`] values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    /// The entries in this tree.
    pub entries: Vec<TreeEntry>,
}

impl Tree {
    /// Sorts entries by name in byte order, producing the canonical form used
    /// for hashing. This operation is idempotent.
    pub fn canonicalize(&mut self) {
        self.entries.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// Author or committer identity with a timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
    /// Unix timestamp in seconds since epoch.
    pub timestamp: i64,
    /// Timezone offset from UTC in minutes (e.g., −480 for PST).
    pub tz_offset_minutes: i16,
}

/// A commit object recording a snapshot of the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    /// The root tree object this commit points to.
    pub tree: ObjectId,
    /// Parent commit ids (empty for the initial commit).
    pub parents: Vec<ObjectId>,
    /// The person who authored the change.
    pub author: Identity,
    /// The person who created this commit object.
    pub committer: Identity,
    /// The commit message.
    pub message: String,
    /// Optional detached cryptographic signature over the commit.
    pub signature: Option<Vec<u8>>,
    /// Monotonically increasing sequence number for fast ordering.
    pub sequence: u64,
}

/// A tag object providing a named, optionally signed reference to another object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// The object id this tag points to.
    pub target: ObjectId,
    /// The type of the target object.
    pub target_type: ObjectType,
    /// The tag name.
    pub tag_name: String,
    /// The person who created the tag.
    pub tagger: Identity,
    /// The tag annotation message.
    pub message: String,
    /// Optional detached cryptographic signature.
    pub signature: Option<Vec<u8>>,
}

/// A version-controlled object in the OVC store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Object {
    /// Raw file content.
    Blob(Vec<u8>),
    /// A directory listing.
    Tree(Tree),
    /// A repository snapshot.
    Commit(Commit),
    /// A named reference.
    Tag(Tag),
}

impl Object {
    /// Returns the [`ObjectType`] discriminant for this object.
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::Blob(_) => ObjectType::Blob,
            Self::Tree(_) => ObjectType::Tree,
            Self::Commit(_) => ObjectType::Commit,
            Self::Tag(_) => ObjectType::Tag,
        }
    }
}
