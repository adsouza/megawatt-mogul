"""Define CI shards and prove that nextest assigns every test exactly once."""

import argparse
import json
from pathlib import Path
import subprocess


SHIPPED = "binary_id(=infrastructure::shipped_regions)"
GAME = "binary_id(=game::game)"
SHARDS = {
    # Hash partitioning spreads independently named region/seed cases over four runners.
    # Use nextest itself for both the inventory proof and execution, never a second hash.
    **{f"shipped-regions-{index}": {"filter": SHIPPED, "partition": f"hash:{index}/4"}
       for index in range(1, 5)},
    "game-integration": {"filter": GAME, "partition": ""},
    "remaining": {"filter": f"not ({SHIPPED} or {GAME})", "partition": ""},
}


def selected_tests(listing):
    return {
        (binary, name)
        for binary, suite in listing["rust-suites"].items()
        for name, test in suite.get("testcases", {}).items()
        if test["filter-match"]["status"] == "matches"
    }


def verify_partition(all_tests, groups):
    assigned = set()
    for name, tests in groups.items():
        if not tests:
            raise ValueError(f"empty shard: {name}")
        if assigned & tests:
            raise ValueError(f"tests assigned to multiple shards: {sorted(assigned & tests)}")
        assigned.update(tests)
    if all_tests - assigned:
        raise ValueError(f"missing tests: {sorted(all_tests - assigned)}")
    if assigned - all_tests:
        raise ValueError(f"unexpected tests: {sorted(assigned - all_tests)}")


def list_tests(filterset=None, partition=None):
    # Cargo reuses the binaries already built by `nextest archive`. Listing executes only
    # the harness's --list command, never a test. Include ignored tests in the inventory
    # proof too; execution still uses the ordinary CI profile's ignored-test policy.
    command = ["cargo", "nextest", "list", "--workspace", "--locked", "--profile", "ci",
               "--run-ignored", "all", "--message-format", "json"]
    if filterset is not None:
        command.extend(["-E", filterset])
    if partition:
        command.extend(["--partition", partition])
    return selected_tests(json.loads(subprocess.check_output(command)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--github-output", type=Path, required=True)
    args = parser.parse_args()
    all_tests = list_tests()
    groups = {name: list_tests(selection["filter"], selection["partition"])
              for name, selection in SHARDS.items()}
    verify_partition(all_tests, groups)
    for name, tests in groups.items():
        print(f"{name}: {len(tests)} tests (including ignored inventory)")
    print(f"All {len(all_tests)} tests assigned exactly once.")
    matrix = {"include": [{"name": name, **selection}
                          for name, selection in SHARDS.items()]}
    with args.github_output.open("a") as output:
        output.write(f"matrix={json.dumps(matrix, separators=(',', ':'))}\n")


if __name__ == "__main__":
    main()
