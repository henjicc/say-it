# Windows 启动 Tauri 时 spawnSync 找不到 npm 命令入口

## 触发条件与根因

Node 启动包装脚本通过 `spawnSync("tauri", ...)` 调用 npm CLI 时，即使本地已安装 `@tauri-apps/cli`，Windows 仍可能报 `spawnSync tauri ENOENT`。npm 提供的是 `.cmd` 命令入口，不能直接当作无 shell 的可执行文件启动；这不代表 Tauri 依赖缺失。

## 正确做法

- 使用 `createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js")` 解析项目本地 CLI 入口。
- 通过 `spawnSync(process.execPath, [cliPath, ...args], options)` 启动，保持参数数组、环境变量、标准流和退出码传递。
- 不依赖全局安装或 npm 注入的 PATH，也不为此增加 shell 字符串拼接，避免中文、空格路径及特殊参数的转义问题。
- Windows 与 macOS 共用同一启动方式，平台专属环境变量与打包后校验仍留在原有分支。

## 回归验证

运行 `node --test scripts/run-tauri.test.mjs`，覆盖真实 CLI 版本查询、dev/build 参数透传、错误退出和缺少命令；随后运行 `npm run tauri:dev` 验证完整启动链路。
