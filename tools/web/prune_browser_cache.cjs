// Called only from the trusted default-branch workflow after a PR closes.
module.exports = async ({github,context,core}) => {
  if(context.payload.action!=='closed')throw new Error('Only closed PR caches may be pruned');
  const number=context.payload.pull_request?.number;
  if(!Number.isSafeInteger(number)||number<=0)throw new Error('Invalid PR number');
  const ref=`refs/pull/${number}/merge`;
  const selected=[];
  // Collect before deleting so removal cannot shift later pages past our cursor.
  for(let page=1;;page++){
    const {data}=await github.request('GET /repos/{owner}/{repo}/actions/caches',{
      ...context.repo,ref,per_page:100,page,
    });
    selected.push(...data.actions_caches.filter(cache=>cache.ref===ref&&cache.key.startsWith('browser-rust-')));
    if(data.actions_caches.length<100)break;
  }
  let removed=0;
  for(const cache of selected){
    try{
      await github.request('DELETE /repos/{owner}/{repo}/actions/caches/{cache_id}',{
        ...context.repo,cache_id:cache.id,
      });
      removed++;
    }catch(error){if(error.status!==404)throw error;}
  }
  core.info(`Removed ${removed} browser caches for ${ref}`);
  return removed;
};
