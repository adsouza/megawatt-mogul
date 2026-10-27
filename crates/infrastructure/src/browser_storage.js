// IndexedDB request chaining stays entirely inside synchronous event callbacks.
// All compression, hashing and other asynchronous work happens before the write transaction.
const DATABASE='megawatt-mogul';
let deploymentBuild='development';
function open() {
  return new Promise((resolve,reject)=>{
    const request=indexedDB.open(DATABASE,1);
    request.onupgradeneeded=()=>{
      const db=request.result;
      for(const name of ['meta','cards','slots','charts','preferences']) db.createObjectStore(name);
      request.transaction.objectStore('meta').put({version:1,build:deploymentBuild},'schema');
    };
    let blocked=false;
    request.onblocked=()=>{blocked=true;reject(new Error('DatabaseBlocked'));};
    request.onerror=()=>reject(request.error);
    request.onsuccess=()=>{
      const db=request.result;
      if(blocked){db.close();return;}
      db.onversionchange=()=>db.close();
      resolve(db);
    };
  });
}
function transaction(db,stores,mode,work) {
  return new Promise((resolve,reject)=>{
    let tx;
    try{ tx=db.transaction(stores,mode,{durability:'strict'}); }
    catch(error){ if(error instanceof TypeError) tx=db.transaction(stores,mode); else {db.close();reject(error);return;} }
    let result, failure;
    const fail=error=>{failure=error;try{tx.abort();}catch{ /* already finished */ }};
    tx.onabort=()=>{db.close();reject(failure||tx.error||new Error('TransactionAborted'));};
    tx.onerror=()=>{}; // Abort owns error reporting; a request's success is never a receipt.
    tx.oncomplete=()=>{db.close();resolve(result);};
    try{work(tx,value=>{result=value;},fail);}catch(error){fail(error);}
  });
}
function requestValue(db,store,key) {
  return transaction(db,[store],'readonly',(tx,done)=>{
    const request=tx.objectStore(store).get(key);request.onsuccess=()=>done(request.result);
  });
}
function guard(fail,callback){return (...args)=>{try{return callback(...args);}catch(error){fail(error);}};}
function range(id){return IDBKeyRange.bound([id,''],[id,'\uffff']);}
function equal(a,b){return a===b||!!a&&!!b&&a.length===b.length&&a.every((v,i)=>v===b[i]);}
async function digest(bytes) {
  return [...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))].map(v=>v.toString(16).padStart(2,'0')).join('');
}
export async function list() {
  return transaction(await open(),['cards'],'readonly',(tx,done)=>{
    const request=tx.objectStore('cards').getAll();request.onsuccess=()=>done(request.result);
  });
}
export async function read(id) {
  const result=await transaction(await open(),['slots','charts'],'readonly',(tx,done,fail)=>{
    const request=tx.objectStore('slots').get(id);
    request.onsuccess=guard(fail,()=>{
      const slot=request.result;
      if(!slot){fail(new Error('SlotChanged'));return;}
      if(slot.schema!==1){fail(new Error('NewerSchema'));return;}
      const files=[];
      const cursor=tx.objectStore('charts').openCursor(range(id));
      cursor.onsuccess=()=>{
        const row=cursor.result;
        if(row){files.push([row.key[1],row.value]);row.continue();}
        else done({manifest:slot.manifest,files});
      };
    });
  });
  result.revision=await digest(result.manifest);
  return result;
}
export async function revision(id) {
  const slot=await requestValue(await open(),'slots',id);
  if(!slot)throw new Error('SlotChanged');
  if(slot.schema!==1)throw new Error('NewerSchema');
  return digest(slot.manifest);
}
export async function mutate(operation,id,expected,archive,entry,abortForTest=false) {
  // Hash the selected source before opening a readwrite transaction. Within that transaction,
  // byte equality against this source closes the race with any other window's publication.
  const before=await requestValue(await open(),'slots',id);
  if(before&&before.schema!==1)throw new Error('NewerSchema');
  const actual=before?await digest(before.manifest):null;
  if(operation==='delete'&&!before)return;
  if(expected!==actual)throw new Error('SlotChanged');
  const next=operation==='write'?await digest(archive.manifest):null;
  return transaction(await open(),['cards','slots','charts'],'readwrite',(tx,done,fail)=>{
    const slots=tx.objectStore('slots');
    const request=slots.get(id);
    request.onsuccess=guard(fail,()=>{
      const current=request.result;
      if(!!current!==!!before || current&&(!equal(current.manifest,before.manifest)||current.schema!==1)) {
        fail(new Error('SlotChanged'));return;
      }
      const cards=tx.objectStore('cards'),charts=tx.objectStore('charts');
      const card=cards.get(id);
      card.onsuccess=guard(fail,()=>{
      if(card.result&&card.result.schema!==1){fail(new Error('NewerSchema'));return;}
      if(operation==='delete'){slots.delete(id);cards.delete(id);}
      else {
        slots.put({schema:1,manifest:archive.manifest,revision:next},id);
        cards.put({id,schema:1,entry,revision:next},id);
        for(const [name,bytes] of archive.files){
          const existing=charts.get([id,name]);
          existing.onsuccess=guard(fail,()=>{if(!equal(existing.result,bytes))charts.put(bytes,[id,name]);});
        }
      }
      const keep=new Set(operation==='write'?archive.files.map(([name])=>name):[]);
      const cursor=charts.openKeyCursor(range(id));
      cursor.onsuccess=()=>{
        const row=cursor.result;
        if(row){if(!keep.has(row.key[1]))charts.delete(row.key);row.continue();}
      };
      done(next);
      if(abortForTest)fail(new Error('InjectedAbort'));
      });
    });
  });
}
export async function preferenceRead(){return (await requestValue(await open(),'preferences','client'))||null;}
export async function preferenceWrite(bytes){
  return transaction(await open(),['preferences'],'readwrite',tx=>{tx.objectStore('preferences').put(bytes,'client');});
}
export async function requestPersistence(){return navigator.storage?.persist?await navigator.storage.persist():false;}
export function randomId(){return crypto.randomUUID().replaceAll('-','');}

let prepared;
export async function prepareCodec(build){
  deploymentBuild=build;
  if(prepared)return;
  const moduleUrl=document.querySelector('link[rel="modulepreload"]')?.href;
  const wasmUrl=document.querySelector('link[rel="preload"][type="application/wasm"]')?.href;
  async function fetchChecked(url){const response=await fetch(url);if(!response.ok)throw new Error('DeploymentChanged');return response;}
  let source=await(await fetchChecked(moduleUrl)).text();
  const imports=[...source.matchAll(/from\s*['"](\.\/[^'"]+)['"]/g)].map(match=>match[1]);
  const rewritten=new Map();
  for(const path of new Set(imports)){
    const text=await(await fetchChecked(new URL(path,moduleUrl))).text();
    const blob=URL.createObjectURL(new Blob([text],{type:'text/javascript'}));
    rewritten.set(path,blob);
  }
  // Change ES module sources only: WASM's import-object keys must retain their original names.
  source=source.replace(/(from\s*['"])(\.\/[^'"]+)(['"])/g,(_,prefix,path,suffix)=>prefix+rewritten.get(path)+suffix);
  const module=URL.createObjectURL(new Blob([source],{type:'text/javascript'}));
  const workerSource=await(await fetchChecked(new URL('codec-worker.js',location.href))).text();
  const worker=URL.createObjectURL(new Blob([workerSource],{type:'text/javascript'}));
  const wasm=await WebAssembly.compile(await(await fetchChecked(wasmUrl)).arrayBuffer());
  prepared={module,worker,wasm};
}
function buffers(value,result=new Set()){
  if(value instanceof Uint8Array)result.add(value.buffer);
  else if(Array.isArray(value))for(const child of value)buffers(child,result);
  else if(value&&typeof value==='object')for(const child of Object.values(value))buffers(child,result);
  return [...result];
}
export function codec(operation,payload){
  return new Promise((resolve,reject)=>{
    if(!prepared){reject(new Error('CodecNotInitialized'));return;}
    const worker=new Worker(prepared.worker,{type:'module'});
    const timeout=setTimeout(()=>finish(new Error('CodecTimeout')),180000);
    function finish(error,value){clearTimeout(timeout);worker.terminate();if(error)reject(error);else resolve(value);}
    worker.onerror=event=>{event.preventDefault();finish(new Error(event.message||'CodecWorkerFailed'));};
    worker.onmessage=event=>event.data.error?finish(Object.assign(new Error(event.data.error.message||String(event.data.error)),event.data.error)):finish(null,event.data.value);
    worker.postMessage({operation,payload,module:prepared.module,wasm:prepared.wasm},buffers(payload));
  });
}
