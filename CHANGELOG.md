# Changelog

Notable changes to gh-pr-digest, newest first. Each version's section is
published verbatim as its GitHub release notes, so write entries for someone
upgrading, not for someone reading the diff — lead with anything that changes
behaviour.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
How to write entries and cut a release: [docs/releasing.md](docs/releasing.md).

## [Unreleased]

### Fixed

- **Behaviour change:** Missing, empty, or repeated pagination cursors now stop
  the download with exit code 1 instead of silently producing an incomplete
  digest or repeatedly fetching the same pages.
- **Behaviour change:** GitHub rate-limit errors now exit with code 6 and retry
  guidance, including limits returned as HTTP 200 or 403. Rate-limited requests
  are no longer retried automatically after one or two seconds.
- Standalone installs and updates now stop when release checksums are missing,
  invalid, or cannot be verified, preserving any previously installed binary.

## [0.1.0] - 2026-09-07

Initial release.

[Unreleased]: https://github.com/InvictusNavarchus/gh-pr-digest/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/InvictusNavarchus/gh-pr-digest/releases/tag/v0.1.0
