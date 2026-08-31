# 百炼 TLS 握手 EOF：检查 AI 代理规则是否抢先匹配

## 定位方法

`provider_realtime_error` 包裹的 `tls handshake eof` 发生在 HTTP 鉴权和识别协议之前，不应直接归因于 API Key、音频或 JavaScript 插件。

1. 不携带凭据或音频，对同一百炼接口进行 HTTPS/WSS 握手；对比应用 TLS 与系统 TLS。
2. 核对本机解析结果与代理日志。`198.18.*` 地址在 Fake-IP 模式下是正常映射，本身不是故障证据。
3. 必要时通过公开 DNS 查询当前服务地址，仅在一次诊断请求中保留原域名/SNI 与证书校验、指定目标 IP 作对照。不要把服务 IP 固定进产品、hosts 或正式配置。
4. 检查实际命中规则及出口。此次 `ai-all` 抢先匹配 `dashscope.aliyuncs.com`，将国内百炼送到海外节点；代理路径握手 EOF，而直连正常返回无凭据请求预期的 HTTP 401。401 只证明握手及 HTTP 可达，不能证明凭据有效或识别成功。

## 持久处理原则

经用户授权，在宽泛 AI 代理规则之前添加 `DOMAIN-SUFFIX,dashscope.aliyuncs.com,DIRECT`，保留其它规则和 Fake-IP。应修改 Clash Verge 的持久自定义规则/扩展入口，不直接编辑重新生成就会覆盖的运行配置。重新加载后验证真实命中规则和握手，再由用户完成识别验收。

无需为了这类路由问题切换 TLS 库、关闭证书校验、重试音频上传，或将所有 AI 流量强制直连。未获授权时只诊断和给出定向修改方案，不改系统代理。

参考：[Clash Verge 自定义规则](https://www.clashverge.dev/guide/rules.html)、[扩展配置的持久入口](https://www.clashverge.dev/guide/extend.html)。
