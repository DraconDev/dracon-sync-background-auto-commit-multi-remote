import test from 'node:test';
import assert from 'node:assert/strict';
import { DEFAULT_ROUTING, validateBudget, shouldRouteEligible } from './routing.mjs';

const normal = { projectedBytes: 10, limitBytes: 1000 };
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
