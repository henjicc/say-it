import { useLayoutEffect, useState, type RefObject } from "react";
import { observeOrbGeometry } from "./observeGeometry";
import { floatingOrbStrokeWidth } from "./stroke";

export function useOrbStroke(orb: RefObject<HTMLButtonElement | null>) {
  const [width, setWidth] = useState(() => floatingOrbStrokeWidth(
    Math.min(window.innerWidth, window.innerHeight), window.devicePixelRatio,
  ));
  useLayoutEffect(() => {
    const button = orb.current;
    const frame = button?.parentElement;
    if (!button || !frame) return;
    // 窗口已由 Rust 根据屏幕分辨率/缩放计算；使用未被按钮入场动画缩放的外框，
    // 不重复乘屏幕比例，也不让描边反过来改变自身测量结果。
    return observeOrbGeometry(frame, () => {
      const rect = frame.getBoundingClientRect();
      setWidth(floatingOrbStrokeWidth(Math.min(rect.width, rect.height), window.devicePixelRatio));
    }, button);
  }, [orb]);
  return width;
}
