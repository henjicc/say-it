import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@/components/ui/Button";

/**
 * 页面渲染出错时只换掉内容区，标题栏和导航照常可用，并把错误显示出来。
 * 没有它时 React 会卸掉整个界面，窗口只剩背景色，也看不到原因。切到别的页面（换 key）即恢复。
 */
export class PageErrorBoundary extends Component<{ children: ReactNode }, { error?: Error }> {
  state: { error?: Error } = {};

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("页面渲染出错", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div role="alert" className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
        <p className="text-sm text-[var(--color-fg)]">这个页面出错了，切到别的页面再回来或重新加载即可恢复。</p>
        <p className="max-w-xl break-all font-mono text-xs text-[var(--color-fg-subtle)]">{error.message}</p>
        <Button size="sm" onClick={() => window.location.reload()}>重新加载</Button>
      </div>
    );
  }
}
