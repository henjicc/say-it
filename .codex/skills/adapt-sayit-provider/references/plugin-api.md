# 「说吧！」供应商插件 API v5

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
  "apiVersion": 5,
  "id": "example-provider",
  "source": { "namespace": "example-provider" },
  "capabilities": [{
    "moduleId": "example-provider.speech-recognition.example-live",
    "kind": "speech-recognition",
    "providerIds": ["example-provider"],
    "modelId": "example-live",
    "operations": ["speech-recognition"],
    "features": ["streaming", "partial-results"],
    "tags": [],
    "executionModes": ["realtime"]
  }],
  "models": [{
    "id": "example-live", "label": "Example Live",
    "providerId": "example-provider",
    "capabilityId": "example-provider.speech-recognition.example-live",
    "isDefaultRealtime": false, "isDefaultFile": false
  }],
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

`localNetwork` 仅用于连接本机服务：允许 `http://` / `ws://` 访问字面主机 `127.0.0.1`、`localhost`、`[::1]`，不要求写入 `allowedHosts`。它不允许局域网 IP、主机别名或公网明文地址。插件若还访问公网，必须同时声明 `network` 并列出最小公网主机白名单。

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

## Capability 与模型目录

Plugin API 只接受 v5。`source.namespace` 必须与插件 ID 一致，宿主会强制补成 SDK source
`{ kind: "plugin", namespace }`，不接受插件伪造内置来源，也不保留 v3/v4 执行分支。

`capabilities` 使用 SDK descriptor 的可移植子集：`moduleId`、`kind`、`providerIds`、`modelId`、
`operations`、`features`、`tags`、`executionModes`。ASR 的 `kind` 为 `speech-recognition`，翻译为
`translation`，LLM 为 `llm`；执行模式只允许 `request-response`、`event-stream`、`realtime`，其中 realtime
仅允许语音识别。宿主使用真实 SDK CapabilityClient 拒绝重复 module ID 和重复 provider/kind/model
坐标，错误会同时指出待注册 module 和已占用 module/source。

`models` 不再重复声明 `category`、`protocol`、`scenes` 或 `supports*`。每项只保存
`id`、`label`、`providerId`、`capabilityId` 与两个可选默认项；宿主从已校验 descriptor 投影应用目录。
真实能力通过 features 声明：`partial-results`、`timestamps`、`vocabulary`、`context`。只有连接器
确实把对应结果或输入透传到宿主时才能声明。每个 capability 必须恰好有一个 models 条目。

LLM module 还必须声明 `acceptedInputKinds`（至少含 `text`，可扩展 `image`、`audio`、`video`）、
`modelDiscovery`，以及可选的 `contextWindow`、`maxOutputTokens`。`executionModes` 仅允许
`request-response`、`event-stream`。`features` 使用 `reasoning`、`usage`、`sampling`、`tool-call`、
`parallel-tools`、`json-output`、`structured-schema`；不得夸大实际能力。模型发现只能返回清单中
已静态声明且已有 module 的模型 ID，不能把临时发现结果直接变成不可执行选项。

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

这些方法是轻量插件 ABI，由宿主的唯一适配器注册成 SDK capability；插件不打包 SDK，也不得复制 CapabilityClient。方法可以返回普通值或 Promise。每个实时会话和一次性调用使用独立上下文，模块全局状态不能跨会话共享。`initialize` 可选，接收非敏感配置、受保护会话和权限快照；secret 字段不在 request.config 中，必须按声明字段调用 `await host.credentials.get(field)`。

`realtimeAudio` 接收单声道 16 kHz PCM16 小端序的 `Uint8Array`。插件不得自行采集麦克风、处理系统设备或注入文本。

一次性调用统一进入 `invoke({ operation, payload })`。常见操作为 `transcribeFile`、`translate`、`recognizeImage`、`setHotwords`、`getHotwords`、`clearHotwords` 和 `action`。文件操作的 `payload.input` 只有 `id`、`name`、`size`；上传时把 `input.id` 交给宿主 HTTP 请求的 `inputId`，不能获得真实路径。

`transcribeFile` 的 `payload` 与 `realtimeStart` 的请求还可能带 `hotwords` 与 `context`，规则见上文的模型能力字段。

翻译操作接收宿主提供的文本、源语言、目标语言与模型等字段；不要依赖未声明字段。流式增量通过 `host.emit({ type: "delta", text })` 发出，最终返回供应商响应中归一化后的结果。

LLM 调用进入 `invoke({ operation: "chat", payload })`。`payload` 是 SDK 聊天请求，包含
`providerId`、`modelId`、`messages`、`requestId`、`mode` 以及可选 reasoning/capabilities/policy。
正文和推理增量分别发送 `{type:"text",text}`、`{type:"reasoning",text}`；用量发送
`{type:"usage",data}`，结束发送 `{type:"finish",finishReason}`，失败发送 `{type:"error",message}`。
最终返回 `{output,reasoningOutput,usage,finishReason}`；SDK 统一发射 `Usage → Finish → Done`。
模型发现进入 `invoke({operation:"discoverModels",payload})`，返回
`[{modelId,displayName,contextWindow,maxOutputTokens}]`。

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
host.credentials.get(field)
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
