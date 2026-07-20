---
name: adapt-sayit-provider
description: 在当前工作目录中创建跨 Windows 与 macOS 的「说吧！」JavaScript 供应商插件。用于调研官方 ASR、OCR、翻译 API 或用户明确提供的逆向项目、实现 API v4 连接器、校验签名并产出单个 .sayit 文件。
---

# 适配「说吧！」供应商

创建由「说吧！」内嵌 JavaScript 运行时执行的 ASR、OCR 或翻译供应商插件。不得修改或依赖「说吧！」源代码，不得生成 EXE、动态库、WASM 或其他平台相关产物。校验和签名脚本需要 Python 3 与 `cryptography`，烟雾测试需要 Node.js。

## 工作目录边界

把任务启动时的当前目录记为 `WORK_ROOT`。默认只在该目录内搜索、读取和写入。用户明确提供外部路径及用途时，才可读取对应路径；生成物仍必须回到 `WORK_ROOT`。

- 固定使用 `WORK_ROOT/sayit-plugin-work/<插件 ID>/`，不要在应用仓库或其他目录写代码。
- `source/` 放可阅读源码，`build/` 放待归档签名包，`keys/` 放发布者私钥，`dist/` 只放最终文件。
- 最终只交付 `dist/<插件 ID>-<版本>.sayit`，不自动安装，也不访问应用安装目录。

## 工作流程

1. 将本 Skill 目录记为 `SKILL_DIR`，运行：`python "$SKILL_DIR/scripts/init_plugin_workspace.py" "$WORK_ROOT/sayit-plugin-work/<插件 ID>" --template "$SKILL_DIR/assets/plugin-template" --work-root "$WORK_ROOT"`。
2. 完整阅读 [插件接口规范](references/plugin-api.md)。涉及网页登录、Cookie、网页逆向或非官方接口时，再完整阅读 [特权与逆向供应商](references/privileged-providers.md)。
3. 仅在 `source/` 内修改 `manifest.json` 与 `connector/`。按实际能力调研并确认鉴权、请求/响应、音频或图像格式、临时/最终结果、收尾、取消、超时和会话续期。实测供应商是否返回中间结果、时间戳与标点，据此回填模型能力字段，不要沿用模板默认值。若网页登录会话还依赖短时签名 URL，必须在 `browserSession.capturedUrlCookie` 声明 Cookie、有效期和 URL 规则；不得要求或实现按插件 ID 的宿主侧特判。
4. 插件默认导出 `createProvider(host)`；只使用规范中的 `host`，不要引用 Node、DOM、文件系统、环境变量、进程、Shell、Tauri IPC 或原生模块。
5. 运行 `python "$SKILL_DIR/scripts/smoke_test_plugin.py" "$PLUGIN_ROOT/source"`，再用模拟响应覆盖已声明方法的解析、错误、取消与断连。
6. 生成纯源码包目录：`python "$SKILL_DIR/scripts/package_plugin.py" "$PLUGIN_ROOT/source" "$PLUGIN_ROOT/build/<插件 ID>-<版本>" --work-root "$WORK_ROOT"`。
7. 签名：`python "$SKILL_DIR/scripts/sign_plugin.py" "$PLUGIN_ROOT/build/<插件 ID>-<版本>" --private-key "$PLUGIN_ROOT/keys/publisher.pem" --key-id <稳定发布者 ID> --work-root "$WORK_ROOT"`。私钥不得放入包内。
8. 校验：`python "$SKILL_DIR/scripts/validate_plugin.py" "$PLUGIN_ROOT/build/<插件 ID>-<版本>"`。
9. 归档：`python "$SKILL_DIR/scripts/archive_plugin.py" "$PLUGIN_ROOT/build/<插件 ID>-<版本>" "$PLUGIN_ROOT/dist/<插件 ID>-<版本>.sayit" --work-root "$WORK_ROOT"`。
10. 解包检查归档只含 `sayit-package.json`、`manifest.json`、`connector/` 中的 JavaScript 与必要数据资源。把 `.sayit` 的绝对路径作为唯一交付物，并明确尚未安装。

## 关键约束

- 新插件默认使用 API v4；运行时固定为 `javascript`，宿主 API 固定为 v1。只有维护既有插件且未使用 `ocr`、`localNetwork` 时才保留 v3。
- 模型协议只能使用 `plugin-realtime-v1`、`plugin-file-v1`、`plugin-translation-v1`、`plugin-ocr-v1`，并与能力、类别和场景严格匹配。
- 模型能力字段必须按实测如实声明，宿主无法探测真实行为，照抄模板默认值就是错的：实时模型必须显式写 `emitsPartialResults`（真流式 `true`，说完一句才整句出字的写 `false`，否则用户看不到「（整句）」标注、以为界面卡住）；`supportsAlignmentTimestamps` 按宿主最终能否拿到时间戳判断，连接器没把时间戳透传进 `sentences` 就必须写 `false`。判定表见 [插件接口规范](references/plugin-api.md) 的「模型能力字段」。
- 音频由宿主完成 DSP 和 PCM16 转换，以 `Uint8Array` 传给 `realtimeAudio`。
- 文件识别只能使用宿主给出的不透明输入句柄；不得接收或猜测本地路径。
- OCR 统一通过 `invoke({ operation: "recognizeImage", payload })`；输入是 PNG Base64 与用途，输出必须是带 0~1 归一化区域的文本块。
- 翻译统一通过 `invoke({ operation: "translate", payload })`；增量用 `host.emit({ type: "delta", ... })`，最终结果由 `invoke` 返回。
- 公网网络仅能访问 `runtime.network.allowedHosts` 中声明的 HTTPS/WSS 主机；跳转目标也必须在白名单内。
- 只有确需连接本机服务时才声明 v4 `localNetwork`。它只额外允许 `127.0.0.1`、`localhost`、`[::1]` 的 HTTP/WS，不得借此访问局域网地址；若还访问公网，必须另加 `network` 与最小 `allowedHosts`。
- Cookie 与登录会话由宿主独立 WebView 和系统凭据库管理，插件只能接收本次调用允许的会话数据。
- `browserSession.capturedUrlCookie` 适用于页面把 `{ issuedAt, url }` 以 Base64URL JSON 写入 Cookie 的短时凭据场景；完整字段、校验规则和示例见 [插件接口规范](references/plugin-api.md)。
- 只声明实际需要的权限、模型、操作和登录 URL；日志必须脱敏，不记录凭据、音频或用户文本。
- 不自动注册账号、绕过验证码或规避风控，不使用用户未授权的会话。

## 完成条件

清单、源码烟雾测试、完整性与签名校验均通过；包内无路径穿越、符号链接、原生可执行文件或动态库；`dist/` 中恰好一个 `.sayit` 文件。任何条件未满足都不能报告完成。
