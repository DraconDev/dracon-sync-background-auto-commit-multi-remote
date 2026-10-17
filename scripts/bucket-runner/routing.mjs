/** Routing never grants eligibility. Call only after trusted provenance and
 * consumer qualification. Budgets are measured operator policy, not repo data.
 */
import defaults from './routing-defaults.json' with { type: 'json' };

// Shared with Rust via include_str!; bundled into the dependency-closed runner.
// Fail closed on schema drift rather than silently restoring unreachable values.
if (Object.keys(defaults).sort().join(',') !== 'largeBytes,pressureLimitBytes,pressureRatio,schemaVersion'
  || defaults.schemaVersion !== 1) throw new Error('invalid-shared-routing-policy');
export const DEFAULT_ROUTING = Object.freeze({ largeBytes: defaults.largeBytes, pressureRatio: defaults.pressureRatio });
export const DEFAULT_PRESSURE_LIMIT_BYTES = defaults.pressureLimitBytes;
validateBudget({ projectedBytes: 0, limitBytes: DEFAULT_PRESSURE_LIMIT_BYTES });

/** @typedef {{projectedBytes:number,limitBytes:number,largeBytes?:number,pressureRatio?:number}} RoutingBudget */
/** @param {RoutingBudget} budget */
export function validateBudget(budget) {
  if (!budget || typeof budget !== 'object' || Array.isArray(budget)
    || Object.keys(budget).some(key => !['projectedBytes', 'limitBytes', 'largeBytes', 'pressureRatio'].includes(key))) {
    throw new Error('invalid-or-unmeasurable-routing-budget');
  }
  const value = { ...DEFAULT_ROUTING, ...budget };
  if (!Number.isSafeInteger(value.projectedBytes) || value.projectedBytes < 0
    || !Number.isSafeInteger(value.limitBytes) || value.limitBytes <= 0
    || !Number.isSafeInteger(value.largeBytes) || value.largeBytes <= 0
    || !Number.isFinite(value.pressureRatio) || value.pressureRatio <= 0 || value.pressureRatio >= 1) {
    throw new Error('invalid-or-unmeasurable-routing-budget');
  }
  return Object.freeze(value);
}

/** @param {{bytes:number,bucketed?:boolean}} eligible @param {RoutingBudget} budget */
export function shouldRouteEligible(eligible, budget) {
  const value = validateBudget(budget);
  if (!Number.isSafeInteger(eligible?.bytes) || eligible.bytes <= 0
    || (eligible.bucketed !== undefined && typeof eligible.bucketed !== 'boolean')) {
    throw new Error('invalid-qualified-asset-size');
  }
  // Existing bucket references cannot silently fall back to raw blobs.
  return eligible.bucketed === true || eligible.bytes >= value.largeBytes
    || value.projectedBytes >= Math.floor(value.limitBytes * value.pressureRatio);
}
