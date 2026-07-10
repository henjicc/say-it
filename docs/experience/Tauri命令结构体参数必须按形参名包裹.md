# Tauri 命令结构体参数必须按形参名包裹

## 触发条件

Rust 命令以结构体作为具名形参，例如 `async fn connect_obs(request: ObsConnectionRequest)`。

## 现象

前端直接调用 `invoke("connect_obs", { host, port, password })` 时，Tauri 报错：`missing required key request`。

## 正确做法

`invoke` 的最外层对象对应 Rust 函数的形参名，结构体字段应放入该形参之下：

```ts
invoke("connect_obs", { request: { host, port, password } });
```

结构体内部使用 `#[serde(flatten)]` 只影响 `request` 内部的 JSON 形状，不会移除最外层的 Rust 形参名。
