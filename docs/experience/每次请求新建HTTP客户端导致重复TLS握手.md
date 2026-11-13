# 每次请求新建 HTTP 客户端导致重复 TLS 握手

## 现象

智能处理首字慢，开发者追踪里连接耗时约 1.3～1.5 秒（走本机代理时更明显），同一供应商连续请求也不见变快。

## 根因

插件运行时（`plugin_runtime.rs`、`sdk_upload.rs`）每次请求都新建 reqwest 客户端（当时大模型还经 genai 直连，也是如此），连接池随之丢弃，每个请求都重做 TCP + TLS 握手。

## 做法

- 走 `providers/shared_http.rs` 的 `SharedHttpClient`，进程内共用客户端。内置 SDK 与插件的请求都经 QuickJS 宿主的这个客户端发出。
- reqwest 只在建客户端时读一次系统代理。`SharedHttpClient` 每次取用都用 `hyper_util` 的 `Matcher::from_system()` 比对，代理一变就重建，否则用户切换代理后请求会一直连旧地址。
- 新增请求代码不要再自己 `reqwest::Client::builder()`，测握手成本的开发探测（`dev_trace.rs` 的 `probe_connection`）除外。

## 边界

- 空闲约 90 秒后，连接池里的连接会被回收（reqwest 默认值，服务端也会关），再请求时仍要握手。实测空闲 150 秒后首字回到约 2.1 秒。
- 只改代理账号密码不会触发重建，要重启应用。
