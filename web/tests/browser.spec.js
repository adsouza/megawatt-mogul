import { test, expect } from '@playwright/test';
test('real browser client boots at the Pages project subpath', async ({page}) => {
  const errors=[];
  page.on('pageerror', e=>errors.push(e.message));
  await page.goto('./');
  await expect(page.locator('body')).toHaveAttribute('data-ready','true', {timeout:150000});
  await expect(page.locator('#game')).toBeVisible();
  await expect(page.locator('#preview')).toContainText('Saved games stay in this browser');
  await expect(page.locator('#status')).toBeHidden();
  expect(errors).toEqual([]);
});

test('IndexedDB retains bytes across reload and rejects an inactive transaction', async ({page}) => {
  await page.goto('./');
  await page.evaluate(async () => {
    const open = indexedDB.open('megawatt-mogul-probe',1);
    open.onupgradeneeded=()=>open.result.createObjectStore('data');
    const db=await new Promise((resolve,reject)=>{open.onsuccess=()=>resolve(open.result);open.onerror=()=>reject(open.error);});
    await new Promise((resolve,reject)=>{
      const tx=db.transaction('data','readwrite');
      tx.objectStore('data').put(new Uint8Array([1,2,3]),'save');
      tx.oncomplete=resolve;tx.onabort=()=>reject(tx.error);
    });
    db.close();
  });
  await page.reload();
  const value=await page.evaluate(async()=>{
    const open=indexedDB.open('megawatt-mogul-probe',1);
    const db=await new Promise((resolve,reject)=>{open.onsuccess=()=>resolve(open.result);open.onerror=()=>reject(open.error);});
    const bytes=await new Promise((resolve,reject)=>{const request=db.transaction('data').objectStore('data').get('save');request.onsuccess=()=>resolve([...request.result]);request.onerror=()=>reject(request.error);});
    const tx=db.transaction('data','readwrite');
    const store=tx.objectStore('data');
    await new Promise(resolve=>{tx.oncomplete=resolve;});
    let error;
    try {store.put('late','other');} catch(e) {error=e.name;}
    db.close(); return {bytes,error};
  });
  expect(value).toEqual({bytes:[1,2,3],error:'TransactionInactiveError'});
});

test('all eight regions complete a day, restore, and continue through backups on WASM', async ({page}) => {
  await page.goto('./');
  await expect(page.locator('body')).toHaveAttribute('data-ready','true',{timeout:150000});
  const metrics=await page.evaluate(async()=>{
    const inventory=await(await fetch('assets/manifest.ron')).text();
    const rows=[...inventory.matchAll(/name:"regions\/([^"]+)\.bundle",url:"([^"]+)"/g)];
    const result=[];
    for(const row of rows) {
      const bytes=new Uint8Array(await(await fetch('assets/'+row[2])).arrayBuffer());
      result.push({region:row[1],timings:window.wasmBindings.browser_probe(row[1],bytes,1)});
    }
    return result;
  });
  expect(metrics).toHaveLength(8);
  for(const row of metrics) expect(row.timings[5]).toBeGreaterThan(0);
  console.log('WASM region measurements',JSON.stringify(metrics));
  if(await page.locator('body').getAttribute('data-app-state')==='MainMenu') await page.mouse.click(230,370);
  await expect(page.locator('body')).toHaveAttribute('data-app-state','RegionSelect');
  await page.mouse.click(860,420);
  await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame');
});

test('a stale page with a removed module offers reload before WASM starts', async ({page}) => {
  await page.route('**/game-*.js', route=>route.fulfill({status:404,body:'',contentType:'text/javascript'}));
  await page.goto('./');
  await expect(page.locator('#reload')).toBeVisible();
  await expect(page.locator('#status')).toContainText('Reload to retry');
  await expect(page.locator('body')).not.toHaveAttribute('data-ready','true');
});

test('a codec worker rejects malformed data without breaking the running client',async({page})=>{
 await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-ready','true',{timeout:150000});
 const failure=await page.evaluate(async()=>{
  const module=document.querySelector('link[rel=modulepreload]').href;
  const source=await(await fetch(module)).text();const path=source.match(/from ['"]([^'"]+browser_storage.js)['"]/)[1];
  const service=await import(new URL(path,module));
  try{await service.codec('decode',{manifest:new Uint8Array([1]),files:[]});return null;}catch(error){return {kind:error.kind,message:error.message};}
 });
 expect(failure.kind).toBe('Malformed');await expect(page.locator('body')).toHaveAttribute('data-ready','true');
 await expect(page.locator('#status')).toBeHidden();
});
