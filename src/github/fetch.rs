//! Fetch a complete pull request, draining every connection.
//!
//! "Complete" is the point. The original paginated only `reviewThreads`, so a
//! pull request with more than 100 reviews or conversation comments, or a
//! thread with more than 50 replies, was silently truncated -- the worst kind
//! of bug in a tool whose entire job is "show me everything I still owe a
//! reply to".

use std::collections::HashSet;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::error::{Error, Result};
use crate::github::client::Client;
use crate::github::model::{
    Connection, IssueComment, PullRequest, Review, ReviewThread, ThreadComment,
};
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

    drain(&mut pr.reviews, "reviews", |cursor| {
        let page: RepositoryField<ReviewsPage> =
            client.query(&query::reviews_page(), with_cursor(&base, cursor))?;
        Ok(unwrap_pr(page, number, repo)?.reviews)
    })?;

    drain(&mut pr.comments, "conversation comments", |cursor| {
        let page: RepositoryField<CommentsPage> =
            client.query(&query::comments_page(), with_cursor(&base, cursor))?;
        Ok(unwrap_pr(page, number, repo)?.comments)
    })?;

    drain(&mut pr.review_threads, "review threads", |cursor| {
        let page: RepositoryField<ThreadsPage> =
            client.query(&query::review_threads_page(), with_cursor(&base, cursor))?;
        Ok(unwrap_pr(page, number, repo)?.review_threads)
    })?;

    for thread in &mut pr.review_threads.nodes {
        let id = thread.id.clone();
        drain(
            &mut thread.comments,
            &format!("comments for review thread {id}"),
            |cursor| {
                let page: NodeField = client.query(
                    &query::thread_comments_page(),
                    json!({ "threadId": id, "cursor": cursor }),
                )?;
                page.node.map(|n| n.comments).ok_or_else(|| {
                    Error::Api(format!("review thread {id} vanished mid-pagination"))
                })
            },
        )?;
    }

    Ok(pr)
}

/// Follow `connection`'s cursor until GitHub runs out of pages, appending each
/// page's nodes in order.
fn drain<T, F>(connection: &mut Connection<T>, name: &str, mut next_page: F) -> Result<()>
where
    F: FnMut(&str) -> Result<Connection<T>>,
{
    // An inconsistent cursor cannot establish completeness. Fail before a
    // partial digest reaches rendering, and never request the same page twice.
    let mut seen = HashSet::new();
    while connection.page_info.has_next_page {
        let cursor = connection
            .page_info
            .end_cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
            .ok_or_else(|| {
                Error::Api(format!(
                    "invalid pagination for {name}: hasNextPage is true but endCursor is missing or empty"
                ))
            })?
            .to_string();
        if !seen.insert(cursor.clone()) {
            return Err(Error::Api(format!(
                "invalid pagination for {name}: endCursor repeated; pagination is not progressing"
            )));
        }

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

#[cfg(test)]
mod tests {
    use crate::github::model::PageInfo;

    use super::*;

    fn page(nodes: Vec<u32>, has_next_page: bool, cursor: Option<&str>) -> Connection<u32> {
        Connection {
            nodes,
            page_info: PageInfo {
                has_next_page,
                end_cursor: cursor.map(str::to_string),
            },
        }
    }

    fn assert_invalid_pagination(error: Error, reason: &str) {
        assert!(matches!(error, Error::Api(_)), "{error}");
        assert_eq!(error.exit_code(), 1);
        let message = error.to_string();
        assert!(
            message.contains("invalid pagination for test comments"),
            "{message}"
        );
        assert!(message.contains(reason), "{message}");
    }

    #[test]
    fn terminal_pages_do_not_require_a_cursor_or_another_request() {
        for nodes in [vec![], vec![1]] {
            let mut connection = page(nodes.clone(), false, None);
            drain(&mut connection, "test comments", |_| {
                panic!("a terminal page must not trigger a request")
            })
            .unwrap();
            assert_eq!(connection.nodes, nodes);
        }
    }

    #[test]
    fn drains_all_pages_in_order_using_the_returned_cursors() {
        let mut connection = page(vec![1], true, Some("first"));
        let mut pages = [
            ("first", page(vec![2, 3], true, Some("second"))),
            ("second", page(vec![4], false, Some("last"))),
        ]
        .into_iter();
        drain(&mut connection, "test comments", |cursor| {
            let (expected_cursor, next) = pages.next().expect("unexpected extra request");
            assert_eq!(cursor, expected_cursor);
            Ok(next)
        })
        .unwrap();
        assert!(pages.next().is_none());
        assert_eq!(connection.nodes, vec![1, 2, 3, 4]);
        assert!(!connection.page_info.has_next_page);
    }

    #[test]
    fn rejects_missing_or_empty_cursors_before_requesting_more() {
        for cursor in [None, Some("")] {
            let mut connection = page(vec![1], true, cursor);
            let error = drain(&mut connection, "test comments", |_| {
                panic!("an invalid cursor must not trigger a request")
            })
            .unwrap_err();
            assert_invalid_pagination(error, "endCursor is missing or empty");
        }
    }

    #[test]
    fn rejects_missing_or_empty_cursors_on_later_pages_too() {
        for cursor in [None, Some("")] {
            let mut connection = page(vec![1], true, Some("first"));
            let mut requests = 0;
            let error = drain(&mut connection, "test comments", |requested| {
                requests += 1;
                assert_eq!(requests, 1, "invalid page must stop pagination");
                assert_eq!(requested, "first");
                Ok(page(vec![2], true, cursor))
            })
            .unwrap_err();
            assert_invalid_pagination(error, "endCursor is missing or empty");
        }
    }

    #[test]
    fn rejects_repeated_cursors_including_cycles_before_requesting_them_again() {
        for cursors in [vec!["first"], vec!["second", "first"]] {
            let mut connection = page(vec![1], true, Some("first"));
            let expected_requests = cursors.len();
            let mut returned_cursors = cursors.into_iter();
            let mut requests = 0;
            let error = drain(&mut connection, "test comments", |_| {
                requests += 1;
                let cursor = returned_cursors.next().expect("repeated request");
                Ok(page(vec![2], true, Some(cursor)))
            })
            .unwrap_err();
            assert_eq!(requests, expected_requests);
            assert_invalid_pagination(error, "endCursor repeated");
        }
    }

    #[test]
    fn propagates_page_fetch_failures() {
        let mut connection = page(vec![1], true, Some("first"));
        let error = drain(&mut connection, "test comments", |_| {
            Err(Error::Network("connection lost".into()))
        })
        .unwrap_err();
        assert!(matches!(error, Error::Network(_)));
        assert_eq!(error.to_string(), "connection lost");
    }
}
