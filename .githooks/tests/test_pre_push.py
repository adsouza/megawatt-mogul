"""Exercise the real hook with temporary Git history and a recording Cargo stub."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


HOOK = Path(__file__).resolve().parents[1] / "pre-push"
ZERO = "0" * 40


class PrePushTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Hook Test")
        self.git("config", "user.email", "hook@example.invalid")
        self.git("config", "core.hooksPath", "/dev/null")
        self.base = self.commit("README.md")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        cargo = self.bin / "cargo"
        cargo.write_text('#!/bin/sh\nprintf "%s\\n" "$1" >> "$HOOK_TEST_LOG"\n'
                         'if [ "$1" = "$HOOK_TEST_FAIL" ]; then exit 1; fi\n')
        cargo.chmod(0o755)
        self.log = self.root / "cargo.log"

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], cwd=self.root, text=True, stderr=subprocess.DEVNULL
        ).strip()

    def commit(self, path):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(target.read_text() + "change\n" if target.exists() else "change\n")
        self.git("add", "--", path)
        self.git("commit", "--quiet", "-m", "test change")
        return self.git("rev-parse", "HEAD")

    def push(self, updates, fail=""):
        if self.log.exists():
            self.log.unlink()
        records = "".join(
            f"refs/heads/local {local} refs/heads/remote {remote}\n"
            for local, remote in updates
        )
        result = subprocess.run(
            ["bash", str(HOOK), "origin", "unused"], cwd=self.root,
            input=records, text=True, capture_output=True,
            env=dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                     HOOK_TEST_LOG=str(self.log), HOOK_TEST_FAIL=fail),
        )
        calls = self.log.read_text().splitlines() if self.log.exists() else []
        return result.returncode, calls

    def test_documentation_and_ci_changes_skip_suite(self):
        for path in ["README.md", "docs/a plan.md", ".github/workflows/ci.yml"]:
            with self.subTest(path=path):
                self.assertEqual(self.push([(self.commit(path), self.base)]), (0, ["clippy"]))

    def test_rust_and_test_inputs_run_suite(self):
        for path in ["src/lib.rs", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
                     ".cargo/config.toml", ".config/nextest.toml", "assets/region.ron",
                     "crates/example/tests/support/input.ron"]:
            with self.subTest(path=path):
                self.git("reset", "--hard", self.base)
                self.assertEqual(self.push([(self.commit(path), self.base)]),
                                 (0, ["clippy", "nextest"]))

    def test_earlier_outgoing_rust_commit_runs_suite(self):
        self.commit("src/lib.rs")
        self.assertEqual(self.push([(self.commit("README.md"), self.base)]),
                         (0, ["clippy", "nextest"]))

    def test_multiple_refs_include_every_update(self):
        docs = self.commit("README.md")
        rust = self.commit("src/lib.rs")
        self.assertEqual(self.push([(docs, self.base), (rust, docs)]),
                         (0, ["clippy", "nextest"]))

    def test_renaming_rust_to_documentation_runs_suite(self):
        rust = self.commit("src/lib.rs")
        self.git("mv", "src/lib.rs", "docs.md")
        self.git("commit", "--quiet", "-m", "rename")
        self.assertEqual(self.push([(self.git("rev-parse", "HEAD"), rust)]),
                         (0, ["clippy", "nextest"]))

    def test_unknown_ranges_run_suite(self):
        for updates in [[], [(self.base, ZERO)], [(self.base, "f" * 40)]]:
            with self.subTest(updates=updates):
                self.assertEqual(self.push(updates), (0, ["clippy", "nextest"]))

    def test_deleted_ref_skips_suite(self):
        self.assertEqual(self.push([(ZERO, self.base)]), (0, ["clippy"]))

    def test_failures_block_push(self):
        rust = self.commit("src/lib.rs")
        self.assertEqual(self.push([(rust, self.base)], fail="clippy"), (1, ["clippy"]))
        self.assertEqual(self.push([(rust, self.base)], fail="nextest"),
                         (1, ["clippy", "nextest"]))


if __name__ == "__main__":
    unittest.main()
