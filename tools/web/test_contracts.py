import pathlib
import re
import unittest
ROOT = pathlib.Path(__file__).resolve().parents[2]
class BrowserContracts(unittest.TestCase):
    def test_shell_fallbacks_match_fluent(self):
        ftl = dict(re.findall(r'^browser-([\w-]+) = (.*)$', (ROOT/'assets/locales/en-US/browser.ftl').read_text(), re.M))
        shell = dict(re.findall(r'^  (\w+): "([^"]+)",$', (ROOT/'web/shell.js').read_text(), re.M))
        self.assertEqual(set(shell), {'loading','reload','failed','source','notices','small'})
        for key, value in shell.items():
            self.assertEqual(value, ftl['small-viewport' if key == 'small' else key])
if __name__ == '__main__': unittest.main()
