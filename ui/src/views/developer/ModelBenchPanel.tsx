import { useEffect, useMemo, useRef, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Group, Row } from "@/components/ui/Group";
import { Textarea } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import { llmModelPickerOptions, llmModelValue, llmPrice, THINKING_OPTIONS, type ThinkingLevel } from "@/features/models/llmModelOptions";
import { adapterOf, llmModelLabel } from "@/features/models/recommendedLlmModels";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";
import { formatMs, formatRate, modelKey } from "./format";

const LIVE_REFRESH_MS = 250;

interface BenchModelOption {
  providerId: string;
  providerName: string;
  model: string;
  /** 智能处理当前用的模型。 */
  current: boolean;
}

interface BenchChoice extends BenchModelOption {
  key: string;
  label: string;
  price?: string;
}

interface BenchSample {
  firstTokenMs: number | null;
  firstOutputMs: number | null;
  lastTokenMs: number;
  outputTokens: number | null;
  tokensEstimated: boolean;
  tokensPerSec: number | null;
  finished: boolean;
  error: string | null;
}

interface BenchSummary {
  firstTokenMs: number | null;
  firstOutputMs: number | null;
  lastTokenMs: number | null;
  tokensPerSec: number | null;
  outputTokens: number | null;
  tokensEstimated: boolean;
  failures: number;
}

export interface BenchRow {
  providerId: string;
  providerName: string;
  modelId: string;
  samples: BenchSample[];
  summary: BenchSummary;
}

interface BenchState {
  running: boolean;
  models: BenchRow[];
}

interface BenchFinished extends BenchState {
  analysis: { summary: string; hints: string[] };
}

type PromptSource = "standard" | "replay" | "custom";

/** 还没跑完一轮时汇总是空的，先显示正在进行的这一轮，数字才会实时动起来。 */
function displayed(row: BenchRow): BenchSummary {
  if (row.summary.firstTokenMs !== null || row.samples.every((sample) => sample.finished)) return row.summary;
  const current = row.samples.find((sample) => !sample.finished);
  return {
    firstTokenMs: current?.firstTokenMs ?? null,
    firstOutputMs: current?.firstOutputMs ?? null,
    lastTokenMs: current?.lastTokenMs || null,
    tokensPerSec: current?.tokensPerSec ?? null,
    outputTokens: current?.outputTokens ?? null,
    tokensEstimated: current?.tokensEstimated ?? false,
    failures: 0,
  };
}

function formatTokens(tokens: number | null, estimated: boolean) {
  return tokens === null ? "—" : `${estimated ? "≈" : ""}${Math.round(tokens)}`;
}

/** 复制出去能直接贴进表格软件。 */
export function benchToTsv(rows: BenchRow[]) {
  const header = ["供应商", "模型", "首字(毫秒)", "正文首字(毫秒)", "token/秒", "总耗时(毫秒)", "输出token", "失败轮数"];
  const lines = rows.map((row) => [
    row.providerName,
    row.modelId,
    row.summary.firstTokenMs ?? "",
    row.summary.firstOutputMs ?? "",
    row.summary.tokensPerSec === null ? "" : Math.round(row.summary.tokensPerSec),
    row.summary.lastTokenMs ?? "",
    row.summary.outputTokens === null ? "" : Math.round(row.summary.outputTokens),
    row.summary.failures,
  ].join("\t"));
  return [header.join("\t"), ...lines].join("\n");
}

function MetricCell({ text, best, ratio }: { text: string; best: boolean; ratio?: number }) {
  return (
    <td className="px-2 py-2.5 text-right align-top tabular-nums">
      <span className={best ? "font-semibold text-[var(--color-fg)]" : "text-[var(--color-fg-muted)]"}>{text}</span>
      {ratio !== undefined && (
        <div className="mt-1 h-1 overflow-hidden rounded-full bg-[var(--color-surface-strong)]">
          <div className="h-full rounded-full bg-[var(--color-fg-subtle)]" style={{ width: `${ratio * 100}%` }} />
        </div>
      )}
    </td>
  );
}

function RoundDetails({ rows }: { rows: BenchRow[] }) {
  return (
    <details className="px-3.5 py-3">
      <summary className="cursor-pointer text-[13px] text-[var(--color-fg)]">每轮明细</summary>
      <table className="mt-2 w-full text-xs tabular-nums text-[var(--color-fg-muted)]">
        <thead className="text-[var(--color-fg-subtle)]">
          <tr>
            <th scope="col" className="py-1 text-left font-normal">模型</th>
            <th scope="col" className="py-1 text-right font-normal">轮</th>
            <th scope="col" className="py-1 text-right font-normal">首字</th>
            <th scope="col" className="py-1 text-right font-normal">正文首字</th>
            <th scope="col" className="py-1 text-right font-normal">token/秒</th>
            <th scope="col" className="py-1 text-right font-normal">总耗时</th>
          </tr>
        </thead>
        <tbody>
          {rows.flatMap((row) => row.samples.map((sample, index) => (
            <tr key={`${row.providerId}/${row.modelId}/${index}`} className="border-t border-[var(--color-line)]">
              <td className="py-1 pr-3 text-left">{row.modelId}</td>
              <td className="py-1 text-right">{index + 1}</td>
              {sample.error ? (
                <td colSpan={4} className="py-1 text-right [overflow-wrap:anywhere] text-[var(--color-err)]">{sample.error}</td>
              ) : (
                <>
                  <td className="py-1 text-right">{formatMs(sample.firstTokenMs)}</td>
                  <td className="py-1 text-right">{formatMs(sample.firstOutputMs)}</td>
                  <td className="py-1 text-right">{formatRate(sample.tokensPerSec, sample.tokensEstimated)}</td>
                  <td className="py-1 text-right">{sample.finished ? formatMs(sample.lastTokenMs) : "…"}</td>
                </>
              )}
            </tr>
          )))}
        </tbody>
      </table>
    </details>
  );
}

/** 测速结果：进行中实时刷新，结束后按首字排序。首字越短越好，速度越高越好。 */
export function BenchResult({ state, analysis }: { state: BenchState; analysis?: BenchFinished["analysis"] }) {
  const [copied, setCopied] = useState(false);
  const rows = state.models;
  const metrics = rows.map(displayed);
  const firstTokens = metrics.flatMap((metric) => metric.firstTokenMs ?? []);
  const firstOutputs = metrics.flatMap((metric) => metric.firstOutputMs ?? []);
  const rates = metrics.flatMap((metric) => metric.tokensPerSec ?? []);
  const totals = metrics.flatMap((metric) => metric.lastTokenMs ?? []);
  const maxFirstToken = Math.max(0, ...firstTokens);
  const maxRate = Math.max(0, ...rates);
  // 跑完才标最优：进行中的数字还在变，标了也会跳来跳去。
  const isBest = (value: number | null, values: number[], pick: (...items: number[]) => number) =>
    !state.running && value !== null && values.length > 1 && value === pick(...values);

  const copy = async () => {
    await navigator.clipboard.writeText(benchToTsv(rows));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <>
      {analysis && (
        <Group title="结论">
          <p className="px-3.5 py-3 text-[13px] text-[var(--color-fg)]">{analysis.summary}</p>
          {analysis.hints.map((hint) => (
            <p key={hint} className="px-3.5 py-3 text-[13px] text-[var(--color-fg-muted)]">{hint}</p>
          ))}
        </Group>
      )}
      <Group
        title={state.running ? "测速中" : "对比"}
        right={!state.running && <Button size="sm" onClick={() => void copy()}>{copied ? "已复制" : "复制表格"}</Button>}
      >
        <div className="overflow-x-auto">
          <table className="w-full min-w-[560px] table-fixed text-[13px]">
            <caption className="sr-only">各模型响应速度对比</caption>
            <thead className="text-xs text-[var(--color-fg-subtle)]">
              <tr>
                <th scope="col" className="px-3.5 py-2.5 text-left font-normal">模型</th>
                <th scope="col" className="w-20 px-2 py-2.5 text-right font-normal">首字</th>
                <th scope="col" className="w-20 px-2 py-2.5 text-right font-normal">正文首字</th>
                <th scope="col" className="w-20 px-2 py-2.5 text-right font-normal">token/秒</th>
                <th scope="col" className="w-20 px-2 py-2.5 text-right font-normal">总耗时</th>
                <th scope="col" className="w-20 px-2 pr-3.5 py-2.5 text-right font-normal">输出 token</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row, index) => {
                const metric = metrics[index];
                const finishedRounds = row.samples.filter((sample) => sample.finished).length;
                const lastError = [...row.samples].reverse().find((sample) => sample.error)?.error;
                return (
                  <tr key={`${row.providerId}/${row.modelId}`} className="border-t border-[var(--color-line)]">
                    <th scope="row" className="min-w-0 px-3.5 py-2.5 text-left font-normal">
                      <div className="[overflow-wrap:anywhere] text-[var(--color-fg)]">{row.modelId}</div>
                      <div className="text-xs text-[var(--color-fg-subtle)]">
                        {row.providerName}
                        {state.running && finishedRounds < row.samples.length && ` · 第 ${finishedRounds + 1}/${row.samples.length} 轮`}
                      </div>
                      {lastError && !state.running && (
                        <div role="alert" className="mt-1 text-xs [overflow-wrap:anywhere] text-[var(--color-err)]">
                          {row.summary.failures}/{row.samples.length} 轮失败：{lastError}
                        </div>
                      )}
                    </th>
                    <MetricCell
                      text={formatMs(metric.firstTokenMs)}
                      best={isBest(metric.firstTokenMs, firstTokens, Math.min)}
                      ratio={maxFirstToken > 0 && metric.firstTokenMs !== null ? metric.firstTokenMs / maxFirstToken : undefined}
                    />
                    <MetricCell text={formatMs(metric.firstOutputMs)} best={isBest(metric.firstOutputMs, firstOutputs, Math.min)} />
                    <MetricCell
                      text={formatRate(metric.tokensPerSec, metric.tokensEstimated)}
                      best={isBest(metric.tokensPerSec, rates, Math.max)}
                      ratio={maxRate > 0 && metric.tokensPerSec !== null ? metric.tokensPerSec / maxRate : undefined}
                    />
                    <MetricCell text={formatMs(metric.lastTokenMs)} best={isBest(metric.lastTokenMs, totals, Math.min)} />
                    <MetricCell text={formatTokens(metric.outputTokens, metric.tokensEstimated)} best={false} />
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <RoundDetails rows={rows} />
      </Group>
    </>
  );
}

/**
 * 只测「用途」里能选的大模型（已填密钥且设为显示），加上智能处理当前用的那个；
 * 供应商远端列表里的语音、翻译等其他模型不列出。
 */
function useBenchChoices(options: BenchModelOption[]): BenchChoice[] {
  const profiles = useProviderStore((state) => state.profiles);
  return useMemo(() => {
    const offered = new Set(llmModelPickerOptions(profiles).map((option) => option.value));
    return options
      .filter((option) => option.current || offered.has(llmModelValue(option.providerId, option.model)))
      .map((option) => {
        const profile = profiles.find((item) => item.id === option.providerId);
        return {
          ...option,
          key: modelKey(option),
          label: profile ? llmModelLabel(profile, option.model) : option.model,
          price: profile && llmPrice(adapterOf(profile.kind), option.model)?.text,
        };
      });
  }, [options, profiles]);
}

export function ModelBenchPanel({ onFinished }: { onFinished: () => void }) {
  const [rawOptions, setRawOptions] = useState<BenchModelOption[]>([]);
  const options = useBenchChoices(rawOptions);
  const [selected, setSelected] = useState<string[]>([]);
  const [source, setSource] = useState<PromptSource>("standard");
  const [prompt, setPrompt] = useState("");
  const [rounds, setRounds] = useState("3");
  const [thinking, setThinking] = useState<"model" | ThinkingLevel>("model");
  const [running, setRunning] = useState(false);
  const [live, setLive] = useState<BenchState | null>(null);
  const [result, setResult] = useState<BenchFinished | null>(null);
  const [error, setError] = useState("");
  // 自己发起的测速由请求结果收尾；轮询到的「未在运行」可能是上一次测速的旧状态。
  const awaitingResult = useRef(false);

  useEffect(() => {
    void cmd<BenchModelOption[]>(CMD.devBenchModels).then((models) => {
      setRawOptions(models);
      // 默认勾上当前在用的模型，拿它和其他模型比。
      setSelected(models.filter((model) => model.current).map(modelKey));
    }).catch((reason) => setError(String(reason)));
    // 离开页面再回来时测速可能还在跑，接着显示进度。
    void cmd<BenchState | null>(CMD.devBenchState).then((state) => {
      if (state?.running) {
        setLive(state);
        setRunning(true);
      }
    }).catch((reason) => setError(String(reason)));
  }, []);

  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      void cmd<BenchState | null>(CMD.devBenchState).then((state) => {
        if (!state) return;
        if (state.running) {
          setLive(state);
        } else if (!awaitingResult.current) {
          setLive(state);
          setRunning(false);
          onFinished();
        }
      }).catch((reason) => setError(String(reason)));
    }, LIVE_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [running, onFinished]);

  const start = async () => {
    awaitingResult.current = true;
    setRunning(true);
    setError("");
    setResult(null);
    setLive(null);
    const models = options
      .filter((option) => selected.includes(option.key))
      .map(({ providerId, model }) => ({ providerId, model }));
    const request = {
      models,
      rounds: Number(rounds),
      ...(thinking === "model" ? {} : { reasoningEffort: thinking }),
      ...(source === "custom" ? { prompt } : {}),
      ...(source === "replay" ? { fromRun: "latest" } : {}),
    };
    try {
      setResult(await cmd<BenchFinished>(CMD.devBenchRun, { request }));
      onFinished();
    } catch (reason) {
      setError(String(reason));
    } finally {
      awaitingResult.current = false;
      setRunning(false);
    }
  };

  const toggle = (key: string, checked: boolean) => {
    setSelected((current) => (checked ? [...current, key] : current.filter((item) => item !== key)));
  };
  const chosen = options.filter((option) => selected.includes(option.key));
  const requests = chosen.length * Number(rounds);
  const allSelected = options.length > 0 && chosen.length === options.length;
  const providers = [...new Set(options.map((option) => option.providerName))];
  const missingPrompt = source === "custom" && !prompt.trim();

  return (
    <InspectorBody>
      <Group
        title={chosen.length ? `模型（已选 ${chosen.length}）` : "模型"}
        right={options.length > 0 && (
          <Button size="sm" onClick={() => setSelected(allSelected ? [] : options.map((option) => option.key))}>
            {allSelected ? "全不选" : "全选"}
          </Button>
        )}
      >
        {options.length === 0 && (
          <p className="px-3.5 py-3 text-[13px] text-[var(--color-fg-muted)]">没有可测的大模型。在「模型」页填好密钥并打开要测的模型后再来。</p>
        )}
        {providers.map((provider) => (
          <div key={provider} role="group" aria-label={provider}>
            <div className="flex items-baseline justify-between px-3.5 pb-1 pt-3 text-xs text-[var(--color-fg-subtle)]">
              <span>{provider}</span>
              {options.some((option) => option.providerName === provider && option.price) && <span>每百万 token 输入 / 输出</span>}
            </div>
            {options.filter((option) => option.providerName === provider).map((option) => {
              const id = `bench-model-${option.key}`;
              return (
                <Row
                  key={option.key}
                  controlId={id}
                  label={(
                    <span className="flex items-center gap-2">
                      {option.label}
                      {option.current && <span className="rounded-full border border-[var(--color-line)] px-2 py-0.5 text-[11px] text-[var(--color-fg-subtle)]">当前</span>}
                    </span>
                  )}
                >
                  {option.price && <span className="text-xs tabular-nums text-[var(--color-fg-subtle)]">{option.price}</span>}
                  <Checkbox
                    id={id}
                    aria-label={`${option.providerName} · ${option.label}`}
                    checked={selected.includes(option.key)}
                    onChange={(event) => toggle(option.key, event.target.checked)}
                  />
                </Row>
              );
            })}
          </div>
        ))}
      </Group>
      <Group>
        <Row label="测试内容">
          <Tabs
            variant="view"
            ariaLabel="测试内容"
            tabs={[
              { key: "standard", label: "标准文本" },
              { key: "replay", label: "重放最近听写" },
              { key: "custom", label: "自定义" },
            ]}
            active={source}
            onChange={(key) => setSource(key as PromptSource)}
          />
        </Row>
        {source === "custom" && (
          <div className="px-3.5 py-3">
            <Textarea aria-label="测试提示词" rows={4} value={prompt} onChange={(event) => setPrompt(event.target.value)} />
          </div>
        )}
        <Row label="思考程度" hint="统一设置所有模型这次测速的思考程度，不改动各模型保存的设置。始终思考的模型选「关闭」时按最低档。">
          <Tabs<"model" | ThinkingLevel>
            variant="view"
            ariaLabel="思考程度"
            tabs={[{ key: "model", label: "按模型设置" }, ...THINKING_OPTIONS]}
            active={thinking}
            onChange={setThinking}
          />
        </Row>
        <Row label="轮数" hint="每个模型重复测几次，结果取中位数。网络波动大时多测几轮更准。">
          <Tabs
            variant="view"
            ariaLabel="轮数"
            tabs={[{ key: "1", label: "1" }, { key: "3", label: "3" }, { key: "5", label: "5" }]}
            active={rounds}
            onChange={setRounds}
          />
        </Row>
        <Row
          label={chosen.length ? `${chosen.length} 个模型同时测速，共 ${requests} 次真实请求` : "先勾选要测的模型"}
          message={<>按量计费的模型会产生费用。{error && <span role="alert" className="text-[var(--color-err)]"> {error}</span>}</>}
        >
          <Button size="sm" variant="primary" disabled={running || chosen.length === 0 || missingPrompt} onClick={() => void start()}>
            {running ? "测速中…" : "开始测速"}
          </Button>
        </Row>
      </Group>
      {result ? <BenchResult state={result} analysis={result.analysis} /> : live && <BenchResult state={live} />}
    </InspectorBody>
  );
}
