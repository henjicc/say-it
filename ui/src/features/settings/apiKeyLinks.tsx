import { ExternalLink } from "lucide-react";
import { CMD, cmd } from "@/lib/tauri";

export const API_KEY_URLS_BY_ADAPTER = {
  groq: "https://console.groq.com/keys",
  openai: "https://platform.openai.com/api-keys",
  anthropic: "https://platform.claude.com/settings/keys",
  gemini: "https://aistudio.google.com/app/apikey",
  volcengine: "https://console.volcengine.com/ark/apiKey",
  kimi: "https://platform.kimi.com/console/api-keys",
  bigmodel: "https://open.bigmodel.cn/usercenter/apikeys",
  deepseek: "https://platform.deepseek.com/api_keys",
  mimo: "https://platform.xiaomimimo.com/",
  bailian: "https://bailian.console.aliyun.com/cn-beijing?tab=globalset#/efm/api_key",
  minimax: "https://platform.minimaxi.com/console/access?tab=api-keys",
  open_router: "https://openrouter.ai/settings/keys",
} as const;

export const ASR_API_KEY_URLS: Readonly<Record<string, string>> = {
  bailian: API_KEY_URLS_BY_ADAPTER.bailian,
  volcengine: "https://console.volcengine.com/speech/new/setting/apikeys",
  siliconflow: "https://cloud.siliconflow.cn/account/ak",
  "llm-groq": API_KEY_URLS_BY_ADAPTER.groq,
};

export function providerApiKeyUrl(provider: { id: string; kind: string }): string | undefined {
  if (provider.kind.startsWith("llm:")) {
    const adapter = provider.kind.slice("llm:".length) as keyof typeof API_KEY_URLS_BY_ADAPTER;
    return API_KEY_URLS_BY_ADAPTER[adapter];
  }
  return ASR_API_KEY_URLS[provider.id];
}

export function ApiKeyLink({
  url,
  label,
  onError,
}: {
  url: string;
  label: string;
  onError: (message: string) => void;
}) {
  return (
    <a
      href={url}
      className="inline-flex items-center gap-1 text-[var(--color-accent-light)] underline-offset-4 hover:underline"
      onClick={(event) => {
        event.preventDefault();
        void cmd(CMD.openExternalLink, { url }).catch((error) => onError(String(error)));
      }}
    >
      {label}
      <ExternalLink className="h-3.5 w-3.5" aria-hidden />
    </a>
  );
}
