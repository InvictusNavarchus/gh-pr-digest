#!/bin/sh
#
# Install gh-pr-digest from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/InvictusNavarchus/gh-pr-digest/master/install.sh | sh
#
# Environment:
#   VERSION      tag to install (default: the latest release)
#   INSTALL_DIR  where to put the binary (default: ~/.local/bin)
#
# If you already use the GitHub CLI, `gh extension install` is the better path;
# this script exists for machines that have no `gh` at all.

set -eu

REPO="InvictusNavarchus/gh-pr-digest"
BIN="gh-pr-digest"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

die() {
    echo "install: $*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required but not installed"
}

# Map uname's vocabulary onto the release asset names, which use Go's.
detect_platform() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux) os=linux ;;
        Darwin) os=darwin ;;
        *)
            die "unsupported operating system: $os.
On Windows, install the GitHub CLI extension or download the binary from
https://github.com/$REPO/releases/latest"
            ;;
    esac

    case "$arch" in
        x86_64 | amd64) arch=amd64 ;;
        aarch64 | arm64) arch=arm64 ;;
        *) die "unsupported architecture: $arch" ;;
    esac

    echo "${os}-${arch}"
}

# Follow the /releases/latest redirect rather than calling the API, so that an
# unauthenticated install is not spending the caller's anonymous rate limit.
latest_version() {
    # A 404 here means the repository has no releases yet, or is private, or
    # was misspelled -- and a network failure lands in exactly the same place,
    # so the message has to cover all of them without guessing.
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/$REPO/releases/latest") ||
        die "could not resolve the latest release of $REPO.
The repository may have no releases yet, or you may be offline.
Set VERSION to install a specific tag."

    version=${url##*/}
    [ -n "$version" ] && [ "$version" != "latest" ] ||
        die "could not determine the latest version; set VERSION explicitly"

    echo "$version"
}

verify() {
    file=$1
    name=$2
    sums=$3

    # Both tools take the same "<hash>  <name>" input; only the spelling of the
    # command differs between Linux and macOS.
    if command -v sha256sum >/dev/null 2>&1; then
        checker="sha256sum"
    elif command -v shasum >/dev/null 2>&1; then
        checker="shasum -a 256"
    else
        echo "install: no sha256 tool found, skipping checksum verification" >&2
        return 0
    fi

    expected=$(grep " \{1,2\}$name\$" "$sums" | cut -d' ' -f1) ||
        die "no checksum recorded for $name"
    [ -n "$expected" ] || die "no checksum recorded for $name"

    actual=$($checker "$file" | cut -d' ' -f1)
    [ "$expected" = "$actual" ] ||
        die "checksum mismatch for $name
  expected $expected
  actual   $actual"
}

main() {
    need curl
    need uname

    platform=$(detect_platform)
    version="${VERSION:-$(latest_version)}"
    asset="${BIN}_${version}_${platform}"
    base="https://github.com/$REPO/releases/download/$version"

    echo "installing $BIN $version ($platform)"

    tmp=$(mktemp -d)
    # `set -e` skips the trap on an explicit exit, so clean up on both paths.
    trap 'rm -rf "$tmp"' EXIT INT TERM

    curl -fsSL "$base/$asset" -o "$tmp/$BIN" ||
        die "no release asset $asset.
Check https://github.com/$REPO/releases for available builds."

    if curl -fsSL "$base/checksums.txt" -o "$tmp/checksums.txt" 2>/dev/null; then
        verify "$tmp/$BIN" "$asset" "$tmp/checksums.txt"
    else
        echo "install: no checksums.txt in this release, skipping verification" >&2
    fi

    chmod +x "$tmp/$BIN"
    mkdir -p "$INSTALL_DIR"
    # `mv` across filesystems is not atomic; cp then rm keeps the failure mode
    # to "temp file left behind" rather than "half a binary on PATH".
    cp "$tmp/$BIN" "$INSTALL_DIR/$BIN.new"
    mv "$INSTALL_DIR/$BIN.new" "$INSTALL_DIR/$BIN"

    echo "installed $INSTALL_DIR/$BIN"

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo
            echo "$INSTALL_DIR is not on your PATH. Add it with:"
            echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
            ;;
    esac
}

main "$@"
