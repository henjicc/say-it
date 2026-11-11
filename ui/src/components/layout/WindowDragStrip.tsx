import { isMacOS } from "@/lib/platform";

/**
 * macOS 专业模式没有单独的标题栏：导航与各栏顶部各留一段与原标题栏同高的空白，
 * 系统红绿灯落在导航顶部，其余部分用来拖动窗口。放在滚动容器外，内容滚动时它不动。
 */
export function WindowDragStrip() {
  return isMacOS ? <div data-tauri-drag-region aria-hidden className="h-[var(--titlebar-h)] flex-none self-stretch" /> : null;
}
