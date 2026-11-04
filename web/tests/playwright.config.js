import { defineConfig } from '@playwright/test';
export default defineConfig({
  timeout: 180000, workers: 1,
  // Split individual tests between CI runners; each runner still renders only one game at a time.
  fullyParallel: true,
  forbidOnly: true,
  use: { baseURL: process.env.WEB_BASE_URL || 'http://127.0.0.1:8765/megawatt-mogul/',
    viewport: {width:1440,height:900}, screenshot:'only-on-failure',
    launchOptions: {args:['--use-gl=angle','--use-angle=swiftshader','--enable-unsafe-swiftshader']} },
});
