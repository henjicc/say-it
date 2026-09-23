import { cloneElement, useEffect, useId, useLayoutEffect, useRef, useState, type HTMLAttributes, type ReactElement, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** 只承载说明文字；链接、错误和操作反馈应留在页面中。 */
export function Tooltip({ content, children }: {
  content?: ReactNode;
  children: ReactElement<HTMLAttributes<HTMLElement>>;
}) {
  const id = useId();
  const anchor = useRef<HTMLElement | null>(null);
  const popup = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const clearTimer = () => { clearTimeout(timer.current); };
  const close = () => { clearTimer(); setOpen(false); };
  const schedule = (element: HTMLElement) => {
    if (!content) return;
    setOpen(false);
    anchor.current = element;
    clearTimer();
    timer.current = setTimeout(() => setOpen(true), 500);
  };
  const leave = () => {
    clearTimer();
    if (anchor.current?.contains(document.activeElement)) return;
    timer.current = setTimeout(() => setOpen(false), 120);
  };

  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // 第一次 Esc 只关提示，不连带关闭所在弹窗。
        if (open) event.stopPropagation();
        close();
      }
    };
    const onScroll = (event: Event) => {
      if (!(event.target instanceof Node) || !popup.current?.contains(event.target)) close();
    };
    document.addEventListener("keydown", dismiss, true);
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("keydown", dismiss, true);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    // 字段整体响应悬停，同时把说明关联到真正获得键盘焦点的控件。
    const focused = document.activeElement;
    const original = focused?.getAttribute("aria-describedby");
    const describeFocus = focused instanceof HTMLElement && anchor.current?.contains(focused) && focused !== anchor.current;
    if (describeFocus) focused.setAttribute("aria-describedby", [original, id].filter(Boolean).join(" "));
    document.dispatchEvent(new Event("sayit:tooltip-open"));
    document.addEventListener("sayit:tooltip-open", close);
    return () => {
      document.removeEventListener("sayit:tooltip-open", close);
      if (describeFocus) {
        if (original) focused.setAttribute("aria-describedby", original);
        else focused.removeAttribute("aria-describedby");
      }
    };
  }, [open, id]);

  useLayoutEffect(() => {
    if (!open || !anchor.current || !popup.current) return;
    const target = anchor.current.getBoundingClientRect();
    const tip = popup.current.getBoundingClientRect();
    const edge = 8;
    const below = target.bottom + edge;
    setPosition({
      left: Math.max(edge, Math.min(target.left, window.innerWidth - tip.width - edge)),
      top: Math.max(edge, below + tip.height <= window.innerHeight - edge ? below : target.top - tip.height - edge),
    });
  }, [open, content]);

  if (!content) return children;
  const props = children.props;
  return <>
    {cloneElement(children, {
      "aria-describedby": [props["aria-describedby"], open ? id : undefined].filter(Boolean).join(" ") || undefined,
      onMouseEnter: (event) => { props.onMouseEnter?.(event); schedule(event.currentTarget); },
      onMouseLeave: (event) => { props.onMouseLeave?.(event); leave(); },
      onFocus: (event) => { props.onFocus?.(event); schedule(event.currentTarget); },
      onBlur: (event) => { props.onBlur?.(event); if (!event.currentTarget.contains(event.relatedTarget)) close(); },
      onPointerDown: (event) => { props.onPointerDown?.(event); close(); },
    })}
    {open && createPortal(<div ref={popup} id={id} role="tooltip" className="ui-tooltip" style={position}
      onMouseEnter={clearTimer} onMouseLeave={leave}>{content}</div>, document.body)}
  </>;
}

/** 直接在标签上悬停或用 Tab 聚焦，不添加额外图标。 */
export function HelpLabel({ children, content }: { children: ReactNode; content?: ReactNode }) {
  if (!content) return <>{children}</>;
  return <Tooltip content={content}><span tabIndex={0} className="ui-help-label">
    {children}
  </span></Tooltip>;
}
