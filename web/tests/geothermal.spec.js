import { test, expect } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

// Uses an isolated, accelerated region; never opens the user's browser save library.
test('geothermal native and WASM exploration decline and makeup boundaries agree', async ({ page }) => {
  const parent = mkdtempSync(join(tmpdir(), 'mm-geothermal-browser-'));
  try {
    const directory = join(parent, 'fixture');
    const tool = resolve(import.meta.dirname, '../../target/debug/examples/geothermal_acceptance');
    execFileSync(tool, [directory]);
    const bytes = readFileSync(join(directory, 'saves/cccccccccccccccccccccccccccccccc.mgsave'));
    const expected = readFileSync(join(directory, 'native-trace.txt'), 'utf8');
    await page.goto('./');
    await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', { timeout: 150000 });
    const actual = await page.evaluate(bytes => window.wasmBindings.browser_geothermal_probe(new Uint8Array(bytes)), [...bytes]);
    expect(actual).toBe(expected);
    expect(actual.split('\n')).toHaveLength(8);
    expect(actual).toContain('dry=1');
    expect(actual).toContain(':1:0:'); // A completed first makeup round.
  } finally {
    rmSync(parent, { recursive: true, force: true });
  }
});
