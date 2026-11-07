export function formatMs(ms: number | null | undefined) {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms} 毫秒` : `${(ms / 1000).toFixed(2)} 秒`;
}

/** 生成速度；带 ≈ 的是按字符估算的。 */
export function formatRate(rate: number | null | undefined, estimated: boolean | undefined) {
  return rate === null || rate === undefined ? "—" : `${estimated ? "≈" : ""}${Math.round(rate)}`;
}

/** 模型勾选项的唯一键。 */
export function modelKey(option: { providerId: string; model: string }) {
  return JSON.stringify([option.providerId, option.model]);
}

export function formatTime(ms: number) {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${date.toLocaleTimeString("zh-CN", { hour12: false })}`;
}
