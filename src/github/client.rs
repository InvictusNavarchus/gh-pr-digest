//! A minimal GraphQL client over the GitHub API.
//!
//! Blocking on purpose: pagination is inherently sequential and this is a
//! one-shot CLI, so an async runtime would buy nothing and cost binary size.

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use ureq::Agent;

use crate::error::{Error, Result};

const USER_AGENT: &str = concat!("gh-pr-digest/", env!("CARGO_PKG_VERSION"));

/// Retry transient server failures; rate limits need a longer, explicit wait.
const MAX_ATTEMPTS: u32 = 3;

#[derive(Default)]
struct RateLimitHeaders {
    retry_after: Option<u64>,
    remaining: Option<u64>,
    reset: Option<u64>,
}

struct ApiResponse {
    status: u16,
    body: String,
    rate_limit: RateLimitHeaders,
}

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

#[derive(Default, Deserialize)]
struct ErrorDetails {
    #[serde(default)]
    errors: Vec<GraphQlError>,
    message: Option<String>,
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
            let response = self.send(&payload)?;

            if let Some(delay) = retry_delay(response.status, attempt) {
                attempt += 1;
                thread::sleep(delay);
                continue;
            }

            return self.interpret(&response);
        }
    }

    fn send(&self, payload: &Value) -> Result<ApiResponse> {
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .send_json(payload)
            .map_err(|e| Error::Network(format!("request to {} failed: {e}", self.endpoint)))?;

        let status = response.status().as_u16();
        let header_number = |name: &str| response.headers().get(name)?.to_str().ok()?.parse().ok();
        let rate_limit = RateLimitHeaders {
            retry_after: header_number("retry-after"),
            remaining: header_number("x-ratelimit-remaining"),
            reset: header_number("x-ratelimit-reset"),
        };
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Network(format!("could not read response body: {e}")))?;

        Ok(ApiResponse {
            status,
            body,
            rate_limit,
        })
    }

    fn interpret<T: DeserializeOwned>(&self, response: &ApiResponse) -> Result<T> {
        if let Some(error) = rate_limit_error(response) {
            return Err(error);
        }

        let status = response.status;
        let body = &response.body;
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
    if status >= 500 {
        // Plain exponential backoff: 1s, then 2s. Long enough to clear a
        // transient blip, short enough that a human does not reach for Ctrl-C.
        Some(Duration::from_secs(1 << (attempt - 1)))
    } else {
        None
    }
}

/// A zero remaining quota on a successful response is not itself an error:
/// that request may have consumed the last available point.
fn rate_limit_error(response: &ApiResponse) -> Option<Error> {
    if !matches!(response.status, 200 | 403 | 429) {
        return None;
    }

    // Ignore the potentially large `data` field rather than allocating a
    // second copy of all PR content just to inspect the errors.
    let details = serde_json::from_str::<ErrorDetails>(&response.body).unwrap_or_default();
    let rate_limited = response.status == 429
        || ((response.status == 403 || !details.errors.is_empty())
            && (response.rate_limit.remaining == Some(0)
                || response.rate_limit.retry_after.is_some()))
        || details.errors.iter().any(|error| {
            error.kind.as_deref() == Some("RATE_LIMITED") || is_rate_limit_message(&error.message)
        })
        || (response.status == 403
            && details
                .message
                .as_deref()
                .is_some_and(is_rate_limit_message));

    if !rate_limited {
        return None;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let wait = rate_limit_wait(&response.rate_limit, now);
    Some(Error::Network(format!(
        "rate limited by GitHub (HTTP {}); wait at least {wait} seconds before trying again",
        response.status
    )))
}

fn is_rate_limit_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("rate limit") || message.contains("abuse detection")
}

fn rate_limit_wait(headers: &RateLimitHeaders, now: u64) -> u64 {
    let reset_wait = if headers.remaining == Some(0) {
        // GitHub requires waiting until after the reset, not just up to it.
        headers
            .reset
            .map(|reset| reset.saturating_sub(now).saturating_add(1))
    } else {
        None
    };
    match (headers.retry_after, reset_wait) {
        (Some(retry), Some(reset)) => retry.max(reset).max(1),
        (Some(wait), None) | (None, Some(wait)) => wait.max(1),
        (None, None) => 60,
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    fn interpret(status: u16, body: &str, rate_limit: RateLimitHeaders) -> Result<Value> {
        Client::new("https://api.github.com/graphql".into(), "test".into()).interpret(
            &ApiResponse {
                status,
                body: body.into(),
                rate_limit,
            },
        )
    }

    #[test]
    fn recognizes_rate_limits_in_graphql_and_http_errors() {
        for (status, body, headers) in [
            (
                200,
                r#"{"errors":[{"type":"RATE_LIMITED","message":"Quota exceeded"}]}"#,
                RateLimitHeaders::default(),
            ),
            (
                200,
                r#"{"errors":[{"message":"You have exceeded a secondary rate limit."}]}"#,
                RateLimitHeaders::default(),
            ),
            (
                200,
                r#"{"errors":[{"message":"Quota exceeded"}]}"#,
                RateLimitHeaders {
                    remaining: Some(0),
                    ..Default::default()
                },
            ),
            (
                403,
                r#"{"message":"API rate limit exceeded"}"#,
                RateLimitHeaders::default(),
            ),
            (
                403,
                r#"{"message":"You have triggered an abuse detection mechanism."}"#,
                RateLimitHeaders::default(),
            ),
            (
                403,
                "Forbidden",
                RateLimitHeaders {
                    remaining: Some(0),
                    ..Default::default()
                },
            ),
            (
                403,
                "Forbidden",
                RateLimitHeaders {
                    retry_after: Some(90),
                    ..Default::default()
                },
            ),
            (429, "Too Many Requests", RateLimitHeaders::default()),
        ] {
            let error = interpret(status, body, headers).unwrap_err();
            assert!(matches!(error, Error::Network(_)), "{status}: {error}");
            assert_eq!(error.exit_code(), 6);
            assert!(error.to_string().contains("seconds before trying again"));
        }
    }

    #[test]
    fn successful_response_can_consume_the_last_quota_point() {
        let data = serde_json::json!({"body": "discuss the rate limit"});
        let body = serde_json::json!({"data": data, "errors": []}).to_string();
        assert_eq!(
            interpret(
                200,
                &body,
                RateLimitHeaders {
                    remaining: Some(0),
                    reset: Some(u64::MAX),
                    ..Default::default()
                }
            )
            .unwrap(),
            data
        );
    }

    #[test]
    fn keeps_authentication_not_found_and_other_api_errors_distinct() {
        for status in [401, 403] {
            let error = interpret(
                status,
                r#"{"message":"Resource not accessible by token"}"#,
                RateLimitHeaders::default(),
            )
            .unwrap_err();
            assert!(matches!(error, Error::Auth(_)));
            assert_eq!(error.exit_code(), 4);
        }
        let error = interpret(
            200,
            r#"{"errors":[{"type":"NOT_FOUND","message":"Not found"}]}"#,
            RateLimitHeaders::default(),
        )
        .unwrap_err();
        assert!(matches!(error, Error::NotFound(_)));
        let error = interpret(
            200,
            r#"{"errors":[{"message":"Unknown field"}]}"#,
            RateLimitHeaders::default(),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Api(_)));
    }

    #[test]
    fn waits_for_applicable_headers_or_falls_back_to_one_minute() {
        let now = 1_000;
        for (headers, expected) in [
            (RateLimitHeaders::default(), 60),
            (
                RateLimitHeaders {
                    retry_after: Some(90),
                    ..Default::default()
                },
                90,
            ),
            (
                RateLimitHeaders {
                    remaining: Some(0),
                    reset: Some(1_120),
                    ..Default::default()
                },
                121,
            ),
            (
                RateLimitHeaders {
                    retry_after: Some(90),
                    remaining: Some(0),
                    reset: Some(1_120),
                },
                121,
            ),
            (
                RateLimitHeaders {
                    retry_after: Some(180),
                    remaining: Some(0),
                    reset: Some(1_120),
                },
                180,
            ),
            (
                RateLimitHeaders {
                    remaining: Some(10),
                    reset: Some(1_120),
                    ..Default::default()
                },
                60,
            ),
            (
                RateLimitHeaders {
                    remaining: Some(0),
                    ..Default::default()
                },
                60,
            ),
            (
                RateLimitHeaders {
                    remaining: Some(0),
                    reset: Some(999),
                    ..Default::default()
                },
                1,
            ),
            (
                RateLimitHeaders {
                    retry_after: Some(0),
                    ..Default::default()
                },
                1,
            ),
        ] {
            assert_eq!(rate_limit_wait(&headers, now), expected);
        }
    }

    #[test]
    fn query_reports_rate_limits_without_retrying_and_reads_headers() {
        for (status, rate_headers, expected_wait) in [
            (200, "Retry-After: 123\r\n", 123),
            (403, "Retry-After: 123\r\n", 123),
            (429, "Retry-After: 123\r\n", 123),
            (
                200,
                "X-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 1\r\n",
                1,
            ),
            (
                429,
                "Retry-After: invalid\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: invalid\r\n",
                60,
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/graphql", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                // Drain the request before closing, so unread bytes do not reset
                // the connection while the client is reading the response.
                loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let headers = String::from_utf8(request).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                stream.read_exact(&mut vec![0; length]).unwrap();
                let body = r#"{"errors":[{"type":"RATE_LIMITED","message":"Quota exceeded"}]}"#;
                write!(stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n{rate_headers}Connection: close\r\n\r\n{body}",
                    body.len()).unwrap();
                // The listener is dropped: a retry would fail as a connection
                // error instead of returning the expected rate-limit message.
            });
            let error = Client::new(endpoint, "test".into())
                .query::<Value>("query { viewer { login } }", serde_json::json!({}))
                .unwrap_err();
            server.join().unwrap();
            assert_eq!(error.exit_code(), 6);
            assert!(
                error
                    .to_string()
                    .contains(&format!("wait at least {expected_wait} seconds")),
                "{error}"
            );
        }
    }

    #[test]
    fn retries_server_errors_until_the_attempt_budget_runs_out() {
        assert!(retry_delay(502, 1).is_some());
        assert!(retry_delay(502, 2).is_some());
        assert!(retry_delay(502, MAX_ATTEMPTS).is_none());
    }

    #[test]
    fn does_not_retry_client_errors() {
        assert!(retry_delay(401, 1).is_none());
        assert!(retry_delay(403, 1).is_none());
        assert!(retry_delay(404, 1).is_none());
        assert!(retry_delay(429, 1).is_none());
        assert!(retry_delay(200, 1).is_none());
    }
}
