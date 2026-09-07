# gh-pr-digest

Download a GitHub pull request's reviews, inline review threads, and conversation
comments into a single Markdown document.

Useful when a review has more feedback than fits comfortably in a browser tab —
a long bot review, a PR that has been round three times, or anything you want to
read offline, grep, or hand to an editor.

## Install

**With the GitHub CLI** — the shortest path if you already have `gh`:

```bash
gh extension install InvictusNavarchus/gh-pr-digest
gh pr-digest 19
```

**Without it** — a static binary, needing neither `gh` nor a Rust toolchain:

```bash
curl -fsSL https://raw.githubusercontent.com/InvictusNavarchus/gh-pr-digest/master/install.sh | sh
```

The script installs to `~/.local/bin` and verifies the download against the
release checksums; set `INSTALL_DIR` or `VERSION` to override either. If you
would rather read it before running it, it lives at
[`install.sh`](install.sh) in this repository.

Windows is not covered by the script — use the `gh` extension, or take
`windows-amd64.exe` from the
[latest release](https://github.com/InvictusNavarchus/gh-pr-digest/releases/latest).

**From source**, if you have Rust:

```bash
cargo install --git https://github.com/InvictusNavarchus/gh-pr-digest
```

All three install the same binary. Linux builds are statically linked against
musl, so there is no glibc version to match, and every release carries a
`checksums.txt`.

### Updating

To update to the latest release, use the command for your install method:

- **With the GitHub CLI**:
  ```bash
  gh extension upgrade gh-pr-digest
  ```
- **Without it**: re-run the install script. It resolves the latest release tag,
  verifies the checksum, and atomically replaces the binary:
  ```bash
  curl -fsSL https://raw.githubusercontent.com/InvictusNavarchus/gh-pr-digest/master/install.sh | sh
  ```
- **From source**:
  ```bash
  cargo install --force --git https://github.com/InvictusNavarchus/gh-pr-digest
  ```

## Usage

```
gh-pr-digest <PR-NUMBER-OR-URL> [OPTIONS]
```

```bash
# Everything, into ./pr-19-digest.md
gh-pr-digest 19

# Just what still needs a reply
gh-pr-digest 19 --active-only

# A repository other than the current directory's
gh-pr-digest 19 -R owner/repo

# A pull request anywhere, by URL
gh-pr-digest https://github.com/owner/repo/pull/19

# Straight into a pager
gh-pr-digest 19 -o - | glow
```

### Options

| Option | Description |
| :--- | :--- |
| `-o, --output <FILE>` | Where to write; `-` for stdout. Default `./pr-<number>-digest.md` |
| `-R, --repo <OWNER/REPO>` | Target repository. Also accepts `HOST/OWNER/REPO` |
| `--hostname <HOST>` | GitHub host, for enterprise instances |
| `--status <STATUS>` | `open`, `resolved`, or `all` (default) |
| `--outdated <OUTDATED>` | `include` (default), `hide`, or `only` |
| `--active-only` | Shorthand for `--status open --outdated hide`. Alias: `--pending` |
| `--token <TOKEN>` | GitHub token, overriding the environment |

### Filtering

A review thread has two independent properties: whether anyone has **resolved**
it, and whether later commits have made it **outdated**. `--status` and
`--outdated` select along those two axes, so every combination is reachable and
means exactly what it says:

|                      | `--outdated include` | `--outdated hide` | `--outdated only` |
| :------------------- | :------------------- | :---------------- | :---------------- |
| `--status open`      | every open thread    | 🔴 needs action    | 🟡 open, superseded |
| `--status resolved`  | every resolved thread | 🟢 resolved, current | ⚪ historical |
| `--status all`       | everything (default) | current threads   | superseded threads |

`--active-only` is the top-middle cell, which is the common case: the threads
that are still open *and* still point at code as it stands.

Filtering never changes thread numbering or the counts in the summary table —
both always describe the whole pull request — so a thread's number is a stable
reference you can quote at a colleague regardless of which flags either of you
used.

## Authentication

A token is looked for in this order, and the first one found wins:

1. `--token`
2. `GH_TOKEN` / `GITHUB_TOKEN`, or `GH_ENTERPRISE_TOKEN` / `GITHUB_ENTERPRISE_TOKEN`
   for a non-github.com host
3. `gh auth token --hostname <host>`

Running as a `gh` extension covers step 2 automatically, since `gh` injects the
token into extension processes. Running standalone on a machine with `gh`
installed and logged in covers step 3. In CI, set `GH_TOKEN`.

`gh` is never required at runtime — it is only ever consulted as a place a token
might already be sitting.

The host is resolved from the PR URL if you gave one, then `--hostname`, then
`-R host/owner/repo`, then the git remote of the current directory, then
`GH_HOST`, then github.com.

## Exit codes

| Code | Meaning |
| :---: | :--- |
| 0 | Success |
| 1 | Unexpected API response, or a filesystem error |
| 2 | Bad arguments |
| 3 | Could not determine the repository |
| 4 | No token found, or GitHub rejected it |
| 5 | Repository or pull request not found |
| 6 | Network failure, or rate limited |

## Output

The document contains the pull request's metadata and description, top-level
review summaries, conversation comments, a status overview table, and every
inline review thread with its diff hunk and replies. Threads are ordered so that
what still needs action comes first.

Everything quoted from GitHub is blockquoted, so a bot review that opens with
`## Walkthrough` cannot collide with the document's own headings, and diff fences
are sized to survive hunks that themselves contain backticks.
