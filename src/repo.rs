//! Working out which repository we are talking about.
//!
//! The original asked `gh repo view` for this. Reading git's own remotes keeps
//! the `gh` dependency optional and works in a bare checkout with no `gh`
//! configuration at all.

use std::process::Command;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// Remote names in the order `gh` itself prefers them.
///
/// `upstream` first matters for fork workflows: the pull request lives on the
/// canonical repository, not on your fork. Repositories with a single remote
/// are unaffected by the ordering.
const REMOTE_PRIORITY: [&str; 3] = ["upstream", "github", "origin"];

/// Infer the repository from the git remotes of the current directory.
pub fn detect() -> Result<Repo> {
    let remotes = list_remotes()?;

    let mut ordered: Vec<&String> = Vec::new();
    for preferred in REMOTE_PRIORITY {
        if let Some(found) = remotes.iter().find(|r| *r == preferred) {
            ordered.push(found);
        }
    }
    ordered.extend(remotes.iter().filter(|r| !REMOTE_PRIORITY.contains(&r.as_str())));

    for remote in ordered {
        if let Some(url) = remote_url(remote)
            && let Some(repo) = parse_remote_url(&url)
        {
            return Ok(repo);
        }
    }

    Err(Error::Repo(
        "could not determine the repository from the current directory.\n\
         Pass -R <owner/repo>, or give a full pull request URL."
            .to_string(),
    ))
}

fn list_remotes() -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("remote")
        .output()
        .map_err(|e| Error::Repo(format!("could not run git: {e}")))?;

    if !output.status.success() {
        return Err(Error::Repo(
            "not inside a git repository. Pass -R <owner/repo>, or give a full \
             pull request URL."
                .to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn remote_url(remote: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// Extract host, owner and name from any remote form git accepts.
fn parse_remote_url(url: &str) -> Option<Repo> {
    let url = url.trim();

    // scp-like syntax has no scheme and separates host from path with a colon:
    // git@github.com:owner/repo.git
    let (authority, path) = if let Some((_, rest)) = url.split_once("://") {
        rest.split_once('/')?
    } else if let Some((left, right)) = url.split_once(':') {
        (left, right)
    } else {
        return None;
    };

    // Strip userinfo (`git@`) and any port.
    let host = authority
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .to_lowercase();
    if host.is_empty() {
        return None;
    }

    let path = path.trim_start_matches('/').trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // Take the last two so that an instance served under a path prefix still
    // yields the owner and repository rather than the prefix.
    let [.., owner, name] = segments.as_slice() else {
        return None;
    };

    Some(Repo {
        host,
        owner: (*owner).to_string(),
        name: (*name).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(host: &str, owner: &str, name: &str) -> Option<Repo> {
        Some(Repo {
            host: host.into(),
            owner: owner.into(),
            name: name.into(),
        })
    }

    #[test]
    fn parses_every_remote_form_git_accepts() {
        let expected = repo("github.com", "owner", "repo");
        for url in [
            "git@github.com:owner/repo.git",
            "git@github.com:owner/repo",
            "ssh://git@github.com/owner/repo.git",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo",
            "https://user@github.com/owner/repo.git",
            "git://github.com/owner/repo.git",
        ] {
            assert_eq!(parse_remote_url(url), expected, "failed on {url}");
        }
    }

    #[test]
    fn keeps_the_enterprise_host_and_drops_the_port() {
        assert_eq!(
            parse_remote_url("ssh://git@ghe.corp:2222/owner/repo.git"),
            repo("ghe.corp", "owner", "repo")
        );
    }

    #[test]
    fn normalises_host_case() {
        assert_eq!(
            parse_remote_url("git@GitHub.com:owner/repo.git"),
            repo("github.com", "owner", "repo")
        );
    }

    #[test]
    fn rejects_paths_without_an_owner() {
        assert_eq!(parse_remote_url("https://github.com/repo"), None);
        assert_eq!(parse_remote_url("not-a-url"), None);
    }
}
