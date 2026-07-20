export default function createProvider(host) {
  let connectionId = null;

  return {
    initialize(request) {
      if (!request.config?.apiKey) {
        host.log("warn", "尚未配置供应商凭据");
      }
    },

    realtimeStart(request) {
      // 模型声明 supportsVocabulary 时 request.hotwords 为 [{ text, weight }]（weight 为 1-5），
      // 声明 supportsContext 时 request.context 为一段文本；字段为空则不会出现。
      // 供应商不支持权重就只取 text，权重区间不同则在这里换算，不要原样透传。
      // 按供应商协议构造 WSS 地址和鉴权信息；不要在日志中输出凭据。
      connectionId = host.websocket.open({
        url: `wss://api.example.com/realtime?model=${encodeURIComponent(request.model)}`,
      }).connectionId;
    },

    realtimeAudio(pcm16) {
      if (!connectionId) throw new Error("实时连接尚未建立");
      host.websocket.send(connectionId, pcm16);
    },

    realtimeFinish() {
      if (connectionId) host.websocket.send(connectionId, JSON.stringify({type: "finish"}));
    },

    realtimeStop() {
      if (connectionId) host.websocket.close(connectionId);
      connectionId = null;
    },

    onHostEvent(event) {
      if (event.connectionId !== connectionId) return;
      if (event.type === "websocketOpen") host.emit({type: "ready"});
      if (event.type === "websocketError") host.emit({type: "error", code: "upstream_error", message: event.message});
      if (event.type === "websocketClose") host.emit({type: "finished"});
      // 在这里解析供应商消息，并发出 partial、final 或 finished 标准事件。
    },
  };
}
