//! GraphQL documents.
//!
//! One "head" operation fetches the pull request plus the first page of each
//! connection; three narrow operations drain whatever is left. The alternative
//! -- a single document juggling three cursors -- is what the original
//! attempted, and it re-fetched the reviews and comments connections on every
//! thread page while discarding all but the first.

/// Page size. 100 is GitHub's per-connection maximum.
pub const PAGE_SIZE: u32 = 100;

const PR_FIELDS: &str = r"
      title
      number
      url
      state
      isDraft
      author { login }
      headRefName
      baseRefName
      createdAt
      updatedAt
      body
";

const REVIEW_FIELDS: &str = r"
      pageInfo { hasNextPage endCursor }
      nodes { author { login } state submittedAt body }
";

const COMMENT_FIELDS: &str = r"
      pageInfo { hasNextPage endCursor }
      nodes { author { login } createdAt body }
";

const THREAD_COMMENT_FIELDS: &str = r"
      pageInfo { hasNextPage endCursor }
      nodes { author { login } createdAt body diffHunk url }
";

/// A function rather than a constant because the nested comment connection has
/// to interpolate the same page size; a hardcoded literal there silently
/// escapes any change to `PAGE_SIZE`.
fn thread_fields() -> String {
    format!(
        r"
      pageInfo {{ hasNextPage endCursor }}
      nodes {{
        id
        isResolved
        isOutdated
        resolvedBy {{ login }}
        path
        line
        originalLine
        comments(first: {PAGE_SIZE}) {{ {THREAD_COMMENT_FIELDS} }}
      }}
"
    )
}

pub fn head() -> String {
    format!(
        r"query($owner: String!, $name: String!, $number: Int!) {{
  repository(owner: $owner, name: $name) {{
    pullRequest(number: $number) {{
      {PR_FIELDS}
      reviews(first: {PAGE_SIZE}) {{ {REVIEW_FIELDS} }}
      comments(first: {PAGE_SIZE}) {{ {COMMENT_FIELDS} }}
      reviewThreads(first: {PAGE_SIZE}) {{ {thread_fields} }}
    }}
  }}
}}",
        thread_fields = thread_fields()
    )
}

pub fn reviews_page() -> String {
    format!(
        r"query($owner: String!, $name: String!, $number: Int!, $cursor: String) {{
  repository(owner: $owner, name: $name) {{
    pullRequest(number: $number) {{
      reviews(first: {PAGE_SIZE}, after: $cursor) {{ {REVIEW_FIELDS} }}
    }}
  }}
}}"
    )
}

pub fn comments_page() -> String {
    format!(
        r"query($owner: String!, $name: String!, $number: Int!, $cursor: String) {{
  repository(owner: $owner, name: $name) {{
    pullRequest(number: $number) {{
      comments(first: {PAGE_SIZE}, after: $cursor) {{ {COMMENT_FIELDS} }}
    }}
  }}
}}"
    )
}

pub fn review_threads_page() -> String {
    format!(
        r"query($owner: String!, $name: String!, $number: Int!, $cursor: String) {{
  repository(owner: $owner, name: $name) {{
    pullRequest(number: $number) {{
      reviewThreads(first: {PAGE_SIZE}, after: $cursor) {{ {thread_fields} }}
    }}
  }}
}}",
        thread_fields = thread_fields()
    )
}

/// Drains a single thread whose own comment list overflowed one page.
///
/// Addressed by node id rather than by position, because the thread's index
/// within `reviewThreads` is not a stable handle across requests.
pub fn thread_comments_page() -> String {
    format!(
        r"query($threadId: ID!, $cursor: String) {{
  node(id: $threadId) {{
    ... on PullRequestReviewThread {{
      comments(first: {PAGE_SIZE}, after: $cursor) {{ {THREAD_COMMENT_FIELDS} }}
    }}
  }}
}}"
    )
}
