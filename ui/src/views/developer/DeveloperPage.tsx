import { useCallback, useEffect, useState } from "react";
import { BarChart3, Cpu, FlaskConical, Gauge, Mic, Sparkles } from "lucide-react";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, ROW_CONTROL_W, Row } from "@/components/ui/Group";
import { Switch } from "@/components/ui/Switch";
import { Textarea } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import { formatMs, formatTime } from "./format";
import { LocalComparePanel } from "./LocalComparePanel";
import { BenchResult, ModelBenchPanel, type BenchRow } from "./ModelBenchPanel";

const RUNS_REFRESH_MS = 3000;
const OVERVIEW = "overview";
const TEST = "test";
const BENCH = "bench";
const LOCAL = "local";

interface RunSummary {
  id: string;
  kind: "dictation" | "assistant" | "smart-test" | "model-bench";
  startedAtMs: number;
  outcome: "succeeded" | "failed" | "cancelled" | "abandoned";
  waitMs?: number;
  totalMs: number;
}

interface Phase {
  name: string;
  ms: number;
  waiting: boolean;
}

interface LlmCall {
  variant?: string;
  scope: string;
  providerName?: string;
  modelId?: string;
  reasoningEffective?: string;
  temperature?: number | null;
  maxTokens?: number | null;
  durationMs?: number;
  runtimeStartMs?: number | null;
  httpMs?: number | null;
  firstReasoningMs?: number | null;
  firstOutputMs?: number | null;
  userPromptChars?: number;
  outputChars?: number;
  reasoningChars?: number;
  usage?: { inputTokens?: number | null; outputTokens?: number | null; reasoningTokens?: number | null } | null;
  systemPrompt?: string;
  userPrompt?: string;
  output?: string;
  error?: string | null;
}

interface RunRecord extends RunSummary {
  error?: string | null;
  phases?: Phase[];
  promptBreakdown?: Record<string, number> | null;
  connection?: { origin: string; freshMs: number; reusedMs: number; connectionMs: number | null } | null;
  llm?: LlmCall[];
  models?: BenchRow[];
  analysis: { summary: string; hints: string[] };
  timeline?: { tMs: number; event: string; data: unknown }[];
}

interface Distribution {
  name?: string;
  samples: number;
  p50Ms: number;
  p90Ms: number;
  avgPromptChars?: number;
}

interface TraceStats {
  runs: number;
  wait: Distribution;
  phases: Distribution[];
  models: Distribution[];
}

interface TraceStatus {
  tracesDir: string;
  apiUrl: string;
}

const VARIANT_LABELS: Record<string, string> = {
  baseline: "基线：极短提示词",
  actual: "真实提示词",
  stream: "真实提示词（流式）",
};

const OUTCOME_LABELS: Record<RunSummary["outcome"], string> = {
  succeeded: "",
  failed: "失败",
  cancelled: "已取消",
  abandoned: "未结束",
};

const PROMPT_SOURCES: [string, string][] = [
  ["templateChars", "模板"],
  ["textChars", "识别文本"],
  ["activeAppContextChars", "软件上下文"],
  ["globalContextChars", "全局上下文"],
  ["hotwordsChars", "热词"],
  ["learningChars", "个性化学习"],
  ["promptChars", "合计"],
];

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const RUN_ICONS: Record<RunSummary["kind"], React.ReactNode> = {
  dictation: <Mic {...iconProps} />,
  assistant: <Sparkles {...iconProps} />,
  "smart-test": <FlaskConical {...iconProps} />,
  "model-bench": <Gauge {...iconProps} />,
};

function runLabel(run: RunSummary) {
  const duration = formatMs(run.waitMs ?? run.totalMs);
  const outcome = OUTCOME_LABELS[run.outcome];
  return `${formatTime(run.startedAtMs)} · ${outcome || duration}`;
}

function Value({ children }: { children: React.ReactNode }) {
  return <span className="text-[13px] tabular-nums text-[var(--color-fg-muted)]">{children}</span>;
}

function LongText({ label, text }: { label: string; text: string }) {
  return (
    <details className="px-3.5 py-3">
      <summary className="cursor-pointer text-[13px] text-[var(--color-fg)]">
        {label}（{text.length} 字符）
      </summary>
      <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-words font-mono text-xs text-[var(--color-fg-muted)]">{text}</pre>
    </details>
  );
}

function PhaseRow({ phase, scale }: { phase: Phase; scale: number }) {
  return (
    <Row label={phase.name}>
      <div className={cn(ROW_CONTROL_W, "flex items-center justify-end gap-3")}>
        {phase.waiting && (
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--color-surface-strong)]">
            <div className="h-full rounded-full bg-[var(--color-fg-subtle)]" style={{ width: `${scale > 0 ? (phase.ms / scale) * 100 : 0}%` }} />
          </div>
        )}
        <span className="w-20 text-right text-xs tabular-nums text-[var(--color-fg-muted)]">{formatMs(phase.ms)}</span>
      </div>
    </Row>
  );
}

function LlmGroup({ call }: { call: LlmCall }) {
  return (
    <Group title={call.variant ? VARIANT_LABELS[call.variant] ?? call.variant : "大模型请求"}>
      <Row label="模型"><Value>{call.providerName} / {call.modelId}</Value></Row>
      <Row label="推理强度"><Value>{call.reasoningEffective || "默认"}</Value></Row>
      <Row label="温度与最大输出"><Value>{call.temperature ?? "默认"} · {call.maxTokens ?? "默认"}</Value></Row>
      <Row label="请求耗时" message={call.error && <span role="alert" className="text-[var(--color-err)]">{call.error}</span>}>
        <Value>{formatMs(call.durationMs)}</Value>
      </Row>
      {typeof call.runtimeStartMs === "number" && <Row label="其中：启动运行时"><Value>{formatMs(call.runtimeStartMs)}</Value></Row>}
      {typeof call.httpMs === "number" && <Row label="其中：网络和模型"><Value>{formatMs(call.httpMs)}</Value></Row>}
      {call.firstReasoningMs !== undefined && <Row label="首个思考内容"><Value>{formatMs(call.firstReasoningMs)}</Value></Row>}
      {call.firstOutputMs !== undefined && <Row label="首个正文内容"><Value>{formatMs(call.firstOutputMs)}</Value></Row>}
      <Row label="字符：提示词 / 输出 / 思考">
        <Value>{call.userPromptChars ?? "—"} / {call.outputChars ?? "—"} / {call.reasoningChars ?? "—"}</Value>
      </Row>
      {call.usage && (
        <Row label="Token：输入 / 输出 / 思考">
          <Value>{call.usage.inputTokens ?? "—"} / {call.usage.outputTokens ?? "—"} / {call.usage.reasoningTokens ?? "—"}</Value>
        </Row>
      )}
      {call.systemPrompt && <LongText label="系统提示词" text={call.systemPrompt} />}
      {call.userPrompt && <LongText label="用户提示词" text={call.userPrompt} />}
      {call.output && <LongText label="模型输出" text={call.output} />}
    </Group>
  );
}

function RunDetail({ record }: { record: RunRecord }) {
  if (record.kind === "model-bench") {
    return (
      <InspectorBody>
        <BenchResult state={{ running: false, models: record.models ?? [] }} analysis={record.analysis} />
      </InspectorBody>
    );
  }
  const phases = record.phases ?? [];
  const scale = Math.max(0, ...phases.filter((phase) => phase.waiting).map((phase) => phase.ms));
  const breakdown = record.promptBreakdown;
  return (
    <InspectorBody>
      <Group title="结论">
        <p className="px-3.5 py-3 text-[13px] text-[var(--color-fg)]">{record.analysis.summary}</p>
        {record.error && <p role="alert" className="px-3.5 py-3 text-[13px] text-[var(--color-err)]">{record.error}</p>}
        {record.analysis.hints.map((hint) => (
          <p key={hint} className="px-3.5 py-3 text-[13px] text-[var(--color-fg-muted)]">{hint}</p>
        ))}
      </Group>
      {phases.length > 0 && (
        <Group title="耗时">
          {phases.map((phase) => <PhaseRow key={phase.name} phase={phase} scale={scale} />)}
        </Group>
      )}
      {record.connection && (
        <Group title={`连接：${record.connection.origin}`}>
          <Row label="新建连接"><Value>{formatMs(record.connection.freshMs)}</Value></Row>
          <Row label="复用连接"><Value>{formatMs(record.connection.reusedMs)}</Value></Row>
          <Row label="每次新建多花（估算）"><Value>{record.connection.connectionMs === null ? "未测出" : formatMs(record.connection.connectionMs)}</Value></Row>
        </Group>
      )}
      {(record.llm ?? []).map((call, index) => <LlmGroup key={index} call={call} />)}
      {breakdown && (
        <Group title="提示词构成（字符）">
          {PROMPT_SOURCES.map(([key, label]) => <Row key={key} label={label}><Value>{breakdown[key] ?? 0}</Value></Row>)}
        </Group>
      )}
      {(record.timeline ?? []).length > 0 && (
        <Group title="事件明细">
          {record.timeline?.map((entry, index) => (
            <details key={index} className="px-3.5 py-2">
              <summary className="cursor-pointer font-mono text-xs text-[var(--color-fg-muted)]">
                {formatMs(entry.tMs)} · {entry.event}
              </summary>
              <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-words font-mono text-xs text-[var(--color-fg-subtle)]">
                {JSON.stringify(entry.data, null, 2)}
              </pre>
            </details>
          ))}
        </Group>
      )}
    </InspectorBody>
  );
}

interface AudioRecording {
  enabled: boolean;
  dir: string;
}

/** 每次听写保存原始麦克风声音和送去识别的声音，用真实录音排查识别问题。 */
function AudioRecordingRow() {
  const [state, setState] = useState<AudioRecording | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    void cmd<AudioRecording>(CMD.devAudioRecording).then(setState).catch((reason) => setError(String(reason)));
  }, []);

  const toggle = (enabled: boolean) => {
    setError("");
    void cmd<AudioRecording>(CMD.devAudioRecording, { enabled }).then(setState).catch((reason) => setError(String(reason)));
  };

  return (
    <Group title="录音">
      <Row
        label="保存听写录音"
        hint={`每次识别保存两份录音：麦克风原始声音和处理后送去识别的声音，只保留最近 40 次。保存在 ${state?.dir ?? "工作区/dev-traces/audio"}`}
        message={error && <span role="alert" className="text-[var(--color-err)]">{error}</span>}
      >
        <Switch label="保存听写录音" checked={state?.enabled ?? false} disabled={!state} onChange={toggle} />
      </Row>
    </Group>
  );
}

function OverviewPanel() {
  const [stats, setStats] = useState<TraceStats | null>(null);
  const [status, setStatus] = useState<TraceStatus | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    void cmd<TraceStats>(CMD.devTraceStats).then(setStats).catch((reason) => setError(String(reason)));
    void cmd<TraceStatus>(CMD.devTraceStatus).then(setStatus).catch((reason) => setError(String(reason)));
  }, []);

  return (
    <InspectorBody>
      {error && <p role="alert" className="text-xs text-[var(--color-err)]">{error}</p>}
      <AudioRecordingRow />
      {stats && (
        <>
          <Group title={`最近 ${stats.runs} 次成功的听写`}>
            <Row label="松开到出字：中位 / 较慢"><Value>{formatMs(stats.wait.p50Ms)} / {formatMs(stats.wait.p90Ms)}</Value></Row>
            {stats.phases.map((phase) => (
              <Row key={phase.name} label={phase.name}><Value>{formatMs(phase.p50Ms)} / {formatMs(phase.p90Ms)}</Value></Row>
            ))}
          </Group>
          {stats.models.length > 0 && (
            <Group title="各模型：中位 / 较慢">
              {stats.models.map((model) => (
                <Row key={model.name} label={model.name} hint={`共 ${model.samples} 次请求，提示词平均 ${model.avgPromptChars} 字符`}>
                  <Value>{formatMs(model.p50Ms)} / {formatMs(model.p90Ms)}</Value>
                </Row>
              ))}
            </Group>
          )}
        </>
      )}
      {status && (
        <Group title="数据位置">
          <Row label="记录目录"><span className="select-text font-mono text-xs text-[var(--color-fg-muted)]">{status.tracesDir}</span></Row>
          <Row label="本地接口"><span className="select-text font-mono text-xs text-[var(--color-fg-muted)]">{status.apiUrl}</span></Row>
        </Group>
      )}
    </InspectorBody>
  );
}

function SmartTestPanel({ onFinished }: { onFinished: () => void }) {
  const [source, setSource] = useState<"replay" | "text">("replay");
  const [text, setText] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<RunRecord | null>(null);

  const start = async () => {
    setRunning(true);
    setError("");
    setResult(null);
    try {
      const request = source === "text" ? { text } : { fromRun: "latest" };
      setResult(await cmd<RunRecord>(CMD.devTraceSmartTest, { request }));
      onFinished();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRunning(false);
    }
  };

  return (
    <InspectorBody>
      <Group>
        <Row label="测试内容">
          <Tabs
            variant="view"
            ariaLabel="测试内容"
            tabs={[{ key: "replay", label: "重放最近一次听写" }, { key: "text", label: "自定义文本" }]}
            active={source}
            onChange={(key) => setSource(key as "replay" | "text")}
          />
        </Row>
        {source === "text" && (
          <div className="px-3.5 py-3">
            <Textarea aria-label="测试文本" rows={4} value={text} onChange={(event) => setText(event.target.value)} />
          </div>
        )}
        <Row
          label="向当前模型发 3 次真实请求"
          message={<>按量计费的模型会产生费用。{error && <span role="alert" className="text-[var(--color-err)]"> {error}</span>}</>}
        >
          <Button size="sm" variant="primary" disabled={running || (source === "text" && !text.trim())} onClick={() => void start()}>
            {running ? "测试中…" : "开始测试"}
          </Button>
        </Row>
      </Group>
      {result && <RunDetail record={result} />}
    </InspectorBody>
  );
}

/** 开发构建专用：查看每次听写的耗时记录，并对比测试智能处理。 */
export function DeveloperPage() {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [active, setActive] = useState<string>(OVERVIEW);
  const [record, setRecord] = useState<RunRecord | null>(null);
  const [error, setError] = useState("");

  const refresh = useCallback(() => {
    void cmd<RunSummary[]>(CMD.devTraceRuns)
      .then((value) => {
        setRuns(value);
        setError("");
      })
      .catch((reason) => setError(String(reason)));
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, RUNS_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    setRecord(null);
    if (active === OVERVIEW || active === TEST || active === BENCH || active === LOCAL) return;
    let stale = false;
    void cmd<RunRecord | null>(CMD.devTraceRun, { id: active })
      .then((value) => {
        if (!stale) setRecord(value);
      })
      .catch((reason) => setError(String(reason)));
    return () => {
      stale = true;
    };
  }, [active]);

  const items: NodeItem<string>[] = [
    { key: OVERVIEW, label: "统计", icon: <BarChart3 {...iconProps} /> },
    { key: TEST, label: "智能处理测试", icon: <FlaskConical {...iconProps} /> },
    { key: BENCH, label: "模型测速", icon: <Gauge {...iconProps} /> },
    { key: LOCAL, label: "本地模型对比", icon: <Cpu {...iconProps} /> },
    ...runs.map((run, index) => ({
      key: run.id,
      label: runLabel(run),
      icon: RUN_ICONS[run.kind],
      dividerBefore: index === 0,
    })),
  ];
  const title = active === OVERVIEW
    ? "统计"
    : active === TEST
      ? "智能处理测试"
      : active === BENCH
        ? "模型测速"
        : active === LOCAL
          ? "本地模型对比"
          : record
            ? formatTime(record.startedAtMs)
            : "";

  return (
    <MasterDetail title="开发者" list={<NodeList ariaLabel="运行记录" items={items} active={active} onSelect={setActive} />}>
      <InspectorHeader title={title} />
      {error && <p role="alert" className="-mt-2 mb-4 text-xs text-[var(--color-err)]">{error}</p>}
      {active === OVERVIEW && <OverviewPanel />}
      {active === TEST && <SmartTestPanel onFinished={refresh} />}
      {active === BENCH && <ModelBenchPanel onFinished={refresh} />}
      {active === LOCAL && <LocalComparePanel />}
      {record && <RunDetail record={record} />}
    </MasterDetail>
  );
}
