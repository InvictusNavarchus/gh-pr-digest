//! The domain model: what a pull request digest actually contains.
//!
//! Converting the wire types here means the renderer never touches a nullable
//! author, a cursor, or a pair of booleans that have to be re-derived into a
//! status at four separate call sites.

use crate::github::model;

/// A pull request participant whose account may since have been deleted.
const GHOST: &str = "ghost";

/// The four states a review thread can be in.
///
/// Declaration order is priority order, so the derived `Ord` is the sort order
/// too: what still needs action comes first. The original recomputed this from
/// two booleans in a `getThreadPriority` function, a `formatStatusBadge`
/// function, and four separate counting passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreadStatus {
    /// Open, on lines the head commit still has. Actionable.
    OpenActive,
    /// Open, but later commits moved the lines out from under it.
    OpenOutdated,
    /// Resolved, on current lines.
    ResolvedActive,
    /// Resolved, and superseded. History.
    ResolvedOutdated,
}

impl ThreadStatus {
    pub fn new(resolved: bool, outdated: bool) -> Self {
        match (resolved, outdated) {
            (false, false) => ThreadStatus::OpenActive,
            (false, true) => ThreadStatus::OpenOutdated,
            (true, false) => ThreadStatus::ResolvedActive,
            (true, true) => ThreadStatus::ResolvedOutdated,
        }
    }

    pub fn is_resolved(self) -> bool {
        matches!(self, ThreadStatus::ResolvedActive | ThreadStatus::ResolvedOutdated)
    }

    pub fn is_outdated(self) -> bool {
        matches!(self, ThreadStatus::OpenOutdated | ThreadStatus::ResolvedOutdated)
    }
}

/// Where a thread is anchored in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// A line that still exists in the head commit.
    Line(u64),
    /// The line the comment was written against, since moved or deleted.
    OriginalLine(u64),
    /// A comment on the file as a whole.
    File,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub author: String,
    pub created_at: String,
    pub body: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Review {
    pub author: String,
    pub state: String,
    pub submitted_at: Option<String>,
    pub body: String,
}

impl Review {
    /// A bare `COMMENTED` review with no body is the empty envelope GitHub
    /// creates to hold inline comments; it carries no information of its own
    /// and the threads themselves are rendered separately.
    pub fn is_substantive(&self) -> bool {
        !self.body.trim().is_empty() || self.state != "COMMENTED"
    }
}

#[derive(Debug, Clone)]
pub struct Thread {
    /// 1-based position in the sorted, *unfiltered* thread list.
    ///
    /// Stable on purpose: the original numbered threads after filtering, so the
    /// same thread was "#3" in one run and "#7" in the next, which makes the
    /// number useless as a reference in a review conversation.
    pub index: usize,
    pub status: ThreadStatus,
    pub resolved_by: Option<String>,
    pub path: String,
    pub location: Location,
    pub url: Option<String>,
    pub diff_hunk: Option<String>,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreadCounts {
    pub open_active: usize,
    pub open_outdated: usize,
    pub resolved_active: usize,
    pub resolved_outdated: usize,
}

impl ThreadCounts {
    pub fn total(&self) -> usize {
        self.open_active + self.open_outdated + self.resolved_active + self.resolved_outdated
    }
}

#[derive(Debug, Clone)]
pub struct Digest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub author: String,
    pub head_ref: String,
    pub base_ref: String,
    pub created_at: String,
    pub updated_at: String,
    pub body: String,
    pub reviews: Vec<Review>,
    pub comments: Vec<Comment>,
    /// Every thread, sorted so that actionable ones come first.
    pub threads: Vec<Thread>,
    /// Counted before any filtering, so the summary always describes the whole
    /// pull request even when the document shows a slice of it.
    pub counts: ThreadCounts,
}

impl Digest {
    pub fn from_wire(pr: model::PullRequest) -> Self {
        let mut threads: Vec<Thread> = pr
            .review_threads
            .nodes
            .into_iter()
            .map(thread_from_wire)
            .collect();

        // Stable sort, so threads keep GitHub's ordering within a status group.
        threads.sort_by_key(|t| t.status);
        for (position, thread) in threads.iter_mut().enumerate() {
            thread.index = position + 1;
        }

        let mut counts = ThreadCounts::default();
        for thread in &threads {
            match thread.status {
                ThreadStatus::OpenActive => counts.open_active += 1,
                ThreadStatus::OpenOutdated => counts.open_outdated += 1,
                ThreadStatus::ResolvedActive => counts.resolved_active += 1,
                ThreadStatus::ResolvedOutdated => counts.resolved_outdated += 1,
            }
        }

        Digest {
            number: pr.number,
            title: pr.title,
            url: pr.url,
            state: pr.state,
            is_draft: pr.is_draft,
            author: login(pr.author),
            head_ref: pr.head_ref_name,
            base_ref: pr.base_ref_name,
            created_at: pr.created_at,
            updated_at: pr.updated_at,
            body: pr.body,
            reviews: pr
                .reviews
                .nodes
                .into_iter()
                .map(|r| Review {
                    author: login(r.author),
                    state: r.state,
                    submitted_at: r.submitted_at,
                    body: r.body,
                })
                .collect(),
            comments: pr
                .comments
                .nodes
                .into_iter()
                .map(|c| Comment {
                    author: login(c.author),
                    created_at: c.created_at,
                    body: c.body,
                    url: Some(c.url),
                })
                .collect(),
            threads,
            counts,
        }
    }
}

fn thread_from_wire(thread: model::ReviewThread) -> Thread {
    let location = match (thread.line, thread.original_line) {
        (Some(line), _) => Location::Line(line),
        (None, Some(original)) => Location::OriginalLine(original),
        (None, None) => Location::File,
    };

    // The hunk belongs to the comment that opened the thread; replies carry a
    // copy of the same one, so any of them would do, but the first is the one
    // whose context the reader is about to read.
    let diff_hunk = thread
        .comments
        .nodes
        .iter()
        .find_map(|c| c.diff_hunk.as_ref())
        .map(|hunk| hunk.trim_end().to_string())
        .filter(|hunk| !hunk.is_empty());

    let url = thread.comments.nodes.first().map(|c| c.url.clone());

    Thread {
        index: 0, // assigned once the full set is sorted
        status: ThreadStatus::new(thread.is_resolved, thread.is_outdated),
        resolved_by: thread.resolved_by.map(|a| a.login),
        path: thread.path,
        location,
        url,
        diff_hunk,
        comments: thread
            .comments
            .nodes
            .into_iter()
            .map(|c| Comment {
                author: login(c.author),
                created_at: c.created_at,
                body: c.body,
                url: Some(c.url),
            })
            .collect(),
    }
}

/// One fallback for a missing author, everywhere.
///
/// The original used "Unknown" for reviews and "ghost" for comments, which is
/// the same deleted account rendered two different ways in one document.
fn login(actor: Option<model::Actor>) -> String {
    actor.map(|a| a.login).unwrap_or_else(|| GHOST.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_ordering_puts_actionable_threads_first() {
        let mut statuses = vec![
            ThreadStatus::ResolvedOutdated,
            ThreadStatus::OpenOutdated,
            ThreadStatus::ResolvedActive,
            ThreadStatus::OpenActive,
        ];
        statuses.sort();
        assert_eq!(
            statuses,
            vec![
                ThreadStatus::OpenActive,
                ThreadStatus::OpenOutdated,
                ThreadStatus::ResolvedActive,
                ThreadStatus::ResolvedOutdated,
            ]
        );
    }

    #[test]
    fn status_predicates_agree_with_the_flags_they_came_from() {
        for resolved in [true, false] {
            for outdated in [true, false] {
                let status = ThreadStatus::new(resolved, outdated);
                assert_eq!(status.is_resolved(), resolved);
                assert_eq!(status.is_outdated(), outdated);
            }
        }
    }

    #[test]
    fn empty_commented_reviews_are_not_substantive() {
        let review = |state: &str, body: &str| Review {
            author: "a".into(),
            state: state.into(),
            submitted_at: None,
            body: body.into(),
        };
        assert!(!review("COMMENTED", "   ").is_substantive());
        assert!(review("COMMENTED", "looks good").is_substantive());
        assert!(review("APPROVED", "").is_substantive());
        assert!(review("CHANGES_REQUESTED", "").is_substantive());
    }
}
