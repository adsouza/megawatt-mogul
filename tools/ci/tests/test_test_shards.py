"""Coverage checks for the three native CI shards, without running game tests."""

import importlib.util
from pathlib import Path
import unittest


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
