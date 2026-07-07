export type DragMode = "move" | "left" | "right";

export interface DragState {
  id: string;
  mode: DragMode;
  startX: number;
  beginMs: number;
  endMs: number;
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
