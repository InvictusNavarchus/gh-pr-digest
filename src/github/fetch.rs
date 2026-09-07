//! Fetch a complete pull request, draining every connection.
//!
//! "Complete" is the point. The original paginated only `reviewThreads`, so a
//! pull request with more than 100 reviews or conversation comments, or a
//! thread with more than 50 replies, was silently truncated -- the worst kind
//! of bug in a tool whose entire job is "show me everything I still owe a
//! reply to".

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::error::{Error, Result};
use crate::github::client::Client;
use crate::github::model::{Connection, IssueComment, PullRequest, Review, ReviewThread, ThreadComment};
use crate::github::query;
use crate::repo::Repo;

#[derive(Deserialize)]
struct RepositoryField<T> {
    repository: Option<PullRequestField<T>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestField<T> {
    pull_request: Option<T>,
}

#[derive(Deserialize)]
struct ReviewsPage {
    reviews: Connection<Review>,
}

#[derive(Deserialize)]
struct CommentsPage {
    comments: Connection<IssueComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsPage {
    review_threads: Connection<ReviewThread>,
}

#[derive(Deserialize)]
struct NodeField {
    node: Option<ThreadCommentsPage>,
}

#[derive(Deserialize)]
struct ThreadCommentsPage {
    comments: Connection<ThreadComment>,
}

pub fn pull_request(client: &Client, repo: &Repo, number: u64) -> Result<PullRequest> {
    let base = json!({ "owner": repo.owner, "name": repo.name, "number": number });

    let head: RepositoryField<PullRequest> = client.query(&query::head(), base.clone())?;
    let mut pr = head
        .repository
        .and_then(|r| r.pull_request)
        .ok_or_else(|| Error::NotFound(format!("pull request #{number} not found in {repo}")))?;

    drain(&mut pr.reviews, |cursor| {
        let page: RepositoryField<ReviewsPage> =
            client.query(&query::reviews_page(), with_cursor(&base, cursor))?;
        Ok(unwrap_pr(page, number, repo)?.reviews)
    })?;

    drain(&mut pr.comments, |cursor| {
        let page: RepositoryField<CommentsPage> =
            client.query(&query::comments_page(), with_cursor(&base, cursor))?;
        Ok(unwrap_pr(page, number, repo)?.comments)
    })?;

    drain(&mut pr.review_threads, |cursor| {
        let page: RepositoryField<ThreadsPage> =
            client.query(&query::review_threads_page(), with_cursor(&base, cursor))?;
        Ok(unwrap_pr(page, number, repo)?.review_threads)
    })?;

    for thread in &mut pr.review_threads.nodes {
        let id = thread.id.clone();
        drain(&mut thread.comments, |cursor| {
            let page: NodeField = client.query(
                &query::thread_comments_page(),
                json!({ "threadId": id, "cursor": cursor }),
            )?;
            page.node
                .map(|n| n.comments)
                .ok_or_else(|| Error::Api(format!("review thread {id} vanished mid-pagination")))
        })?;
    }

    Ok(pr)
}

/// Follow `connection`'s cursor until GitHub runs out of pages, appending each
/// page's nodes in order.
fn drain<T, F>(connection: &mut Connection<T>, mut next_page: F) -> Result<()>
where
    F: FnMut(&str) -> Result<Connection<T>>,
{
    // `hasNextPage` without an `endCursor` would loop forever on the same page,
    // so the cursor -- not the flag -- is what actually drives the loop.
    while connection.page_info.has_next_page {
        let Some(cursor) = connection.page_info.end_cursor.clone() else {
            break;
        };

        let mut page = next_page(&cursor)?;
        connection.nodes.append(&mut page.nodes);
        connection.page_info = page.page_info;
    }

    Ok(())
}

fn with_cursor(base: &serde_json::Value, cursor: &str) -> serde_json::Value {
    let mut vars = base.clone();
    vars["cursor"] = json!(cursor);
    vars
}

fn unwrap_pr<T: DeserializeOwned>(
    response: RepositoryField<T>,
    number: u64,
    repo: &Repo,
) -> Result<T> {
    response
        .repository
        .and_then(|r| r.pull_request)
        .ok_or_else(|| {
            Error::NotFound(format!(
                "pull request #{number} in {repo} disappeared mid-pagination"
            ))
        })
}
