import { useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { Select, Textarea } from "@/components/ui/Input";
import { Tabs } from "@/components/ui/Tabs";
import { CMD, cmd } from "@/lib/tauri";
import { formatMs } from "./format";

interface SideResult {
  output?: string;
  error?: string;
  ms?: number;
}

interface LocalCompareResult {
  rawText: string;
  appName: string;
  scene: string;
  model: string;
  local: SideResult & { startupMs?: number | null };
  cloud: SideResult & { providerId: string; model: string };
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

function Output({ side }: { side: SideResult }) {
  if (side.error) return <p role="alert" className="px-3.5 py-3 text-[13px] text-[var(--color-err)]">{side.error}</p>;
  return <p className="select-text whitespace-pre-wrap break-words px-3.5 py-3 text-[13px] text-[var(--color-fg)]">{side.output}</p>;
}

function Duration({ ms }: { ms?: number }) {
  return <span className="text-[13px] tabular-nums text-[var(--color-fg-muted)]">{formatMs(ms)}</span>;
}

function CompareResult({ result }: { result: LocalCompareResult }) {
  return (
    <>
      <Group title={`识别原文 · ${result.appName || "自定义文本"} · ${sceneLabel(result.scene)}`}>
        <p className="select-text whitespace-pre-wrap break-words px-3.5 py-3 text-[13px] text-[var(--color-fg-muted)]">{result.rawText}</p>
      </Group>
      <Group title={`本地：${result.model}`}>
        <Row label="请求耗时"><Duration ms={result.local.ms} /></Row>
        {typeof result.local.startupMs === "number" && <Row label="加载模型"><Duration ms={result.local.startupMs} /></Row>}
        <Output side={result.local} />
      </Group>
      <Group title={`云端：${result.cloud.providerId} / ${result.cloud.model || "默认模型"}`}>
        <Row label="请求耗时"><Duration ms={result.cloud.ms} /></Row>
        <Output side={result.cloud} />
      </Group>
    </>
  );
}

/** 同一段听写分别交给本地微调模型和当前云端智能处理，并排看效果和耗时。 */
export function LocalComparePanel() {
  const [source, setSource] = useState<"replay" | "text">("replay");
  const [text, setText] = useState("");
  const [scene, setScene] = useState("assistant");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<LocalCompareResult | null>(null);

  const start = async () => {
    setRunning(true);
    setError("");
    setResult(null);
    try {
      const request = source === "text" ? { text, scene } : {};
      setResult(await cmd<LocalCompareResult>(CMD.devLocalCompare, { request }));
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
          label="本地和云端各请求 1 次"
          message={<>云端按量计费的模型会产生费用；第一次对比要先加载本地模型。{error && <span role="alert" className="text-[var(--color-err)]"> {error}</span>}</>}
        >
          <Button size="sm" variant="primary" disabled={running || (source === "text" && !text.trim())} onClick={() => void start()}>
            {running ? "对比中…" : "开始对比"}
          </Button>
        </Row>
      </Group>
      {result && <CompareResult result={result} />}
    </InspectorBody>
  );
}
