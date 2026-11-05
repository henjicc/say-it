export function formatMs(ms: number | null | undefined) {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms} 毫秒` : `${(ms / 1000).toFixed(2)} 秒`;
}

export function formatTime(ms: number) {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${date.toLocaleTimeString("zh-CN", { hour12: false })}`;
}
