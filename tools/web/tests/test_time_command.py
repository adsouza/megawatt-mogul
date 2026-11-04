"""Timing must preserve command arguments and failures, including in the CI summary."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "time_command", Path(__file__).resolve().parents[1] / "time_command.py"
)
timing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(timing)


class TimingTests(unittest.TestCase):
    def test_arguments_are_literal_and_working_directory_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            summary = root / "summary.md"
            argument = 'spaces; $(false) `false`'
            command = [sys.executable, "-c",
                       "from pathlib import Path; import sys; Path('result').write_text(sys.argv[1])",
                       argument]
            self.assertEqual(timing.run_timed("Build", command, cwd=root, summary=summary), 0)
            self.assertEqual((root / "result").read_text(), argument)
            self.assertIn("Build", summary.read_text())
            self.assertIn("exit 0", summary.read_text())

    def test_failure_is_reported_and_propagated_without_overwriting_prior_timings(self):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary.md"
            summary.write_text("Earlier step\n")
            status = timing.run_timed("Tests", [sys.executable, "-c", "raise SystemExit(7)"],
                                      summary=summary)
            self.assertEqual(status, 7)
            self.assertTrue(summary.read_text().startswith("Earlier step\n"))
            self.assertIn("exit 7", summary.read_text())


if __name__ == "__main__":
    unittest.main()
