import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Switch } from "@/components/ui/Switch";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { useTauriEvent } from "@/hooks/useTauriEvent";

import {
  CMD,
  EVT,
  cmd,
  type DataRootMigrationEvent,
  type DataRootStatus,
} from "@/lib/tauri";

interface StartupStatus {
  autostart?: boolean;
  silentStart?: boolean;
}

/** 设置开关行：标题 + 说明 + 右侧开关，统一高密度设置面板的行布局。 */
function ToggleRow({
  title,
  description,
  checked,
  onChange,
  disabled,
}: {
  title: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-center gap-4 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3.5">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-[var(--color-fg)]">{title}</p>
        <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-subtle)]">{description}</p>
      </div>
      <Switch checked={checked} onChange={onChange} disabled={disabled} label={title} />
    </div>
  );
}

function DataRootSection() {
  const [status, setStatus] = useState<DataRootStatus | null>(null);
  const [migrating, setMigrating] = useState(false);
  const [progress, setProgress] = useState<DataRootMigrationEvent | null>(null);
  const [message, setMessage] = useState("");
  const [messageTone, setMessageTone] = useState<"info" | "error">("info");

  const refresh = () =>
    cmd<DataRootStatus>(CMD.getDataRootStatus)
      .then(setStatus)
      .catch((e) => {
        setMessage(`读取数据目录状态失败：${String(e)}`);
        setMessageTone("error");
      });

  useEffect(() => {
    void refresh();
  }, []);

  useTauriEvent<DataRootMigrationEvent>(
    EVT.dataRootMigration,
    (payload) => {
      if (payload.phase === "copying") setProgress(payload);
    },
    migrating,
  );

  const migrateTo = async (target: string) => {
    setMigrating(true);
    setProgress(null);
    setMessage("");
    try {
      const next = await cmd<DataRootStatus>(CMD.migrateDataRoot, { target });
      setStatus(next);
      setMessage("迁移完成。新位置将在重启后生效。");
      setMessageTone("info");
    } catch (e) {
      setMessage(`迁移失败：${String(e)}`);
      setMessageTone("error");
      void refresh();
    } finally {
      setMigrating(false);
      setProgress(null);
    }
  };

  const pickAndMigrate = async () => {
    const picked = await open({ directory: true, multiple: false, title: "选择新的数据目录" });
    if (typeof picked !== "string" || !picked) return;
    await migrateTo(picked);
  };

  const percent =
    progress && progress.totalBytes > 0
      ? Math.min(100, Math.round((progress.copiedBytes / progress.totalBytes) * 100))
      : null;

  return (
    <SettingsSection title="数据目录">
      <div className="flex flex-col gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3.5">
        <div className="min-w-0">
          <p className="text-sm font-medium text-[var(--color-fg)]">存储位置</p>
          <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-subtle)]">
            设置、插件和模型统一保存在此目录；更改位置会把全部数据迁移过去，完成后需要重启生效。
          </p>
          <p className="mt-1.5 break-all font-mono text-xs text-[var(--color-fg-subtle)]">
            {status ? status.configuredRoot : "读取中…"}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="ghost" size="sm" disabled={migrating || !status} onClick={() => void pickAndMigrate()}>
            {migrating ? "迁移中…" : "更改位置"}
          </Button>
          {status?.isCustom && (
            <Button
              variant="ghost"
              size="sm"
              disabled={migrating}
              onClick={() => void migrateTo(status.defaultRoot)}
            >
              恢复默认位置
            </Button>
          )}
          {status?.restartRequired && (
            <Button variant="primary" size="sm" disabled={migrating} onClick={() => void cmd(CMD.restartApp)}>
              立即重启
            </Button>
          )}
        </div>
        {migrating && (
          <div className="flex items-center gap-3">
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--color-surface-strong)]">
              <div
                className="h-full rounded-full bg-[var(--color-accent)] transition-[width] duration-150"
                style={{ width: `${percent ?? 0}%` }}
              />
            </div>
            <span className="shrink-0 text-xs tabular-nums text-[var(--color-fg-subtle)]">
              {progress ? `${progress.copiedFiles}/${progress.totalFiles} 个文件` : "准备中…"}
            </span>
          </div>
        )}
        {message && (
          <p className={`text-xs ${messageTone === "error" ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]"}`}>
            {message}
          </p>
        )}
      </div>
    </SettingsSection>
  );
}

export function SettingsStartupPanel() {
  const [startup, setStartup] = useState<StartupStatus>({});
  const [startupMsg, setStartupMsg] = useState("");

  useEffect(() => {
    cmd<StartupStatus>(CMD.getStartupSettings)
      .then(setStartup)
      .catch((e) => setStartupMsg(`读取启动设置失败：${String(e)}`));
  }, []);

  const saveStartup = async (next: StartupStatus, message: string) => {
    try {
      const status = await cmd<StartupStatus>(CMD.setStartupSettings, {
        autostart: next.autostart,
        silentStart: next.silentStart,
      });
      setStartup(status);
      setStartupMsg(message);
    } catch (e) {
      setStartupMsg(`保存启动设置失败：${String(e)}`);
      cmd<StartupStatus>(CMD.getStartupSettings).then(setStartup).catch(() => {});
    }
  };

  return (
    <div className="flex flex-col gap-7">
    <SettingsSection title="启动设置">
      <ToggleRow
        title="开机自启"
        description="登录系统后自动运行本程序。"
        checked={!!startup.autostart}
        onChange={(v) =>
          saveStartup(
            { autostart: v, silentStart: startup.silentStart },
            v ? "已开启开机自启。" : "已关闭开机自启。",
          )
        }
      />
      <ToggleRow
        title="静默启动"
        description="仅开机自启时生效：启动后不弹出窗口，直接驻留托盘。"
        checked={!!startup.silentStart}
        disabled={!startup.autostart}
        onChange={(v) =>
          saveStartup(
            { autostart: startup.autostart, silentStart: v },
            v ? "已开启静默启动（仅开机自启时生效）。" : "已关闭静默启动。",
          )
        }
      />
      {startupMsg && <p className="text-xs text-[var(--color-fg-subtle)]">{startupMsg}</p>}
    </SettingsSection>
    <DataRootSection />
    </div>
  );
}
