# git 列文件时中文路径被转义，按扩展名筛选会漏掉文件

## 现象

用 `git ls-files | grep -E '\.(png|jpg)$'` 盘点仓库里的图片，带中文名的图片一个都没列出来，
误以为仓库里只有图标。

## 原因

`core.quotepath` 默认开启，`git ls-files`、`git diff --name-only`、`git status` 会把非 ASCII
路径写成 `"docs/images/\346\210\252.jpg"`：两端加引号、中文转成八进制。行尾是引号，`\.jpg$`
匹配不上。

## 做法

凡是要按路径或扩展名筛选 git 输出，一律加 `-c core.quotepath=false`，脚本里再配合 `-z`
按 `\0` 分割：

```sh
git -c core.quotepath=false ls-files -z
```

`scripts/check-private-content.mjs` 调 git 时已统一带上这个参数。
