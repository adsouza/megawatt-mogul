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
    def test_async_storage_imports_take_js_owned_bytes(self):
        # wasm-bindgen passes &[u8] as a view of wasm memory; an import that awaits before using it
        # can find the view detached by heap growth (a lost progress write did exactly that).
        source = (ROOT/'crates/infrastructure/src/browser.rs').read_text()
        imports = re.findall(r'fn (\w+)\(([^)]*)\)\s*->\s*js_sys::Promise;', source)
        self.assertTrue(imports)
        for name, params in imports:
            self.assertNotIn('&[u8]', params, name)
if __name__ == '__main__': unittest.main()
