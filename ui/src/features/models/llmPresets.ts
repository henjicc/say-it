import { API_KEY_URLS_BY_ADAPTER } from "@/features/settings/apiKeyLinks";

/**
 * 可添加的大模型服务。阿里云百炼、火山引擎（方舟）与 Groq 已内置且包含大模型，不在这里出现。
 * 有推荐模型目录的供应商以目录第一项为默认模型，没有目录的才在这里写默认模型。
 */
export const LLM_PROVIDER_PRESETS = [
  { adapter: "openai", name: "OpenAI", model: "gpt-4o-mini", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.openai },
  { adapter: "anthropic", name: "Anthropic", model: "claude-haiku-4-5", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.anthropic },
  { adapter: "gemini", name: "Google Gemini", model: "gemini-2.5-flash", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.gemini },
  { adapter: "kimi", name: "Kimi", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.kimi },
  { adapter: "bigmodel", name: "智谱 GLM", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.bigmodel },
  { adapter: "deepseek", name: "DeepSeek", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.deepseek },
  { adapter: "mimo", name: "小米 MiMo", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.mimo },
  { adapter: "minimax", name: "MiniMax", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.minimax },
  { adapter: "ppio", name: "派欧云", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.ppio },
  { adapter: "open_router", name: "OpenRouter", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.open_router },
  { adapter: "custom", name: "自定义 OpenAI 兼容接口", apiKeyUrl: null },
] as const;
