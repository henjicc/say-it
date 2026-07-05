# GitHub Release 从 CHANGELOG 提取版本更新日志

## 触发条件

项目使用 tag 触发 GitHub Release，但工作流里的发布说明是写死的，导致新版本发布后 Release 页面仍显示旧说明或“初始发布版本”。

## 正确做法

维护单独的 `CHANGELOG.md`，按版本分节，例如 `## [0.2.0] - 2026-07-05`。

GitHub Actions 发布时根据 `package.json` 版本号，从 `CHANGELOG.md` 提取对应版本段落并写入 `gh release create --notes-file`。

这样 README、CHANGELOG 和 GitHub Releases 只需要维护一份版本说明，发版时不会漏改或写错。
