//! Totals that count each file once (phase 6 items 10 and 11, catalog
//! deduplication rules). Hard links are counted once inside a candidate by
//! the walker; this module handles candidates that overlap each other.
//! Tool rows are never merged with a delete: their upper bound is shown on
//! its own line.

use crate::model::{Action, Candidate, CandidateId, Scope};

/// Whether deleting `outer` also removes everything `inner` would.
/// `outer_first` breaks the tie between two candidates for the same folder,
/// so exactly one of them covers the other.
fn covers(outer: &Candidate, inner: &Candidate, outer_first: bool) -> bool {
    if outer.action != Action::Delete || inner.action != Action::Delete {
        return false;
    }
    if outer.file_id == inner.file_id && outer.scope == inner.scope {
        return outer_first;
    }
    match outer.scope {
        // The entry goes whole, with everything below it.
        Scope::Entry => inner.path.starts_with(&outer.path),
        // Everything inside goes; the folder itself stays.
        Scope::Contents => inner.path != outer.path && inner.path.starts_with(&outer.path),
        Scope::Tool | Scope::Report => false,
    }
}

/// Fills each candidate's `covers` list.
pub(crate) fn link_covers(candidates: &mut [Candidate]) {
    let lists: Vec<Vec<CandidateId>> = candidates
        .iter()
        .enumerate()
        .map(|(i, outer)| {
            candidates
                .iter()
                .enumerate()
                .filter(|&(j, inner)| i != j && covers(outer, inner, i < j))
                .map(|(_, inner)| inner.id)
                .collect()
        })
        .collect();
    for (candidate, list) in candidates.iter_mut().zip(lists) {
        candidate.covers = list;
    }
}

/// The chosen candidates that no other chosen candidate covers. A covered
/// one would only be counted twice, then found gone.
pub(crate) fn uncovered<'a>(chosen: &[&'a Candidate]) -> Vec<&'a Candidate> {
    chosen
        .iter()
        .filter(|c| !chosen.iter().any(|d| d.covers.contains(&c.id)))
        .copied()
        .collect()
}
