import test from 'node:test';
import assert from 'node:assert/strict';
import { DEFAULT_ROUTING, DEFAULT_PRESSURE_LIMIT_BYTES, validateBudget, shouldRouteEligible } from './routing.mjs';
import { renderSolidPng, CAPS } from './adapters.mjs';
import { readFileSync } from 'node:fs';

const normal = { projectedBytes: 10, limitBytes: 1000 };
test('routing: shared policy is exact and a real accepted PNG reaches the threshold', () => {
  const policy = JSON.parse(readFileSync(new URL('./routing-defaults.json', import.meta.url)));
  assert.equal(policy.schemaVersion, 1);
  assert.equal(DEFAULT_ROUTING.largeBytes, policy.largeBytes);
  assert.equal(DEFAULT_ROUTING.pressureRatio, policy.pressureRatio);
  assert.equal(DEFAULT_PRESSURE_LIMIT_BYTES, policy.pressureLimitBytes);
  assert.equal(policy.largeBytes, 1024);
  assert.equal(CAPS.maxDimension, 512);
  assert.equal(CAPS.maxSourceBytes, 2097152);
  const bytes = renderSolidPng({ width: 512, height: 512, rgba: [32, 64, 96, 255] });
  assert.ok(bytes.length >= policy.largeBytes && bytes.length <= CAPS.maxSourceBytes);
  assert.equal(shouldRouteEligible({ bytes: bytes.length }, normal), true);
});
test('routing: exact size and pressure boundaries are inclusive', () => {
  for (const [bytes, expected] of [[1023, false], [1024, true], [1025, true]]) {
    assert.equal(shouldRouteEligible({ bytes }, normal), expected);
  }
  for (const [projectedBytes, expected] of [[799, false], [800, true], [801, true]]) {
    assert.equal(shouldRouteEligible({ bytes: 103 }, { projectedBytes, limitBytes: 1000 }), expected);
  }
});
test('routing: small qualified assets remain local below measured pressure', () => {
  assert.equal(shouldRouteEligible({ bytes: 103 }, normal), false);
  assert.equal(shouldRouteEligible({ bytes: DEFAULT_ROUTING.largeBytes }, normal), true);
  assert.equal(shouldRouteEligible({ bytes: 103 }, { projectedBytes: 800, limitBytes: 1000 }), true);
  assert.equal(shouldRouteEligible({ bytes: 103, bucketed: true }, normal), true);
});
test('routing: invalid or absent budgets never become implicit routing authority', () => {
  for (const value of [undefined, null, {}, { ...normal, limitBytes: 0 },
    { ...normal, projectedBytes: NaN }, { ...normal, projectedBytes: -1 },
    { ...normal, pressureRatio: 1 }, { ...normal, largeBytes: 0 }, { ...normal, extra: true }]) {
    assert.throws(() => validateBudget(value), /routing-budget/);
  }
  assert.throws(() => shouldRouteEligible({ bytes: NaN }, normal), /qualified-asset/);
});
test('routing: thresholds are copied, immutable and policy-selectable', () => {
  const input = { ...normal, largeBytes: 50, pressureRatio: 0.5 };
  const copy = validateBudget(input);
  input.largeBytes = 10000;
  assert.ok(Object.isFrozen(copy));
  assert.equal(shouldRouteEligible({ bytes: 60 }, copy), true);
  assert.equal(shouldRouteEligible({ bytes: 40 }, copy), false);
});
