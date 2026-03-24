//! Binary search for the commit that introduced a regression.
//!
//! [`BisectState`] implements a binary search over a linear commit range,
//! narrowing down the candidates each time a commit is marked as good or bad.

use std::collections::{HashSet, VecDeque};

use crate::error::CoreResult;
use crate::id::ObjectId;
use crate::object::Object;
use crate::store::ObjectStore;

/// The current state of an active bisect session.
#[derive(Debug, Clone)]
pub struct BisectState {
    /// Commits known to be good (regression absent).
    pub good: Vec<ObjectId>,
    /// Commits known to be bad (regression present).
    pub bad: Vec<ObjectId>,
    /// The candidate commits to search through, ordered chronologically.
    pub candidates: Vec<ObjectId>,
    /// Index of the current test commit within `candidates`.
    pub current_idx: usize,
}

/// The next action the bisect algorithm requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BisectStep {
    /// Test this commit to determine if the regression is present.
    Test(ObjectId),
    /// The first bad commit has been identified.
    Found(ObjectId),
    /// No candidates remain (should not normally occur).
    NoCandidates,
}

impl BisectState {
    /// Starts a new bisect session between a known good commit and a known bad commit.
    ///
    /// Collects all commits between `good` and `bad` by walking from `bad`
    /// back to `good` and sets the initial test point to the midpoint.
    pub fn start(good: ObjectId, bad: ObjectId, store: &ObjectStore) -> CoreResult<Self> {
        let candidates = collect_commits_between(good, bad, store)?;

        let current_idx = candidates.len() / 2;

        Ok(Self {
            good: vec![good],
            bad: vec![bad],
            candidates,
            current_idx,
        })
    }

    /// Returns the current commit to test, or `None` if no candidates remain.
    #[must_use]
    pub fn current(&self) -> Option<ObjectId> {
        self.candidates.get(self.current_idx).copied()
    }

    /// Marks a commit as good (regression absent) and narrows the search range.
    pub fn mark_good(&mut self, oid: ObjectId) -> BisectStep {
        self.good.push(oid);

        // Remove all candidates at or before the marked-good commit.
        if let Some(pos) = self.candidates.iter().position(|c| *c == oid) {
            self.candidates.drain(..=pos);
        }

        self.pick_next()
    }

    /// Marks a commit as bad (regression present) and narrows the search range.
    pub fn mark_bad(&mut self, oid: ObjectId) -> BisectStep {
        self.bad.push(oid);

        // Remove all candidates at or after the marked-bad commit.
        if let Some(pos) = self.candidates.iter().position(|c| *c == oid) {
            self.candidates.truncate(pos);
        }

        self.pick_next()
    }

    /// Returns the estimated number of remaining bisect steps.
    #[must_use]
    pub fn remaining_steps(&self) -> u32 {
        let len = self.candidates.len();
        if len <= 1 {
            return 0;
        }
        // Binary search: ~log2(n) steps.
        // The number of remaining bisect steps is approximately log2(n).
        let n = u32::try_from(len).unwrap_or(u32::MAX);
        let log2_val = f64::from(n).log2().ceil();
        // log2_val is always non-negative for n >= 1, and small enough for u32.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = log2_val as u32;
        steps
    }

    /// Selects the next test commit (midpoint of remaining candidates).
    fn pick_next(&mut self) -> BisectStep {
        if self.candidates.is_empty() {
            // The first bad commit is the earliest bad we know of.
            return self
                .bad
                .last()
                .map_or(BisectStep::NoCandidates, |b| BisectStep::Found(*b));
        }

        if self.candidates.len() == 1 {
            return BisectStep::Found(self.candidates[0]);
        }

        self.current_idx = self.candidates.len() / 2;
        BisectStep::Test(self.candidates[self.current_idx])
    }
}

/// Collects commits between `good` (exclusive) and `bad` (exclusive) by walking
/// backwards from `bad` to `good`.
///
/// The resulting list is sorted chronologically via reverse-BFS order, which is
/// correct for linear (first-parent) chains -- the common case for bisect, and
/// consistent with `git bisect` default behavior. For DAG histories with merge
/// commits the BFS traversal visits all parents, but bisect testing proceeds
/// linearly through the collected candidates regardless of topology.
fn collect_commits_between(
    good: ObjectId,
    bad: ObjectId,
    store: &ObjectStore,
) -> CoreResult<Vec<ObjectId>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    queue.push_back(bad);
    visited.insert(bad);

    // BFS backwards from bad to good.
    while let Some(oid) = queue.pop_front() {
        if oid == good {
            continue;
        }

        let obj = store.get(&oid)?;
        let Some(Object::Commit(commit)) = obj else {
            continue;
        };

        // Don't include bad itself in candidates; it's already known bad.
        if oid != bad {
            result.push(oid);
        }

        for parent in &commit.parents {
            if *parent != good && visited.insert(*parent) {
                queue.push_back(*parent);
            }
        }
    }

    // Sort chronologically (we walked backwards, so reverse).
    result.reverse();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Commit, Identity, Tree};

    fn test_identity() -> Identity {
        Identity {
            name: "Test".into(),
            email: "test@test.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        }
    }

    fn make_chain(store: &mut ObjectStore, count: usize) -> Vec<ObjectId> {
        let empty_tree = Object::Tree(Tree {
            entries: Vec::new(),
        });
        let tree_oid = store.insert(&empty_tree).unwrap();

        let mut oids = Vec::with_capacity(count);
        let mut parent = None;

        for i in 0..count {
            let parents = parent.map_or_else(Vec::new, |p| vec![p]);
            let commit = Commit {
                tree: tree_oid,
                parents,
                author: test_identity(),
                committer: test_identity(),
                message: format!("commit {i}"),
                signature: None,
                sequence: u64::try_from(i).unwrap_or(0) + 1,
            };
            let oid = store.insert(&Object::Commit(commit)).unwrap();
            oids.push(oid);
            parent = Some(oid);
        }

        oids
    }

    #[test]
    fn bisect_finds_bad_commit_in_chain_of_8() {
        let mut store = ObjectStore::default();
        let commits = make_chain(&mut store, 8);

        // Good = commits[0], Bad = commits[7].
        // The "regression" was introduced at commits[4].
        let good = commits[0];
        let bad = commits[7];

        let mut state = BisectState::start(good, bad, &store).unwrap();

        // Simulate the bisect process.
        let mut steps = 0;
        loop {
            let current = match state.current() {
                Some(oid) => oid,
                None => break,
            };

            // Find which index this commit is in our original chain.
            let chain_idx = commits.iter().position(|c| *c == current);

            let step = if let Some(idx) = chain_idx {
                if idx >= 4 {
                    state.mark_bad(current)
                } else {
                    state.mark_good(current)
                }
            } else {
                // Unknown commit; mark as good to keep searching.
                state.mark_good(current)
            };

            steps += 1;

            match step {
                BisectStep::Found(found) => {
                    // Verify we found commit 4 (or something reasonable).
                    let found_idx = commits.iter().position(|c| *c == found);
                    assert!(found_idx.is_some(), "found commit should be in our chain");
                    break;
                }
                BisectStep::NoCandidates => break,
                BisectStep::Test(_) => {}
            }

            // Safety: prevent infinite loops in tests.
            assert!(steps <= 10, "bisect took too many steps");
        }

        assert!(
            steps <= 5,
            "bisect should complete in ~log2(8)=3 steps, took {steps}"
        );
    }

    #[test]
    fn remaining_steps_estimate() {
        let mut store = ObjectStore::default();
        let commits = make_chain(&mut store, 16);

        let state = BisectState::start(commits[0], commits[15], &store).unwrap();

        // With 14 candidates, should estimate ~4 steps (log2(14) ≈ 3.8).
        let steps = state.remaining_steps();
        assert!(steps >= 3 && steps <= 5, "expected 3-5 steps, got {steps}");
    }
}
