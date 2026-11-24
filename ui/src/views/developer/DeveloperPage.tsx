import { useEffect, useState } from "react";
import { AppWindow, BarChart3, Cpu, Database, FlaskConical, Gauge, History, MousePointer2 } from "lucide-react";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, ROW_CONTROL_W, Row } from "@/components/ui/Group";
import { Switch } from "@/components/ui/Switch";
import { Select, Textarea } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd } from "@/lib/tauri";
import { formatMs, formatTime } from "./format";
import { LocalComparePanel } from "./LocalComparePanel";
import { BenchResult, ModelBenchPanel, type BenchRow } from "./ModelBenchPanel";
import { NativeWindowsPanel } from "./NativeWindowsPanel";
import { PointerProbePanel } from "./PointerProbePanel";
import { SmartPreviewPanel } from "./SmartPreviewPanel";
import { setQueryData, useCachedQuery, type Query } from "@/hooks/useCachedQuery";

const RUNS_REFRESH_MS = 3000;
const COLLECT = "collect";
const OVERVIEW = "overview";
const RUNS = "runs";
const TEST = "test";
const BENCH = "bench";
const LOCAL = "local";
const WINDOWS = "windows";
const POINTER = "pointer";

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
  reasoningConfigured?: string;
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

const KIND_LABELS: Record<RunSummary["kind"], string> = {
  dictation: "听写",
  assistant: "助手",
  "smart-test": "智能处理测试",
  "model-bench": "模型测速",
};

function runLabel(run: RunSummary) {
  const duration = formatMs(run.waitMs ?? run.totalMs);
  const outcome = OUTCOME_LABELS[run.outcome];
  return `${formatTime(run.startedAtMs)} · ${KIND_LABELS[run.kind]} · ${outcome || duration}`;
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
      <Row label="推理强度"><Value>{call.reasoningConfigured || "默认"}</Value></Row>
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

interface CollectSwitch {
  enabled: boolean;
}

/** 每次听写保存原始麦克风声音和送去识别的声音，用真实录音排查识别问题。 */
export const audioRecordingQuery: Query<CollectSwitch & { dir: string }> = {
  key: "devAudioRecording",
  load: () => cmd(CMD.devAudioRecording),
};

/** 每次语音输入成功后保存一条训练样本，供以后微调本地整理模型。 */
export const trainingSamplesQuery: Query<CollectSwitch & { path: string }> = {
  key: "devTrainingSamples",
  load: () => cmd(CMD.devTrainingSamples),
};

function CollectSwitchRow<T extends CollectSwitch>({ query, command, label, hint }: {
  query: Query<T>;
  command: string;
  label: string;
  hint: (state: T | undefined) => string;
}) {
  const current = useCachedQuery(query);
  const state = current.data;
  const [saveError, setError] = useState("");
  const error = saveError || current.error;

  const toggle = (enabled: boolean) => {
    setError("");
    void cmd<T>(command, { enabled })
      .then((next) => setQueryData(query, next))
      .catch((reason) => setError(String(reason)));
  };

  return (
    <Row
      label={label}
      hint={hint(state)}
      message={error && <span role="alert" className="text-[var(--color-err)]">{error}</span>}
    >
      <Switch label={label} checked={state?.enabled ?? false} disabled={!state} onChange={toggle} />
    </Row>
  );
}

function CollectPanel() {
  const statusQuery = useCachedQuery(traceStatusQuery);
  const status = statusQuery.data;
  return (
    <InspectorBody>
      {statusQuery.error && <p role="alert" className="text-xs text-[var(--color-err)]">{statusQuery.error}</p>}
      <Group>
        <CollectSwitchRow
          query={trainingSamplesQuery}
          command={CMD.devTrainingSamples}
          label="保存训练样本"
          hint={(state) => `每次语音输入成功后记下识别原文、插入的文字和软件上下文，供以后微调本地整理模型。不会自动清理，保存在 ${state?.path ?? "数据目录的 dev-traces/training-samples.jsonl"}`}
        />
        <CollectSwitchRow
          query={audioRecordingQuery}
          command={CMD.devAudioRecording}
          label="保存语音输入录音"
          hint={(state) => `每次识别保存两份录音：麦克风原始声音和处理后送去识别的声音，只保留最近 40 次。保存在 ${state?.dir ?? "数据目录的 dev-traces/audio"}`}
        />
      </Group>
      {status && (
        <Group title="数据位置">
          <Row label="记录目录"><span className="select-text font-mono text-xs text-[var(--color-fg-muted)]">{status.tracesDir}</span></Row>
          <Row label="本地接口"><span className="select-text font-mono text-xs text-[var(--color-fg-muted)]">{status.apiUrl}</span></Row>
        </Group>
      )}
    </InspectorBody>
  );
}

export const traceStatsQuery: Query<TraceStats> = { key: "devTraceStats", load: () => cmd<TraceStats>(CMD.devTraceStats) };
export const traceStatusQuery: Query<TraceStatus> = { key: "devTraceStatus", load: () => cmd<TraceStatus>(CMD.devTraceStatus) };

function OverviewPanel() {
  const statsQuery = useCachedQuery(traceStatsQuery);
  const stats = statsQuery.data;

  return (
    <InspectorBody>
      {statsQuery.error && <p role="alert" className="text-xs text-[var(--color-err)]">{statsQuery.error}</p>}
      {stats && (
        <>
          <Group title={`最近 ${stats.runs} 次成功的语音输入`}>
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
      <SmartPreviewPanel />
      <Group>
        <Row label="测试内容">
          <Tabs
            variant="view"
            ariaLabel="测试内容"
            tabs={[{ key: "replay", label: "重放最近一次语音输入" }, { key: "text", label: "自定义文本" }]}
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

export const traceRunsQuery: Query<RunSummary[]> = { key: "devTraceRuns", load: () => cmd<RunSummary[]>(CMD.devTraceRuns) };

/** 一次运行的完整记录；记录写完就不再变，切回来直接用上次读到的。 */
function traceRunQuery(id: string): Query<RunRecord | null> {
  return { key: `devTraceRun:${id}`, load: async () => (id ? cmd<RunRecord | null>(CMD.devTraceRun, { id }) : null) };
}

/** 开发者模式：采集训练数据、查看每次听写的耗时记录，并测试智能处理和模型速度。 */
export function DeveloperPage() {
  const runsQuery = useCachedQuery(traceRunsQuery);
  const runs = runsQuery.data ?? [];
  const [active, setActive] = useState<string>(COLLECT);
  // 没选过时跟着最新一次走；选过就停在那一次，新记录进来也不跳。
  const [pickedRun, setPickedRun] = useState("");
  const runId = active === RUNS ? pickedRun || runs[0]?.id || "" : "";
  const recordQuery = useCachedQuery(traceRunQuery(runId));
  const record = runId ? recordQuery.data ?? null : null;
  const error = runsQuery.error || (runId ? recordQuery.error : "");
  const refresh = runsQuery.refresh;

  useEffect(() => {
    const timer = window.setInterval(() => void refresh(), RUNS_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const items: NodeItem<string>[] = [
    { key: COLLECT, label: "数据采集", icon: <Database {...iconProps} /> },
    { key: OVERVIEW, label: "统计", icon: <BarChart3 {...iconProps} /> },
    { key: RUNS, label: "运行记录", icon: <History {...iconProps} /> },
    { key: TEST, label: "智能处理测试", icon: <FlaskConical {...iconProps} />, dividerBefore: true },
    { key: BENCH, label: "模型测速", icon: <Gauge {...iconProps} /> },
    { key: WINDOWS, label: "原生窗口测试", icon: <AppWindow {...iconProps} /> },
    ...(isMacOS ? [] : [{ key: POINTER, label: "指向测试", icon: <MousePointer2 {...iconProps} /> }]),
    // 本地模型对比依赖仓库工作区里的模型和 llama.cpp，只有开发构建有。
    ...(import.meta.env.DEV ? [{ key: LOCAL, label: "本地模型对比", icon: <Cpu {...iconProps} /> }] : []),
  ];
  const activeItem = items.find((item) => item.key === active);
  const runPicker = active === RUNS && runs.length > 0 && (
    <Select aria-label="选择运行记录" className="w-80" size="sm" value={runId} onChange={(event) => setPickedRun(event.target.value)}>
      {runs.map((run) => <option key={run.id} value={run.id}>{runLabel(run)}</option>)}
    </Select>
  );

  return (
    <MasterDetail wide title="开发者" list={<NodeList ariaLabel="开发者工具" items={items} active={active} onSelect={setActive} />}>
      <InspectorHeader title={activeItem?.label ?? ""} right={runPicker} />
      {error && <p role="alert" className="-mt-2 mb-4 text-xs text-[var(--color-err)]">{error}</p>}
      {active === COLLECT && <CollectPanel />}
      {active === OVERVIEW && <OverviewPanel />}
      {active === RUNS && runsQuery.data && runs.length === 0 && (
        <p className="text-[13px] text-[var(--color-fg-muted)]">还没有记录，开发者模式下语音输入一次就会出现。</p>
      )}
      {active === TEST && <SmartTestPanel onFinished={refresh} />}
      {active === BENCH && <ModelBenchPanel onFinished={refresh} />}
      {active === LOCAL && <LocalComparePanel />}
      {active === WINDOWS && <NativeWindowsPanel />}
      {active === POINTER && <PointerProbePanel />}
      {record && <RunDetail record={record} />}
    </MasterDetail>
  );
}
