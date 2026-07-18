# Tauri Windows 覆盖配置会重新引入基础配置已移除的资源

## 触发条件

项目同时使用 `src-tauri/tauri.conf.json` 与 `src-tauri/tauri.windows.conf.json`，并需要从安装包移除某类 resource。

## 现象

只从基础配置的 `bundle.resources` 删除资源后，`tauri build` 仍成功，最终 NSIS 安装包却继续包含该资源。原因是 Windows 覆盖配置也独立声明了同一路径，打包时仍会合并进最终配置。

## 正确做法

1. 修改打包资源时同时检查基础配置和平台覆盖配置，不能只看 `tauri.conf.json`。
2. 完整构建后直接检查最终安装包内容；编译成功不能证明资源已经移除。
3. Windows NSIS 可用 `7z l <setup.exe>` 核对目标文件是否存在，避免仅凭配置差异推断最终产物。
