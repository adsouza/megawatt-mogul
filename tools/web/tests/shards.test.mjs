import {test} from 'node:test';
import assert from 'node:assert/strict';
import {selectedTests, verifyPartition} from '../test_shards.mjs';

test('inventory includes nested suites and distinguishes projects', () => {
  const listing = {suites: [{specs: [{id: 'first', tests: [{projectId: ''}]}],
    suites: [{specs: [{id: 'nested', tests: [{projectId: 'chromium'}, {projectId: 'firefox'}]}]}]}]};
  assert.deepEqual([...selectedTests(listing)], ['first:', 'nested:chromium', 'nested:firefox']);
});

test('a complete disjoint partition passes', () => {
  verifyPartition(new Set(['a', 'b', 'c']), [new Set(['a']), new Set(['b']), new Set(['c'])]);
});

test('missing, overlapping, unexpected and empty shards fail closed', () => {
  const inventory = new Set(['a', 'b', 'c']);
  assert.throws(() => verifyPartition(inventory, [new Set(['a']), new Set(['b'])]), /missing/);
  assert.throws(() => verifyPartition(inventory, [new Set(['a', 'b']), new Set(['b', 'c'])]), /multiple/);
  assert.throws(() => verifyPartition(inventory, [new Set(['a', 'b', 'c', 'd'])]), /unexpected/);
  assert.throws(() => verifyPartition(inventory, [inventory, new Set()]), /empty/);
});

test('Playwright collection errors cannot masquerade as an empty inventory', () => {
  assert.throws(() => selectedTests({errors: [{message: 'bad import'}], suites: []}), /bad import/);
});
