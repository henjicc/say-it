# 特权与逆向供应商

当供应商需要官方网站登录、Cookie、页面运行时生成的设备参数或非官方浏览器接口时，使用本宿主路径。不得把供应商专属登录逻辑写入「说吧！」应用。

## 通用宿主契约

在清单中声明 `browserSession`，并同时声明 `browserSession` 与 `cookies` 权限：

```json
{
  "provider": {
    "actions": ["openLogin", "syncSession", "clearSession", "diagnose"]
  },
  "runtime": {
    "permissions": ["network", "browserSession", "cookies"]
  },
  "browserSession": {
    "loginUrl": "https://vendor.example/login",
    "allowedUrls": ["https://vendor.example/", "https://ws.vendor.example/"],
    "windowTitle": "供应商登录",
    "userAgent": "可选的精确浏览器 UA",
    "initializationScript": "可选的捕获脚本，最大 64 KiB"
  }
}
```

- `openLogin` 创建专用且持久的 WebView，绝不会使用「说吧！」主窗口或悬浮窗的 WebView。
- `syncSession` 只读取声明的 HTTPS URL 对应 Cookie，序列化时不把值暴露给 React，并使用当前 Windows 用户的 DPAPI 加密保存。
- 解密后的会话仅通过 JSONL stdin 的 `session` 传给连接器，绝不放入配置、命令行参数或环境变量。
- `clearSession` 删除加密快照并清除专用 WebView 数据。
- 其他已声明操作以 `action` 调用传给连接器。

初始化脚本可以观察页面自身 fetch/XHR 的查询参数，并写入一个短期第一方标记 Cookie。标记含义必须由连接器解释，不能由宿主硬编码。脚本仅限已声明供应商域名，绝不能收集无关浏览数据。

## 逆向供应商约束

- 只使用用户显式登录建立的会话；不得静默导入其他浏览器配置。
- 分离长期 Cookie、短期令牌和捕获到的设备/请求参数。
- 检测会话过期并返回清晰的重新登录错误。
- 固定端点与数据结构假设，并提供 `diagnose` 操作来报告兼容性，诊断不得泄露秘密。
- 不得自动解验证码、伪造指纹、绕过风控、创建账号或收集凭据。
- 非官方 URL 构造、签名、WebSocket 解析与上游回退必须全部留在连接器可执行文件内。

## 验收测试

确认登录窗口隔离、允许域名 Cookie 收集、DPAPI 在「说吧！」重启后仍可读取、会话清除、主窗口关闭后的 ASR、上游结构变化诊断，以及 stdout/stderr/领域事件中零秘密泄露。
