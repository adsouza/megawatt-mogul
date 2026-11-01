import { test, expect } from '@playwright/test';

test('dismissing the storage notice reclaims space and survives reload', async ({page}) => {
  await page.goto('./');
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', {timeout:150000});
  const before = await page.locator('#surface').boundingBox();
  await page.getByRole('button', {name:'Dismiss save notice'}).focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#save-notice')).toBeHidden();
  await expect.poll(async () => (await page.locator('#surface').boundingBox()).height)
    .toBeGreaterThan(before.height);
  await page.reload();
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', {timeout:150000});
  await expect(page.locator('#save-notice')).toBeHidden();
});

test('an undersized window shows dimensions and defers startup until it fits', async ({page}) => {
  await page.setViewportSize({width:1000,height:700});
  const assets = [];
  page.on('request', request => { if (request.url().includes('assets/manifest.ron')) assets.push(request.url()); });
  await page.goto('./');
  await page.waitForFunction(() => window.wasmBindings);
  await expect(page.locator('#viewport-warning')).toBeVisible();
  await expect(page.locator('#viewport-warning')).toContainText('1000 × 700');
  await expect(page.locator('#viewport-warning')).toContainText('1280 × 640');
  const screen = await page.evaluate(() => `${window.screen.width} × ${window.screen.height}`);
  await expect(page.locator('#viewport-warning')).toContainText(screen);
  await expect(page.locator('#game')).toBeHidden();
  await expect(page.locator('#status')).toBeHidden();
  await expect(page.locator('body')).not.toHaveAttribute('data-ready', 'true');
  expect(assets).toEqual([]);
  await page.setViewportSize({width:1440,height:900});
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', {timeout:150000});
  await expect(page.locator('#viewport-warning')).toBeHidden();
  const surface = await page.locator('#surface').boundingBox();
  await page.setViewportSize({width:1280,height:640 + surface.y});
  await expect(page.locator('#viewport-warning')).toBeHidden();
  await page.setViewportSize({width:1280,height:639 + surface.y});
  await expect(page.locator('#viewport-warning')).toBeVisible();
  await page.setViewportSize({width:1440,height:900});
  await page.getByRole('button', {name:'Dismiss save notice'}).click();
  await page.setViewportSize({width:1280,height:640});
  await expect(page.locator('#game')).toBeVisible();
  await expect(page.locator('#viewport-warning')).toBeHidden();
  await page.setViewportSize({width:1279,height:640});
  await expect(page.locator('#viewport-warning')).toContainText('1279 × 640');
  await expect(page.locator('#game')).toBeHidden();
  await expect(page.locator('body')).toHaveAttribute('data-speed', '0');
  await page.setViewportSize({width:1280,height:639});
  await expect(page.locator('#viewport-warning')).toContainText('1280 × 639');
  await page.setViewportSize({width:1280,height:640});
  await expect(page.locator('#game')).toBeVisible();
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true');
});

test('shrinking an operating game pauses it and restores the same session', async ({page}) => {
  await page.goto('./');
  await expect(page.locator('body')).toHaveAttribute('data-app-state', 'RegionSelect', {timeout:150000});
  const settled = () => page.evaluate(() => new Promise(resolve =>
    requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(resolve)))));
  await settled();
  await page.mouse.click(296,130);
  await expect(page.locator('body')).toHaveAttribute('data-tutorial-pending', 'false');
  await page.mouse.click(860,420);
  await expect(page.locator('body')).toHaveAttribute('data-app-state', 'InGame');
  await settled();
  await page.mouse.click(1390,672);
  await expect(page.locator('body')).toHaveAttribute('data-speed', '1');
  await page.setViewportSize({width:1000,height:700});
  await expect(page.locator('#game')).toBeHidden();
  await expect(page.locator('body')).toHaveAttribute('data-speed', '0');
  const tick = await page.locator('body').getAttribute('data-sim-tick');
  await settled();
  await expect(page.locator('body')).toHaveAttribute('data-sim-tick', tick);
  await page.setViewportSize({width:1440,height:900});
  await expect(page.locator('#game')).toBeVisible();
  await expect(page.locator('body')).toHaveAttribute('data-app-state', 'InGame');
  await expect(page.locator('body')).toHaveAttribute('data-speed', '0');
  await expect(page.locator('body')).toHaveAttribute('data-sim-tick', tick);
});

test('the storage notice can be dismissed when browser preferences are blocked', async ({page}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'localStorage', {get() { throw new DOMException('Blocked', 'SecurityError'); }});
  });
  await page.goto('./');
  await page.getByRole('button', {name:'Dismiss save notice'}).click();
  await expect(page.locator('#save-notice')).toBeHidden();
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', {timeout:150000});
});
