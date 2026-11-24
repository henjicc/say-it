import { cn } from "@/lib/cn";
import "./FeatureDemo.css";

export type FeatureDemoKind = "translate" | "edit" | "ask" | "polish" | "orb" | "shake" | "shakeDirect";

/** 波形条高度：左侧几根渐隐，与原生胶囊一致。 */
const WAVE = [6, 10, 14, 8, 18, 12, 22, 16, 10, 20, 12, 8, 4];
const WAVE_FADE = [0.35, 0.48, 0.61, 0.74, 0.87];
const ORB_WAVE = [6, 12, 16, 10, 5];
const LOOP_S = 8;
/** 只有输入框和悬浮球的画面内容少，输入框下移，让整组在示意区里上下居中。 */
const LOW_INPUT_TOP = 30;
/** 实时文字逐字出现的时段（占一轮的百分比），与 CSS 的说话阶段对齐。 */
const SAY_SPAN: [number, number] = [22, 46];
const ANSWER_SPAN: [number, number] = [68, 82];

const MIC_PATH = ["M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z", "M19 10v2a7 7 0 0 1-14 0v-2", "M12 19v3"];

/** 智能处理示意的一段文字：原样保留，或在第 slot 个时刻（1–4，见 CSS）被删掉或改写。 */
type Edit = string | { from: string; to?: string; slot: 1 | 2 | 3 | 4 };

const POLISH: Edit[] = [
  { from: "嗯那个，", slot: 1 },
  "我们明天下午",
  { from: "呃大概", slot: 2 },
  "三点",
  { from: "左右开个会吧", to: "开会", slot: 3 },
  "，你看",
  { from: "行不行啊", to: "可以吗？", slot: 4 },
];

function stepsOf(kind: FeatureDemoKind, keyLabel: string, hold: boolean): [string, string, string] {
  const press = `${hold ? "按住" : "按"} ${keyLabel}`;
  switch (kind) {
    case "translate": return [press, "说话", "译文出现在输入框"];
    case "edit": return [`选中文字，${press}`, "说出修改要求", "原文被替换"];
    case "ask": return [press, "说出问题", "回答浮在旁边"];
    case "polish": return ["口语原话", "删掉口头语、改写措辞", "整理好的文字"];
    case "orb": return ["点悬浮球", "说话，再点一下", "文字出现在输入框"];
    case "shake": return ["晃动鼠标，点出现的悬浮球", "说话，再点一下", "文字出现在输入框"];
    case "shakeDirect": return ["晃动鼠标", "说话", "文字出现在输入框"];
  }
}

/**
 * 功能示意：用一小段循环动画说明「怎么触发 → 说什么 → 结果出现在哪」。
 * 系统要求减少动态效果时停在代表帧。
 */
export function FeatureDemo({
  kind,
  keyLabel = "快捷键",
  hold = false,
}: {
  kind: FeatureDemoKind;
  keyLabel?: string;
  hold?: boolean;
}) {
  const steps = stepsOf(kind, keyLabel, hold);
  return (
    <div role="img" aria-label={steps.join("，")} className={cn("fd", `fd-${kind === "orb" ? "orb-click" : kind}`, hold && "fd-hold")}>
      <div className="fd-scene" aria-hidden>
        <Scene kind={kind} keyLabel={keyLabel} />
      </div>
      <div className="fd-steps" aria-hidden>
        {steps.map((step, index) => <span key={index} className={`fd-s${index + 1}`}><b>{index + 1}</b>{step}</span>)}
      </div>
    </div>
  );
}

function Scene({ kind, keyLabel }: { kind: FeatureDemoKind; keyLabel: string }) {
  switch (kind) {
    case "translate":
      return <>
        <InputBox><Output text="I'll send you the proposal tomorrow afternoon." /></InputBox>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say="明天下午把方案发你" />
      </>;
    case "edit":
      return <>
        <InputBox>
          <span className="fd-sel">
            <span className="fd-sel-old"><span className="fd-sel-bg" /><span>这个方案我觉得还行吧，你看看</span></span>
            <span className="fd-sel-new">该方案整体可行，请您审阅。</span>
          </span>
        </InputBox>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say="改得正式一点" />
      </>;
    case "ask":
      return <>
        <div className="fd-doc">
          <div className="fd-ph" style={{ width: 150 }} />
          <div className="fd-ph" style={{ width: 170, marginLeft: 16 }} />
          <div className="fd-ph" style={{ width: 90, marginLeft: 16 }} />
        </div>
        <div className="fd-ans"><div className="fd-ans-label">回答</div><div className="fd-txt"><Chars text="按行读取配置，跳过空行。" span={ANSWER_SPAN} /></div></div>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say="这段代码在做什么？" />
      </>;
    case "polish":
      return <>
        <div className="fd-para">
          {POLISH.map((part, index) => typeof part === "string"
            ? <span key={index}>{part}</span>
            : <span key={index}>
              <span className={`fd-old fd-e${part.slot}`} style={widthOf(part.from)}>{part.from}</span>
              {part.to && <span className={`fd-new fd-e${part.slot}`} style={widthOf(part.to)}>{part.to}</span>}
            </span>)}
        </div>
      </>;
    case "orb":
      return <>
        <InputBox top={LOW_INPUT_TOP}><Output text="下午三点开会，记得带周报" /></InputBox>
        <Orb x={230} y={92} />
        <div className="fd-rp fd-rp-start" style={{ left: 250, top: 112 }} />
        <div className="fd-rp fd-rp-stop" style={{ left: 250, top: 112 }} />
        <Cursor x={250} y={112} />
      </>;
    case "shake":
      return <>
        <InputBox top={LOW_INPUT_TOP}><Output text="下午三点开会，记得带周报" /></InputBox>
        <Orb x={230} y={92} />
        <div className="fd-rp fd-rp-start" style={{ left: 250, top: 112 }} />
        <div className="fd-rp fd-rp-stop" style={{ left: 250, top: 112 }} />
        <Cursor x={250} y={112} className="fd-shake-cur" />
      </>;
    case "shakeDirect":
      return <>
        <InputBox><Output text="下午三点开会，记得带周报" /></InputBox>
        <Cursor x={340} y={96} className="fd-shake-cur" />
        <Capsule say="下午三点开会，记得带周报" />
      </>;
  }
}

/** 单行输入框，右侧是发送按钮；有文字时按钮亮起。 */
function InputBox({ top, children }: { top?: number; children: React.ReactNode }) {
  return (
    <div className="fd-input" style={{ top }}>
      <div className="fd-txt">{children}</div>
      <span className="fd-send">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 19V5" /><path d="m6 11 6-6 6 6" /></svg>
      </span>
    </div>
  );
}

function Output({ text }: { text: string }) {
  return <><span className="fd-out">{text}</span><span className="fd-caret" /></>;
}

/** 宽度上限：收起或展开时，max-width 在 0 与它之间过渡。 */
function widthOf(text: string) {
  return { "--w": `${[...text].length}em` } as React.CSSProperties;
}

/** 逐字淡入：字一开始就排好，只是透明；第 i 个字在 span 时段里依次出现。 */
function Chars({ text, span: [from, to] }: { text: string; span: [number, number] }) {
  const chars = [...text];
  const step = (to - from) / chars.length;
  return <>{chars.map((char, index) => (
    <span key={index} className="fd-ch" style={{ animationDelay: `${((from + index * step) / 100) * LOOP_S}s` }}>{char}</span>
  ))}</>;
}

/** 听写胶囊与上方的实时文字，二者同一中轴。 */
function Capsule({ say }: { say: string }) {
  return <>
    <div className="fd-bub"><div><Chars text={say} span={SAY_SPAN} /></div></div>
    <div className="fd-capbox">
      <div className="fd-cap">
        <div className="fd-wave">{WAVE.map((height, index) => <i key={index} style={{ height, opacity: WAVE_FADE[index] ?? 1 }} />)}</div>
        <div className="fd-dots">{Array.from({ length: 7 }, (_, index) => <i key={index} />)}</div>
      </div>
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

function Cursor({ x, y, className }: { x: number; y: number; className?: string }) {
  return (
    <svg className={cn("fd-cur", className)} style={{ left: x, top: y }} viewBox="0 0 18 22">
      <path d="M1 1 L1 17 L5.5 13 L8.5 20 L11 19 L8 12.2 L14 12.2 Z" strokeWidth="1.2" strokeLinejoin="round" />
    </svg>
  );
}
