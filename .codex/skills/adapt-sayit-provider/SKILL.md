---
name: adapt-sayit-provider
description: Create an isolated, manually installable SayIt (说吧！) speech-provider plugin in a user-specified external workspace. Use when Codex must research an ASR API or an existing reverse-engineered provider project, implement the SayIt process-jsonl-v2 connector, create manifest.json, validate and package it. Never read, modify, or install into the SayIt source repository unless the user explicitly asks for installation.
---

# Adapt a SayIt provider

Build an isolated provider package. Never patch the installed SayIt executable or copy provider-specific state machines into its frontend.
The validation/signing tools require Python 3 and the `cryptography` package.

## Non-negotiable workspace boundary

Before inspecting a provider or creating any file, obtain an absolute `PLUGIN_WORKSPACE` from the user. It must be a new or empty directory **outside** the SayIt repository. If no suitable path was supplied, stop and ask for one; do not infer the current directory.

- Treat the SayIt repository as forbidden: do not read, search, list, create, edit, build, or write anywhere below it. The only permitted exception is reading this Skill's own files under `.codex/skills/adapt-sayit-provider/`.
- Never create `plugins/`, `src-tauri/`, `ui/`, `docs/`, `skills/`, source files, build outputs, or package artifacts in the SayIt repository.
- Run every provider command with its working directory set to `PLUGIN_WORKSPACE` or an explicitly user-supplied external provider project. Never run provider build tools from SayIt's working directory.
- The deliverable is a self-contained directory at `PLUGIN_WORKSPACE/dist/<plugin-id>-<version>/`. Do not copy it into SayIt's local plugin directory and do not invoke an installer unless the user separately asks to install it.
- A packaged directory is the installable artifact. It may additionally be zipped for transfer, but SayIt's current plugin manager installs the extracted directory selected by the user.

## Workflow

1. Set `SKILL_DIR` to this Skill directory. Initialize the external workspace before any provider work:
   `python "$SKILL_DIR/scripts/init_plugin_workspace.py" "$PLUGIN_WORKSPACE" --template "$SKILL_DIR/assets/plugin-template" --forbid-root "<absolute SayIt repository path>"`.
2. Work only in `PLUGIN_WORKSPACE/source/`. Inspect the provider's official documentation or a user-supplied external implementation. Identify authentication, streaming transport, PCM requirements, partial/final semantics, finish behavior, and session renewal.
3. Read [references/plugin-api.md](references/plugin-api.md) completely before creating files. For login WebViews, cookies, captured browser parameters, or unofficial endpoints, also read [references/privileged-providers.md](references/privileged-providers.md).
4. Replace every placeholder in `source/manifest.json`. Keep IDs stable, lowercase, and globally unique.
5. Implement the connector only under `source/` as a standalone executable. Keep credentials in `start.config`; never print them to stdout or stderr.
6. Build the release executable to `source/<runtime.entrypoint>`.
7. Start with `python "$SKILL_DIR/scripts/smoke_test_plugin.py" "$PLUGIN_WORKSPACE/source"`, then exercise every declared operation locally, including malformed input, cancellation, timeout, and upstream disconnect.
8. Create the installable artifact only under `dist/`: `python "$SKILL_DIR/scripts/package_plugin.py" "$PLUGIN_WORKSPACE/source" "$PLUGIN_WORKSPACE/dist/<plugin-id>-<version>" --workspace "$PLUGIN_WORKSPACE" --forbid-root "<absolute SayIt repository path>"`.
9. Sign it: `python "$SKILL_DIR/scripts/sign_plugin.py" "$PLUGIN_WORKSPACE/dist/<plugin-id>-<version>" --private-key "$PLUGIN_WORKSPACE/keys/publisher.pem" --key-id <stable-publisher-id> --workspace "$PLUGIN_WORKSPACE" --forbid-root "<absolute SayIt repository path>"`. Never put the private key inside the package.
10. Run `python "$SKILL_DIR/scripts/validate_plugin.py" "$PLUGIN_WORKSPACE/dist/<plugin-id>-<version>"` and report that directory as the sole deliverable.
11. Only if the user explicitly asks to install, tell them to select that directory in SayIt's plugin manager, or then use `install_plugin.py`. Never install automatically as part of adaptation.

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

Do not report success until the manifest validator and signature check pass, the release entrypoint exists, every declared operation has a smoke test, privileged sessions have expiry/clear tests when applicable, and `PLUGIN_WORKSPACE/dist/` contains the only runtime artifact. Report the artifact path and explicitly state that it has not been installed.
