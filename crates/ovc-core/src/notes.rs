//! Commit annotations (notes).
//!
//! Notes are stored as a `BTreeMap<ObjectId, String>` in the superblock,
//! allowing arbitrary text to be attached to any commit without modifying
//! the commit object itself.

use std::collections::BTreeMap;

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;

/// Retrieves the note attached to a commit, if any.
#[must_use]
pub fn get_note<'a>(notes: &'a BTreeMap<ObjectId, String>, oid: &ObjectId) -> Option<&'a String> {
    notes.get(oid)
}

/// Adds or replaces a note on a commit.
pub fn set_note(notes: &mut BTreeMap<ObjectId, String>, oid: ObjectId, message: String) {
    notes.insert(oid, message);
}

/// Removes the note from a commit. Returns an error if no note exists.
pub fn remove_note(notes: &mut BTreeMap<ObjectId, String>, oid: &ObjectId) -> CoreResult<()> {
    notes
        .remove(oid)
        .map(|_| ())
        .ok_or_else(|| CoreError::FormatError {
            reason: format!("no note found for commit {oid}"),
        })
}
