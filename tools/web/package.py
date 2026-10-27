"""Trunk's native packaging hook. The Rust packager owns canonical region assembly."""
import os
import pathlib
import subprocess
root = pathlib.Path(__file__).resolve().parents[2]
build = os.environ.get("MM_BUILD_SHA", "development")
subprocess.run(["cargo", "run", "--locked", "-p", "infrastructure", "--example", "package_web", "--",
                str(root / "assets"), str(root / "web/assets"), build], cwd=root, check=True)

subprocess.run(["python3", str(root / "tools/web/licenses.py")], cwd=root, check=True)
