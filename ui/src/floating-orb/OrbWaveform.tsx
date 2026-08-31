import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { floatingOrbWaveLayout } from "./waveform";
import { observeOrbGeometry } from "./observeGeometry";

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
    return observeOrbGeometry(element, measure);
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
