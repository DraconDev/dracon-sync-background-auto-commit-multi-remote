# Filter-aware outgoing accounting: enforced commit-ceiling reservations

Supersedes the unsupported-filter boundary in bucket-outgoing-accounting-2026-10-17.md.

## Invariant

Plaintext length does not bound arbitrary clean output. Rather than running
a filter merely for routing or recognizing a driver name as trustworthy,
reserve max_stage_file_bytes for EACH potentially committed filtered path.
This is a bound on ADMISSIBLE COMMIT output, not a prediction of driver
output: normal post-stage hygiene measures real index blobs and unstages
oversized/unmeasurable output. The bucket Prepared gate rechecks this same
ceiling against actual staged blobs before allowing commit.

- Independently enumerate tracked filter attributes; raw filter-disabled
  status can hide a tracked plaintext blob whose normal clean output differs.
- Reserve per path, not plaintext OID (encryption can be nondeterministic).
- No plaintext read/hash needed for the filtered reservation. Unfiltered
  paths retain raw-byte/OID measurement and existing bounds.
- Reserve another full ceiling for the runner-created/updated manifest.
- Zero/non-JS-safe ceilings, checked-add overflow, unsupported encoding,
  malformed attribute results and snapshot/identity races fail closed.
  A zero ceiling means "no filtered output may be admitted": accounting
  refuses the measurement instead of treating filtered paths as free.
- Snapshots now bind effective attributes and Git config as well as
  HEAD/index/tracking/status. Config bytes are internal and never logged.
- Literal filesystem paths may contain colon; accounting does not treat
  them as object keys or revision expressions. Git receives paths after
  -- or through NUL stdin; OIDs remain separately validated.
- Existing object/count/output/deadline limits remain unchanged. Attribute
  enumeration is batched to keep large tracked repositories bounded.

## Tradeoff

The upper bound can substantially overestimate actual ciphertext: a large
repository with many filtered paths may route eligible assets under storage
pressure even if the next commit is small. It cannot authorize new asset
eligibility, unsupported consumers, raw materialization staging or oversize
pushes. This conservatism is intentional; an exact transform prediction
would require a separately versioned trusted filter-bound API.

## Coverage

Tests prove no sentinel driver executes during accounting; identical plaintext
gets independent reservations; raw-clean tracked paths remain covered;
unsupported encodings/unbounded ceilings refuse; literal colon paths work;
manifest request budgets reserve its potential growth; final staged clean
expansion above ceiling refuses while the source remains intact.

## Separate unsupported-root finding

The platform root is a monorepo, outside the standalone plain-Vite consumer
adapter's supported lifecycle. After accounting was repaired this refusal
became visible. The operator explicitly approved disabling ONLY the invalid
root opt-in (no manifest or binding exists). The qualified fixture remains
enabled; all normal Git/Warden/forward-only/size guards remain enforced.
