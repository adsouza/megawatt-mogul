"""Exercise release tagging against a disposable local Git remote, without running Cargo."""

from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class ReleaseScriptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.remote = self.directory / "origin.git"
        self.work = self.directory / "checkout with spaces"
        self.marker = self.directory / "pre-push-ran"
        self.git(self.directory, "init", "--bare", "--initial-branch=main", str(self.remote))
        self.git(self.directory, "init", "--initial-branch=main", str(self.work))
        for repository in (self.remote, self.work):
            self.git(repository, "config", "tag.gpgSign", "false")
            self.git(repository, "config", "commit.gpgSign", "false")
        self.git(self.work, "config", "user.name", "Release test")
        self.git(self.work, "config", "user.email", "release@example.invalid")
        self.git(self.work, "config", "core.hooksPath", ".githooks")
        (self.work / "tools/release").mkdir(parents=True)
        shutil.copyfile(ROOT / "tools/release.sh", self.work / "tools/release.sh")
        (self.work / "tools/release.sh").chmod(0o755)
        shutil.copyfile(ROOT / "tools/release/release.py", self.work / "tools/release/release.py")
        (self.work / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/game"]\n[workspace.package]\nversion = "0.1.0"\n'
        )
        (self.work / "crates/game").mkdir(parents=True)
        (self.work / "crates/game/Cargo.toml").write_text(
            '[package]\nname = "game"\nversion.workspace = true\n'
        )
        (self.work / "Cargo.lock").write_text('version = 4\n[[package]]\nname = "game"\nversion = "0.1.0"\n')
        hook = self.work / ".githooks/pre-push"
        hook.parent.mkdir()
        hook.write_text(f"#!/bin/sh\nprintf 'ran\\n' >> {shlex.quote(str(self.marker))}\n")
        hook.chmod(0o755)
        self.git(self.work, "add", ".")
        self.git(self.work, "commit", "-m", "Initial fixture")
        self.git(self.work, "remote", "add", "origin", str(self.remote))
        self.git(self.work, "push", "-u", "origin", "main")
        self.marker.unlink()

    def git(self, cwd, *args):
        result = subprocess.run(["git", *args], cwd=cwd, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, f"git {args}: {result.stderr}")
        return result.stdout.strip()

    def run_script(self, *args):
        # The caller can be outside the checkout; subprocess cwd never changes this test process.
        return subprocess.run(
            [str(self.work / "tools/release.sh"), *args], cwd=self.directory,
            text=True, capture_output=True,
        )

    def assert_no_release(self):
        self.assertEqual(self.git(self.work, "tag", "--list"), "")
        self.assertEqual(self.git(self.remote, "tag", "--list"), "")
        self.assertFalse(self.marker.exists())

    def test_pulls_main_then_pushes_only_the_annotated_release_tag_with_hooks(self):
        upstream = self.directory / "upstream"
        self.git(self.directory, "clone", str(self.remote), str(upstream))
        self.git(upstream, "config", "user.name", "Release test")
        self.git(upstream, "config", "user.email", "release@example.invalid")
        self.git(upstream, "config", "commit.gpgSign", "false")
        for filename in ("Cargo.toml", "Cargo.lock"):
            path = upstream / filename
            path.write_text(path.read_text().replace('"0.1.0"', '"0.2.0"'))
        (upstream / "release-note.txt").write_text("A merged change")
        self.git(upstream, "add", ".")
        self.git(upstream, "commit", "-m", "Merged change")
        self.git(upstream, "push", "origin", "main")
        expected = self.git(upstream, "rev-parse", "HEAD")
        self.git(self.work, "tag", "unrelated-local-tag")

        result = self.run_script("v0.2.0")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git(self.work, "rev-parse", "HEAD"), expected)
        self.assertEqual(self.git(self.remote, "rev-parse", "v0.2.0^{}"), expected)
        self.assertEqual(self.git(self.remote, "cat-file", "-t", "v0.2.0"), "tag")
        self.assertIn("Megawatt Mogul 0.2.0", self.git(self.remote, "cat-file", "-p", "v0.2.0"))
        self.assertEqual(self.git(self.remote, "tag", "--list"), "v0.2.0")
        self.assertTrue(self.marker.exists())

    def test_requires_exactly_one_tag_argument(self):
        for args in ((), ("v0.1.0", "extra")):
            result = self.run_script(*args)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("usage:", result.stderr)
            self.assert_no_release()

    def test_rejects_a_version_mismatch_without_creating_a_tag(self):
        result = self.run_script("v9.9.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("workspace version", result.stderr)
        self.assert_no_release()

    def test_rejects_dirty_checkouts(self):
        (self.work / "untracked.txt").write_text("Not committed")
        result = self.run_script("v0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("clean", result.stderr)
        self.assert_no_release()

    def test_rejects_other_branches(self):
        self.git(self.work, "switch", "-c", "feature")
        result = self.run_script("v0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("main", result.stderr)
        self.assert_no_release()

    def test_rejects_unpushed_main_commits(self):
        self.git(self.work, "commit", "--allow-empty", "-m", "Not pushed")
        result = self.run_script("v0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("origin/main", result.stderr)
        self.assert_no_release()

    def test_rejects_existing_remote_tags_without_creating_a_local_tag(self):
        self.git(self.remote, "tag", "v0.1.0", "main")
        result = self.run_script("v0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(self.git(self.work, "tag", "--list", "v0.1.0"), "")
        self.assertFalse(self.marker.exists())

    def test_rejects_existing_local_tags(self):
        self.git(self.work, "tag", "v0.1.0")
        result = self.run_script("v0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(self.git(self.remote, "tag", "--list", "v0.1.0"), "")
        self.assertFalse(self.marker.exists())

    def test_failed_push_keeps_the_local_tag_and_explains_how_to_retry(self):
        hook = self.remote / "hooks/pre-receive"
        hook.write_text("#!/bin/sh\nexit 1\n")
        hook.chmod(0o755)
        result = self.run_script("v0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.git(self.work, "cat-file", "-t", "v0.1.0"), "tag")
        self.assertEqual(self.git(self.remote, "tag", "--list"), "")
        self.assertIn("git push origin", result.stderr)
        self.assertTrue(self.marker.exists())


if __name__ == "__main__":
    unittest.main()
