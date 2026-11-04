"""Run a command unchanged and append its elapsed time and exit status to the CI summary."""
import argparse
import os
from pathlib import Path
import subprocess
import time


def run_timed(label, command, *, cwd=None, summary=None):
    start = time.monotonic()
    result = subprocess.run(command, cwd=cwd, check=False)
    elapsed = time.monotonic() - start
    status = result.returncode if result.returncode >= 0 else 128 - result.returncode
    line = f"- **{label}**: {elapsed:.2f} s (exit {status})\n"
    print(line, end="", flush=True)
    if summary:
        with Path(summary).open("a") as output:
            output.write(line)
    return status


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("label")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a command is required")
    raise SystemExit(run_timed(args.label, command, summary=os.environ.get("GITHUB_STEP_SUMMARY")))
