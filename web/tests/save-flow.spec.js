import {test,expect} from '@playwright/test';
async function settled(page){await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))));}


test('manual save submits with Enter, survives reload, and resumes through the library',async({page})=>{
 const errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto('./');
 await expect(page.locator('body')).toHaveAttribute('data-app-state','RegionSelect',{timeout:150000});
 await page.mouse.click(860,420);
 await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame');
 // Codec workers retain their boot-time code and compiled module across a deployment.
 for(const pattern of ['**/*.wasm','**/game-*.js','**/codec-worker.js','**/snippets/**'])
   await page.route(pattern,route=>route.fulfill({status:404,body:''}));
 await page.mouse.click(1345,40);
 await expect(page.locator('body')).toHaveAttribute('data-save-screen','Save');
 await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');
 await settled(page);
 await page.mouse.click(650,434);
 await page.keyboard.type('Browser acceptance',{delay:100});
 await page.keyboard.press('Enter');
 await expect(page.locator('body')).toHaveAttribute('data-save-screen','Closed',{timeout:30000});
 await expect(page.locator('body')).toHaveAttribute('data-save-error','false');
 await page.unrouteAll();
 await page.reload();
 await expect(page.locator('body')).toHaveAttribute('data-app-state','MainMenu',{timeout:150000});
 await expect(page.locator('body')).toHaveAttribute('data-save-count','1');
 const cards=await page.evaluate(async()=>{
   const request=indexedDB.open('megawatt-mogul',1);const db=await new Promise(resolve=>{request.onsuccess=()=>resolve(request.result);});
   const values=await new Promise(resolve=>{const request=db.transaction('cards').objectStore('cards').getAll();request.onsuccess=()=>resolve(request.result);});db.close();return values;
 });
 expect(cards[0].entry).toContain('name:"Browser acceptance"');
 await page.mouse.click(230,423);
 await expect(page.locator('body')).toHaveAttribute('data-save-screen','Browser');
 await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');
 await settled(page);
 await page.mouse.click(745,346);
 await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame',{timeout:30000});
 await expect(page.locator('body')).toHaveAttribute('data-save-screen','Closed');
 await expect(page.locator('body')).toHaveAttribute('data-speed','0');
 await expect(page.locator('body')).toHaveAttribute('data-sim-tick','0');
 expect(errors).toEqual([]);
});

test('monthly autosave retains Review and browser speed preferences across reload',async({page})=>{
 await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-app-state','RegionSelect',{timeout:150000});
 await page.mouse.click(860,420);await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame');await settled(page);
 await page.mouse.click(1390,672);await expect(page.locator('body')).toHaveAttribute('data-speed','1');await settled(page);
 await page.mouse.click(234,64);await expect(page.locator('body')).toHaveAttribute('data-speed','32');
 await expect(page.locator('body')).toHaveAttribute('data-sim-tick','1440',{timeout:45000});
 await expect(page.locator('body')).toHaveAttribute('data-save-busy','false',{timeout:30000});
 await expect(page.locator('body')).toHaveAttribute('data-save-screen','Closed');
 await expect(page.locator('body')).toHaveAttribute('data-save-error','false');
 await page.reload();await expect(page.locator('body')).toHaveAttribute('data-app-state','MainMenu',{timeout:150000});
 await expect(page.locator('body')).toHaveAttribute('data-save-count','1');await settled(page);
 await page.keyboard.press('Enter');
 await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame',{timeout:30000});
 await expect(page.locator('body')).toHaveAttribute('data-sim-tick','1440');await expect(page.locator('body')).toHaveAttribute('data-speed','0');
 const preference=await page.evaluate(async()=>{
  const request=indexedDB.open('megawatt-mogul',1);const db=await new Promise(resolve=>{request.onsuccess=()=>resolve(request.result);});
  const value=await new Promise(resolve=>{const request=db.transaction('preferences').objectStore('preferences').get('client');request.onsuccess=()=>resolve(request.result);});db.close();return new TextDecoder().decode(value);
 });
 expect(preference).toContain('speed:32');
});

test('a hidden-page visibility signal pauses play and returning stays paused',async({page})=>{
 await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-app-state','RegionSelect',{timeout:150000});
 await page.mouse.click(860,420);await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame');await settled(page);
 await page.mouse.click(1390,672);await expect(page.locator('body')).toHaveAttribute('data-speed','1');
 const before=Number(await page.locator('body').getAttribute('data-sim-tick'));
 // Headless Chromium reports every tab visible; inject the browser signal into the real game.
 await page.evaluate(()=>Object.defineProperty(document,'hidden',{configurable:true,get:()=>true}));
 await expect(page.locator('body')).toHaveAttribute('data-speed','0');
 await page.evaluate(()=>{delete document.hidden;});await settled(page);
 await expect(page.locator('body')).toHaveAttribute('data-speed','0');
 expect(Number(await page.locator('body').getAttribute('data-sim-tick'))).toBeLessThanOrEqual(before+4);
});

test('unavailable storage reports a save error and keeps an unsaved session playable',async({page})=>{
 await page.addInitScript(()=>{
  IDBFactory.prototype.open=function(){throw new DOMException('Site storage is disabled','SecurityError');};
  if(navigator.storage){navigator.storage.persist=async()=>false;navigator.storage.persisted=async()=>false;}
 });
 await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-app-state','MainMenu',{timeout:150000});await settled(page);
 await page.mouse.click(230,412);await expect(page.locator('body')).toHaveAttribute('data-app-state','RegionSelect');await settled(page);
 await page.mouse.click(860,420);await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame');await settled(page);
 await page.mouse.click(1345,40);await expect(page.locator('body')).toHaveAttribute('data-save-screen','Save');await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');
 await expect(page.locator('body')).toHaveAttribute('data-save-error','true');
 await page.keyboard.press('Escape');await expect(page.locator('body')).toHaveAttribute('data-save-screen','Closed');
 await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame');await expect(page.locator('body')).toHaveAttribute('data-sim-tick','0');
 await expect(page.locator('#preview')).toContainText('Automatic storage cleanup may remove saves');
});
