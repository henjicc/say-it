import { cn } from "@/lib/cn";
import "./FeatureDemo.css";

export type FeatureDemoKind = "translate" | "edit" | "ask" | "polish" | "orb" | "shake" | "shakeDirect";

/** 波形条高度：左侧几根渐隐，与原生胶囊一致。 */
const WAVE = [6, 10, 14, 8, 18, 12, 22, 16, 10, 20, 12, 8, 4];
const WAVE_FADE = [0.35, 0.48, 0.61, 0.74, 0.87];
const ORB_WAVE = [6, 12, 16, 10, 5];
const LOOP_S = 8;
/** 实时文字逐段出现的时刻（占一轮的百分比），与 CSS 的说话阶段对齐。 */
const SAY_AT = [22, 29, 36, 43];
const ANSWER_AT = [68, 72, 76];

const MIC_PATH = ["M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z", "M19 10v2a7 7 0 0 1-14 0v-2", "M12 19v3"];

/** 说出的话按段给出；filler 段在智能处理时被划掉。 */
type Part = string | { filler: string };

function stepsOf(kind: FeatureDemoKind, keyLabel: string, hold: boolean): [string, string, string] {
  const press = `${hold ? "按住" : "按"} ${keyLabel}`;
  switch (kind) {
    case "translate": return [press, "说话", "译文出现在输入框"];
    case "edit": return [`选中文字，${press}`, "说出修改要求", "原文被替换"];
    case "ask": return [press, "说出问题", "回答浮在旁边"];
    case "polish": return [`${press} 说话`, "去掉口头语、补上标点", "整理好的文字上屏"];
    case "orb": return ["按住拖到顺手的位置", "点一下开始，说完再点一下", "文字出现在输入框"];
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
        <Capsule say={["明天下午", "把方案", "发你"]} />
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
        <Capsule say={["改得", "正式一点"]} />
      </>;
    case "ask":
      return <>
        <div className="fd-doc">
          <div className="fd-ph" style={{ width: 150 }} />
          <div className="fd-ph" style={{ width: 170, marginLeft: 16 }} />
          <div className="fd-ph" style={{ width: 90, marginLeft: 16 }} />
        </div>
        <div className="fd-ans"><div className="fd-ans-label">回答</div><div className="fd-txt"><Chunks parts={["按行读取配置，", "跳过", "空行。"]} at={ANSWER_AT} /></div></div>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say={["这段代码", "在做什么？"]} />
      </>;
    case "polish":
      return <>
        <InputBox><Output text="我们明天下午三点开会吧。" /></InputBox>
        <div className="fd-key">{keyLabel}</div>
        <Capsule say={[{ filler: "嗯，那个" }, "我们明天下午", { filler: "呃" }, "三点开会吧"]} tail={<span className="fd-add">。</span>} />
      </>;
    case "orb":
      return <>
        <InputBox><Output text="下午三点开会，记得带周报" /></InputBox>
        <div className="fd-drag" style={{ left: 356, top: 104 }}>
          <Orb />
          <div className="fd-rp fd-rp-start" style={{ left: 20, top: 20 }} />
          <div className="fd-rp fd-rp-stop" style={{ left: 20, top: 20 }} />
          <Cursor x={20} y={20} />
        </div>
      </>;
    case "shake":
      return <>
        <InputBox><Output text="下午三点开会，记得带周报" /></InputBox>
        <div className="fd-drag" style={{ left: 180, top: 90 }}><Orb /></div>
        <div className="fd-trail" style={{ left: 166, top: 118 }} />
        <div className="fd-trail" style={{ left: 220, top: 118 }} />
        <div className="fd-rp fd-rp-start" style={{ left: 200, top: 110 }} />
        <div className="fd-rp fd-rp-stop" style={{ left: 200, top: 110 }} />
        <Cursor x={200} y={110} className="fd-shake-cur" />
      </>;
    case "shakeDirect":
      return <>
        <InputBox><Output text="下午三点开会，记得带周报" /></InputBox>
        <div className="fd-trail" style={{ left: 336, top: 104 }} />
        <div className="fd-trail" style={{ left: 388, top: 104 }} />
        <Cursor x={364} y={96} className="fd-shake-cur" />
        <Capsule say={["下午三点开会，", "记得带周报"]} />
      </>;
  }
}

/** 单行输入框，右侧是发送按钮；有文字时按钮亮起。 */
function InputBox({ children }: { children: React.ReactNode }) {
  return (
    <div className="fd-input">
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

/** 逐段出现的文字：第 i 段在一轮的 at[i]% 处展开。 */
function Chunks({ parts, at }: { parts: Part[]; at: number[] }) {
  return <>
    {parts.map((part, index) => {
      const text = typeof part === "string" ? part : part.filler;
      const style = { "--w": `${[...text].length}em`, animationDelay: `${(at[index] / 100) * LOOP_S}s` } as React.CSSProperties;
      return (
        <span key={index} className="fd-chunk" style={style}>
          {typeof part === "string" ? text : <span className="fd-filler" style={{ "--w": `${[...text].length}em` } as React.CSSProperties}>{text}</span>}
        </span>
      );
    })}
  </>;
}

/** 听写胶囊与上方的实时文字，二者同一中轴。 */
function Capsule({ say, tail }: { say: Part[]; tail?: React.ReactNode }) {
  return <>
    <div className="fd-bub"><div><Chunks parts={say} at={SAY_AT} />{tail}</div></div>
    <div className="fd-capbox">
      <div className="fd-cap">
        <div className="fd-wave">{WAVE.map((height, index) => <i key={index} style={{ height, opacity: WAVE_FADE[index] ?? 1 }} />)}</div>
        <div className="fd-dots">{Array.from({ length: 7 }, (_, index) => <i key={index} />)}</div>
      </div>
    </div>
  </>;
}

function Orb() {
  return (
    <div className="fd-orb" style={{ left: 0, top: 0 }}>
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
