//! Host and credential resolution.
//!
//! The tool has two deployment shapes -- a standalone binary and a `gh`
//! extension -- and one code path serves both. `gh` injects `GH_TOKEN` and
//! `GH_HOST` into extension processes, so the environment lookup covers the
//! extension case; the `gh auth token` fallback covers a standalone binary on a
//! developer machine; and `--token` covers CI images with neither.

use std::env;
use std::process::Command;

use crate::error::{Error, Result};

pub const DEFAULT_HOST: &str = "github.com";

/// Decide which GitHub instance to talk to.
///
/// `explicit` is a host the user named -- in a URL target, in `-R`, or via
/// `--hostname` -- and always wins. `inferred` is a host we worked out for
/// ourselves from a git remote, which beats the environment but loses to
/// anything the user said out loud.
pub fn resolve_host(explicit: Option<&str>, inferred: Option<&str>) -> String {
    explicit
        .map(str::to_string)
        .or_else(|| inferred.map(str::to_string))
        .or_else(|| env::var("GH_HOST").ok().filter(|h| !h.is_empty()))
        .unwrap_or_else(|| DEFAULT_HOST.to_string())
}

/// github.com serves its API from a separate domain; every enterprise
/// installation serves it from a path on the instance itself.
pub fn graphql_endpoint(host: &str) -> String {
    if is_dotcom(host) {
        "https://api.github.com/graphql".to_string()
    } else {
        format!("https://{host}/api/graphql")
    }
}

fn is_dotcom(host: &str) -> bool {
    matches!(host, DEFAULT_HOST | "api.github.com" | "www.github.com")
}

/// Find a credential for `host`, in descending order of explicitness.
pub fn resolve_token(explicit: Option<&str>, host: &str) -> Result<String> {
    if let Some(token) = explicit.map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(token.to_string());
    }

    // `gh` reads the plain names for github.com and the ENTERPRISE names for
    // anything else, so that a dotcom token is never accidentally sent to a
    // third-party host. Mirror that.
    let vars: &[&str] = if is_dotcom(host) {
        &["GH_TOKEN", "GITHUB_TOKEN"]
    } else {
        &["GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"]
    };

    for var in vars {
        if let Ok(value) = env::var(var) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_string());
            }
        }
    }

    if let Some(token) = token_from_gh_cli(host) {
        return Ok(token);
    }

    Err(Error::Auth(format!(
        "no GitHub token found for {host}.\n\
         Looked at: --token, ${}, and `gh auth token --hostname {host}`.\n\
         Run `gh auth login` or set ${}.",
        vars.join(", $"),
        vars[0]
    )))
}

/// Ask `gh` for its stored token. Absent or unauthenticated `gh` is not an
/// error here -- it just means this link in the chain has nothing to offer.
fn token_from_gh_cli(host: &str) -> Option<String> {
    let output = Command::new("gh")
        .args(["auth", "token", "--hostname", host])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_host_outranks_an_inferred_one() {
        assert_eq!(resolve_host(Some("ghe.corp"), Some("other")), "ghe.corp");
        assert_eq!(resolve_host(None, Some("ghe.corp")), "ghe.corp");
    }

    #[test]
    fn dotcom_uses_the_api_subdomain_and_enterprise_uses_a_path() {
        assert_eq!(
            graphql_endpoint("github.com"),
            "https://api.github.com/graphql"
        );
        assert_eq!(graphql_endpoint("ghe.corp"), "https://ghe.corp/api/graphql");
    }

    #[test]
    fn explicit_token_wins() {
        assert_eq!(resolve_token(Some("abc"), DEFAULT_HOST).unwrap(), "abc");
    }
}
