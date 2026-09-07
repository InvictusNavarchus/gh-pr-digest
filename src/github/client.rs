//! A minimal GraphQL client over the GitHub API.
//!
//! Blocking on purpose: pagination is inherently sequential and this is a
//! one-shot CLI, so an async runtime would buy nothing and cost binary size.

use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use ureq::Agent;

use crate::error::{Error, Result};

const USER_AGENT: &str = concat!("gh-pr-digest/", env!("CARGO_PKG_VERSION"));

/// GitHub's GraphQL endpoint is occasionally flaky and enforces a secondary
/// rate limit; a paginating client that gives up on the first 502 is useless.
const MAX_ATTEMPTS: u32 = 3;

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

pub struct Client {
    agent: Agent,
    endpoint: String,
    token: String,
}

impl Client {
    pub fn new(endpoint: String, token: String) -> Self {
        // Non-2xx responses carry a JSON body we want to read (GitHub explains
        // itself there), so status codes are handled as data, not as errors.
        let config = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build();

        Client {
            agent: config.into(),
            endpoint,
            token,
        }
    }

    /// Execute one GraphQL operation and deserialize its `data` field.
    pub fn query<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        let payload = serde_json::json!({ "query": query, "variables": variables });
        let mut attempt = 1;

        loop {
            let (status, body) = self.send(&payload)?;

            if let Some(delay) = retry_delay(status, attempt) {
                attempt += 1;
                thread::sleep(delay);
                continue;
            }

            return self.interpret(status, &body);
        }
    }

    fn send(&self, payload: &Value) -> Result<(u16, String)> {
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .send_json(payload)
            .map_err(|e| Error::Network(format!("request to {} failed: {e}", self.endpoint)))?;

        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Network(format!("could not read response body: {e}")))?;

        Ok((status, body))
    }

    fn interpret<T: DeserializeOwned>(&self, status: u16, body: &str) -> Result<T> {
        match status {
            401 => {
                return Err(Error::Auth(
                    "GitHub rejected the token (401). It may be expired or revoked; \
                     try `gh auth login` or supply a fresh --token."
                        .to_string(),
                ));
            }
            403 => {
                return Err(Error::Auth(format!(
                    "GitHub refused the request (403). The token may lack `repo` scope, \
                     or SSO authorization for this organization.\n{}",
                    api_message(body).unwrap_or_default()
                )));
            }
            429 => {
                return Err(Error::Network(
                    "rate limited by GitHub (429); try again later".to_string(),
                ));
            }
            404 => {
                return Err(Error::NotFound(format!(
                    "no GraphQL endpoint at {} (404). Check --hostname.",
                    self.endpoint
                )));
            }
            s if s >= 500 => {
                return Err(Error::Network(format!(
                    "GitHub returned {s} after {MAX_ATTEMPTS} attempts"
                )));
            }
            _ => {}
        }

        let envelope: Envelope<T> = serde_json::from_str(body)
            .map_err(|e| Error::Api(format!("could not parse the GraphQL response: {e}")))?;

        if !envelope.errors.is_empty() {
            // A NOT_FOUND is the overwhelmingly common case (wrong repo, wrong
            // PR number, or a private repo the token cannot see) and deserves
            // its own exit code rather than being lumped in with API errors.
            let not_found = envelope
                .errors
                .iter()
                .any(|e| e.kind.as_deref() == Some("NOT_FOUND"));

            let joined = envelope
                .errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");

            return Err(if not_found {
                Error::NotFound(joined)
            } else {
                Error::Api(format!("GraphQL error: {joined}"))
            });
        }

        envelope
            .data
            .ok_or_else(|| Error::Api("GraphQL response carried no data".to_string()))
    }
}

/// How long to wait before retrying, or `None` if this response is final.
fn retry_delay(status: u16, attempt: u32) -> Option<Duration> {
    if attempt >= MAX_ATTEMPTS {
        return None;
    }
    if status >= 500 || status == 429 {
        // Plain exponential backoff: 1s, then 2s. Long enough to clear a
        // transient blip, short enough that a human does not reach for Ctrl-C.
        Some(Duration::from_secs(1 << (attempt - 1)))
    } else {
        None
    }
}

/// GitHub's REST-style error bodies carry a human-readable `message`.
fn api_message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_server_errors_until_the_attempt_budget_runs_out() {
        assert!(retry_delay(502, 1).is_some());
        assert!(retry_delay(502, 2).is_some());
        assert!(retry_delay(502, MAX_ATTEMPTS).is_none());
    }

    #[test]
    fn does_not_retry_client_errors() {
        assert!(retry_delay(401, 1).is_none());
        assert!(retry_delay(404, 1).is_none());
        assert!(retry_delay(200, 1).is_none());
    }
}
