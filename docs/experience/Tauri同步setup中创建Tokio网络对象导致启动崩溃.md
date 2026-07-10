# Tauri 同步 setup 中创建 Tokio 网络对象导致启动崩溃

## 触发条件

在 Tauri 的同步 `setup` 回调调用链中，将 `std::net::TcpListener`、socket 等标准库网络对象转换为 Tokio 网络对象。

## 现象

应用启动时 panic：`there is no reactor running, must be called from the context of a Tokio 1.x runtime`。

## 原因与正确做法

`tokio::net::TcpListener::from_std` 会立即向当前 Tokio reactor 注册 IO；仅仅在后续使用 `tauri::async_runtime::spawn` 并不能为此前的转换补充运行时上下文。

同步阶段只做端口绑定和非阻塞配置，把 `tokio::net::*::from_std` 放入 `tauri::async_runtime::spawn(async move { ... })` 内执行，确保创建 Tokio 网络对象时已经进入 Tauri 管理的异步运行时。
