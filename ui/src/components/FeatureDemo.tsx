import { useState } from "react";
import { cn } from "@/lib/cn";
import "./FeatureDemo.css";

export type FeatureDemoKind = "translate" | "edit" | "ask" | "orb" | "shake" | "shakeDirect" | "clicks";

/** 波形条高度：左侧几根渐隐，与原生胶囊一致。 */
const WAVE = [6, 10, 14, 8, 18, 12, 22, 16, 10, 20, 12, 8, 4];
const WAVE_FADE = [0.35, 0.48, 0.61, 0.74, 0.87];
const ORB_WAVE = [6, 12, 16, 10, 5];

const MIC_PATH = ["M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z", "M19 10v2a7 7 0 0 1-14 0v-2", "M12 19v3"];

function stepsOf(kind: FeatureDemoKind, keyLabel: string, hold: boolean, clickCount: number): [string, string, string] {
  const press = `${hold ? "按住" : "按"} ${keyLabel}`;
  switch (kind) {
    case "translate": return [press, "说话", "译文出现在光标处"];
    case "edit": return [`选中文字，${press}`, "说出修改要求", "原文被替换"];
    case "ask": return [press, "说出问题", "回答浮在旁边"];
    case "orb": return ["点悬浮球", "说话，再点一下", "文字上屏"];
    case "shake": return ["晃动鼠标，点出现的悬浮球", "说话，再点一下", "文字上屏"];
    case "shakeDirect": return ["晃动鼠标", "说话", "文字上屏"];
    case "clicks": return [`左键快速连击 ${clickCount} 次`, "说话", "文字上屏"];
  }
}

/**
 * 功能示意：用一小段动画说明「怎么触发 → 说什么 → 结果出现在哪」。
 * 平时静止，鼠标停在示意上或 play 为真时循环播放；系统要求减少动态效果时不播放。
 */
export function FeatureDemo({
  kind,
  play = false,
  keyLabel = "快捷键",
  hold = false,
  clickCount = 3,
}: {
  kind: FeatureDemoKind;
  play?: boolean;
  keyLabel?: string;
  hold?: boolean;
  clickCount?: number;
}) {
  const [hover, setHover] = useState(false);
  const steps = stepsOf(kind, keyLabel, hold, clickCount);
  return (
    <div
      role="img"
      aria-label={steps.join("，")}
      className={cn("fd", `fd-${kind === "orb" ? "orb-click" : kind}`, hold && "fd-hold", (play || hover) && "fd-play")}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      <div className="fd-scene" aria-hidden>
        <Scene kind={kind} keyLabel={keyLabel} clickCount={clickCount} />
      </div>
      <div className="fd-steps" aria-hidden>
        {steps.map((step, index) => <span key={index} className={`fd-s${index + 1}`}><b>{index + 1}</b>{step}</span>)}
      </div>
    </div>
  );
}

function Scene({ kind, keyLabel, clickCount }: { kind: FeatureDemoKind; keyLabel: string; clickCount: number }) {
  switch (kind) {
    case "translate":
      return <>
        <Win><div className="fd-ph" style={{ width: 140 }} /><Output text="I'll send you the proposal tomorrow afternoon." /></Win>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say="明天下午把方案发你" />
      </>;
    case "edit":
      return <>
        <Win>
          <div className="fd-ph" style={{ width: 260 }} />
          <div className="fd-txt"><span className="fd-sel"><span className="fd-sel-bg" /><span className="fd-sel-old">这个方案我觉得还行吧，你看看</span><span className="fd-sel-new">该方案整体可行，请您审阅。</span></span></div>
        </Win>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say="改得正式一点" />
      </>;
    case "ask":
      return <>
        <Win width={240}>
          <div className="fd-ph" style={{ width: 150 }} />
          <div className="fd-ph" style={{ width: 170, marginLeft: 16 }} />
          <div className="fd-ph" style={{ width: 90, marginLeft: 16 }} />
        </Win>
        <div className="fd-ans"><div className="fd-ans-label">回答</div><div className="fd-txt"><span className="fd-ty">按行读取配置，跳过空行。</span></div></div>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say="这段代码在做什么？" />
      </>;
    case "orb":
      return <>
        <Win><div className="fd-ph" style={{ width: 140 }} /><Output text="下午三点开会，记得带周报" /></Win>
        <Orb x={356} y={104} />
        <div className="fd-rp fd-rp-start" style={{ left: 376, top: 124 }} />
        <div className="fd-rp fd-rp-stop" style={{ left: 376, top: 124 }} />
        <Cursor x={376} y={124} />
      </>;
    case "shake":
      return <>
        <Win><div className="fd-ph" style={{ width: 140 }} /><Output text="下午三点开会，记得带周报" /></Win>
        <Orb x={236} y={92} />
        <div className="fd-trail" style={{ left: 172, top: 118 }} />
        <div className="fd-trail" style={{ left: 224, top: 118 }} />
        <div className="fd-rp fd-rp-start" style={{ left: 256, top: 112 }} />
        <div className="fd-rp fd-rp-stop" style={{ left: 256, top: 112 }} />
        <Cursor x={200} y={110} rest="translate(54px, 4px)" />
      </>;
    case "shakeDirect":
      return <>
        <Win><div className="fd-ph" style={{ width: 140 }} /><Output text="下午三点开会，记得带周报" /></Win>
        <div className="fd-trail" style={{ left: 316, top: 40 }} />
        <div className="fd-trail" style={{ left: 368, top: 40 }} />
        <Cursor x={344} y={32} />
        <Capsule say="下午三点开会，记得带周报" />
      </>;
    case "clicks":
      return <>
        <Win><div className="fd-ph" style={{ width: 140 }} /><Output text="好的，我马上处理" /></Win>
        <div className="fd-rp fd-rp-c1" style={{ left: 360, top: 40 }} />
        <div className="fd-rp fd-rp-c2" style={{ left: 360, top: 40 }} />
        <div className="fd-rp fd-rp-c3" style={{ left: 360, top: 40 }} />
        <div className="fd-badge" style={{ left: 378, top: 14 }}>×{clickCount}</div>
        <Cursor x={360} y={40} />
        <Capsule say="好的，我马上处理" />
      </>;
  }
}

function Win({ width = 400, children }: { width?: number; children: React.ReactNode }) {
  return <div className="fd-win" style={{ width }}>{children}</div>;
}

function Output({ text }: { text: string }) {
  return <div className="fd-txt"><span className="fd-ty fd-out">{text}</span><span className="fd-caret" /></div>;
}

/** 听写胶囊与上方的实时文字。 */
function Capsule({ say }: { say: string }) {
  return <>
    <div className="fd-bub"><div><span className="fd-ty">{say}</span></div></div>
    <div className="fd-cap">
      <div className="fd-wave">{WAVE.map((height, index) => <i key={index} style={{ height, opacity: WAVE_FADE[index] ?? 1 }} />)}</div>
      <div className="fd-dots">{Array.from({ length: 7 }, (_, index) => <i key={index} />)}</div>
    </div>
  </>;
}

function Orb({ x, y }: { x: number; y: number }) {
  return (
    <div className="fd-orb" style={{ left: x, top: y }}>
      <span className="fd-orb-mic"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">{MIC_PATH.map((d) => <path key={d} d={d} />)}</svg></span>
      <span className="fd-orb-wave">{ORB_WAVE.map((height, index) => <i key={index} style={{ height }} />)}</span>
    </div>
  );
}

function Cursor({ x, y, rest }: { x: number; y: number; rest?: string }) {
  return (
    <svg className="fd-cur" style={{ left: x, top: y, transform: rest }} viewBox="0 0 18 22">
      <path d="M1 1 L1 17 L5.5 13 L8.5 20 L11 19 L8 12.2 L14 12.2 Z" strokeWidth="1.2" strokeLinejoin="round" />
    </svg>
  );
}
