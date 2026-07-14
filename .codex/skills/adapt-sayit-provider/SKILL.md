---
name: adapt-sayit-provider
description: 在当前工作目录中创建可分发的「说吧！」供应商插件。用于调研语音识别 API 或用户提供的逆向项目、实现 process-jsonl-v2 连接器、生成清单、校验、签名并产出单个 .sayit 文件。
---

# 适配「说吧！」供应商

创建独立的供应商插件包。不得修改「说吧！」应用本体，也不得把供应商专属状态机复制进前端。
校验和签名工具需要 Python 3 与 `cryptography` 包。

## 当前工作目录边界

把当前任务工作目录视为 `WORK_ROOT`。默认只能在此目录内搜索、读取、创建、编辑、构建和写入；只有用户明确提供某个外部路径及用途时，才可访问该外部路径，例如官方文档或逆向供应商项目。

- 所有生成内容固定放到 `WORK_ROOT/sayit-plugin-work/<插件 ID>/`，不得顺手在根目录创建 `plugins/` 等目录。
- `source/` 放可编辑的连接器源码，`build/` 放已签名运行时目录，`keys/` 放发布者私钥，`dist/` 只放最终分发文件。
- 供应商构建命令的工作目录必须是 `source/`；不要把无关目录当构建或输出目录。
- 最终只能交付一个 `dist/<插件 ID>-<版本>.sayit` 文件；除非用户明确要求，不得自动安装。

## 工作流程

1. 将本 Skill 所在目录记为 `SKILL_DIR`，将 `PLUGIN_ROOT` 设为 `WORK_ROOT/sayit-plugin-work/<插件 ID>`。开始前初始化：
   `python "$SKILL_DIR/scripts/init_plugin_workspace.py" "$PLUGIN_ROOT" --template "$SKILL_DIR/assets/plugin-template" --work-root "$WORK_ROOT"`。
2. 只在 `PLUGIN_ROOT/source/` 中工作。调研官方文档或用户提供的外部实现，确认鉴权、流式传输、PCM 要求、临时/最终结果语义、收尾方式与会话续期机制。
3. 创建文件前完整阅读 [插件接口规范](references/plugin-api.md)。涉及网页登录、Cookie、页面参数或非官方接口时，再完整阅读 [特权与逆向供应商](references/privileged-providers.md)。
4. 替换 `source/manifest.json` 中所有占位符。ID 必须稳定、全小写且全局唯一。
5. 只在 `source/` 下实现独立连接器。凭据从宿主发来的配置读取；不得向 stdout 或 stderr 输出凭据。
6. 将发行版可执行文件构建到 `source/<runtime.entrypoint>`。
7. 先执行 `python "$SKILL_DIR/scripts/smoke_test_plugin.py" "$PLUGIN_ROOT/source"`，再覆盖测试每项已声明操作：畸形输入、取消、超时和上游断连。
8. 在 `build/` 生成运行时目录：`python "$SKILL_DIR/scripts/package_plugin.py" "$PLUGIN_ROOT/source" "$PLUGIN_ROOT/build/<插件 ID>-<版本>" --work-root "$WORK_ROOT"`。
9. 签名：`python "$SKILL_DIR/scripts/sign_plugin.py" "$PLUGIN_ROOT/build/<插件 ID>-<版本>" --private-key "$PLUGIN_ROOT/keys/publisher.pem" --key-id <稳定发布者 ID> --work-root "$WORK_ROOT"`。私钥绝不能放进插件包。
10. 校验：`python "$SKILL_DIR/scripts/validate_plugin.py" "$PLUGIN_ROOT/build/<插件 ID>-<版本>"`；随后归档：`python "$SKILL_DIR/scripts/archive_plugin.py" "$PLUGIN_ROOT/build/<插件 ID>-<版本>" "$PLUGIN_ROOT/dist/<插件 ID>-<版本>.sayit" --work-root "$WORK_ROOT"`。
11. 将 `.sayit` 文件作为唯一交付物报告给用户。仅当用户明确要求安装时，才提示其在「说吧！」插件管理器中选择该文件；不得把它自动安装到应用目录。

## 宿主能力边界

- 插件 API 版本为 `2`，仍兼容 v1 实时插件。
- 支持进程隔离的 `process-jsonl-v2` 实时协议，以及 v2 一次性调用协议。
- 支持实时 ASR、文件 ASR、字幕翻译和热词定制。
- 音频输入为单声道、16 kHz、PCM16 小端序，以 Base64 放进 JSONL 消息。
- 特权供应商使用独立网页登录 WebView、按 URL 白名单读取 Cookie、并通过 DPAPI 保护会话；不会接触主窗口或悬浮窗的 WebView 数据。
- `.sayit` 是统一压缩包后缀。宿主先读取包内 `sayit-package.json` 的 `kind` 与 `entry`，再按规范分派；当前支持 `provider-plugin`。
- 包内文件要做完整性校验；首次使用新签名密钥需要显式信任；更新保留回滚备份。

## 实现约束

- 供应商专属协议、鉴权、重连与解析必须留在连接器可执行文件内。
- stdout 是协议通道，每行只能输出一个 JSON 对象，不能输出日志。
- stderr 只能输出脱敏诊断，不得包含 Cookie、令牌、音频数据或用户文本。
- 只能返回接口规范定义的标准事件。
- 遇到错误采样率或畸形宿主消息时，返回结构化 `error` 事件。
- 对逆向接口固定端点和数据结构假设；上游变化时给出明确兼容性错误。
- 只声明实际需要的能力、操作、浏览器 URL 和权限；宿主拒绝未声明操作。
- 不得自动注册账号、绕过验证码、规避风控，也不得使用用户未明确提供的会话。

## 完成条件

只有在清单校验与签名验证通过、发行版入口存在、每个声明操作都有烟雾测试、特权会话有过期/清除测试，并且 `PLUGIN_ROOT/dist/` 中恰好有一个 `.sayit` 文件时，才能报告完成。报告该文件的绝对路径，并明确说明它尚未被安装。
