import { estimateAudioDurationCost, type CapabilityPricing, type TokenListPricing } from "@henjicc/ai-sdk/pricing";

/** 后端模型目录下发的 SDK 参考价（官方按量标准价，不含免费额度与限时优惠）。 */
export interface SdkPricing {
  /** 按 SDK 能力模块 ID。 */
  capabilities: Record<string, CapabilityPricing>;
  /** 按大模型供应商预设 ID（与 `recommendedLlmModels` 的 adapter 一致），再按模型 ID。 */
  llm: Record<string, Record<string, TokenListPricing>>;
}

/** 选择面板里显示的参考价；perMillionTokens 为 true 时 text 是「输入 / 输出」每百万 token 的单价。 */
export interface ModelPrice {
  text: string;
  perMillionTokens?: boolean;
}

export const FREE_PRICE: ModelPrice = { text: "免费" };

const symbol = (currency: "CNY" | "USD") => (currency === "USD" ? "$" : "¥");

/** 一元以上保留两位小数，以下保留两位有效数字（如 $0.075），都去掉末尾的 0。 */
function amount(value: number): string {
  return String(Number(value >= 1 ? value.toFixed(2) : value.toPrecision(2)));
}

/** 分档计价按最低一档显示，加「起」；分时计价显示高峰价。 */
export function tokenPrice(pricing: TokenListPricing): ModelPrice {
  const unit = symbol(pricing.currency);
  const from = pricing.tiers?.length ? " 起" : "";
  return {
    text: `${unit}${amount(pricing.inputPerMillionTokens)} / ${unit}${amount(pricing.outputPerMillionTokens)}${from}`,
    perMillionTokens: true,
  };
}

/** 识别模型按每小时音频显示；按 token 计费的用 SDK 登记的折算比例估算，前面加「约」。 */
export function audioPrice(pricing: CapabilityPricing | undefined): ModelPrice | undefined {
  const hour = pricing && estimateAudioDurationCost(pricing, 3_600_000);
  if (!hour) return undefined;
  if (hour.amount === 0) return FREE_PRICE;
  return { text: `${pricing.billing === "tokens" ? "约 " : ""}${symbol(hour.currency)}${amount(hour.amount)}/时` };
}
