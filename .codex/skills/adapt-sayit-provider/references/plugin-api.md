# SayIt provider plugin API v2

## Runtime boundary

SayIt owns microphone capture, DSP, model/provider selection, task state, file paths, subtitles and text injection. A provider plugin is an isolated executable that implements vendor authentication and transport, then returns normalized JSONL events. API v1 realtime plugins remain loadable; all new plugins should use API/protocol v2.

Package layout after packaging:

```text
<plugin-id>/
├── manifest.json
└── bin/
    ├── connector.exe
    └── required-runtime-files
```

The host scans `%LOCALAPPDATA%\com.henjicc.sayit\plugins`. IDs use lowercase ASCII letters, digits, dots and hyphens, up to 64 characters. The entrypoint and integrity paths must remain inside the package and symlinks are rejected.

## Manifest capabilities and models

- `asr`: realtime and/or file ASR.
- `translation`: subtitle translation.
- `customization`: set/get/clear hotwords.
- Realtime model: category `realtime`, protocol `process-jsonl-v2`, scene `dictationRealtime` and/or `subtitles`.
- File model: category `file`, protocol `process-file-v2`, scene `dictationFile` and/or `transcription`.
- Translation model: category `translation`, protocol `process-translation-v2`, scene `subtitleTranslation`.

Use the template manifest as the complete ordinary-provider example. Supported config field types are `text`, `password`, `number` and `boolean`. Secret fields are removed from frontend snapshots. Supported permissions are `network`, `browserSession` and `cookies`.

## Realtime stream

Every message is one UTF-8 JSON object followed by `\n`. `start` is first:

```json
{"type":"start","protocolVersion":2,"sessionId":"uuid","providerId":"vendor","model":"vendor-live","sampleRate":16000,"config":{},"session":null,"permissions":["network"]}
```

The host then sends repeated Base64 PCM16 little-endian mono 16 kHz audio messages, followed by `finish` or `stop`:

```json
{"type":"audio","pcm16Base64":"AAABAP//"}
{"type":"finish"}
```

The plugin emits `ready`, `partial`, `final`, then `finished`; fatal errors use `error`:

```json
{"type":"ready"}
{"type":"partial","text":"临时结果"}
{"type":"final","text":"最终结果"}
{"type":"finished"}
{"type":"error","code":"auth_failed","message":"认证失败"}
```

## One-shot invoke protocol

For non-realtime capabilities the host starts a fresh connector and sends exactly one `invoke`; stdin then closes. `config` contains provider configuration, `session` contains an OS-protected browser session only when the user explicitly synchronized one, and `payload` is operation-specific.

```json
{"type":"invoke","protocolVersion":2,"requestId":"uuid","operation":"translate","providerId":"vendor","config":{},"session":null,"permissions":["network"],"payload":{}}
```

The plugin may emit `progress`, `delta` or `event`, then exactly one `completed`; or terminate with `error`.
If the user cancels a file task, the host terminates the one-shot connector process; do not rely on receiving an additional JSON message after stdin closes.

```json
{"type":"delta","text":"新增译文"}
{"type":"completed","result":{}}
{"type":"error","code":"upstream_changed","message":"上游协议已变化"}
```

Operations:

| operation | payload | completed.result |
|---|---|---|
| `transcribeFile` | `filePath`, `params` | `TranscriptionResult` |
| `translate` | `model`, `text`, `source`, `target` | `{ "text": "..." }` |
| `setHotwords` | `hotwords[]` | object |
| `getHotwords` | object | `{ "hotwords": [...] }` |
| `clearHotwords` | object | object |
| `action` | `action` | object with optional `status`/`message` |

`TranscriptionResult` uses camelCase:

```json
{
  "durationMs": 1200,
  "transcripts": [{
    "channelId": null,
    "text": "完整文本",
    "sentences": [{
      "beginTime": 0,
      "endTime": 1200,
      "text": "完整文本",
      "sentenceId": null,
      "speakerId": null,
      "words": [{"beginTime":0,"endTime":500,"text":"完整","punctuation":null}]
    }]
  }]
}
```

## Package trust and updates

`integrity.files` must list every package file except `manifest.json`, using SHA-256. `signature` is Ed25519 over `sayit-plugin-signature-v1\n` plus canonical JSON of the normalized manifest with `signature.value` empty. The supplied signer implements the exact algorithm.

The host distinguishes `trusted`, `signed-untrusted`, `integrity-only` and `unsigned`. New keys and unsigned packages require explicit confirmation. Updates are staged and verified before activation; the prior version is moved to `plugin-backups` and can be rolled back.
An installed `signed-untrusted` plugin is visible for diagnosis but cannot run until its publisher key is explicitly trusted. Copying an unsigned directory directly into the plugins folder is a local developer escape hatch; normal distribution must use the plugin manager or installer confirmation flow.

## Protocol discipline

- stdout is protocol-only; sanitized diagnostics go to stderr.
- Never log credentials, cookies, tokens, audio payloads or user text.
- Validate malformed host messages and return structured errors.
- Close upstream resources on host cancellation, timeout and disconnect.
- Do not access files outside the explicit input path and plugin data directory.
