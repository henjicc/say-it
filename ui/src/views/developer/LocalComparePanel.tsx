import { useEffect, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Group, Row } from "@/components/ui/Group";
import { Select, Textarea } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import { CMD, cmd } from "@/lib/tauri";
import { formatMs, formatRate, modelKey } from "./format";
import { useCachedQuery } from "@/hooks/useCachedQuery";
import { benchModelsQuery } from "@/views/developer/ModelBenchPanel";

const STATUS_REFRESH_MS = 500;
const LOCAL_KEY = "local";

interface ModelOption {
  providerId: string;
  providerName: string;
  model: string;
  current: boolean;
}

type LoadStatus =
  | { state: "idle" | "loading" }
  | { state: "ready"; loadMs: number | null }
  | { state: "failed"; error: string };

interface Sample {
  firstTokenMs?: number | null;
  lastTokenMs?: number;
  outputTokens?: number | null;
  tokensEstimated?: boolean;
  tokensPerSec?: number | null;
  error?: string | null;
}

interface CompareRow {
  providerId: string;
  providerName: string;
  modelId: string;
  output?: string;
  sample: Sample;
}

interface CompareResult {
  rawText: string;
  appName: string;
  scene: string;
  rows: CompareRow[];
}

/** 与训练集的场景标签一一对应。 */
const SCENES: [string, string][] = [
  ["assistant", "向 AI 提需求"],
  ["code", "写代码"],
  ["chat", "聊天"],
  ["writing", "写作"],
  ["search", "搜索框"],
  ["general", "通用"],
];

function sceneLabel(scene: string) {
  return SCENES.find(([key]) => key === scene)?.[1] ?? scene;
}

function statusText(status: LoadStatus | null) {
  if (status?.state === "failed") return status.error;
  if (status?.state !== "ready") return "加载中…";
  return status.loadMs === null ? "已就绪" : `已就绪（加载 ${formatMs(status.loadMs)}）`;
}

function Metric({ children }: { children: React.ReactNode }) {
  return <td className="px-2 py-2.5 text-right tabular-nums text-[var(--color-fg-muted)]">{children}</td>;
}

function Result({ result }: { result: CompareResult }) {
  return (
    <>
      <Group title={`识别原文 · ${result.appName || "自定义文本"} · ${sceneLabel(result.scene)}`}>
        <p className="select-text whitespace-pre-wrap break-words px-3.5 py-3 text-[13px] text-[var(--color-fg-muted)]">{result.rawText}</p>
      </Group>
      <Group title="速度">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[480px] table-fixed text-[13px]">
            <caption className="sr-only">各模型速度对比</caption>
            <thead className="text-xs text-[var(--color-fg-subtle)]">
              <tr>
                <th scope="col" className="px-3.5 py-2.5 text-left font-normal">模型</th>
                <th scope="col" className="w-24 px-2 py-2.5 text-right font-normal">首字</th>
                <th scope="col" className="w-24 px-2 py-2.5 text-right font-normal">token/秒</th>
                <th scope="col" className="w-24 px-2 pr-3.5 py-2.5 text-right font-normal">总耗时</th>
              </tr>
            </thead>
            <tbody>
              {result.rows.map((row) => (
                <tr key={`${row.providerId}/${row.modelId}`} className="border-t border-[var(--color-line)]">
                  <th scope="row" className="min-w-0 px-3.5 py-2.5 text-left font-normal">
                    <div className="[overflow-wrap:anywhere] text-[var(--color-fg)]">{row.modelId}</div>
                    <div className="text-xs text-[var(--color-fg-subtle)]">{row.providerName}</div>
                  </th>
                  <Metric>{formatMs(row.sample.firstTokenMs)}</Metric>
                  <Metric>{formatRate(row.sample.tokensPerSec, row.sample.tokensEstimated)}</Metric>
                  <Metric>{row.sample.error ? "—" : formatMs(row.sample.lastTokenMs)}</Metric>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Group>
      {result.rows.map((row) => (
        <Group key={`${row.providerId}/${row.modelId}`} title={`${row.providerName} · ${row.modelId}`}>
          {row.sample.error ? (
            <p role="alert" className="px-3.5 py-3 text-[13px] [overflow-wrap:anywhere] text-[var(--color-err)]">{row.sample.error}</p>
          ) : (
            <p className="select-text whitespace-pre-wrap break-words px-3.5 py-3 text-[13px] text-[var(--color-fg)]">{row.output}</p>
          )}
        </Group>
      ))}
    </>
  );
}

/** 同一段听写同时交给本地微调模型和选中的云端模型，并排看效果和速度。本地模型打开页面就开始预加载。 */
export function LocalComparePanel() {
  const models = useCachedQuery(benchModelsQuery);
  const options: ModelOption[] = models.data ?? [];
  // 默认勾上本地模型和当前在用的模型；列表第一次读到时才设。
  const [picked, setSelected] = useState<string[]>();
  const selected = picked ?? [LOCAL_KEY, ...options.filter((option) => option.current).map(modelKey)];
  const [status, setStatus] = useState<LoadStatus | null>(null);
  const [source, setSource] = useState<"replay" | "text">("replay");
  const [text, setText] = useState("");
  const [scene, setScene] = useState("assistant");
  const [localInput, setLocalInput] = useState<"full" | "trained">("full");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<CompareResult | null>(null);

  useEffect(() => {
    void cmd(CMD.devLocalPreload).catch((reason) => setError(String(reason)));
  }, []);

  const loading = !status || status.state === "idle" || status.state === "loading";
  useEffect(() => {
    if (!loading) return;
    const refresh = () => void cmd<LoadStatus>(CMD.devLocalStatus).then(setStatus).catch((reason) => setError(String(reason)));
    refresh();
    const timer = window.setInterval(refresh, STATUS_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [loading]);

  const toggle = (key: string, checked: boolean) => {
    setSelected(checked ? [...selected, key] : selected.filter((item) => item !== key));
  };

  const start = async () => {
    setRunning(true);
    setError("");
    setResult(null);
    const models = options
      .filter((option) => selected.includes(modelKey(option)))
      .map(({ providerId, model }) => ({ providerId, model }));
    const request = {
      models,
      local: selected.includes(LOCAL_KEY),
      localFullPrompt: localInput === "full",
      ...(source === "text" ? { text, scene } : {}),
    };
    try {
      setResult(await cmd<CompareResult>(CMD.devLocalCompare, { request }));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRunning(false);
      // 对比时本地服务可能被重新加载或启动失败，刷新一次状态。
      void cmd<LoadStatus>(CMD.devLocalStatus).then(setStatus).catch((reason) => setError(String(reason)));
    }
  };

  const cloudCount = selected.filter((key) => key !== LOCAL_KEY).length;

  return (
    <InspectorBody>
      <Group title="模型">
        <div className="max-h-72 overflow-y-auto [&>*+*]:border-t [&>*+*]:border-[var(--color-line)]">
          <Row
            label="本地微调 0.8B"
            controlId="local-compare-local"
            message={<span role={status?.state === "failed" ? "alert" : undefined} className={status?.state === "failed" ? "text-[var(--color-err)]" : undefined}>{statusText(status)}</span>}
          >
            <Checkbox id="local-compare-local" checked={selected.includes(LOCAL_KEY)} onChange={(event) => toggle(LOCAL_KEY, event.target.checked)} />
          </Row>
          {options.map((option) => {
            const key = modelKey(option);
            const id = `local-compare-${key}`;
            return (
              <Row key={key} label={`${option.providerName} · ${option.model}${option.current ? "（当前）" : ""}`} controlId={id}>
                <Checkbox id={id} checked={selected.includes(key)} onChange={(event) => toggle(key, event.target.checked)} />
              </Row>
            );
          })}
        </div>
      </Group>
      <Group>
        <Row
          label="本地模型输入"
          hint="与云端相同：本地也收到完整模板、软件上下文和热词，速度和云端在同样输入下比较；模型没按这种输入训练过，效果只作参考。训练格式：只给场景和识别原文，看微调后的真实效果。"
        >
          <Tabs
            variant="view"
            ariaLabel="本地模型输入"
            tabs={[{ key: "full", label: "与云端相同" }, { key: "trained", label: "训练格式" }]}
            active={localInput}
            onChange={(key) => setLocalInput(key as "full" | "trained")}
          />
        </Row>
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
          <>
            <Row label="场景">
              <Select className="w-64" value={scene} aria-label="场景" onChange={(event) => setScene(event.target.value)}>
                {SCENES.map(([key, label]) => <option key={key} value={key}>{label}</option>)}
              </Select>
            </Row>
            <div className="px-3.5 py-3">
              <Textarea aria-label="测试文本" rows={4} value={text} onChange={(event) => setText(event.target.value)} />
            </div>
          </>
        )}
        <Row
          label={`同时请求，云端 ${cloudCount} 次`}
          message={<>云端按量计费的模型会产生费用；本地模型的加载时间不计入。{(error || models.error) && <span role="alert" className="text-[var(--color-err)]"> {error || `读取模型列表失败：${models.error}`}</span>}</>}
        >
          <Button
            size="sm"
            variant="primary"
            disabled={running || selected.length === 0 || (source === "text" && !text.trim())}
            onClick={() => void start()}
          >
            {running ? "对比中…" : "开始对比"}
          </Button>
        </Row>
      </Group>
      {result && <Result result={result} />}
    </InspectorBody>
  );
}
