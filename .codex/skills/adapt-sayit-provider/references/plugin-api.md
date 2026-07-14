# 「说吧！」供应商插件 API v3

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
  "apiVersion": 3,
  "runtime": {
    "kind": "javascript",
    "entrypoint": "connector/index.js",
    "hostApiVersion": 1,
    "permissions": ["network"],
    "network": { "allowedHosts": ["api.example.com", "*.example.com"] }
  }
}
```

权限只有 `network`、`browserSession`、`cookies`。声明 `network` 时白名单不能为空；仅允许精确主机或 `*.` 开头的子域规则，不写协议、端口和路径。

模型协议与场景：

- `plugin-realtime-v1`：`realtime`，场景含 `dictationRealtime` 或 `subtitles`。
- `plugin-file-v1`：`file`，场景含 `dictationFile` 或 `transcription`。
- `plugin-translation-v1`：`translation`，场景含 `subtitleTranslation`。

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

方法可以返回普通值或 Promise。每个实时会话和一次性调用使用独立上下文，模块全局状态不能跨会话共享。`initialize` 可选，接收供应商配置、受保护会话和权限快照。

`realtimeAudio` 接收单声道 16 kHz PCM16 小端序的 `Uint8Array`。插件不得自行采集麦克风、处理系统设备或注入文本。

一次性调用统一进入 `invoke({ operation, payload })`。常见操作为 `transcribeFile`、`translate`、`setHotwords`、`getHotwords`、`clearHotwords` 和 `action`。文件操作的 `payload.input` 只有 `id`、`name`、`size`；上传时把 `input.id` 交给宿主 HTTP 请求的 `inputId`，不能获得真实路径。

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
