import {test,expect} from '@playwright/test';
import fs from 'node:fs';
const source=fs.readFileSync(new URL('../../crates/infrastructure/src/browser_storage.js',import.meta.url),'utf8');
test.beforeEach(async({page})=>{
  await page.goto('./');
  await page.evaluate(async source=>{
    window.dbAdapter=await import(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));
  },source);
});
test('publication, revision conflicts and deletion are atomic across tabs',async({page,context})=>{
 const revision=await page.evaluate(()=>window.dbAdapter.mutate('write','slot',null,{manifest:new Uint8Array([1]),files:[['month',new Uint8Array([2])]]},'first'));
 await page.reload();
 await page.evaluate(async source=>{window.dbAdapter=await import(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));},source);
 expect(await page.evaluate(()=>window.dbAdapter.list())).toEqual([{id:'slot',schema:1,entry:'first',revision}]);
 const other=await context.newPage();await other.goto('./');await other.evaluate(async source=>{window.dbAdapter=await import(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));},source);
 await other.evaluate(revision=>window.dbAdapter.mutate('write','slot',revision,{manifest:new Uint8Array([3]),files:[['newmonth',new Uint8Array([4])]]},'second'),revision);
 expect(await page.evaluate(async revision=>{try{await window.dbAdapter.mutate('delete','slot',revision);return 'wrong';}catch(e){return e.message;}},revision)).toBe('SlotChanged');
 expect(await page.evaluate(async()=>{const value=await window.dbAdapter.read('slot');return [value.manifest[0],value.files.map(([name,bytes])=>[name,[...bytes]])];})).toEqual([3,[['newmonth',[4]]]]);
});
test('failed publication preserves cards, manifest and chart parts',async({page})=>{
 const result=await page.evaluate(async()=>{
  const {mutate,read,list}=window.dbAdapter;
  const rev=await mutate('write','slot',null,{manifest:new Uint8Array([1]),files:[['month',new Uint8Array([2])]]},'first');
  let failed=false;
  try{await mutate('write','slot',rev,{manifest:new Uint8Array([3]),files:[['newmonth',new Uint8Array([4])]]},'second',true);}catch(e){failed=true;}
  const value=await read('slot');return {failed,cards:await list(),manifest:[...value.manifest],parts:value.files.map(([name,bytes])=>[name,[...bytes]])};
 });
 expect(result.failed).toBe(true);expect(result.cards[0].entry).toBe('first');expect(result.manifest).toEqual([1]);expect(result.parts).toEqual([['month',[2]]]);
});

test('listing reads only metadata, and unknown newer cards cannot be overwritten',async({page})=>{
 const result=await page.evaluate(async()=>{
  const {mutate,list}=window.dbAdapter;
  const rev=await mutate('write','slot',null,{manifest:new Uint8Array([1]),files:[['month',new Uint8Array([2])]]},'first');
  const original=IDBDatabase.prototype.transaction;const stores=[];
  IDBDatabase.prototype.transaction=function(names,...rest){stores.push(names);return original.call(this,names,...rest);};
  await list();IDBDatabase.prototype.transaction=original;
  const request=indexedDB.open('megawatt-mogul',1);
  const db=await new Promise((resolve,reject)=>{request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);});
  await new Promise((resolve,reject)=>{const tx=db.transaction('cards','readwrite');tx.objectStore('cards').put({id:'slot',schema:2,entry:'future',revision:rev},'slot');tx.oncomplete=resolve;tx.onabort=()=>reject(tx.error);});db.close();
  let error;try{await mutate('delete','slot',rev);}catch(e){error=e.message;}
  return {stores,error,cards:await list()};
 });
 expect(result.stores).toEqual([['cards']]);expect(result.error).toBe('NewerSchema');expect(result.cards[0].entry).toBe('future');
});

test('quota failure after requests were queued rolls the whole publication back',async({page})=>{
 const errors=[];page.on('pageerror',error=>errors.push(error.message));
 const result=await page.evaluate(async()=>{
  const {mutate,read,list}=window.dbAdapter;
  const revision=await mutate('write','slot',null,{manifest:new Uint8Array([1]),files:[['month',new Uint8Array([2])]]},'first');
  const original=IDBObjectStore.prototype.put;
  IDBObjectStore.prototype.put=function(...args){if(this.name==='charts')throw new DOMException('Injected quota failure','QuotaExceededError');return original.apply(this,args);};
  let error;try{await mutate('write','slot',revision,{manifest:new Uint8Array([3]),files:[['newmonth',new Uint8Array([4])]]},'second');}catch(e){error=e.name;}finally{IDBObjectStore.prototype.put=original;}
  const data=await read('slot');return {error,cards:await list(),manifest:[...data.manifest],files:data.files.map(([name,bytes])=>[name,[...bytes]])};
 });
 expect(result.error).toBe('QuotaExceededError');expect(result.cards[0].entry).toBe('first');expect(result.manifest).toEqual([1]);expect(result.files).toEqual([['month',[2]]]);expect(errors).toEqual([]);
});

test('an older build refuses a newer database without deleting its saves',async({page})=>{
 const result=await page.evaluate(async()=>{
  const {mutate,list}=window.dbAdapter;
  await mutate('write','slot',null,{manifest:new Uint8Array([1]),files:[]},'first');
  const upgrade=indexedDB.open('megawatt-mogul',2);
  const newer=await new Promise((resolve,reject)=>{upgrade.onsuccess=()=>resolve(upgrade.result);upgrade.onerror=()=>reject(upgrade.error);});newer.close();
  let error;try{await list();}catch(e){error=e.name;}
  const request=indexedDB.open('megawatt-mogul');
  const db=await new Promise(resolve=>{request.onsuccess=()=>resolve(request.result);});
  const value=await new Promise(resolve=>{const request=db.transaction('slots').objectStore('slots').get('slot');request.onsuccess=()=>resolve(request.result);});db.close();
  return {error,manifest:[...value.manifest]};
 });
 expect(result).toEqual({error:'VersionError',manifest:[1]});
});
