// The same build's isolated WASM instance performs codecs without owning the live game.
self.onmessage=async({data})=>{
  try {
    const bindings=await import(data.module);
    await bindings.default({module_or_path:data.wasm});
    const value=bindings.browser_codec(data.operation,data.payload);
    const transfer=new Set();
    function buffers(value){if(value instanceof Uint8Array)transfer.add(value.buffer);else if(Array.isArray(value))value.forEach(buffers);else if(value&&typeof value==='object')Object.values(value).forEach(buffers);}
    buffers(value);self.postMessage({value},[...transfer]);
  } catch(error) { self.postMessage({error:error&&typeof error==='object' ? {kind:error.kind||'Browser',message:error.message||String(error),...error} : {kind:'Browser',message:String(error)}}); }
};
