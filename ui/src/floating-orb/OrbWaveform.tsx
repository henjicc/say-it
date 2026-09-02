import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { floatingOrbWaveLayout } from "./waveform";
import { observeOrbGeometry } from "./observeGeometry";

interface OrbWaveformProps {
  levels: number[];
  variant?: "default" | "dense";
}

export function OrbWaveform({ levels, variant = "default" }: OrbWaveformProps) {
  const container = useRef<HTMLSpanElement>(null);
  const barCount = Math.max(1, levels.length);
  const [layout, setLayout] = useState(() => floatingOrbWaveLayout(0, 0, 1, { barCount }));

  useLayoutEffect(() => {
    const element = container.current;
    if (!element) return;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      setLayout(floatingOrbWaveLayout(rect.width, rect.left, window.devicePixelRatio, {
        barCount,
        barRatio: variant === "dense" ? 0.03 : 0.1,
        gapRatio: variant === "dense" ? 0.07 : 0.08,
      }));
    };
    return observeOrbGeometry(element, measure);
  }, [barCount, variant]);

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
