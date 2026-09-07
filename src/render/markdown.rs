//! Markdown rendering.

use std::fmt::Write;

use crate::digest::{Comment, Digest, Location, Thread, ThreadStatus};
use crate::filter::Filter;

pub fn render(digest: &Digest, filter: Filter) -> String {
    let mut out = String::new();

    header(&mut out, digest);
    description(&mut out, digest);
    reviews(&mut out, digest);
    conversation(&mut out, digest);
    threads(&mut out, digest, filter);

    out
}

fn header(out: &mut String, digest: &Digest) {
    let draft = if digest.is_draft { " (Draft)" } else { "" };
    let _ = writeln!(out, "# PR #{}: {}", digest.number, digest.title);
    let _ = writeln!(out);
    let _ = writeln!(out, "- **URL**: {}", digest.url);
    let _ = writeln!(out, "- **Author**: @{}", digest.author);
    let _ = writeln!(out, "- **State**: `{}`{draft}", digest.state);
    let _ = writeln!(
        out,
        "- **Branches**: `{}` -> `{}`",
        digest.head_ref, digest.base_ref
    );
    let _ = writeln!(out, "- **Created**: {}", digest.created_at);
    let _ = writeln!(out, "- **Updated**: {}", digest.updated_at);
    let _ = writeln!(out);
}

fn description(out: &mut String, digest: &Digest) {
    if digest.body.trim().is_empty() {
        return;
    }
    let _ = writeln!(out, "## Description");
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", quote(&digest.body));
    let _ = writeln!(out);
}

fn reviews(out: &mut String, digest: &Digest) {
    let substantive: Vec<_> = digest
        .reviews
        .iter()
        .filter(|r| r.is_substantive())
        .collect();
    if substantive.is_empty() {
        return;
    }

    let _ = writeln!(out, "## Top-Level Reviews");
    let _ = writeln!(out);
    for review in substantive {
        let when = review.submitted_at.as_deref().unwrap_or("pending");
        let _ = writeln!(out, "### @{} - `{}` ({when})", review.author, review.state);
        let _ = writeln!(out);
        if review.body.trim().is_empty() {
            let _ = writeln!(out, "_No review summary body provided._");
        } else {
            let _ = writeln!(out, "{}", quote(&review.body));
        }
        let _ = writeln!(out);
    }
}

fn conversation(out: &mut String, digest: &Digest) {
    if digest.comments.is_empty() {
        return;
    }

    let _ = writeln!(out, "## Conversation Comments");
    let _ = writeln!(out);
    for (position, comment) in digest.comments.iter().enumerate() {
        let _ = writeln!(
            out,
            "### Comment #{} by @{} ({})",
            position + 1,
            comment.author,
            comment.created_at
        );
        let _ = writeln!(out);
        if let Some(url) = &comment.url {
            let _ = writeln!(out, "[Link]({url})");
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "{}", quote(&comment.body));
        let _ = writeln!(out);
    }
}

fn threads(out: &mut String, digest: &Digest, filter: Filter) {
    let selected = filter.apply(&digest.threads);

    let _ = writeln!(out, "## Inline Review Threads");
    let _ = writeln!(out);
    overview(out, digest, selected.len(), filter);

    if selected.is_empty() {
        let _ = writeln!(
            out,
            "_No inline review threads match the selected criteria._"
        );
        let _ = writeln!(out);
        return;
    }

    for thread in selected {
        thread_section(out, thread);
    }
}

fn overview(out: &mut String, digest: &Digest, shown: usize, filter: Filter) {
    let counts = &digest.counts;

    let _ = writeln!(out, "### Thread Status Overview");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Status | Count | Notes |");
    let _ = writeln!(out, "| :--- | :---: | :--- |");
    let _ = writeln!(
        out,
        "| 🔴 **Open (Active)** | **{}** | Actionable items on the latest commit |",
        counts.open_active
    );
    let _ = writeln!(
        out,
        "| 🟡 **Open (Outdated)** | **{}** | Open, but later commits moved the lines |",
        counts.open_outdated
    );
    let _ = writeln!(
        out,
        "| 🟢 **Resolved (Active)** | **{}** | Addressed and resolved |",
        counts.resolved_active
    );
    let _ = writeln!(
        out,
        "| ⚪ **Resolved (Outdated)** | **{}** | Historical resolved discussions |",
        counts.resolved_outdated
    );
    let _ = writeln!(
        out,
        "| **Total** | **{}** | Showing {shown} in this document |",
        counts.total()
    );
    let _ = writeln!(out);

    // Counts describe the whole pull request, so a filtered document has to say
    // so -- otherwise the table looks like it disagrees with the body.
    if !filter.is_permissive() {
        let _ = writeln!(
            out,
            "> Filtered view: `--status {}`, `--outdated {}`. Thread numbers refer to positions in the full, unfiltered list.",
            status_name(filter),
            outdated_name(filter)
        );
        let _ = writeln!(out);
    }
}

fn thread_section(out: &mut String, thread: &Thread) {
    let _ = writeln!(
        out,
        "### Thread #{}: `{}` - {}",
        thread.index,
        location(thread),
        badge(thread)
    );
    let _ = writeln!(out);

    if let Some(url) = &thread.url {
        let _ = writeln!(out, "[Link]({url})");
        let _ = writeln!(out);
    }

    if let Some(hunk) = &thread.diff_hunk {
        let fence = fence_for(hunk);
        let _ = writeln!(out, "{fence}diff");
        let _ = writeln!(out, "{hunk}");
        let _ = writeln!(out, "{fence}");
        let _ = writeln!(out);
    }

    for comment in &thread.comments {
        thread_comment(out, comment);
    }

    let _ = writeln!(out, "---");
    let _ = writeln!(out);
}

fn thread_comment(out: &mut String, comment: &Comment) {
    let _ = writeln!(out, "> **@{}** ({}):", comment.author, comment.created_at);
    let _ = writeln!(out, ">");
    let _ = writeln!(out, "{}", quote(&comment.body));
    let _ = writeln!(out);
}

fn location(thread: &Thread) -> String {
    match thread.location {
        Location::Line(line) => format!("{}:{line}", thread.path),
        Location::OriginalLine(line) => format!("{}:{line} (outdated diff)", thread.path),
        Location::File => format!("{} (file-level)", thread.path),
    }
}

fn badge(thread: &Thread) -> String {
    let resolver = thread
        .resolved_by
        .as_ref()
        .map(|who| format!(" by @{who}"))
        .unwrap_or_default();

    match thread.status {
        ThreadStatus::OpenActive => "🔴 `[OPEN · ACTIVE]`".to_string(),
        ThreadStatus::OpenOutdated => "🟡 `[OPEN · OUTDATED]`".to_string(),
        ThreadStatus::ResolvedActive => format!("🟢 `[RESOLVED{resolver}]`"),
        ThreadStatus::ResolvedOutdated => format!("⚪ `[RESOLVED{resolver} · OUTDATED]`"),
    }
}

fn status_name(filter: Filter) -> &'static str {
    use crate::cli::StatusFilter::*;
    match filter.status {
        Open => "open",
        Resolved => "resolved",
        All => "all",
    }
}

fn outdated_name(filter: Filter) -> &'static str {
    use crate::cli::OutdatedFilter::*;
    match filter.outdated {
        Include => "include",
        Hide => "hide",
        Only => "only",
    }
}

/// Blockquote a body we did not write.
///
/// Every embedded body -- descriptions, review summaries, comments -- goes
/// through here. Bot reviewers in particular open their summaries with `##`
/// headings, and raw interpolation lets those collide with the document's own
/// section structure. Quoting contains them without rewriting anyone's text.
fn quote(body: &str) -> String {
    body.trim()
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pick a fence long enough to survive whatever backticks the content holds.
///
/// A diff of a Markdown file contains ``` lines, which end a three-backtick
/// fence early and derail the rest of the document.
fn fence_for(content: &str) -> String {
    let longest = content
        .split(|c| c != '`')
        .map(|run| run.len())
        .max()
        .unwrap_or(0);

    "`".repeat(longest.max(2) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_content_gets_a_three_backtick_fence() {
        assert_eq!(fence_for("fn main() {}"), "```");
        assert_eq!(fence_for("a ` b `` c"), "```");
    }

    #[test]
    fn fences_outgrow_backticks_in_the_content() {
        assert_eq!(fence_for("+```rust\n+fn main() {}\n+```"), "````");
        assert_eq!(fence_for("+````\n+nested\n+````"), "`````");
    }

    #[test]
    fn quoting_keeps_blank_lines_as_bare_markers() {
        assert_eq!(quote("first\n\nsecond"), "> first\n>\n> second");
    }

    #[test]
    fn quoting_contains_headings_from_embedded_bodies() {
        assert_eq!(quote("## Walkthrough\ntext"), "> ## Walkthrough\n> text");
    }
}
