import {test} from 'node:test';
import assert from 'node:assert/strict';
import {balanceTests, selectedTests, testCases, verifyPartition} from '../test_shards.mjs';

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

test('test-list selectors preserve nested titles and projects without depending on line numbers', () => {
  const listing = {suites: [{title: 'file.spec.js', specs: [], suites: [{title: 'outer',
    specs: [{id: 'nested', file: 'file.spec.js', title: 'a [literal] test', line: 42,
      tests: [{projectId: 'chromium', projectName: 'chromium'}]}]}]}]};
  assert.deepEqual(testCases(listing), [{id: 'nested:chromium',
    selector: '[chromium] › file.spec.js › outer › a [literal] test'}]);
});

function cases(...durations) {
  return durations.map((duration, index) => ({id: String(index), selector: `test ${index}`, duration}));
}

test('runtime balancing spreads the longest tests instead of grouping adjacent save flows', () => {
  const tests = cases(90, 60, 30, 30, 20, 20, 10, 10);
  const timings = {defaultDurationMs: 15, tests: Object.fromEntries(tests.map(t => [t.selector, t.duration]))};
  const groups = balanceTests(tests, timings);
  assert.deepEqual(groups.map(group => group.estimatedMs), [90, 90, 90]);
  verifyPartition(new Set(tests.map(t => t.id)), groups.map(group => new Set(group.tests.map(t => t.id))));
});

test('assignment is deterministic even if discovery order changes', () => {
  const tests = cases(10, 10, 10, 10, 10, 10);
  const timings = {defaultDurationMs: 10, tests: {}};
  assert.deepEqual(balanceTests(tests, timings), balanceTests([...tests].reverse(), timings));
});

test('new tests use the default estimate and remain in the partition', () => {
  const tests = cases(90, 30, 0, 0, 0);
  const groups = balanceTests(tests, {defaultDurationMs: 30, tests: {'test 0': 90, 'test 1': 30}});
  assert.deepEqual(groups.map(group => group.estimatedMs), [90, 60, 60]);
  verifyPartition(new Set(tests.map(t => t.id)), groups.map(group => new Set(group.tests.map(t => t.id))));
});

test('invalid runtime estimates fail rather than produce a misleading balance', () => {
  for (const duration of [0, -1, NaN, Infinity, '30']) {
    assert.throws(() => balanceTests(cases(1, 2, 3), {defaultDurationMs: duration, tests: {}}), /duration/i);
    assert.throws(() => balanceTests(cases(1, 2, 3), {defaultDurationMs: 30, tests: {'test 0': duration}}), /duration/i);
  }
});

test('small workloads always retain every test once within the greedy load bound', () => {
  // Exhaust all positive workloads of 3..7 tests whose durations are 1, 2 or 3.
  for (let size = 3; size <= 7; size++) {
    for (let encoding = 0; encoding < 3 ** size; encoding++) {
      let remaining = encoding;
      const durations = Array.from({length: size}, () => {
        const duration = remaining % 3 + 1;
        remaining = Math.floor(remaining / 3);
        return duration;
      });
      const tests = cases(...durations);
      const timings = {defaultDurationMs: 1, tests: Object.fromEntries(tests.map(t => [t.selector, t.duration]))};
      const groups = balanceTests(tests, timings);
      verifyPartition(new Set(tests.map(t => t.id)), groups.map(group => new Set(group.tests.map(t => t.id))));
      const total = durations.reduce((sum, duration) => sum + duration, 0);
      assert.equal(groups.reduce((sum, group) => sum + group.estimatedMs, 0), total);
      assert.ok(Math.max(...groups.map(group => group.estimatedMs)) <= total / 3 + Math.max(...durations));
    }
  }
});
