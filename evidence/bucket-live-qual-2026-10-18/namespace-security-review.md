## Review

- **Correct:** Ownership checks fail closed rather than treating overflow UID65534 as root: `consumer.mjs:70–77`, `bindings.mjs:26–39`, `runner.mjs:46–55`, `storage.mjs:16–19`, `restore.mjs:15–29` (all under `dracon-sync/scripts/bucket-runner/`).
- Fixing only consumer/bindings would be incomplete: runner, credential ancestry, and restoration enforce the same root/euid assumption.
- Storage role separation remains explicit: `storage.mjs:122–136` selects separate credential files; `restore.mjs:88–91` requires a transport without upload authority.
- No files changed, tests executed, credentials accessed, or network operations performed. Runtime observations below are supplied evidence, not independently reproduced here.

## Recommendation

**Prefer an administrator-managed system service running as the operator UID, retaining the existing confinement and avoiding the implicit single-user user namespace.**

This is the smallest maintainable compatibility solution: preserve ownership semantics rather than introducing a second filesystem-authority protocol across five validators.

Required boundary:
- Administrator installs/configures the **system-manager** service with explicit `User`/`Group` and `PrivateUsers=no`.
- Preserve `ProtectSystem=strict`, `ProtectHome=read-only`, write exceptions, private temporary storage, empty capabilities, namespace restrictions, and syscall filtering (`dracon-sync.service:32–68`).
- Replace user-unit `%h` assumptions with explicit operator paths; separately verify user-session dependencies such as SSH-agent access.
- Verify the actual execution namespace exposes host root ownership correctly. Merely displaying `PrivateUsers=no` is insufficient given the supplied reproduction.
- Coordinate migration without duplicate schedulers or an unbounded sync outage.

This requires explicit administrator approval and privileged unit installation; it is **not** an IAM change or permission to run the daemon as root.

## Namespace-aware validation

**Namespace-local `stat` plus `uid_map` cannot recover root provenance for an unmapped owner.** Multiple host UIDs collapse to the same overflow value. Recognizing that collapse does not identify which host UID owned an inode.

A trusted host observer can supply the missing information, but the proposed stat helper is **conditional**, not a complete race/security design:

1. Launching it normally from the sandbox inherits the namespace and does not solve the problem.
2. A host-side user-manager service could observe host ownership, but must expose only a fixed metadata operation—not arbitrary transient commands, executables, environment overrides, or filesystem writes.
3. Pin the helper, interpreter/runtime, endpoint and configuration outside repository control. Restrict requests to operator-selected paths and exact ancestor chains; never read file contents or credentials.
4. Authenticate the endpoint/request-response association. Peer UID alone does not distinguish the helper from other processes sharing the operator UID.
5. Compare namespace/host device, inode, type, mode and nanosecond timestamps before/after; accept only host root/operator ownership while retaining each caller’s existing mode/sticky/no-link rules.
6. **Matching metadata snapshots do not eliminate pathname ABA races.** Use descriptor-relative, no-symlink traversal with pinned directory handles and bind subsequent use to those handles where adversarial replacement is possible.
7. Fail closed on inaccessible paths, mismatched mount views, private `/tmp`, stale replies, helper failure, foreign owners, or ambiguous identity.
8. Keep credential ancestry stricter: writable sticky ancestors remain forbidden (`storage.mjs:16–19,49–58`).

A read-only helper also needs an explicit confinement design that does not recreate the original user namespace. Calling a fixed unit “read-only” is not enforcement. The helper therefore adds implementation and maintenance burden beyond the system-service migration.

The existing trust boundary already trusts operator-owned files; neither approach protects against a fully compromised operator account.

## Files and tests needed

**Recommended migration:**
- Add a distinct system-service deployment unit/documented procedure rather than silently reinterpreting the user unit.
- Add offline integration coverage under the actual production sandbox: root ancestors accepted; foreign ownership, symlinks, unsafe modes and credential ancestors rejected.
- Keep source ownership checks unchanged.

**If the helper is selected instead:**
- Introduce one shared ancestry/provenance module consumed by consumer, bindings, runner, storage and restore.
- Add helper protocol/installation tests plus negative tests for foreign-unmapped owners, forged/replayed replies, symlink/rename races, mismatched mounts and helper unavailability.
- Preserve existing leaf-owner, private-state and credential rules.
- Rebuild the bundle and update its operator-authorized digest through the existing installation process (`capabilities.mjs:139–149`).

## BLOCKERS

- **P1 — Qualification remains blocked:** the supplied sandbox reproduction contradicts ownership assumptions at the cited validators. Do not authorize UID65534 globally or weaken confinement.
- A deployment decision and approval are required: privileged system-service installation, or a separately specified trusted host metadata boundary.
- After remediation, scheduled upload, independent scoped readback, and fresh restoration still require actual qualification evidence. This review does not establish goal completion.

**Merge verdict: BLOCK qualification; recommend administrator-managed, operator-UID system service.**