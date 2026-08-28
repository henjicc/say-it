type JsonRecord = Record<string, unknown>

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isTextOnlyContent(value: unknown): boolean {
  if (value === undefined || value === null || typeof value === 'string') return true
  if (!Array.isArray(value)) return false
  return value.every(part => (
    isRecord(part)
    && part.type === 'text'
    && typeof part.text === 'string'
    && Object.keys(part).every(key => key === 'type' || key === 'text')
  ))
}

function invalidTextOnlyRequest(message: string): Error {
  return Object.assign(new Error(message), {
    code: 'SAYIT_TEXT_ONLY',
    statusCode: 400,
  })
}

/**
 * Say-It 的内置 Groq 会话当前只接受文本。SDK 0.2.2 的通用 LLM chat 入口会静态带入
 * 全部 generation 媒体上传策略；在构建期把那层替换为这个明确拒绝多媒体的边界，既不
 * 复制 SSE/LLM 执行内核，也不让不相关供应商上传和 generation credential 进入 bundle。
 */
export async function preprocessRequestBody(
  providerId: string,
  route: string,
  body: unknown,
): Promise<unknown> {
  if (providerId.trim().toLowerCase() !== 'groq' || route !== '/v1/chat/completions') {
    throw invalidTextOnlyRequest('Say-It Groq 文本预处理器只允许 Groq chat/completions')
  }
  if (!isRecord(body) || !Array.isArray(body.messages)) {
    throw invalidTextOnlyRequest('Say-It Groq 请求缺少 messages')
  }
  for (const message of body.messages) {
    if (!isRecord(message) || !isTextOnlyContent(message.content)) {
      throw invalidTextOnlyRequest('Say-It 内置 Groq 当前只接受文本消息')
    }
  }
  return body
}
