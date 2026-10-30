"""Coverage checks for native CI shards, without running game tests."""

import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "test_shards", Path(__file__).resolve().parents[1] / "test_shards.py"
)
shards = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(shards)


def listing(*tests):
    suites = {}
    for binary, name, ignored, matches in tests:
        suite = suites.setdefault(binary, {"testcases": {}})
        suite["testcases"][name] = {
            "ignored": ignored,
            "filter-match": {"status": "matches" if matches else "mismatch"},
        }
    return {"rust-suites": suites}


class ShardCoverageTests(unittest.TestCase):
    def test_region_and_game_shards_use_four_distinct_hash_partitions_each(self):
        regions = [selection for name, selection in shards.SHARDS.items()
                   if name.startswith("shipped-regions-")]
        self.assertEqual(regions, [
            {"filter": shards.SHIPPED, "partition": f"hash:{index}/4"}
            for index in range(1, 5)
        ])
        games = [selection for name, selection in shards.SHARDS.items()
                 if name.startswith("game-integration-")]
        self.assertEqual(games, [
            {"filter": shards.GAME, "partition": f"hash:{index}/4"}
            for index in range(1, 5)
        ])
        self.assertEqual(shards.SHARDS["remaining"],
                         {"filter": f"not ({shards.SHIPPED} or {shards.GAME})",
                          "partition": ""})

    @patch.object(shards.subprocess, "check_output", return_value=b'{"rust-suites": {}}')
    def test_inventory_passes_partition_to_nextest(self, check_output):
        shards.list_tests(shards.SHIPPED, "hash:2/4")
        command = check_output.call_args.args[0]
        self.assertEqual(command[command.index("-E") + 1], shards.SHIPPED)
        self.assertEqual(command[command.index("--partition") + 1], "hash:2/4")
        self.assertEqual(command[command.index("--run-ignored") + 1], "all")

    @patch.object(shards.subprocess, "check_output", return_value=b'{"rust-suites": {}}')
    def test_unpartitioned_inventory_omits_partition(self, check_output):
        shards.list_tests(shards.GAME, "")
        self.assertNotIn("--partition", check_output.call_args.args[0])

    def setUp(self):
        self.groups = {
            "shipped-regions": {("infrastructure::shipped_regions", "year_one")},
            "game-integration": {("game::game", "save_resume")},
            "remaining": {("game", "presenter"), ("domain", "future_test")},
        }
        self.all_tests = set().union(*self.groups.values())

    def test_disjoint_exhaustive_shards_pass(self):
        shards.verify_partition(self.all_tests, self.groups)

    def test_missing_test_fails(self):
        self.groups["remaining"].remove(("domain", "future_test"))
        with self.assertRaisesRegex(ValueError, "missing"):
            shards.verify_partition(self.all_tests, self.groups)

    def test_duplicate_test_fails(self):
        self.groups["remaining"].update(self.groups["game-integration"])
        with self.assertRaisesRegex(ValueError, "multiple shards"):
            shards.verify_partition(self.all_tests, self.groups)

    def test_unknown_test_fails(self):
        self.groups["remaining"].add(("unknown", "unexpected"))
        with self.assertRaisesRegex(ValueError, "unexpected"):
            shards.verify_partition(self.all_tests, self.groups)

    def test_empty_shard_fails_instead_of_hiding_a_renamed_binary(self):
        self.groups["shipped-regions"].clear()
        with self.assertRaisesRegex(ValueError, "empty"):
            shards.verify_partition(self.all_tests, self.groups)

    def test_listing_counts_only_matching_tests_and_preserves_ignored_inventory(self):
        data = listing(
            ("game", "shared_name", False, True),
            ("game::game", "shared_name", False, True),
            ("game::game", "ignored_budget", True, True),
            ("domain", "filtered_out", False, False),
        )
        self.assertEqual(shards.selected_tests(data), {
            ("game", "shared_name"), ("game::game", "shared_name"),
            ("game::game", "ignored_budget"),
        })


if __name__ == "__main__":
    unittest.main()
