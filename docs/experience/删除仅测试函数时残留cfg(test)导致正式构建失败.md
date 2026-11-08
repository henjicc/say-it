# 删除仅测试函数时残留 cfg(test) 导致正式构建失败

## 现象

重构后 `npm run test:rust` 全部通过，`tauri:dev` 却编译失败，报 `cannot find function ...`。

## 原因

删掉一个带 `#[cfg(test)]` 的包装函数时，只删了函数体，属性留了下来。属性于是附到下一个函数上，那个函数在正式构建里消失了。测试构建会编进所有 `cfg(test)` 项，所以测试照样通过，问题完全被掩盖。

## 做法

- 删除或合并带 `#[cfg(test)]`、`#[cfg(debug_assertions)]` 等条件属性的函数时，连同属性一起删除；删完看一眼下一项上方是否多出了属性。
- 改动 Rust 函数签名或删除函数后，除了跑测试，还要跑一次正式构建的检查：`cargo check --manifest-path src-tauri/Cargo.toml -p say-it`（不带 `--tests`）。
