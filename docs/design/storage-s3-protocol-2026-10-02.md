# S3 immutable transfer protocol (2026-10-02)

The source protocol driver is `src/storage_core/s3.rs`. It is not wired to fleet
policy and does not make network requests yet. Its transport boundary is intended
for an operator-resolved, credential-bearing signed HTTP implementation. This is
one implementation step within the existing object-storage roadmap, not a
replacement for worker, enrollment, provider certification or release gates.

## Required operations

Capture a bounded stream into an anonymous private seekable spool, hash the exact
bytes, then upload under the ciphertext fingerprint key. Send an atomic
`PutObject` with signed `If-None-Match: *`. A preflight HEAD followed by an
unconditional PUT is invalid: concurrent writers can overwrite preserved bytes.
After either creation or an existing-object response, GET the complete stored
object and independently verify its length and SHA-256. Only that successful
readback permits a transfer receipt. ETags and provider checksum claims are not
proof. The driver's transport interface exposes no deletion, listing, bucket
creation or unconditional overwrite operation.

AWS documents conditional PUT success as HTTP 200, existing keys as HTTP 412,
and concurrent deletion conflicts as HTTP 409; conditional requests require
Signature Version 4. See [AWS conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html).
The transport maps only successful creation and 412 into the two driver results.
A conflict remains an error for the durable worker's retry policy; it must not
fall back to an unconditional write. Cloudflare also lists conditional `PutObject` support in its
[R2 compatibility table](https://developers.cloudflare.com/r2/api/s3/api/).
This documented support still requires an actual endpoint capability test.
A provider ignoring the conditional header cannot be approved merely because normal round trips pass.

## Signed HTTP transport remains required

Resolve endpoint, bucket, region, credentials and prefix from operator-owned
bindings, never from a repository override or protected restore manifest. Use
HTTPS, disable redirects and credential-bearing proxy inheritance, sign the
exact request path and conditional header, enforce whole-request/body deadlines,
and return redacted failures without provider XML/HTML bodies or URL credentials.
Validate signing against published [AWS SigV4 examples](https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html)
and test expired/session credentials, wrong region, redirects, corrupt/truncated
responses, throttling and ambiguous network outcomes. Provider capability tests
must demonstrate refusal to overwrite an existing different object, not only
idempotent uploads of the same object. No provider is certified by this driver.

The initial implementation uses single-request bounded uploads. Multipart support
must independently preserve create-only completion, bound retained parts and
recover interrupted uploads before increasing the supported object budget. It
must not silently select a weaker publication method for larger objects.

## Verification scope

Synthetic transport tests cover new and existing full readback, unchanged corrupt
existing objects, conflicting writes, failed readback, source failures, provider
body failures, destination failures and byte budgets. A seekable private-file
transport exercises a 101 MiB stream with 64 KiB read requests and exact readback
without a payload-sized memory buffer. These tests prove the protocol driver's
invariants; actual HTTP signing, provider behavior, independent cold recovery,
credential isolation and deployment remain separate acceptance gates.
