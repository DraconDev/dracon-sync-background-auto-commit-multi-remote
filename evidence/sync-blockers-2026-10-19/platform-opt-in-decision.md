# Platform root: unsupported enrollment resolved by explicit operator choice

After the conservative accounting fix was deployed and executed, the next
runner result was `unsupported` (12:13:30 local). Source inspection of
bucket-runner/consumer.mjs establishes the adapter is intentionally limited
to standalone plain-Vite apps: it rejects nested Git/package/workspace
structures. Platform has no bucket-binding.json or bucket-assets manifest;
state bindings contain ONLY the independent bridge-fixture repository.

The operator explicitly chose "Remove invalid root opt-in" in a focused
scope/authority question. Only dracon-platform/bucket-runner.json enabled
was set false. Qualified fixture runner remains enabled. Warden, Git,
forward-only asset guard and high-water sizing guards are all unchanged.
No manifest, binding or remote object was created, removed or fabricated.
This is correction of an unsupported enrollment configuration, not silent
fallback on managed publication failure. Accounting support remains fixed
for supported enabled consumers (including Warden-protected fixture inputs).
