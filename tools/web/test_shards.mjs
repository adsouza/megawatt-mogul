// Ask Playwright itself to prove coverage; listing never launches a browser or runs a test.
import {spawnSync} from 'node:child_process';
import {appendFileSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {parseArgs} from 'node:util';

export function testCases(listing) {
  if (listing.errors?.length) throw new Error(listing.errors.map(error => error.message).join('\n'));
  const cases = [];
  function visit(suites, parents = null) {
    for (const suite of suites) {
      // The outermost suite is the file. Nested suite titles belong in the selector.
      const titles = parents === null ? [] : [...parents, suite.title];
      for (const spec of suite.specs || []) {
        for (const test of spec.tests) {
          const title = [spec.file, ...titles, spec.title].join(' › ');
          cases.push({id: `${spec.id}:${test.projectId}`,
            selector: test.projectName ? `[${test.projectName}] › ${title}` : title});
        }
      }
      visit(suite.suites || [], titles);
    }
  }
  visit(listing.suites);
  return cases;
}

export function selectedTests(listing) {
  return new Set(testCases(listing).map(test => test.id));
}

export function balanceTests(tests, timings) {
  function duration(value) {
    if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) {
      throw new Error(`Invalid test duration: ${value}`);
    }
    return value;
  }
  const fallback = duration(timings.defaultDurationMs);
  const weighted = tests.map(test => ({test,
    duration: duration(Object.hasOwn(timings.tests, test.selector) ? timings.tests[test.selector] : fallback)}));
  // Longest-processing-time first. Stable title and shard-index ties make the plan reproducible.
  weighted.sort((a, b) => b.duration - a.duration ||
    (a.test.selector < b.test.selector ? -1 : a.test.selector > b.test.selector ? 1 : 0));
  const groups = [1, 2, 3].map(index => ({index, estimatedMs: 0, tests: []}));
  for (const {test, duration} of weighted) {
    const group = groups.reduce((best, next) => next.estimatedMs < best.estimatedMs ? next : best);
    group.tests.push(test);
    group.estimatedMs += duration;
  }
  return groups;
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

function listTests(testList) {
  const cwd = fileURLToPath(new URL('../../web/tests/', import.meta.url));
  const command = ['node_modules/@playwright/test/cli.js', 'test', '--list', '--reporter=json'];
  if (testList) command.push(`--test-list=${testList}`);
  const result = spawnSync(process.execPath, command, {cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit']});
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Playwright inventory failed: ${result.stdout}`);
  return JSON.parse(result.stdout);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const {values} = parseArgs({options: {'github-output': {type: 'string'},
    'output-dir': {type: 'string', default: 'target/browser-shards'}}});
  const listing = listTests();
  const timings = JSON.parse(readFileSync(new URL('./browser_test_durations.json', import.meta.url), 'utf8'));
  const groups = balanceTests(testCases(listing), timings);
  mkdirSync(values['output-dir'], {recursive: true});
  const include = groups.map(group => ({index: group.index, partition: `${group.index}/3`,
    testList: `${values['output-dir']}/${group.index}.txt`}));
  const selections = groups.map((group, index) => {
    const path = include[index].testList;
    writeFileSync(path, group.tests.map(test => test.selector).join('\n') + '\n');
    const selected = selectedTests(listTests(resolve(path)));
    if (selected.size !== group.tests.length || group.tests.some(test => !selected.has(test.id))) {
      throw new Error(`Playwright selection differs from the runtime plan for shard ${group.index}`);
    }
    console.log(`Browser shard ${group.index}/3: ${selected.size} tests, estimated ${(group.estimatedMs / 1000).toFixed(1)} s`);
    return selected;
  });
  verifyPartition(selectedTests(listing), selections);
  console.log(`All ${selectedTests(listing).size} browser tests assigned exactly once.`);
  if (values['github-output']) {
    appendFileSync(values['github-output'], `matrix=${JSON.stringify({include})}\n`);
  }
}
