import { useEffect, useId, useState } from "react";
import { CheckCircle2, CircleAlert } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, cmd, type SetupStatus } from "@/lib/tauri";

function EnvironmentStatus() {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [error, setError] = useState("");
  const [checking, setChecking] = useState(true);
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let active = true;
    setChecking(true);
    setError("");
    setStatus(null);
    void cmd<SetupStatus>(CMD.getSetupStatus)
      .then((next) => { if (active) setStatus(next); })
      .catch((cause) => { if (active) setError(String(cause)); })
      .finally(() => { if (active) setChecking(false); });
    return () => { active = false; };
  }, [revision]);

  const pending = status?.checks.filter((check) => check.status !== "ready").length ?? 0;
  return <div className="flex flex-col gap-3">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <p role="status" className="min-w-0 break-words text-sm text-[var(--color-fg-muted)]">
        {checking ? "正在检查环境…" : error ? "环境检查失败" : !status?.checks.length ? "暂无环境检查结果" : pending ? `${pending} 项需要处理` : "所有关键能力均可用"}
      </p>
      <Button size="sm" disabled={checking} onClick={() => setRevision((current) => current + 1)}>重新检查</Button>
    </div>
    {error && <p role="alert" className="break-words text-sm text-[var(--color-err)]">{error}</p>}
    {status && <ul className="divide-y divide-[var(--color-line)]">
      {status.checks.map((check) => <li key={check.id} className="flex items-start gap-3 py-3">
        {check.status === "ready"
          ? <CheckCircle2 aria-hidden className="mt-0.5 h-4 w-4 shrink-0 text-[var(--color-ok)]" />
          : <CircleAlert aria-hidden className="mt-0.5 h-4 w-4 shrink-0 text-[var(--color-warn)]" />}
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-[var(--color-fg)]">{check.title}<span className="ml-2 text-xs font-normal text-[var(--color-fg-muted)]">{check.status === "ready" ? "可用" : "待处理"}</span></p>
          <p className="mt-1 break-words text-xs leading-5 text-[var(--color-fg-muted)]">{check.message}</p>
        </div>
      </li>)}
    </ul>}
  </div>;
}

export function SettingsSetupPanel() {
  const [showStatus, setShowStatus] = useState(false);
  const statusId = useId();
  return (
    <SettingsSection title="使用引导">
      <div className="flex flex-wrap gap-2">
        <Button onClick={() => window.dispatchEvent(new Event("sayit-open-setup"))}>重新运行使用引导</Button>
        <Button aria-expanded={showStatus} aria-controls={statusId} onClick={() => setShowStatus((current) => !current)}>环境状态</Button>
      </div>
      <div id={statusId} hidden={!showStatus}>
        {showStatus && <EnvironmentStatus />}
      </div>
    </SettingsSection>
  );
}
