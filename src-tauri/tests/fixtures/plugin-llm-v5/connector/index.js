export default function createProvider(host) {
  return {
    invoke(request) {
      if (request.operation === "discoverModels") {
        return [{modelId: "chat", displayName: "Fixture Chat", contextWindow: 32768, maxOutputTokens: 4096}];
      }
      if (request.operation !== "chat") throw new Error(`unexpected operation: ${request.operation}`);
      host.emit({type: "reasoning", text: "fixture reasoning"});
      host.emit({type: "text", text: "fixture answer"});
      host.emit({type: "usage", data: {inputTokens: 1, outputTokens: 1, reasoningTokens: 1, totalTokens: 3}});
      host.emit({type: "finish", finishReason: "stop"});
      return {output: "fixture answer", reasoningOutput: "fixture reasoning", usage: null, finishReason: "stop"};
    },
  };
}
