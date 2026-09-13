import { useCallback, useRef, useState } from "react";

import { Button } from "./Button";
import { Modal } from "./Modal";

export interface ConfirmOptions {
  /** 弹窗标题，例如「确认删除」。 */
  title: string;
  /** 正文说明。后果不可撤销时务必写清楚。 */
  message: React.ReactNode;
  /** 确认按钮文案，默认「确定」。 */
  confirmLabel?: string;
  /** 取消按钮文案，默认「取消」。 */
  cancelLabel?: string;
  /**
   * 确认按钮是否用危险样式，默认 true。
   * 删除/清空这类不可撤销的操作保持 true；仅仅是二次告知（例如"将把数据发往云端"）
   * 可以传 false。
   */
  danger?: boolean;
}

interface PendingConfirm extends ConfirmOptions {
  resolve: (confirmed: boolean) => void;
}

/**
 * 应用内二次确认弹窗，用来替代 `window.confirm`。
 *
 * `window.confirm` 在本项目这套 `decorations: false` 的自绘 Tauri/WebView2 窗口里
 * **不会呈现对话框**，`if (!window.confirm(...)) return;` 会直接放行——破坏性操作
 * 在用户看不到任何提示的情况下就执行了。详见
 * `docs/experience/window.confirm在设置窗口不弹出导致破坏性操作无确认.md`。
 *
 * 用法刻意做成和 `window.confirm` 近似，改造时每处只需加 `await` 并渲染 `{dialog}`：
 *
 * ```tsx
 * const { confirm, dialog } = useConfirm();
 * async function remove() {
 *   if (!(await confirm({ title: "确认删除", message: "删除后不可恢复。" }))) return;
 *   // …执行删除
 * }
 * return (<>{dialog}<Button onClick={() => void remove()}>删除</Button></>);
 * ```
 */
export function useConfirm() {
  const [pending, setPending] = useState<PendingConfirm | null>(null);
  // 关闭动画期间 pending 已被清空，用 ref 保证 resolve 只会被调用一次。
  const settledRef = useRef<((confirmed: boolean) => void) | null>(null);

  const settle = useCallback((confirmed: boolean) => {
    const resolve = settledRef.current;
    settledRef.current = null;
    setPending(null);
    resolve?.(confirmed);
  }, []);

  const confirm = useCallback(
    (options: ConfirmOptions) =>
      new Promise<boolean>((resolve) => {
        // 同一时刻只允许一个确认框：上一个若还没结果，按取消处理，避免调用方的
        // await 永远悬着。
        settledRef.current?.(false);
        settledRef.current = resolve;
        setPending({ ...options, resolve });
      }),
    [],
  );

  const dialog = (
    <Modal
      open={Boolean(pending)}
      onClose={() => settle(false)}
      title={pending?.title}
      showCloseButton={false}
      className="max-w-[430px]"
    >
      <div className="p-5">
        <div className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">{pending?.message}</div>
        <div className="mt-6 flex justify-end gap-2">
          <Button
            size="sm"
            variant={pending?.danger === false ? "primary" : "dangerHover"}
            onClick={() => settle(true)}
          >
            {pending?.confirmLabel ?? "确定"}
          </Button>
          {/* 取消是默认焦点：破坏性弹窗里回车不应当直接落在确认上。 */}
          <Button size="sm" variant="primary" autoFocus onClick={() => settle(false)}>
            {pending?.cancelLabel ?? "取消"}
          </Button>
        </div>
      </div>
    </Modal>
  );

  return { confirm, dialog };
}
