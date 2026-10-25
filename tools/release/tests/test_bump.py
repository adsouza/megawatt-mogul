"""Version bumps and end-to-end releases against disposable Git remotes."""

import importlib.util
import shutil
import subprocess
import sys
import tomllib
import unittest

from test_release_script import GitReleaseFixture, ROOT


SPEC = importlib.util.spec_from_file_location("bump", ROOT / "tools/bump.py")
bump = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bump)


class VersionBumpTests(unittest.TestCase):
    def test_component_bumps_reset_less_significant_components(self):
        for part, expected in (("patch", "2.3.5"), ("minor", "2.4.0"), ("major", "3.0.0")):
            with self.subTest(part=part):
                self.assertEqual(bump.bump_version("2.3.4", part), expected)
        self.assertEqual(bump.bump_version("0.0.0", "patch"), "0.0.1")
        self.assertEqual(bump.bump_version("1.2.3-rc.1+build.7", "patch"), "1.2.4")

    def test_only_the_workspace_version_changes_and_formatting_survives(self):
        source = '''[workspace.package]
version = '0.1.0' # release version
edition = "2024"
[workspace.dependencies]
example = { version = "0.1.0" }
'''
        expected = source.replace("version = '0.1.0'", "version = '0.1.1'")
        self.assertEqual(bump.replace_workspace_version(source, "0.1.0", "0.1.1"), expected)


class BumpScriptTests(GitReleaseFixture):
    def setUp(self):
        super().setUp()
        shutil.copyfile(ROOT / "tools/bump.py", self.work / "tools/bump.py")
        (self.work / ".gitignore").write_text("__pycache__/\n/target/\n")
        (self.work / "crates/game/src").mkdir()
        (self.work / "crates/game/src/lib.rs").write_text("pub const FIXTURE: u32 = 1;\n")
        self.git(self.work, "add", ".")
        self.git(self.work, "commit", "-m", "Add bump fixture")
        self.git(self.work, "push", "origin", "main")
        self.marker.unlink()

    def run_bump(self, *args):
        return subprocess.run(
            [sys.executable, str(self.work / "tools/bump.py"), *args],
            cwd=self.directory, text=True, capture_output=True,
        )

    def remote_version(self):
        manifest = self.git(self.remote, "show", "main:Cargo.toml")
        return tomllib.loads(manifest)["workspace"]["package"]["version"]

    def assert_bumped_and_released(self, expected):
        self.assertEqual(self.remote_version(), expected)
        lock = tomllib.loads(self.git(self.remote, "show", "main:Cargo.lock"))
        self.assertEqual(lock["package"][0]["version"], expected)
        self.assertEqual(self.git(self.remote, "cat-file", "-t", f"v{expected}"), "tag")
        self.assertEqual(
            self.git(self.remote, "rev-parse", f"v{expected}^{{}}"),
            self.git(self.remote, "rev-parse", "main"),
        )
        self.assertEqual(self.git(self.work, "status", "--porcelain"), "")
        self.assertEqual(self.marker.read_text().splitlines(), ["ran", "ran"])

    def test_patch_is_the_default_and_runs_cargo_and_both_git_hooks(self):
        result = self.run_bump()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_bumped_and_released("0.1.1")

    def test_minor_flag(self):
        result = self.run_bump("-minor")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_bumped_and_released("0.2.0")

    def test_major_flag(self):
        result = self.run_bump("-major")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_bumped_and_released("1.0.0")

    def test_flags_are_mutually_exclusive(self):
        result = self.run_bump("-major", "-minor")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assert_no_release()

    def test_dirty_checkout_is_refused(self):
        (self.work / "untracked.txt").write_text("Not committed")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("clean", result.stderr)
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assert_no_release()

    def test_other_branches_are_refused(self):
        self.git(self.work, "switch", "-c", "feature")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("main", result.stderr)
        self.assert_no_release()

    def test_unpushed_main_commits_are_refused(self):
        self.git(self.work, "commit", "--allow-empty", "-m", "Not pushed")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("origin/main", result.stderr)
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assert_no_release()

    def test_uses_the_version_pulled_from_main(self):
        upstream = self.directory / "upstream"
        self.git(self.directory, "clone", str(self.remote), str(upstream))
        self.git(upstream, "config", "user.name", "Release test")
        self.git(upstream, "config", "user.email", "release@example.invalid")
        self.git(upstream, "config", "commit.gpgSign", "false")
        for filename in ("Cargo.toml", "Cargo.lock"):
            path = upstream / filename
            path.write_text(path.read_text().replace('"0.1.0"', '"0.2.0"'))
        self.git(upstream, "add", ".")
        self.git(upstream, "commit", "-m", "Merged version bump")
        self.git(upstream, "push", "origin", "main")
        result = self.run_bump()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_bumped_and_released("0.2.1")

    def test_existing_local_tag_is_refused_before_editing(self):
        self.git(self.work, "tag", "v0.1.1")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(self.git(self.work, "status", "--porcelain"), "")
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assertFalse(self.marker.exists())

    def test_existing_remote_tag_is_refused_before_editing(self):
        self.git(self.remote, "tag", "v0.1.1", "main")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(self.git(self.work, "status", "--porcelain"), "")
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assertFalse(self.marker.exists())

    def test_cargo_failure_restores_version_files_without_committing_or_releasing(self):
        manifest = self.work / "crates/game/Cargo.toml"
        manifest.write_text(manifest.read_text() + '\n[dependencies]\nmissing = { path = "../../missing" }\n')
        self.git(self.work, "add", ".")
        self.git(self.work, "commit", "-m", "Invalid Cargo target fixture")
        self.git(self.work, "push", "origin", "main")
        self.marker.unlink()
        head = self.git(self.work, "rev-parse", "HEAD")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.git(self.work, "rev-parse", "HEAD"), head)
        self.assertEqual(self.git(self.work, "status", "--porcelain"), "")
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assert_no_release()

    def test_failed_main_push_retains_the_commit_and_never_starts_release(self):
        hook = self.remote / "hooks/pre-receive"
        hook.write_text("#!/bin/sh\nexit 1\n")
        hook.chmod(0o755)
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.remote_version(), "0.1.0")
        local = tomllib.loads((self.work / "Cargo.toml").read_text())
        self.assertEqual(local["workspace"]["package"]["version"], "0.1.1")
        self.assertEqual(self.git(self.work, "tag", "--list"), "")
        self.assertIn("git push origin main", result.stderr)
        self.assertIn("release.sh v0.1.1", result.stderr)
        self.assertEqual(self.marker.read_text().splitlines(), ["ran"])

    def test_commit_hook_failure_preserves_prepared_changes_without_pushing(self):
        hook = self.work / ".githooks/pre-commit"
        hook.write_text("#!/bin/sh\ngit diff --cached --quiet -- Cargo.toml\n")
        hook.chmod(0o755)
        self.git(self.work, "add", ".")
        self.git(self.work, "commit", "-m", "Reject version commits in this fixture")
        self.git(self.work, "push", "origin", "main")
        self.marker.unlink()
        head = self.git(self.work, "rev-parse", "HEAD")
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Commit did not finish cleanly", result.stderr)
        self.assertEqual(self.git(self.work, "rev-parse", "HEAD"), head)
        self.assertEqual(self.remote_version(), "0.1.0")
        self.assertEqual(
            set(self.git(self.work, "diff", "--cached", "--name-only").splitlines()),
            {"Cargo.toml", "Cargo.lock"},
        )
        self.assert_no_release()

    def test_failed_tag_push_keeps_the_landed_version_and_the_local_tag(self):
        hook = self.remote / "hooks/pre-receive"
        hook.write_text('''#!/bin/sh
while read -r old new ref; do
    case "$ref" in refs/tags/*) exit 1;; esac
done
''')
        hook.chmod(0o755)
        result = self.run_bump()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.remote_version(), "0.1.1")
        self.assertEqual(self.git(self.remote, "tag", "--list"), "")
        self.assertEqual(self.git(self.work, "cat-file", "-t", "v0.1.1"), "tag")
        self.assertIn("git push origin refs/tags/v0.1.1", result.stderr)
        self.assertEqual(self.marker.read_text().splitlines(), ["ran", "ran"])


if __name__ == "__main__":
    unittest.main()
