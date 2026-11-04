// Ask Playwright itself to prove coverage; listing never launches a browser or runs a test.
import {spawnSync} from 'node:child_process';
import {appendFileSync} from 'node:fs';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {parseArgs} from 'node:util';

export function selectedTests(listing) {
  if (listing.errors?.length) throw new Error(listing.errors.map(error => error.message).join('\n'));
  const selected = new Set();
  function visit(suites) {
    for (const suite of suites) {
      for (const spec of suite.specs || []) {
        for (const test of spec.tests) selected.add(`${spec.id}:${test.projectId}`);
      }
      visit(suite.suites || []);
    }
  }
  visit(listing.suites);
  return selected;
}

export function verifyPartition(inventory, groups) {
  const assigned = new Set();
  for (const group of groups) {
    if (!group.size) throw new Error('empty browser shard');
    for (const test of group) {
      if (assigned.has(test)) throw new Error(`test assigned to multiple shards: ${test}`);
      if (!inventory.has(test)) throw new Error(`unexpected test: ${test}`);
      assigned.add(test);
    }
  }
  for (const test of inventory) {
    if (!assigned.has(test)) throw new Error(`missing test: ${test}`);
  }
}

function listTests(partition) {
  const cwd = fileURLToPath(new URL('../../web/tests/', import.meta.url));
  const command = ['node_modules/@playwright/test/cli.js', 'test', '--list', '--reporter=json'];
  if (partition) command.push(`--shard=${partition}`);
  const result = spawnSync(process.execPath, command, {cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit']});
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Playwright inventory failed: ${result.stdout}`);
  return selectedTests(JSON.parse(result.stdout));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const {values} = parseArgs({options: {'github-output': {type: 'string'}}});
  const include = [1, 2, 3].map(index => ({index, partition: `${index}/3`}));
  const inventory = listTests();
  const groups = include.map(({partition}) => listTests(partition));
  verifyPartition(inventory, groups);
  groups.forEach((group, index) => console.log(`Browser shard ${include[index].partition}: ${group.size} tests`));
  console.log(`All ${inventory.size} browser tests assigned exactly once.`);
  if (values['github-output']) {
    appendFileSync(values['github-output'], `matrix=${JSON.stringify({include})}\n`);
  }
}
