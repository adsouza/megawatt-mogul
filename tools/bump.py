#!/usr/bin/env python3
"""Bump the workspace version (patch by default), push main, then run tools/release.sh. Python 3.11+."""

import argparse
import importlib.util
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("release_metadata", ROOT / "tools/release/release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


def bump_version(version, part):
    match = release.SEMVER.fullmatch(version)
    if not match:
        raise ValueError(f"Invalid workspace SemVer: {version!r}")
    major, minor, patch = map(int, match["core"].split("."))
    if part == "major":
        major, minor, patch = major + 1, 0, 0
    elif part == "minor":
        minor, patch = minor + 1, 0
    elif part == "patch":
        patch += 1
    else:
        raise ValueError(f"Unknown bump component: {part}")
    return f"{major}.{minor}.{patch}"


def replace_workspace_version(source, old, new):
    """Edit just the version value, preserving comments, quotes and dependency versions."""
    section = re.search(
        r"(?ms)^[ \t]*\[workspace\.package\][^\n]*\n(?P<body>.*?)(?=^[ \t]*\[|\Z)", source
    )
    assignment = re.search(
        r'''(?m)^[ \t]*version[ \t]*=[ \t]*(['"])(?P<value>[^'"\r\n]+)\1''',
        section["body"] if section else "",
    )
    if not assignment or assignment["value"] != old:
        raise ValueError("Cannot locate the version assignment in [workspace.package].")
    start = section.start("body") + assignment.start("value")
    end = section.start("body") + assignment.end("value")
    return source[:start] + new + source[end:]


def run(root, *args, capture=False):
    return subprocess.run(args, cwd=root, check=True, text=True, capture_output=capture)


def git_output(root, *args):
    return run(root, "git", *args, capture=True).stdout.strip()


def require_clean(root):
    if git_output(root, "status", "--porcelain", "--untracked-files=all"):
        raise RuntimeError("A clean checkout is required; commit or stash changes first.")


def bump_and_release(root, part):
    if git_output(root, "branch", "--show-current") != "main":
        raise RuntimeError("Run this from the main branch.")
    require_clean(root)
    run(root, "git", "pull", "--ff-only", "--no-tags", "origin", "main")
    if git_output(root, "rev-parse", "HEAD") != git_output(root, "rev-parse", "refs/remotes/origin/main"):
        raise RuntimeError("main must match origin/main; publish any local commits first.")

    current = release.metadata(root)["version"]
    version = bump_version(current, part)
    tag = f"v{version}"
    local = subprocess.run(
        ["git", "show-ref", "--verify", "--quiet", f"refs/tags/{tag}"], cwd=root
    )
    if local.returncode == 0:
        raise RuntimeError(f"Tag {tag} already exists locally.")
    if local.returncode != 1:
        local.check_returncode()
    if git_output(root, "ls-remote", "--tags", "origin", f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"):
        raise RuntimeError(f"Tag {tag} already exists on origin.")

    print(f"Bumping {current} -> {version}; committing/pushing main, then releasing {tag}.", flush=True)
    paths = [root / "Cargo.toml", root / "Cargo.lock"]
    originals = {path: path.read_bytes() for path in paths}
    try:
        paths[0].write_bytes(replace_workspace_version(
            originals[paths[0]].decode("utf-8"), current, version
        ).encode("utf-8"))
        run(root, "cargo", "update", "--workspace")
        if release.metadata(root)["version"] != version:
            raise RuntimeError("Workspace version changed unexpectedly during lockfile refresh.")
        changed = set(git_output(root, "diff", "--name-only").splitlines())
        if changed != {"Cargo.toml", "Cargo.lock"} or git_output(root, "ls-files", "--others", "--exclude-standard"):
            raise RuntimeError("Unexpected file changes during lockfile refresh.")
    except BaseException:
        # No Git index/history changes yet: a failed refresh can safely restore our two files.
        for path, content in originals.items():
            path.write_bytes(content)
        print("bump: Version files restored; no bump commit was created.", file=sys.stderr)
        raise

    try:
        run(root, "git", "add", "--", "Cargo.toml", "Cargo.lock")
        run(root, "git", "commit", "-m", f"chore: bump workspace version to {version}")
        require_clean(root)
    except (subprocess.CalledProcessError, RuntimeError) as error:
        raise RuntimeError(
            f"Commit did not finish cleanly. Inspect the prepared changes and finish the commit, "
            f"then git push origin main and ./tools/release.sh {tag}."
        ) from error
    try:
        run(root, "git", "push", "origin", "main")
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"The bump commit remains local. Resolve the failure, then run: git push origin main\n"
            f"After that succeeds, run: ./tools/release.sh {tag}"
        ) from error
    try:
        run(root, str(root / "tools/release.sh"), tag)
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"Version {version} is already on origin/main. Follow release.sh's retry message above; "
            "do not bump again."
        ) from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("-major", "--major", dest="part", action="store_const", const="major",
                       help="increment major and reset minor/patch")
    group.add_argument("-minor", "--minor", dest="part", action="store_const", const="minor",
                       help="increment minor and reset patch")
    parser.set_defaults(part="patch")
    args = parser.parse_args()
    try:
        bump_and_release(ROOT, args.part)
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"bump: {error}\n")


if __name__ == "__main__":
    main()
