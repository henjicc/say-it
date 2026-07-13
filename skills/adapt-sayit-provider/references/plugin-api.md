# SayIt provider plugin API v1

## Package layout

```text
<plugin-id>/
├── manifest.json
└── bin/
    └── connector.exe
```

SayIt scans `%LOCALAPPDATA%\com.henjicc.sayit\plugins\*/manifest.json` at startup. A malformed plugin is isolated and reported without preventing the application from starting.

## Manifest

```json
{
  "apiVersion": 1,
  "id": "vendor-asr",
  "name": "Vendor ASR",
  "version": "1.0.0",
  "provider": {
    "id": "vendor-asr",
    "displayName": "Vendor ASR",
    "authKind": "api-key",
    "capabilities": ["asr"],
    "config": { "apiKey": "", "region": "" },
    "configFields": [
      { "key": "apiKey", "label": "API Key", "fieldType": "password", "secret": true },
      { "key": "region", "label": "Region", "fieldType": "text", "secret": false }
    ],
    "actions": []
  },
  "models": [
    {
      "id": "vendor-realtime-v1",
      "label": "Vendor Realtime V1",
      "providerId": "vendor-asr",
      "category": "realtime",
      "protocol": "process-jsonl-v1",
      "supportsVocabulary": false,
      "supportsAlignmentTimestamps": false,
      "scenes": ["dictationRealtime", "subtitles"],
      "isDefaultRealtime": false,
      "isDefaultFile": false
    }
  ],
  "runtime": {
    "kind": "process",
    "entrypoint": "bin/connector.exe",
    "args": [],
    "protocolVersion": 1,
    "permissions": ["network"]
  }
}
```

IDs accept lowercase ASCII letters, digits, dots, and hyphens; maximum length is 64. The entrypoint must be a relative path inside the plugin directory. Supported permissions are `network`, `browserSession`, and `cookies`.

Supported `fieldType` values in the generic UI are `text`, `password`, `number`, and `boolean`. Secret fields are stored by the backend and omitted from frontend snapshots.

## Host-to-plugin JSONL

Every message is a single UTF-8 JSON object followed by `\n`.

Start is always first:

```json
{"type":"start","protocolVersion":1,"sessionId":"uuid","providerId":"vendor-asr","model":"vendor-realtime-v1","sampleRate":16000,"config":{"apiKey":"..."},"permissions":["network"]}
```

Audio may repeat:

```json
{"type":"audio","pcm16Base64":"AAABAP//"}
```

Normal completion:

```json
{"type":"finish"}
```

Immediate cancellation:

```json
{"type":"stop"}
```

## Plugin-to-host JSONL

After upstream readiness:

```json
{"type":"ready"}
```

Recognition events:

```json
{"type":"partial","text":"临时结果"}
{"type":"final","text":"最终结果"}
```

After all final results following `finish`:

```json
{"type":"finished"}
```

Fatal failure:

```json
{"type":"error","code":"auth_failed","message":"认证失败"}
```

Optional diagnostic event:

```json
{"type":"event","name":"reconnected"}
```

SayIt terminates the process after `finished`, `error`, cancellation, stdout closure, invalid JSON, or an eight-second finish timeout.

## Audio and result rules

- Decode `pcm16Base64` into mono signed PCM16 little-endian at the declared 16 kHz rate.
- Do not reinterpret it as float samples or add a container header unless the upstream API requires one.
- Map unstable hypotheses to `partial` and committed sentences to `final`.
- Emit `finished` only after all upstream final messages have been forwarded.
- Never include protocol logs on stdout.
