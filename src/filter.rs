//! Thread selection.
//!
//! Kept out of the renderer on purpose. The original filtered inside its
//! Markdown generator, which is what made the thread numbering depend on the
//! flags and made "what is in this document" impossible to test without
//! generating a document.

use crate::cli::{OutdatedFilter, StatusFilter};
use crate::digest::Thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    pub status: StatusFilter,
    pub outdated: OutdatedFilter,
}

impl Filter {
    pub fn new(status: StatusFilter, outdated: OutdatedFilter) -> Self {
        Filter { status, outdated }
    }

    /// The two axes are independent predicates, so a thread has to satisfy
    /// both. No combination is contradictory: asking for resolved-and-outdated
    /// threads is a perfectly good question about a pull request's history.
    pub fn matches(&self, thread: &Thread) -> bool {
        let status_ok = match self.status {
            StatusFilter::All => true,
            StatusFilter::Open => !thread.status.is_resolved(),
            StatusFilter::Resolved => thread.status.is_resolved(),
        };

        let outdated_ok = match self.outdated {
            OutdatedFilter::Include => true,
            OutdatedFilter::Hide => !thread.status.is_outdated(),
            OutdatedFilter::Only => thread.status.is_outdated(),
        };

        status_ok && outdated_ok
    }

    pub fn apply<'a>(&self, threads: &'a [Thread]) -> Vec<&'a Thread> {
        threads.iter().filter(|t| self.matches(t)).collect()
    }

    /// Whether this filter would let everything through, used to decide
    /// whether the document needs to explain that it is showing a subset.
    pub fn is_permissive(&self) -> bool {
        self.status == StatusFilter::All && self.outdated == OutdatedFilter::Include
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::{Location, ThreadStatus};

    fn thread(status: ThreadStatus) -> Thread {
        Thread {
            index: 1,
            status,
            resolved_by: None,
            path: "src/main.rs".into(),
            location: Location::Line(1),
            url: None,
            diff_hunk: None,
            comments: Vec::new(),
        }
    }

    fn all() -> Vec<Thread> {
        vec![
            thread(ThreadStatus::OpenActive),
            thread(ThreadStatus::OpenOutdated),
            thread(ThreadStatus::ResolvedActive),
            thread(ThreadStatus::ResolvedOutdated),
        ]
    }

    fn selected(status: StatusFilter, outdated: OutdatedFilter) -> Vec<ThreadStatus> {
        let threads = all();
        Filter::new(status, outdated)
            .apply(&threads)
            .iter()
            .map(|t| t.status)
            .collect()
    }

    #[test]
    fn default_filter_selects_everything() {
        assert_eq!(selected(StatusFilter::All, OutdatedFilter::Include).len(), 4);
    }

    #[test]
    fn the_two_axes_compose_into_every_cell_of_the_matrix() {
        assert_eq!(
            selected(StatusFilter::Open, OutdatedFilter::Hide),
            vec![ThreadStatus::OpenActive]
        );
        assert_eq!(
            selected(StatusFilter::Open, OutdatedFilter::Only),
            vec![ThreadStatus::OpenOutdated]
        );
        assert_eq!(
            selected(StatusFilter::Resolved, OutdatedFilter::Hide),
            vec![ThreadStatus::ResolvedActive]
        );
        assert_eq!(
            selected(StatusFilter::Resolved, OutdatedFilter::Only),
            vec![ThreadStatus::ResolvedOutdated]
        );
    }

    #[test]
    fn each_axis_alone_selects_a_row_or_a_column() {
        assert_eq!(
            selected(StatusFilter::Open, OutdatedFilter::Include),
            vec![ThreadStatus::OpenActive, ThreadStatus::OpenOutdated]
        );
        assert_eq!(
            selected(StatusFilter::All, OutdatedFilter::Only),
            vec![ThreadStatus::OpenOutdated, ThreadStatus::ResolvedOutdated]
        );
    }
}
