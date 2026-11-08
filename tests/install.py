"""Test standalone installation with local release fixtures and no network."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[1]
MOCK = r'''#!/usr/bin/env python3
import hashlib, json, os, pathlib, shutil, sys
name, args = pathlib.Path(sys.argv[0]).name, sys.argv[1:]
with open(os.environ["COMMAND_LOG"], "a") as log:
    log.write(json.dumps([name, *args]) + "\n")
if name == "uname": print(os.environ.get("MOCK_OS", "Linux") if args == ["-s"] else os.environ.get("MOCK_ARCH", "x86_64"))
elif name == "shasum":
    print(hashlib.sha256(pathlib.Path(args[-1]).read_bytes()).hexdigest() + "  " + args[-1])
elif name == "curl":
    if "-w" in args:
        print("https://github.com/InvictusNavarchus/gh-pr-digest/releases/tag/v0.1.1", end="")
    else:
        url = next(a for a in args if a.startswith("https://"))
        source = pathlib.Path(os.environ["RELEASE_DIR"]) / url.rsplit("/", 1)[1]
        if not source.is_file(): sys.exit(22)
        shutil.copyfile(source, args[args.index("-o") + 1])
'''


class Installation(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="digest-install-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.script = self.root / "install.sh"
        self.script.write_text((REPO / "install.sh").read_text().replace("$HOME", "${TEST_USER_HOME}"))
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("uname", "curl", "shasum"):
            mock = self.bin / name
            mock.write_text(MOCK)
            mock.chmod(0o755)
        self.releases = self.root / "releases"
        self.releases.mkdir()
        checksums = []
        for platform in ("linux-amd64", "linux-arm64", "darwin-amd64", "darwin-arm64"):
            name = "gh-pr-digest_v0.1.1_" + platform
            binary = self.releases / name
            binary.write_bytes(b"#!/bin/sh\necho installed release\n")
            checksums.append(hashlib.sha256(binary.read_bytes()).hexdigest() + "  " + name)
        (self.releases / "checksums.txt").write_text("\n".join(checksums) + "\n")
        self.log = self.root / "commands.jsonl"
        self.installed = self.root / "user/.local/bin/gh-pr-digest"
        self.installed.parent.mkdir(parents=True)
        self.installed.write_text("previous binary")
        self.env = dict(os.environ, PATH=str(self.bin) + ":" + os.environ["PATH"],
                        TEST_USER_HOME=str(self.root / "user"), RELEASE_DIR=str(self.releases), COMMAND_LOG=str(self.log))
        self.env.pop("INSTALL_DIR", None)
        self.env.pop("VERSION", None)

    def run_installer(self, env=None, ok=True):
        result = subprocess.run(["/bin/sh", str(self.script)], cwd=self.root,
                                env=env or self.env, text=True, capture_output=True)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        if not ok:
            self.assertEqual(self.installed.read_text(), "previous binary")
        return result

    def restricted_path(self, with_shasum=False):
        # Deliberately omit sha256sum to exercise shasum and missing-tool paths.
        for name in ("python3", "mktemp", "awk", "cut", "chmod", "mkdir", "cp", "mv", "rm"):
            (self.bin / name).symlink_to(shutil.which(name))
        if not with_shasum:
            (self.bin / "shasum").unlink()
        return dict(self.env, PATH=str(self.bin))

    def test_install_and_update_replace_binary_after_verification(self):
        self.run_installer()
        self.assertIn("installed release", self.installed.read_text())
        self.assertTrue(os.access(self.installed, os.X_OK))
        self.assertFalse(self.installed.with_name("gh-pr-digest.new").exists())
        self.run_installer()

    def test_macos_uses_shasum_when_sha256sum_is_unavailable(self):
        env = self.restricted_path(with_shasum=True)
        self.run_installer(env=dict(env, MOCK_OS="Darwin", MOCK_ARCH="arm64"))
        commands = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertTrue(any(c[0] == "shasum" for c in commands))

    def test_missing_verification_tool_preserves_existing_binary(self):
        self.run_installer(env=self.restricted_path(), ok=False)

    def test_missing_checksums_preserve_existing_binary(self):
        (self.releases / "checksums.txt").unlink()
        self.run_installer(ok=False)

    def test_corrupt_download_preserves_existing_binary(self):
        (self.releases / "gh-pr-digest_v0.1.1_linux-amd64").write_text("corrupt")
        self.run_installer(ok=False)

    def test_checksum_must_match_the_exact_asset_name(self):
        checksum = self.releases / "checksums.txt"
        checksum.write_text(checksum.read_text().replace("_linux-amd64", "_linux-amd64.extra"))
        self.run_installer(ok=False)

    def test_duplicate_or_invalid_checksums_are_rejected(self):
        checksum = self.releases / "checksums.txt"
        line = checksum.read_text().splitlines()[0]
        checksum.write_text(line + "\n" + line + "\n")
        self.run_installer(ok=False)
        checksum.write_text("invalid  gh-pr-digest_v0.1.1_linux-amd64\n")
        self.run_installer(ok=False)

    def test_custom_directory_and_pinned_version(self):
        directory = self.root / "custom bin"
        self.run_installer(env=dict(self.env, INSTALL_DIR=str(directory), VERSION="v0.1.1"))
        self.assertTrue((directory / "gh-pr-digest").exists())
        self.assertEqual(self.installed.read_text(), "previous binary")
        commands = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertFalse(any(c[0] == "curl" and "-w" in c for c in commands))


if __name__ == "__main__":
    unittest.main()
