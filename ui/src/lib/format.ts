// 纯函数工具，移植自旧 ui/app.js 的工具段。

export function formatTime(ts: number | string | null | undefined): string {
  if (!ts) return "-";
  const d = new Date(Number(ts));
  if (Number.isNaN(d.getTime())) return "-";
  return d.toLocaleString();
}

export function formatToMime(format: string | undefined): string {
  const f = (format || "").toLowerCase();
  if (f === "mp3") return "audio/mpeg";
  if (f === "wav") return "audio/wav";
  if (f === "pcm") return "audio/L16";
  return "audio/aac";
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
  }
  return btoa(binary);
}

export function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function compactLogJson(value: unknown, limit = 420): string {
  let text = "";
  try {
    text = JSON.stringify(value);
  } catch {
    text = String(value);
  }
  if (text.length <= limit) return text;
  return `${text.slice(0, limit)} ...(${text.length - limit} chars truncated)`;
}

/** 追加一行带时间戳的日志，保留末尾 maxLines 行。 */
export function appendLogLine(prev: string, payload: unknown, maxLines = 20): string {
  const line = `${new Date().toLocaleTimeString()} ${compactLogJson(payload)}`;
  const base = prev ? `${prev}\n` : "";
  return `${base}${line}`.split("\n").slice(-maxLines).join("\n");
}
