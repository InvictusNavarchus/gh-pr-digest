//! Command-line surface: parsing and validation.
//!
//! Everything here is pure -- no network, no filesystem -- so the entire
//! argument surface is exercisable in unit tests.

use std::path::PathBuf;
use std::str::FromStr;

use clap::{Parser, ValueEnum};

/// Which review threads to include, by resolution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    /// Threads nobody has marked resolved.
    Open,
    /// Threads someone has marked resolved.
    Resolved,
    /// Both.
    All,
}

/// Which review threads to include, by whether later commits moved the lines.
///
/// Orthogonal to [`StatusFilter`] on purpose: a thread's resolution state and
/// its outdatedness are independent facts, and every combination of the two is
/// a meaningful query. Collapsing them into one flat list of boolean flags is
/// what lets contradictory requests like `--hide-outdated --outdated-only`
/// exist in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutdatedFilter {
    /// Outdated and current threads alike.
    Include,
    /// Drop threads GitHub marks outdated.
    Hide,
    /// Keep only threads GitHub marks outdated.
    Only,
}

/// Where the rendered digest goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    Stdout,
    File(PathBuf),
}

impl FromStr for OutputTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "-" {
            Ok(OutputTarget::Stdout)
        } else if s.is_empty() {
            Err("output path is empty".to_string())
        } else {
            Ok(OutputTarget::File(PathBuf::from(s)))
        }
    }
}

/// A repository named on the command line, optionally qualified by host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    pub host: Option<String>,
    pub owner: String,
    pub name: String,
}

impl FromStr for RepoSpec {
    type Err = String;

    /// Accepts `owner/repo` and `host/owner/repo`, matching `gh -R`.
    ///
    /// The original script split on `/` and took the first two fields
    /// unconditionally, so `-R owner` silently paired the given owner with the
    /// *current directory's* repository name. Both halves are required here.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().trim_end_matches('/');
        let parts: Vec<&str> = trimmed.split('/').collect();

        let (host, owner, name) = match parts.as_slice() {
            [owner, name] => (None, *owner, *name),
            [host, owner, name] => (Some((*host).to_string()), *owner, *name),
            _ => {
                return Err(format!("expected OWNER/REPO or HOST/OWNER/REPO, got {s:?}"));
            }
        };

        if owner.is_empty() || name.is_empty() {
            return Err(format!(
                "owner and repository must both be non-empty, got {s:?}"
            ));
        }

        Ok(RepoSpec {
            host,
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }
}

/// The pull request the user asked for: either a bare number, or a URL that
/// carries its own host and repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrTarget {
    Number(u64),
    Url {
        host: String,
        owner: String,
        repo: String,
        number: u64,
    },
}

impl PrTarget {
    pub fn number(&self) -> u64 {
        match self {
            PrTarget::Number(n) => *n,
            PrTarget::Url { number, .. } => *number,
        }
    }
}

impl FromStr for PrTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s.contains('/') {
            return parse_url(s);
        }

        // A leading `#` is how people write PR references in prose.
        let digits = s.strip_prefix('#').unwrap_or(s);

        // Full-string parse, not a prefix parse: the original used
        // `parseInt`, which happily read "19abc" as PR 19.
        let number: u64 = digits
            .parse()
            .map_err(|_| format!("expected a PR number or URL, got {s:?}"))?;

        if number == 0 {
            return Err("pull request numbers start at 1".to_string());
        }

        Ok(PrTarget::Number(number))
    }
}

/// Pull `host`, `owner`, `repo` and the number out of a PR URL.
///
/// Locates the `pull` segment rather than matching a fixed github.com shape, so
/// enterprise hosts and trailing segments (`/files`, `#discussion_r123`) work.
fn parse_url(s: &str) -> Result<PrTarget, String> {
    let without_scheme = s.split_once("://").map(|(_, rest)| rest).unwrap_or(s);
    let path = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);

    let segments: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();

    let pull_at = segments
        .iter()
        .position(|seg| *seg == "pull" || *seg == "pulls")
        .ok_or_else(|| format!("no /pull/<number> segment in {s:?}"))?;

    if pull_at < 3 || pull_at + 1 >= segments.len() {
        return Err(format!("expected HOST/OWNER/REPO/pull/NUMBER, got {s:?}"));
    }

    let number: u64 = segments[pull_at + 1]
        .parse()
        .map_err(|_| format!("{:?} is not a pull request number", segments[pull_at + 1]))?;

    if number == 0 {
        return Err("pull request numbers start at 1".to_string());
    }

    Ok(PrTarget::Url {
        host: segments[pull_at - 3].to_string(),
        owner: segments[pull_at - 2].to_string(),
        repo: segments[pull_at - 1].to_string(),
        number,
    })
}

#[derive(Debug, Parser)]
#[command(
    name = "gh-pr-digest",
    version,
    about = "Download GitHub PR reviews, inline threads, and comments to Markdown",
    after_help = "\
EXAMPLES:
  gh-pr-digest 19
  gh-pr-digest 19 --active-only
  gh-pr-digest 19 --status open -o pending.md
  gh-pr-digest https://github.com/owner/repo/pull/19
  gh-pr-digest 19 -o - | glow"
)]
pub struct Cli {
    /// PR number (e.g. 19) or full URL
    pub target: PrTarget,

    /// Where to write the digest; '-' for stdout
    ///
    /// Defaults to ./pr-<number>-digest.md
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<OutputTarget>,

    /// Target repository; defaults to the current git repository
    #[arg(short = 'R', long = "repo", value_name = "OWNER/REPO")]
    pub repo: Option<RepoSpec>,

    /// GitHub host for enterprise instances
    #[arg(long, value_name = "HOST")]
    pub hostname: Option<String>,

    /// Include threads by resolution state
    #[arg(long, value_enum, default_value_t = StatusFilter::All)]
    pub status: StatusFilter,

    /// Include threads by whether later commits moved their lines
    #[arg(long, value_enum, default_value_t = OutdatedFilter::Include)]
    pub outdated: OutdatedFilter,

    /// Shorthand for --status open --outdated hide
    #[arg(long, visible_alias = "pending", conflicts_with_all = ["status", "outdated"])]
    pub active_only: bool,

    /// GitHub token; overrides GH_TOKEN and GITHUB_TOKEN
    #[arg(long, value_name = "TOKEN")]
    pub token: Option<String>,
}

impl Cli {
    /// The effective filters, after expanding the `--active-only` shorthand.
    pub fn filters(&self) -> (StatusFilter, OutdatedFilter) {
        if self.active_only {
            (StatusFilter::Open, OutdatedFilter::Hide)
        } else {
            (self.status, self.outdated)
        }
    }

    /// Reject argument combinations clap cannot express.
    pub fn validate(&self) -> Result<(), String> {
        if matches!(self.target, PrTarget::Url { .. }) {
            if self.repo.is_some() {
                return Err(
                    "--repo cannot be combined with a full PR URL; the URL already names the repository"
                        .to_string(),
                );
            }
            if self.hostname.is_some() {
                return Err(
                    "--hostname cannot be combined with a full PR URL; the URL already names the host"
                        .to_string(),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(s: &str) -> Result<PrTarget, String> {
        s.parse()
    }

    #[test]
    fn parses_bare_numbers() {
        assert_eq!(pr("19").unwrap(), PrTarget::Number(19));
        assert_eq!(pr("#19").unwrap(), PrTarget::Number(19));
    }

    #[test]
    fn rejects_trailing_garbage_after_a_number() {
        // `parseInt("19abc")` returned 19; a full-string parse must not.
        assert!(pr("19abc").is_err());
        assert!(pr("").is_err());
        assert!(pr("0").is_err());
    }

    #[test]
    fn parses_pr_urls() {
        assert_eq!(
            pr("https://github.com/owner/repo/pull/19").unwrap(),
            PrTarget::Url {
                host: "github.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
                number: 19,
            }
        );
    }

    #[test]
    fn parses_urls_with_trailing_segments_and_fragments() {
        let expected = PrTarget::Url {
            host: "github.com".into(),
            owner: "owner".into(),
            repo: "repo".into(),
            number: 19,
        };
        assert_eq!(
            pr("https://github.com/owner/repo/pull/19/files").unwrap(),
            expected
        );
        assert_eq!(
            pr("https://github.com/owner/repo/pull/19#discussion_r123").unwrap(),
            expected
        );
        assert_eq!(pr("github.com/owner/repo/pull/19").unwrap(), expected);
    }

    #[test]
    fn parses_enterprise_urls() {
        assert_eq!(
            pr("https://ghe.corp.example/owner/repo/pull/7").unwrap(),
            PrTarget::Url {
                host: "ghe.corp.example".into(),
                owner: "owner".into(),
                repo: "repo".into(),
                number: 7,
            }
        );
    }

    #[test]
    fn rejects_urls_without_a_pull_segment() {
        assert!(pr("https://github.com/owner/repo/issues/19").is_err());
        assert!(pr("https://github.com/owner/repo").is_err());
    }

    #[test]
    fn repo_spec_requires_both_halves() {
        assert!("owner".parse::<RepoSpec>().is_err());
        assert!("owner/".parse::<RepoSpec>().is_err());
        assert!("/repo".parse::<RepoSpec>().is_err());
        assert_eq!(
            "owner/repo".parse::<RepoSpec>().unwrap(),
            RepoSpec {
                host: None,
                owner: "owner".into(),
                name: "repo".into()
            }
        );
        assert_eq!(
            "ghe.corp/owner/repo".parse::<RepoSpec>().unwrap(),
            RepoSpec {
                host: Some("ghe.corp".into()),
                owner: "owner".into(),
                name: "repo".into()
            }
        );
    }

    #[test]
    fn output_target_treats_dash_as_stdout() {
        assert_eq!("-".parse::<OutputTarget>().unwrap(), OutputTarget::Stdout);
        assert_eq!(
            "out.md".parse::<OutputTarget>().unwrap(),
            OutputTarget::File(PathBuf::from("out.md"))
        );
    }
}
