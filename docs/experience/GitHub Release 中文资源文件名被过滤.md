# GitHub Release 中文资源文件名被过滤

## 触发条件

在 GitHub Actions 中上传 Release asset 时，如果安装包文件名包含中文，GitHub Release 最终展示和下载 URL 可能会过滤中文字符，例如 `说吧_0.1.0_x64-setup.exe` 变成 `_0.1.0_x64-setup.exe`。

## 正确做法

Release asset 的真实文件名使用 ASCII，例如 `SayIt_0.1.0_x64-setup.exe`。

默认不要设置 asset label，让 GitHub Releases 直接显示真实英文文件名。

如果以后明确需要中文展示名，再使用 GitHub CLI 支持的 asset label 语法：

```powershell
gh release create v0.1.0 "path/to/SayIt_0.1.0_x64-setup.exe#说吧 v0.1.0 x64 安装包"
```

这样下载文件名保持稳定兼容，GitHub 页面可用中文 label 展示给用户。
