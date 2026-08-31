import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { floatingOrbWaveLayout } from "./waveform";

export function OrbWaveform({ levels }: { levels: number[] }) {
  const container = useRef<HTMLSpanElement>(null);
  const [layout, setLayout] = useState(() => floatingOrbWaveLayout(0, 0, 1));

  useLayoutEffect(() => {
    const element = container.current;
    if (!element) return;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      setLayout(floatingOrbWaveLayout(rect.width, rect.left, window.devicePixelRatio));
    };
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    // 手势临时球的入场动画缩放父按钮，不会触发 ResizeObserver；结束后再按最终坐标对齐。
    const parent = element.parentElement;
    parent?.addEventListener("animationend", measure);
    let resolution: MediaQueryList;
    const watchResolution = () => {
      resolution?.removeEventListener("change", watchResolution);
      resolution = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      resolution.addEventListener("change", watchResolution);
      measure();
    };
    watchResolution();
    return () => {
      observer.disconnect();
      parent?.removeEventListener("animationend", measure);
      resolution.removeEventListener("change", watchResolution);
    };
  }, []);

  return (
    <span ref={container} className="orb-waveform" aria-hidden>
      {layout.offsets.map((left, index) => (
        <span
          key={index}
          className="orb-wave-bar"
          style={{
            left,
            width: layout.width,
            minHeight: layout.width,
            "--bar-scale": Math.max(0.18, levels[index] ?? 0),
          } as CSSProperties}
        />
      ))}
    </span>
  );
}
