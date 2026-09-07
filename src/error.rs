//! Fatal error taxonomy.
//!
//! Every variant maps to a distinct process exit code so that callers can
//! branch on *why* we failed without scraping stderr. Exit code 2 is
//! deliberately absent: clap owns it for usage errors.

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The target repository could not be determined, or was malformed.
    #[error("{0}")]
    Repo(String),

    /// No credential was found, or GitHub rejected the one we had.
    #[error("{0}")]
    Auth(String),

    /// The repository or pull request does not exist, or is not visible to us.
    #[error("{0}")]
    NotFound(String),

    /// Transport-level failure, or GitHub asked us to back off.
    #[error("{0}")]
    Network(String),

    /// GitHub accepted the request but the response carried errors.
    #[error("{0}")]
    Api(String),

    #[error("{0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Repo(_) => 3,
            Error::Auth(_) => 4,
            Error::NotFound(_) => 5,
            Error::Network(_) => 6,
            Error::Api(_) | Error::Io(_) => 1,
        }
    }
}
