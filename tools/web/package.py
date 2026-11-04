"""Trunk's native packaging hook. The Rust packager owns canonical region assembly."""
import os
import pathlib
from time_command import run_timed
root = pathlib.Path(__file__).resolve().parents[2]
build = os.environ.get("MM_BUILD_SHA", "development")
summary = os.environ.get("GITHUB_STEP_SUMMARY")
status = run_timed("Package browser assets", ["cargo", "run", "--locked", "-p", "infrastructure",
                   "--example", "package_web", "--", str(root / "assets"), str(root / "web/assets"), build],
                   cwd=root, summary=summary)
if status:
    raise SystemExit(status)

raise SystemExit(run_timed("Bundle dependency notices", ["python3", str(root / "tools/web/licenses.py")],
                          cwd=root, summary=summary))
