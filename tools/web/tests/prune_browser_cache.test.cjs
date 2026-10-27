const {test} = require('node:test');
const assert = require('node:assert/strict');
const prune = require('../prune_browser_cache.cjs');
const context = {repo:{owner:'owner',repo:'game'},payload:{action:'closed',pull_request:{number:13}}};
const ref='refs/pull/13/merge';
const core={info(){}};

test('only this closed PR browser caches are deleted, after all pages are collected',async()=>{
 const pages=[Array.from({length:100},(_,id)=>({id,key:'native-cache',ref})),[
  {id:101,key:'browser-rust-v1-ubuntu24',ref},
  {id:102,key:'browser-rust-v1-ubuntu24',ref:'refs/heads/main'},
  {id:103,key:'browser-rust-v1-ubuntu24',ref:'refs/pull/14/merge'},
  {id:104,key:'release-cache',ref},
 ]];
 pages[0][3]={id:3,key:'browser-rust-v1-ubuntu24',ref};
 const reads=[],deleted=[];
 const github={async request(route,args){
  assert.equal(args.owner,'owner');assert.equal(args.repo,'game');
  if(route.startsWith('GET')){assert.equal(args.ref,ref);reads.push(args.page);return{data:{actions_caches:pages[args.page-1]}};}
  assert.deepEqual(reads,[1,2]);deleted.push(args.cache_id);
 }};
 assert.equal(await prune({github,context,core}),2);
 assert.deepEqual(deleted,[3,101]);
});

test('invalid PR identity cannot make API calls',async()=>{
 const github={request(){assert.fail('unexpected API call');}};
 await assert.rejects(()=>prune({github,context:{...context,payload:{action:'closed',pull_request:{number:'13'}}},core}),/PR number/);
 await assert.rejects(()=>prune({github,context:{...context,payload:{...context.payload,action:'opened'}},core}),/closed PR/);
});

test('a concurrently deleted cache is harmless, while permission failures propagate',async()=>{
 const github={async request(route){
  if(route.startsWith('GET'))return{data:{actions_caches:[{id:1,key:'browser-rust-v1-x',ref}]}};
  throw Object.assign(new Error('gone'),{status:404});
 }};
 assert.equal(await prune({github,context,core}),0);
 github.request=async route=>{
  if(route.startsWith('GET'))return{data:{actions_caches:[{id:1,key:'browser-rust-v1-x',ref}]}};
  throw Object.assign(new Error('denied'),{status:403});
 };
 await assert.rejects(()=>prune({github,context,core}),/denied/);
});
