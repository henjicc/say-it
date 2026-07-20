# 「说吧！」供应商插件 API v4

## 包结构

```text
provider.sayit
├── sayit-package.json
├── manifest.json
└── connector/
    ├── index.js
    └── 可选的相对导入模块与数据资源
```

`.sayit` 是 ZIP 格式，但只能包含声明、清单、可阅读 JavaScript 与必要数据资源。禁止 EXE、DLL、SO、DYLIB、Node 原生模块、Mach-O、ELF、PE、WASM 和符号链接。

## 清单

`sayit-package.json` 固定为：

```json
{"formatVersion":1,"kind":"provider-plugin","entry":"manifest.json"}
```

`manifest.json` 的运行时固定为：

```json
{
  "apiVersion": 4,
  "runtime": {
    "kind": "javascript",
    "entrypoint": "connector/index.js",
    "hostApiVersion": 1,
    "permissions": ["network"],
    "network": { "allowedHosts": ["api.example.com", "*.example.com"] }
  }
}
```

权限只有 `network`、`localNetwork`、`browserSession`、`cookies`。声明 `network` 时白名单不能为空；仅允许精确主机或 `*.` 开头的子域规则，不写协议、端口和路径。

`localNetwork` 仅用于连接本机服务：允许 `http://` / `ws://` 访问字面主机 `127.0.0.1`、`localhost`、`[::1]`，不要求写入 `allowedHosts`。它不允许局域网 IP、主机别名或公网明文地址。插件若还访问公网，必须同时声明 `network` 并列出最小公网主机白名单。v3 不允许声明 `localNetwork`。

网页会话插件可在 `browserSession` 中声明 `requiredCookieNames`。它是会话完整性校验用的非敏感 Cookie 名列表；宿主在保存前必须能从 `allowedUrls` 读取所有名称，否则拒绝覆盖原有受保护会话。`allowedUrls` 要覆盖实际登录页及需要读取路径级 Cookie 的页面，例如登录页为 `/chat` 时不要只写站点根路径。

若网页会话还依赖页面运行时生成的短时 URL（例如签名 WebSocket URL），在同一对象中声明 `capturedUrlCookie`，不要为该供应商修改宿主代码。该 Cookie 值必须是 Base64URL 编码 JSON，包含 `issuedAt`（毫秒时间戳）与 `url`：

```json
{
  "browserSession": {
    "loginUrl": "https://vendor.example/login",
    "allowedUrls": ["https://vendor.example/"],
    "requiredCookieNames": ["session", "temporary-url"],
    "capturedUrlCookie": {
      "cookieName": "temporary-url",
      "maxAgeMs": 240000,
      "freshnessSlackMs": 15000,
      "url": {
        "scheme": "wss",
        "host": "stream.vendor.example",
        "path": "/v1/live",
        "requiredQueryNames": ["client", "signature"]
      }
    }
  }
}
```

`cookieName` 必须同时出现在 `requiredCookieNames`。宿主会在同步会话和每次运行前按此规则校验短时凭据的格式、时效、目标 URL 与必要参数；任何插件都可使用这项声明。

模型协议与场景：

- `plugin-realtime-v1`：`realtime`，场景含 `dictationRealtime` 或 `subtitles`。
- `plugin-file-v1`：`file`，场景含 `dictationFile` 或 `transcription`。
- `plugin-translation-v1`：`translation`，场景含 `subtitleTranslation`。
- `plugin-ocr-v1`：`ocr`，场景含 `activeAppContext`。纯 OCR 插件可不声明模型，此时宿主按供应商生成场景下拉项。

## 模型能力字段（必须逐项如实声明）

这些字段直接决定用户在下拉里看到什么、以及哪些功能对该模型开放。宿主无法探测真实行为，
声明错了就是错的，**不要照抄模板默认值**。

### `category` + `emitsPartialResults`：出字方式

两者共同决定下拉标注，用户靠它判断"要不要等"：

| 实际行为 | `category` | `emitsPartialResults` | 下拉显示 |
| --- | --- | --- | --- |
| 边说边出字，中间结果可变 | `realtime` | `true` | 不加后缀 |
| 走实时会话，但说完一句才整句出字，无中间态 | `realtime` | `false` | `（整句）` |
| 停止后才开始识别 | `file` | `false` | `（非实时）` |

`category` 只区分"实时会话"与"文件批处理"，**装不下第二种**。只要模型不产出可变的中间
结果，就必须显式写 `"emitsPartialResults": false`，否则会被当成真流式，用户以为卡住了。

省略该字段时宿主按 `category` 兜底（`realtime` → `true`），仅为兼容旧清单，新模型一律显式声明。

### `supportsAlignmentTimestamps`：是否返回时间戳

指识别结果能否带**逐句或逐词的时间信息**。它 gate 了录音识别里的**文稿对齐**功能，
声明 `false` 的模型会被挡在功能外并提示用户换模型。

- 只有结果里真的带可用时间戳才写 `true`。
- 判断依据是**宿主最终能拿到什么**，不是模型理论上支持什么。若模型能返回时间戳但连接器
  没有把它透传进 `sentences`，仍然写 `false`——写 `true` 会让功能拿到空时间轴而失败。
- 文件识别结果的 `sentences` 为空数组时，必须写 `false`。

### `supportsVocabulary` 与 `supportsContext`：热词与上下文

宿主只维护**一份全局热词与上下文**（用户在「热词上下文」页面配置），按模型声明分别下发：

| 字段 | 含义 | 下发内容 |
| --- | --- | --- |
| `supportsVocabulary` | 供应商接受带权重的词表 | `hotwords: [{ text, weight }]` |
| `supportsContext` | 供应商接受一段上下文文本，靠其中出现的原词纠正专有名词 | `context: "..."`（已渲染并截断到 400 字符） |

两者相互独立：可以都声明、都不声明，或只声明其一；都声明时两个字段一起下发。
`supportsContext` 是可选字段，**省略等于 `false`**，宿主不会向未声明的模型下发上下文。
只有连接器真的把对应内容送给供应商并生效时才写 `true`——写错就是静默失效，宿主无法探测。

**权重的适配责任在连接器**：宿主的权重固定为 1–5 的整数。若供应商不支持权重，直接忽略
`weight` 只取 `text`；若供应商的权重区间不同（如 0–1 的浮点或 1–10 的整数），在连接器里
线性换算，不要把 1–5 原样透传。若供应商只接受纯词列表，用分隔符拼接 `text` 即可。

热词与上下文在下面这些调用里出现（**字段为空时不会出现**，据此可以区分"用户没配"和"模型不支持"）：

```js
// 实时：模型声明的能力决定收到哪些字段
realtimeStart({ providerId, model, sampleRate, config, hotwords, context })
// 文件：同上
invoke({ operation: "transcribeFile", payload: { filePath, params, hotwords, context } })
```

`setHotwords` / `getHotwords` / `clearHotwords` 是另一回事：它们用于**需要预先在云端建词表**的
供应商，由用户在「热词上下文 → 供应商同步」里触发。随请求下发的模型不需要实现这三个操作。

### 声明前先实测

新接入的模型必须跑一遍真实音频，并据实回填上述字段；同时留意结果**是否带标点**——
不带标点的模型在长听写和字幕场景体验差异很大，应在 `label` 或插件说明里提示用户。

API v3 兼容说明：宿主继续接受 v3 的 `asr`、`translation`、`customization` JavaScript 插件；v3 不得声明 `ocr`、`localNetwork` 或 `model-pack`。新插件默认使用 v4，不要为了兼容主动降级。

## 入口接口

入口模块默认导出同步工厂函数：

```js
export default function createProvider(host) {
  return {
    initialize(request) {},
    realtimeStart(request) {},
    realtimeAudio(pcm16) {},
    realtimeFinish() {},
    realtimeStop() {},
    invoke(request) {},
    onHostEvent(event) {},
  };
}
```

方法可以返回普通值或 Promise。每个实时会话和一次性调用使用独立上下文，模块全局状态不能跨会话共享。`initialize` 可选，接收供应商配置、受保护会话和权限快照；实时方法的请求不保证再次附带 `session`，需要在 `initialize` 时把当前会话保留在该会话上下文中，不能用空请求覆盖它。

`realtimeAudio` 接收单声道 16 kHz PCM16 小端序的 `Uint8Array`。插件不得自行采集麦克风、处理系统设备或注入文本。

一次性调用统一进入 `invoke({ operation, payload })`。常见操作为 `transcribeFile`、`translate`、`recognizeImage`、`setHotwords`、`getHotwords`、`clearHotwords` 和 `action`。文件操作的 `payload.input` 只有 `id`、`name`、`size`；上传时把 `input.id` 交给宿主 HTTP 请求的 `inputId`，不能获得真实路径。

`transcribeFile` 的 `payload` 与 `realtimeStart` 的请求还可能带 `hotwords` 与 `context`，规则见上文的模型能力字段。

翻译操作接收宿主提供的文本、源语言、目标语言与模型等字段；不要依赖未声明字段。流式增量通过 `host.emit({ type: "delta", text })` 发出，最终返回供应商响应中归一化后的结果。

OCR 操作固定为：

```js
const result = await provider.invoke({
  operation: "recognizeImage",
  payload: { imageBase64: "<PNG Base64>", purpose: "activeAppContext" },
});
```

返回值固定为 `{ blocks: [{ text, region: { x, y, width, height }, confidence? }] }`。`region` 使用相对原图的 0~1 坐标；无文字时返回空 `blocks`，不得把失败伪装为空结果。图像可能包含用户正在编辑的内容，严禁写日志、存储或转发到清单未声明的主机。

## 宿主 API

```js
host.http.request({ method, url, headers, bodyText, bodyBase64, inputId })
host.websocket.open({ url, headers })
host.websocket.send(connectionId, stringOrUint8Array)
host.websocket.close(connectionId)
host.base64.encode(bytes)
host.base64.decode(text)
host.text.decodeUtf8(bytes)
host.crypto.randomBytes(size)
host.crypto.sha256(textOrBytes)
host.crypto.hmacSha256(key, data)
host.time.now()
host.time.sleep(milliseconds)
host.storage.get(key)
host.storage.set(key, value)
host.storage.delete(key)
host.resource.readBytes(relativePath)
host.resource.readText(relativePath)
host.cancellation.isCancelled()
host.emit(event)
host.log(level, message)
```

HTTP 返回 `{ status, headers, bodyText, bodyBase64 }`。请求、重定向与 WebSocket 都受白名单限制。WebSocket 事件串行交给 `onHostEvent`，类型为 `websocketOpen`、`websocketMessage`、`websocketError`、`websocketClose`，并包含 `connectionId`。

`host.storage` 仅保存非敏感、小型 JSON 状态。`host.resource` 只能读取包内不超过 1 MiB 的相对资源。密钥、Cookie 和令牌应来自配置或会话，不得写入存储或资源。取消或超时会中断 JavaScript，并关闭宿主管理的网络资源。

QuickJS 不提供 Node 或浏览器 DOM。需要把 UTF-8 字节转为文本时，使用 `host.text.decodeUtf8(bytes)`；为兼容旧插件，运行时仅提供 UTF-8 版 `TextDecoder`，不要依赖其它浏览器 API 或 `TextDecoder` 的流式/编码选项。

## 标准事件

实时识别通过 `host.emit` 发出：

```js
host.emit({ type: "ready" });
host.emit({ type: "partial", text: "临时文本" });
host.emit({ type: "final", text: "最终文本" });
host.emit({ type: "finished" });
host.emit({ type: "error", code: "upstream_error", message: "可诊断信息" });
```

一次性调用的进度或增量也用 `host.emit({ type: "progress" | "delta" | "event", ... })`，最终值由 `invoke` 返回。错误应抛出 `Error`，不要伪造成功结果。

## 不存在的能力

运行时没有 Node、DOM、`fetch`、文件系统、环境变量、进程、Shell、原生模块、Tauri IPC、主窗口或悬浮窗访问能力。模块只能相对导入插件目录内的 `.js`/`.mjs`；裸模块、绝对路径和目录穿越都会被拒绝。
