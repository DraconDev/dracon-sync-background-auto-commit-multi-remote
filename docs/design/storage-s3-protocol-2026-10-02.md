# S3 immutable transfer protocol (2026-10-02)

The source protocol driver is `src/storage_core/s3.rs`. Its signed HTTP adapter
is `src/storage_core/s3/http.rs`. Explicit CLI recovery and captured-job advancement resolve operator bindings and private credentials;
automatic fleet upload/routing remains unfinished. Adapter construction performs no network requests; explicit
transfer operations perform bounded signed PUT/GET requests. This is
one implementation step within the existing object-storage roadmap, not a
replacement for automatic scheduling, enrollment, provider certification or release gates.

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

## Signed HTTP transport and remaining activation gates

The adapter accepts explicitly resolved endpoint, bucket, region, credentials
and prefix, rejects unsafe origins/namespaces, and uses HTTPS, no redirects, no
inherited proxies and no response decompression. It signs the actual ciphertext
hash, content length and overwrite condition; session tokens are signed and marked
sensitive alongside Authorization. Owned credential buffers and signing keys
clear on drop, with no Debug/Serialize credential implementation. Each request
checks optional credential expiry. Provider response bodies and arbitrary HTTP
errors are not surfaced; failures retain capacity/security/integrity/transient
classification. The explicit request timeout includes the complete response body.
Use the synchronous adapter from a blocking worker outside the async runtime.

The implemented read-only resolver obtains named private JSON credentials from
an explicitly selected owned directory, rejects links/unsafe permissions and
tracked or unignored credential files, and checks expiry. Global S3 bindings
supply the required region and optional prefix. These fields come from operator-owned
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
without a payload-sized memory buffer. Nine adapter tests additionally check two published AWS signatures, actual
loopback HTTP headers/body/path/readback, session-token signing and expiry,
conditional conflicts, redacted provider errors, response lengths, partial/encoded
responses, redirect refusal and a total deadline under trickled bytes. Loopback
HTTP is accessible only through the private test constructor; production
construction rejects plaintext HTTP. These tests prove protocol and signing
behavior, not an endpoint capability certificate. A separate operational cold-clone test now exercises actual signed HTTPS CLI
recovery using a temporary CA and synthetic protected metadata, preserving the
index and working pointer. Actual provider behavior, encrypted independent cold
recovery, automatic upload/routing and deployment remain separate gates.


## Capability approval and selected-version worker

`SignedHttpTransport::verify_conditional_writes` consumes the exact configured
transport. It publishes a random 64-byte control under the reserved probe prefix:
two competing conditional creates must produce one 200 and one 412, complete
GET must match, a different payload at the same key must produce 412, and another
complete GET must still match. Both successful creates, both refusals, false
412/overwritten bytes, partial responses or any ambiguous result deny approval.
The control is retained; no delete/list operation is introduced.

The resulting write transport cannot be constructed or rebound externally and
expires after one hour, rejecting future clock observations. Explicit
`storage advance-job` always obtains this approval afresh for each required S3
binding. Probe reports are informational and cannot grant writes. This observed
check does not certify lifecycle settings, independent providers or durability.

The durable worker prepares captured snapshots through Warden (or explicitly
approved non-sensitive retention), then independently reads every required copy
before returning `ReadyToStage`. A selected-input reader checks exact length and
SHA-256 at EOF **before** Local/S3 private spools publish/send asset bytes. A
mutated retained payload therefore cannot publish under a new unexpected hash.
Backend claims without complete input consumption also fail. Current working
files, Git/index locks and published history are untouched. Automatic capture,
routing, enrollment, commit/push scheduling and provider certification remain
separate unfinished roadmap gates.

An isolated TLS CLI fixture now exercises capability checking, captured-job
preparation, signed conditional asset PUT, full readback, retry against an
existing object, and refusal of a provider that ignores the conditional header.
It uses synthetic protected metadata and an explicitly non-sensitive grant; it
is not an actual-key encrypted S3 or live independent-provider certificate.
