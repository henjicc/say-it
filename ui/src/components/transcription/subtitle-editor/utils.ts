import type { EditableCue } from "@/features/transcription/subtitles";

export type DragMode = "move" | "left" | "right";

export interface DragState {
  id: string;
  mode: DragMode;
  startX: number;
  beginMs: number;
  endMs: number;
  /** 拖动开始前的字幕数组快照，供拖动结束时判断是否需要写入一条撤销历史。 */
  beforeCues: EditableCue[];
}

export interface PanState {
  startX: number;
  startScrollLeft: number;
}

export interface CueNeighbors {
  prevEnd: number;
  nextBegin: number;
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function formatZoom(scale: number) {
  return `${Math.round(scale * 100)}%`;
}

export function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tagName = target.tagName;
  // 不含 BUTTON：编辑页内空格键始终用于播放/暂停，不应被"此前点过的按钮仍持有焦点"劫持
  // （典型场景：点击窗口标题栏的最大化按钮后再按空格，浏览器会把空格当成对该按钮的默认点击）。
  return tagName === "INPUT" || tagName === "TEXTAREA" || tagName === "SELECT";
}

export function isInteractiveTarget(target: EventTarget | null) {
  return target instanceof HTMLElement
    && !!target.closest("button, input, textarea, select, a, label");
}

export function yieldToMain() {
  return new Promise<void>((resolve) => setTimeout(resolve, 0));
}

export function joinTexts(a: string, b: string) {
  const left = a.trimEnd();
  const right = b.trimStart();
  if (!left) return right;
  if (!right) return left;
  return /[a-zA-Z0-9]$/.test(left) && /^[a-zA-Z0-9]/.test(right) ? `${left} ${right}` : `${left}${right}`;
}

/**
 * 把字幕时间码统一取整到整数毫秒。
 *
 * 所有基于像素的写回路径（拖动整块、拉伸左右边缘、按播放头拆分、"设为播放头位置"）
 * 算出的都是浮点毫秒——100% 缩放下 1px = 1000/60 ≈ 16.67ms。而导出走的 Rust
 * `SubtitleCue` 的 `begin_ms`/`end_ms` 是 `i64`，serde 会以
 * `invalid type: floating point` 拒收，导致整条 `save_subtitle_srt` 命令失败。
 *
 * 值已经是整数时原样返回该 cue 对象，避免制造无谓的新引用。
 */
export function roundCueTimes(cues: EditableCue[]): EditableCue[] {
  return cues.map((cue) => {
    const beginMs = Math.round(cue.beginMs);
    const endMs = Math.round(cue.endMs);
    return beginMs === cue.beginMs && endMs === cue.endMs ? cue : { ...cue, beginMs, endMs };
  });
}
