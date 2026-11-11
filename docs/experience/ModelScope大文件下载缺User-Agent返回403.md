# ModelScope 大文件下载缺 User-Agent 返回 403

## 现象

从官方列表下载 Qwen3-ASR 等模型包，界面报「下载模型失败：HTTP 403 Forbidden」。同一个包里的许可证文本能下载，用 curl 访问同一地址也正常。

## 根因

reqwest 默认不发送 User-Agent。ModelScope 对走 LFS 存储的大文件（权重）拒绝不带 User-Agent 的请求，小文本文件不受这个限制。curl 自带 User-Agent，所以无法用它复现。

## 做法

- 模型相关下载统一走 `model_download::https_client()`，在这里设置 `user_agent("SayIt")`。不要在别处另建不带 User-Agent 的客户端去下模型。
- 用 curl 排查时，加 `-A ''` 模拟不带 User-Agent 的请求：

```bash
curl -s -o /dev/null -w '%{http_code}' -A '' -r 0-0 -L <权重地址>
```
