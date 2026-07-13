---
name: adapt-sayit-provider
description: Adapt a new speech-recognition model or provider into a SayIt (说吧！) installable provider plugin without modifying the SayIt source tree. Use when Codex must research an ASR API or an existing reverse-engineered provider project, implement the SayIt process-jsonl-v2 connector, create manifest.json, validate/package/install the plugin, or diagnose an installed provider plugin.
---

# Adapt a SayIt provider

Build an isolated provider package. Never patch the installed SayIt executable or copy provider-specific state machines into its frontend.
The validation/signing tools require Python 3 and the `cryptography` package.

## Workflow

1. Inspect the provider's official API documentation or the user-supplied implementation. Identify authentication, streaming transport, PCM requirements, partial/final semantics, finish behavior, and session renewal.
2. Read [references/plugin-api.md](references/plugin-api.md) completely before creating files.
3. For login WebViews, cookies, captured browser parameters, or unofficial endpoints, also read [references/privileged-providers.md](references/privileged-providers.md).
4. Copy `assets/plugin-template/` into a new working directory. Do not edit the template in place.
5. Replace every placeholder in `manifest.json`. Keep IDs stable, lowercase, and globally unique.
6. Implement the connector as a standalone executable. Keep credentials in `start.config`; never print them to stdout or stderr.
7. Build the release executable to the manifest's `runtime.entrypoint`.
8. Start with `python scripts/smoke_test_plugin.py <source-dir>`, then exercise every declared operation locally, including malformed input, cancellation, timeout, and upstream disconnect.
9. Create a minimal runtime package: `python scripts/package_plugin.py <source-dir> <package-dir>`.
10. Sign it: `python scripts/sign_plugin.py <package-dir> --private-key <publisher.pem> --key-id <stable-publisher-id>`. Never put the private key inside the package.
11. Run `python scripts/validate_plugin.py <package-dir>`.
12. Install through SayIt's plugin manager. For CLI installation, use `python scripts/install_plugin.py <package-dir> --trust-key` only after verifying the publisher fingerprint. Use `--allow-unsigned` only for an intentional local development package.
13. Reload providers, configure the provider, then test each declared model and action with the main window both open and closed.

## Current host boundary

- Plugin API version: `2` (v1 realtime compatibility is retained).
- Supported runtime: out-of-process `process-jsonl-v2` plus v2 one-shot invokes.
- Supported models: realtime ASR, file ASR and subtitle translation; hotwords use the customization operations.
- Input audio: mono signed PCM16 little-endian, 16 kHz, Base64 in JSONL messages.
- Privileged providers use a dedicated host WebView and DPAPI-protected, allowlisted Cookie snapshot. They never receive SayIt's main WebView or unrelated browser data.
- Package files are integrity checked; signed publishers require explicit trust on first install, and updates retain a rollback backup.

## Design rules

- Keep provider-specific protocol, authentication, reconnect, and parsing logic inside the plugin executable.
- Treat stdout as the protocol channel: emit exactly one JSON object per line and no logs.
- Send diagnostics to stderr without secrets, cookies, tokens, audio payloads, or user text.
- Return only the normalized events defined by the protocol reference.
- Reject incompatible sample rates or malformed host messages with a structured `error` event.
- Pin upstream assumptions in code and surface a clear compatibility error when a reverse-engineered endpoint changes.
- Declare only the minimum capabilities, actions, browser URLs and permissions; the host rejects undeclared actions.
- Do not automate account creation, CAPTCHA bypass, risk-control evasion, or use sessions not explicitly supplied by the user.

## Completion gate

Do not report success until the manifest validator and signature check pass, the release entrypoint exists, every declared operation has a smoke test, privileged sessions have expiry/clear tests when applicable, and the installed directory contains only required runtime files.
