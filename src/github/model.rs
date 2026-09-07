//! Wire types: the exact shape GitHub's GraphQL API returns.
//!
//! Kept deliberately separate from the domain model in [`crate::digest`] so
//! that rendering never has to reason about nullable authors or cursors.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Actor {
    pub login: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection<T> {
    pub page_info: PageInfo,
    pub nodes: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub title: String,
    pub number: u64,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub author: Option<Actor>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub created_at: String,
    pub updated_at: String,
    pub body: String,
    pub reviews: Connection<Review>,
    pub comments: Connection<IssueComment>,
    pub review_threads: Connection<ReviewThread>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub author: Option<Actor>,
    pub state: String,
    /// Null for a review that is still pending submission.
    pub submitted_at: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueComment {
    pub author: Option<Actor>,
    pub created_at: String,
    pub body: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewThread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub resolved_by: Option<Actor>,
    pub path: String,
    /// Null once the line no longer exists in the head commit.
    pub line: Option<u64>,
    /// The line as it stood when the comment was written.
    pub original_line: Option<u64>,
    pub comments: Connection<ThreadComment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadComment {
    pub author: Option<Actor>,
    pub created_at: String,
    pub body: String,
    pub diff_hunk: Option<String>,
    pub url: String,
}
