# 「说吧！」供应商插件 API v2

## 运行时边界

「说吧！」负责麦克风采集、DSP、模型与供应商选择、任务状态、文件路径、字幕和文本注入。供应商插件是独立可执行程序，只负责供应商鉴权与传输，并返回标准 JSONL 事件。v1 实时插件仍可加载；新插件必须使用 API/协议 v2。

最终分发物是一个 `.sayit` 文件。它是 ZIP 压缩包，宿主解压后根目录必须如下：

```text
<说吧包根目录>/
├── sayit-package.json
├── manifest.json
└── bin/
    ├── connector.exe
    └── 运行所需的其他文件
```

`sayit-package.json` 是统一类型声明。当前供应商插件必须声明：

```json
{
  "formatVersion": 1,
  "kind": "provider-plugin",
  "entry": "manifest.json"
}
```

`.sayit` 文件允许未来承载其他「说吧！」类型；宿主先读取 `kind` 再分派。目前仅支持 `provider-plugin`。宿主从插件管理器接收该文件，解压到私有暂存目录，验证后才安装到 `%LOCALAPPDATA%\com.henjicc.sayit\plugins`。

构建必须位于当前工作根目录的 `sayit-plugin-work/` 中。除非用户明确给出外部路径，不得访问当前工作目录外的文件。ID 使用全小写 ASCII 字母、数字、点和连字符，最长 64 个字符。入口与完整性路径必须留在包内，禁止符号链接。

## 清单能力与模型

- `asr`：实时和/或文件语音识别。
- `translation`：字幕翻译。
- `customization`：设置、读取、清除热词。
- 实时模型：`category` 为 `realtime`，`protocol` 为 `process-jsonl-v2`，场景为 `dictationRealtime` 和/或 `subtitles`。
- 文件模型：`category` 为 `file`，`protocol` 为 `process-file-v2`，场景为 `dictationFile` 和/或 `transcription`。
- 翻译模型：`category` 为 `translation`，`protocol` 为 `process-translation-v2`，场景为 `subtitleTranslation`。

普通供应商应以模板 `manifest.json` 为完整示例。配置字段类型支持 `text`、`password`、`number`、`boolean`。密钥字段不会出现在前端快照中。权限支持 `network`、`browserSession`、`cookies`。

## 实时流协议

每条消息是一个 UTF-8 JSON 对象并以 `\n` 结尾。第一条为 `start`：

```json
{"type":"start","protocolVersion":2,"sessionId":"uuid","providerId":"vendor","model":"vendor-live","sampleRate":16000,"config":{},"session":null,"permissions":["network"]}
```

随后宿主连续发送 Base64 编码的 PCM16 小端序、单声道 16 kHz 音频，最后发送 `finish` 或 `stop`：

```json
{"type":"audio","pcm16Base64":"AAABAP//"}
{"type":"finish"}
```

连接器依次输出 `ready`、`partial`、`final`、`finished`；致命错误使用 `error`：

```json
{"type":"ready"}
{"type":"partial","text":"临时结果"}
{"type":"final","text":"最终结果"}
{"type":"finished"}
{"type":"error","code":"auth_failed","message":"认证失败"}
```

## 一次性调用协议

对于非实时能力，宿主每次启动一个新的连接器，只发送一个 `invoke`，然后关闭 stdin。`config` 是供应商配置；`session` 仅在用户显式同步后包含受系统保护的浏览器会话；`payload` 按操作定义。

```json
{"type":"invoke","protocolVersion":2,"requestId":"uuid","operation":"translate","providerId":"vendor","config":{},"session":null,"permissions":["network"],"payload":{}}
```

连接器可以输出 `progress`、`delta`、`event`，最后恰好输出一个 `completed`；也可输出 `error`。用户取消文件任务时，宿主会终止连接器进程；stdin 关闭后不得依赖还能收到额外 JSON 消息。

```json
{"type":"delta","text":"新增译文"}
{"type":"completed","result":{}}
{"type":"error","code":"upstream_changed","message":"上游协议已变化"}
```

操作定义：

| 操作 | `payload` | `completed.result` |
|---|---|---|
| `transcribeFile` | `filePath`、`params` | `TranscriptionResult` |
| `translate` | `model`、`text`、`source`、`target` | `{ "text": "..." }` |
| `setHotwords` | `hotwords[]` | 对象 |
| `getHotwords` | 对象 | `{ "hotwords": [...] }` |
| `clearHotwords` | 对象 | 对象 |
| `action` | `action` | 可包含 `status` / `message` 的对象 |

`TranscriptionResult` 使用 camelCase：

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

## 包信任与更新

`integrity.files` 必须列出除 `manifest.json` 外的每个包内文件，并使用 SHA-256。`signature` 使用 Ed25519，对 `sayit-plugin-signature-v1\n` 加上规范化清单 JSON 签名；计算时 `signature.value` 为空。随 Skill 提供的签名器实现了同一算法。

宿主状态包括 `trusted`、`signed-untrusted`、`integrity-only`、`unsigned`。新密钥和未签名包都需要用户明确确认。更新先在暂存目录完成验证，再启用新版本；旧版本会移入 `plugin-backups`，可回滚。签名有效但密钥未信任的插件仅可诊断，不能执行。

把未签名目录直接复制进插件目录仅用于本地开发兼容；正常分发必须由用户在插件管理器中手动选择 `.sayit` 文件并确认信任。

## 协议纪律

- stdout 只能传协议；脱敏诊断写入 stderr。
- 不得记录凭据、Cookie、令牌、音频载荷或用户文本。
- 校验畸形宿主消息，并返回结构化错误。
- 在取消、超时和断连时关闭上游资源。
- 不得访问显式输入文件与插件数据目录之外的文件。
