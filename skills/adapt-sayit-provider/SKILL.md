---
name: adapt-sayit-provider
description: Adapt a new speech-recognition model or provider into a SayIt (说吧！) installable provider plugin without modifying the SayIt source tree. Use when Codex must research an ASR API or an existing reverse-engineered provider project, implement the SayIt process-jsonl-v1 connector, create manifest.json, validate/package/install the plugin, or diagnose an installed provider plugin.
---

# Adapt a SayIt provider

Build an isolated provider package. Never patch the installed SayIt executable or copy provider-specific state machines into its frontend.

## Workflow

1. Inspect the provider's official API documentation or the user-supplied implementation. Identify authentication, streaming transport, PCM requirements, partial/final semantics, finish behavior, and session renewal.
2. Read [references/plugin-api.md](references/plugin-api.md) completely before creating files.
3. For login WebViews, cookies, captured browser parameters, or unofficial endpoints, also read [references/privileged-providers.md](references/privileged-providers.md).
4. Copy `assets/plugin-template/` into a new working directory. Do not edit the template in place.
5. Replace every placeholder in `manifest.json`. Keep IDs stable, lowercase, and globally unique.
6. Implement the connector as a standalone executable. Keep credentials in `start.config`; never print them to stdout or stderr.
7. Build the release executable to the manifest's `runtime.entrypoint`.
8. Run `python scripts/validate_plugin.py <plugin-dir>`.
9. Exercise the connector protocol locally with representative start/audio/finish input. Verify partial, final, error, timeout, and upstream-disconnect paths.
10. Install only after validation: `python scripts/install_plugin.py <plugin-dir>`. Use `--force` only for an intentional update.
11. Restart SayIt, inspect `list_provider_plugins`, configure the provider, then test dictation and subtitles.

## Current host boundary

- Plugin API version: `1`.
- Supported runtime: out-of-process `process-jsonl-v1`.
- Supported models: realtime ASR for `dictationRealtime` and/or `subtitles`.
- Input audio: mono signed PCM16 little-endian, 16 kHz, Base64 in JSONL messages.
- File transcription and translation plugins are not supported by API v1. Do not claim otherwise or bypass the backend boundary.
- Permissions are declared for review. They do not grant access to SayIt's memory, WebViews, or stored browser sessions.

## Design rules

- Keep provider-specific protocol, authentication, reconnect, and parsing logic inside the plugin executable.
- Treat stdout as the protocol channel: emit exactly one JSON object per line and no logs.
- Send diagnostics to stderr without secrets, cookies, tokens, audio payloads, or user text.
- Return only the normalized events defined by the protocol reference.
- Reject incompatible sample rates or malformed host messages with a structured `error` event.
- Pin upstream assumptions in code and surface a clear compatibility error when a reverse-engineered endpoint changes.
- Do not automate account creation, CAPTCHA bypass, risk-control evasion, or use sessions not explicitly supplied by the user.

## Completion gate

Do not report success until the manifest validator passes, the release entrypoint exists, a protocol smoke test passes, and the installed directory contains only required runtime files.
