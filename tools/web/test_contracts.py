import pathlib
import re
import unittest
ROOT = pathlib.Path(__file__).resolve().parents[2]
class BrowserContracts(unittest.TestCase):
    def test_shell_fallbacks_match_fluent(self):
        ftl = dict(re.findall(r'^browser-([\w-]+) = (.*)$', (ROOT/'assets/locales/en-US/browser.ftl').read_text(), re.M))
        shell = dict(re.findall(r'^  (\w+): "([^"]+)",$', (ROOT/'web/shell.js').read_text(), re.M))
        self.assertEqual(set(shell), {'loading','reload','failed','source','notices','dismiss','saves',
                                      'viewportTitle','viewportDimensions','viewportMinimum','viewportHelp'})
        for key, value in shell.items():
            fluent_key = 'local-saves' if key == 'saves' else re.sub(r'[A-Z]', lambda m: '-' + m[0].lower(), key)
            self.assertEqual(value, ftl[fluent_key])
    def test_shell_minimum_matches_game_content_size(self):
        shell = (ROOT/'web/shell.js').read_text()
        game = (ROOT/'crates/game/src/window.rs').read_text()
        screen = re.search(r'MIN_SCREEN_SIZE: Vec2 = Vec2::new\(([\d.]+), ([\d.]+)\)', game)
        chrome = re.search(r'DESKTOP_CHROME_SIZE: Vec2 = Vec2::new\(([\d.]+), ([\d.]+)\)', game)
        minimum = re.search(r'const minWidth = (\d+), minHeight = (\d+);', shell)
        self.assertEqual(tuple(map(float, minimum.groups())),
                         tuple(float(s) - float(c) for s, c in zip(screen.groups(), chrome.groups())))
    def test_async_storage_imports_take_js_owned_bytes(self):
        # wasm-bindgen passes &[u8] as a view of wasm memory; an import that awaits before using it
        # can find the view detached by heap growth (a lost progress write did exactly that).
        source = (ROOT/'crates/infrastructure/src/browser.rs').read_text()
        imports = re.findall(r'fn (\w+)\(([^)]*)\)\s*->\s*js_sys::Promise;', source)
        self.assertTrue(imports)
        for name, params in imports:
            self.assertNotIn('&[u8]', params, name)
if __name__ == '__main__': unittest.main()
