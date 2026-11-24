import { useEffect, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, ROW_CONTROL_W, Row } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { CMD, cmd } from "@/lib/tauri";

const POLL_MS = 500;

interface ElementInfo {
  controlType: string;
  name: string;
  className: string;
  rect: [number, number, number, number];
}

interface Pointing {
  tMs: number;
  x: number;
  y: number;
  ocrText: string | null;
  ocrMs: number;
  element: ElementInfo | null;
  region: { element: ElementInfo; coverage: number } | null;
  trailRect: [number, number, number, number] | null;
  samplesUsed: number;
}

interface ProbeState {
  status: "idle" | "waiting" | "running" | "done" | "failed";
  seconds?: number;
  error?: string;
  file?: string;
  writeError?: string | null;
  report?: { samples: unknown[]; pointings: Pointing[]; uiaP50Ms: number; uiaMaxMs: number };
}

function describe(element: ElementInfo | null | undefined) {
  if (!element) return "取不到";
  const [left, top, right, bottom] = element.rect;
  const name = element.name.trim() || "（无名称）";
  return `${element.controlType || element.className || "元素"}「${name}」 ${right - left}×${bottom - top}`;
}

/** 沿着鼠标轨迹取光标下的控件，和动态上下文现有的截图取词放在一起对比。只读屏幕，不录音。 */
export function PointerProbePanel() {
  const [seconds, setSeconds] = useState("15");
  const [state, setState] = useState<ProbeState>({ status: "idle" });
  const [message, setMessage] = useState("");
  const active = state.status === "waiting" || state.status === "running";

  useEffect(() => {
    void cmd<ProbeState>(CMD.devPointerProbeState).then(setState).catch(() => {});
  }, []);

  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => {
      cmd<ProbeState>(CMD.devPointerProbeState).then(setState).catch((reason: unknown) => setMessage(String(reason)));
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, [active]);

  const start = () => {
    setMessage("");
    cmd<ProbeState>(CMD.devPointerProbeStart, { seconds: Number(seconds) })
      .then(setState)
      .catch((reason: unknown) => setMessage(String(reason)));
  };
  const stop = () => {
    cmd<ProbeState>(CMD.devPointerProbeCancel).then(setState).catch((reason: unknown) => setMessage(String(reason)));
  };

  const report = state.report;
  const status = state.status === "waiting"
    ? "3 秒后开始，请切到要测试的软件"
    : state.status === "running"
      ? "正在记录，晃动或停住鼠标指着要测的地方"
      : state.status === "failed"
        ? `失败：${state.error ?? ""}`
        : report
          ? `采样 ${report.samples.length} 次，认出指向 ${report.pointings.length} 次；取控件中位 ${report.uiaP50Ms} ms，最慢 ${report.uiaMaxMs} ms`
          : "";

  return (
    <InspectorBody>
      <Group>
        <Row label="测试时长">
          <Select className={ROW_CONTROL_W} aria-label="测试时长" value={seconds} disabled={active} onChange={(event) => setSeconds(event.target.value)}>
            <option value="10">10 秒</option>
            <option value="15">15 秒</option>
            <option value="30">30 秒</option>
          </Select>
        </Row>
        <Row
          label="指向测试"
          hint="点开始后有 3 秒切到别的软件。之后在要测的地方停住鼠标、绕圈或来回晃，每认出一次指向，就把截图取词、光标下的控件和晃动覆盖的区域记成一条。"
          message={(status || message) && (
            <span role={state.status === "failed" || message ? "alert" : "status"} className={state.status === "failed" || message ? "text-[var(--color-err)]" : undefined}>
              {message || status}
            </span>
          )}
        >
          {active
            ? <Button size="sm" onClick={stop}>停止</Button>
            : <Button size="sm" variant="primary" onClick={start}>开始</Button>}
        </Row>
      </Group>

      {report?.pointings.map((pointing, index) => (
        <Group key={`${pointing.tMs}-${index}`} title={`第 ${index + 1} 次 · ${(pointing.tMs / 1000).toFixed(1)} 秒`}>
          <Row label="截图取词"><span className="truncate text-right" title={pointing.ocrText ?? ""}>{pointing.ocrText ?? "没取到"}（{pointing.ocrMs} ms）</span></Row>
          <Row label="光标下的控件"><span className="truncate text-right" title={describe(pointing.element)}>{describe(pointing.element)}</span></Row>
          <Row label="晃动覆盖的区域">
            <span className="truncate text-right" title={describe(pointing.region?.element)}>
              {pointing.region ? `${describe(pointing.region.element)}（覆盖 ${Math.round(pointing.region.coverage * 100)}%）` : "取不到"}
            </span>
          </Row>
        </Group>
      ))}
      {state.file && <p className="text-xs text-[var(--color-fg-subtle)] break-all">完整记录：{state.file}</p>}
    </InspectorBody>
  );
}
