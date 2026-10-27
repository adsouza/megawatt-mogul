import {test,expect} from '@playwright/test';
import {execFileSync} from 'node:child_process';
import {mkdtempSync,rmSync,readFileSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'../..');
const tool=resolve(root,'target/debug/examples/save_backup');
const fixture=resolve(root,'crates/infrastructure/tests/support/monthly-directory-v4/cccccccccccccccccccccccccccccccc.mgsave');
async function settled(page){await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))));}
async function service(page){await page.evaluate(async()=>{
 const module=document.querySelector('link[rel=modulepreload]').href;
 const source=await(await fetch(module)).text();const path=source.match(/from ['"]([^'"]+browser_storage.js)['"]/)[1];
 window.backupService=await import(new URL(path,module));
});}

test('native backup crosses the worker boundary, keeps every state hash, and imports into fresh slots',async({page})=>{
 const directory=mkdtempSync(join(tmpdir(),'mm-backup-browser-'));
 try{
  const input=join(directory,'native.mgsave.zip');execFileSync(tool,['export',fixture,input]);
  const before=execFileSync(tool,['validate',input],{encoding:'utf8'});
  await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-ready','true',{timeout:150000});await service(page);
  const output=await page.evaluate(async input=>{
   const s=window.backupService, results=[];
   for(const slot of ['d'.repeat(32),'e'.repeat(32)]){
    const archive=await s.codec('import',{bytes:new Uint8Array(input),slot});
    await s.mutate('write',slot,null,archive,archive.card);
    const zip=await s.codec('export',await s.read(slot));results.push([...zip]);
   }
   return {results,count:(await s.list()).length};
  },[...readFileSync(input)]);
  expect(output.count).toBe(2);
  for(const [index,bytes] of output.results.entries()){
   const path=join(directory,`browser-${index}.mgsave.zip`);writeFileSync(path,Buffer.from(bytes));
   expect(execFileSync(tool,['validate',path],{encoding:'utf8'})).toBe(before);
   const native=join(directory,`imported-${index}`);const id=execFileSync(tool,['import',path,native],{encoding:'utf8'}).trim();
   const returned=join(directory,`returned-${index}.mgsave.zip`);execFileSync(tool,['export',join(native,`${id}.mgsave`),returned]);
   expect(execFileSync(tool,['validate',returned],{encoding:'utf8'})).toBe(before);
  }
  const failure=await page.evaluate(async()=>{
   try{await window.backupService.codec('import',{bytes:new Uint8Array([1,2,3]),slot:'f'.repeat(32)});return null;}catch(e){return {kind:e.kind,count:(await window.backupService.list()).length};}
  });expect(failure).toEqual({kind:'Malformed',count:2});
  await expect(page.locator('body')).toHaveAttribute('data-ready','true');
 }finally{rmSync(directory,{recursive:true,force:true});}
});

test('library downloads a backup and uploads it into a new slot; malformed and canceled uploads preserve saves',async({page})=>{
 const directory=mkdtempSync(join(tmpdir(),'mm-backup-controls-'));
 try{
  const input=join(directory,'native.mgsave.zip');execFileSync(tool,['export',fixture,input]);
  const baseline=execFileSync(tool,['validate',input],{encoding:'utf8'});
  await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-ready','true',{timeout:150000});await service(page);
  await page.evaluate(async bytes=>{
   const s=window.backupService,slot='d'.repeat(32),archive=await s.codec('import',{bytes:new Uint8Array(bytes),slot});
   await s.mutate('write',slot,null,archive,archive.card);
  },[...readFileSync(input)]);
  await page.reload();await expect(page.locator('body')).toHaveAttribute('data-app-state','MainMenu',{timeout:150000});
  await page.mouse.click(230,423);await expect(page.locator('body')).toHaveAttribute('data-save-screen','Browser');
  await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');await settled(page);
  const downloadEvent=page.waitForEvent('download');await page.mouse.click(745,390);const download=await downloadEvent;
  expect(download.suggestedFilename()).toBe(`${'d'.repeat(32)}.mgsave.zip`);
  const output=join(directory,'download.mgsave.zip');await download.saveAs(output);
  expect(execFileSync(tool,['validate',output],{encoding:'utf8'})).toBe(baseline);
  await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');await settled(page);
  const chooserEvent=page.waitForEvent('filechooser');await page.mouse.click(216,734);const chooser=await chooserEvent;await chooser.setFiles(output);
  await expect(page.locator('body')).toHaveAttribute('data-save-count','2',{timeout:30000});
  await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');await expect(page.locator('body')).toHaveAttribute('data-save-error','false');await settled(page);
  const invalidEvent=page.waitForEvent('filechooser');await page.mouse.click(216,734);const invalid=await invalidEvent;
  await invalid.setFiles({name:'broken.mgsave.zip',mimeType:'application/zip',buffer:Buffer.from([1,2,3])});
  await expect(page.locator('body')).toHaveAttribute('data-save-error','true');await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');
  await expect(page.locator('body')).toHaveAttribute('data-save-count','2');await expect(page.locator('body')).toHaveAttribute('data-app-state','MainMenu');
  await page.keyboard.press('Escape');await expect(page.locator('body')).toHaveAttribute('data-save-screen','Closed');await settled(page);
  await page.mouse.click(230,423);await expect(page.locator('body')).toHaveAttribute('data-save-screen','Browser');await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');await settled(page);
  const cancelEvent=page.waitForEvent('filechooser');await page.mouse.click(216,734);const canceled=await cancelEvent;
  await canceled.element().evaluate(input=>input.dispatchEvent(new Event('cancel')));
  await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');await expect(page.locator('body')).toHaveAttribute('data-save-error','false');await expect(page.locator('body')).toHaveAttribute('data-save-count','2');
  await page.keyboard.press('Escape');await expect(page.locator('body')).toHaveAttribute('data-save-screen','Closed');await settled(page);
  await page.mouse.click(230,423);await expect(page.locator('body')).toHaveAttribute('data-save-screen','Browser');await expect(page.locator('body')).toHaveAttribute('data-save-busy','false');await settled(page);
  await page.mouse.click(745,346);await expect(page.locator('body')).toHaveAttribute('data-app-state','InGame',{timeout:30000});
  await expect(page.locator('body')).toHaveAttribute('data-sim-tick','900');await expect(page.locator('body')).toHaveAttribute('data-speed','0');
 }finally{rmSync(directory,{recursive:true,force:true});}
});

test('file selection rejects oversized input before reading bytes',async({page})=>{
 await page.goto('./');await expect(page.locator('body')).toHaveAttribute('data-ready','true',{timeout:150000});await service(page);
 await page.evaluate(()=>{File.prototype.arrayBuffer=()=>{throw new Error('must not read oversized file');};});
 const event=page.waitForEvent('filechooser');
 const result=page.evaluate(()=>window.backupService.chooseBackup(2).then(()=>null,error=>error.message));
 const chooser=await event;await chooser.setFiles({name:'oversized.mgsave.zip',mimeType:'application/zip',buffer:Buffer.from([1,2,3])});
 expect(await result).toBe('BackupTooLarge');
 await expect(page.locator('body')).toHaveAttribute('data-ready','true');
});
