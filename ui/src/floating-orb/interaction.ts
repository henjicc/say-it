export type OrbPhase =
  | "idle"
  | "armed"
  | "moving"
  | "positioning"
  | "recording"
  | "processing"
  | "smartProcessing"
  | "success"
  | "fallback"
  | "error"
  | "cancelled"
  | "busy"
  | "submitting"
  | "submitted";

export type OrbClickAction = "activate" | "stop" | "submit" | "showError";
export type OrbContextAction = "cancel" | "dismissSubmit" | "dismissError" | "menu";

export const ORB_DRAG_THRESHOLD = 5;
/**
 * 悬浮球大小，单位为屏幕参考边长（显示器逻辑分辨率较短边）的十分之一
 * 百分比，例如 45 表示 4.5%。换算像素时由后端综合当前显示器的分辨率与
 * 缩放比例计算，确保同一百分比在不同屏幕上呈现一致的相对视觉尺寸。
 */
export const FLOATING_ORB_SIZE_PERCENT_RANGE = { min: 25, max: 80 } as const;
export const FLOATING_ORB_OPACITY_RANGE = { min: 40, max: 100 } as const;
export const FLOATING_ORB_GLASS_TINT_RANGE = { min: 0, max: 40 } as const;
export const FLOATING_ORB_GLASS_BORDER_RANGE = { min: 0, max: 30 } as const;
export const FLOATING_ORB_GLASS_MATERIALS = ["underWindow", "content", "sidebar"] as const;
export type FloatingOrbGlassMaterial = (typeof FLOATING_ORB_GLASS_MATERIALS)[number];
export const DEFAULT_FLOATING_ORB_APPEARANCE = {
  sizePercent: 45,
  opacity: 100,
  glassEnabled: false,
  glassMaterial: "sidebar" as FloatingOrbGlassMaterial,
  glassTint: 8,
  glassBorder: 0,
} as const;

export interface FloatingOrbAppearance {
  sizePercent: number;
  opacity: number;
  glassEnabled: boolean;
  glassMaterial: FloatingOrbGlassMaterial;
  glassTint: number;
  glassBorder: number;
}

function clampInteger(value: unknown, min: number, max: number, fallback: number): number {
  const number = Number(value);
  return Number.isFinite(number) ? Math.round(Math.max(min, Math.min(max, number))) : fallback;
}

export function normalizeFloatingOrbAppearance(payload: {
  sizePercent?: number;
  opacity?: number;
  glassEnabled?: boolean;
  glassMaterial?: string;
  glassTint?: number;
  glassBorder?: number;
}): FloatingOrbAppearance {
  return {
    sizePercent: clampInteger(
      payload.sizePercent,
      FLOATING_ORB_SIZE_PERCENT_RANGE.min,
      FLOATING_ORB_SIZE_PERCENT_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.sizePercent,
    ),
    opacity: clampInteger(
      payload.opacity,
      FLOATING_ORB_OPACITY_RANGE.min,
      FLOATING_ORB_OPACITY_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.opacity,
    ),
    glassEnabled: payload.glassEnabled === true,
    glassMaterial: FLOATING_ORB_GLASS_MATERIALS.includes(
      payload.glassMaterial as FloatingOrbGlassMaterial,
    )
      ? payload.glassMaterial as FloatingOrbGlassMaterial
      : DEFAULT_FLOATING_ORB_APPEARANCE.glassMaterial,
    glassTint: clampInteger(
      payload.glassTint,
      FLOATING_ORB_GLASS_TINT_RANGE.min,
      FLOATING_ORB_GLASS_TINT_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.glassTint,
    ),
    glassBorder: clampInteger(
      payload.glassBorder,
      FLOATING_ORB_GLASS_BORDER_RANGE.min,
      FLOATING_ORB_GLASS_BORDER_RANGE.max,
      DEFAULT_FLOATING_ORB_APPEARANCE.glassBorder,
    ),
  };
}

export function shouldStartOrbDrag(deltaX: number, deltaY: number): boolean {
  return Math.hypot(deltaX, deltaY) >= ORB_DRAG_THRESHOLD;
}

export function shouldHandleOrbClick(detail: number, dragged: boolean): boolean {
  // 键盘激活没有鼠标点击次数；拖动结束产生的合成 click 不得启动听写。
  return detail === 0 || !dragged;
}

export function floatingOrbWaveScale(value: number): number {
  const normalized = Math.max(0, Math.min(1, Number(value) || 0));
  return Math.min(1, Math.sqrt(normalized) * 1.8);
}

export function floatingOrbClickAction(
  phase: OrbPhase,
  canSubmit: boolean,
): OrbClickAction | null {
  if (phase === "idle" || phase === "armed") return "activate";
  if (phase === "recording") return "stop";
  if (phase === "success" && canSubmit) return "submit";
  if (phase === "error") return "showError";
  return null;
}

export function floatingOrbContextAction(
  phase: OrbPhase,
  canSubmit: boolean,
  transient: boolean,
): OrbContextAction | null {
  if (phase === "error") return "dismissError";
  if (phase === "recording") return "cancel";
  if (phase === "success" && canSubmit) return "dismissSubmit";
  if (phase === "idle" && !transient) return "menu";
  return null;
}

export function floatingOrbLabel(
  phase: Exclude<OrbPhase, "idle" | "busy">,
  message: string,
): string {
  if (message) return message;
  return ({
    moving: "正在启动…",
    positioning: "正在定位…",
    armed: "点击开始语音输入",
    recording: "聆听中…",
    processing: "识别中…",
    smartProcessing: "处理中…",
    success: "已完成并复制",
    fallback: "已复制，请手动粘贴",
    error: "语音输入失败",
    cancelled: "已取消",
    submitting: "正在发送回车…",
    submitted: "已发送回车",
  } as const)[phase];
}
