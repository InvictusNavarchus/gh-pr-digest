//! Where the digest and the diagnostics go.
//!
//! Diagnostics go to stderr, unconditionally. The original printed its
//! progress line to stdout and worked around the consequences by suppressing
//! it whenever the output target was stdout -- so `-o - | glow` was correct by
//! luck rather than by construction.

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;

use crate::cli::OutputTarget;
use crate::error::{Error, Result};

/// Where the digest lands when the user names no target.
pub fn default_path(number: u64) -> PathBuf {
    PathBuf::from(format!("pr-{number}-digest.md"))
}

/// Write the rendered digest, returning the path if it went to a file.
pub fn write(target: &OutputTarget, content: &str) -> Result<Option<PathBuf>> {
    match target {
        OutputTarget::Stdout => {
            io::stdout().write_all(content.as_bytes())?;
            io::stdout().flush()?;
            Ok(None)
        }
        OutputTarget::File(path) => {
            write_atomically(path, content)?;
            Ok(Some(path.clone()))
        }
    }
}

/// Write via a temporary file in the same directory, then rename.
///
/// A digest is something people re-generate over an existing file while
/// reading it. A partial write on a full disk or an interrupted run should
/// leave the previous digest intact rather than a truncated one.
fn write_atomically(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = parent {
        fs::create_dir_all(dir)?;
    }

    let temporary = with_suffix(path, &format!(".tmp-{}", process::id()));

    // Rename is only atomic within a filesystem, which is why the temporary
    // sits beside the destination rather than in the system temp directory.
    fs::write(&temporary, content).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("could not write {}: {e}", temporary.display()),
        ))
    })?;

    fs::rename(&temporary, path).map_err(|e| {
        let _ = fs::remove_file(&temporary);
        Error::Io(io::Error::new(
            e.kind(),
            format!("could not replace {}: {e}", path.display()),
        ))
    })?;

    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Colour is applied only when stderr is a terminal, so redirected logs stay
/// free of escape sequences.
fn paint(code: &str, text: &str) -> String {
    if io::stderr().is_terminal() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn info(message: &str) {
    eprintln!("{} {message}", paint("34", "info:"));
}

pub fn success(message: &str) {
    eprintln!("{} {message}", paint("32", "done:"));
}

pub fn error(message: &str) {
    eprintln!("{} {message}", paint("31", "error:"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_file_sits_beside_the_destination() {
        let path = Path::new("out/pr-9-digest.md");
        let temporary = with_suffix(path, ".tmp-1");
        assert_eq!(temporary.parent(), path.parent());
        assert_eq!(temporary.file_name().unwrap(), "pr-9-digest.md.tmp-1");
    }

    #[test]
    fn default_path_is_named_after_the_pull_request() {
        assert_eq!(default_path(19), PathBuf::from("pr-19-digest.md"));
    }
}
