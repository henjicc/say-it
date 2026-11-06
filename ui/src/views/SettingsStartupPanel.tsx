import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { Switch } from "@/components/ui/Switch";
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
    <Group title="数据目录">
      <Row
        label="存储位置"
        hint="设置、插件和模型保存在这个文件夹。更改位置会搬移全部数据，完成后重启生效。"
        message={
          <>
            <span className="block break-all font-mono">{status ? status.configuredRoot : "读取中…"}</span>
            {status?.restartRequired && (
              <span className="mt-1.5 block leading-relaxed text-[var(--color-warn)]">
                上一次更改尚未生效，重启后才会切换到该目录；在此之前无法再次更改位置。
              </span>
            )}
          </>
        }
      >
        {/* 上一次迁移尚未重启生效时禁止再迁：此时进程仍在旧根目录，而旧目录的数据
            已经被搬空，再迁一次会把指针指向一个空目录，重启后数据"消失"。
            后端 migrate_blocking 也有同样的校验，这里只是提前把入口关掉。 */}
        <Button size="sm" disabled={migrating || !status || status.restartRequired} onClick={() => void pickAndMigrate()}>
          {migrating ? "迁移中…" : "更改位置"}
        </Button>
        {status?.isCustom && (
          <Button size="sm" disabled={migrating || status.restartRequired} onClick={() => void migrateTo(status.defaultRoot)}>
            恢复默认位置
          </Button>
        )}
        {status?.restartRequired && (
          <Button variant="primary" size="sm" disabled={migrating} onClick={() => void cmd(CMD.restartApp)}>
            立即重启
          </Button>
        )}
      </Row>
      {(migrating || message) && (
        <div className="flex flex-col gap-2 px-3.5 py-3">
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
            <p role="status" className={`text-xs ${messageTone === "error" ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]"}`}>
              {message}
            </p>
          )}
        </div>
      )}
    </Group>
  );
}

/** 读取并保存开机自启与静默启动；设置页与简单模式共用。 */
export function useStartupSettings() {
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

  return { startup, startupMsg, saveStartup };
}

export function SettingsStartupPanel() {
  const { startup, startupMsg, saveStartup } = useStartupSettings();
  return (
    <InspectorBody>
      <Group title="启动设置">
        <Row label="开机自启" hint="登录系统后自动运行本程序。">
          <Switch
            checked={!!startup.autostart}
            label="开机自启"
            onChange={(v) =>
              saveStartup(
                { autostart: v, silentStart: startup.silentStart },
                v ? "已开启开机自启。" : "已关闭开机自启。",
              )
            }
          />
        </Row>
        <Row
          label="静默启动"
          hint="仅开机自启时生效：启动后不弹出窗口，直接驻留托盘。"
          disabled={!startup.autostart}
        >
          <Switch
            checked={!!startup.silentStart}
            disabled={!startup.autostart}
            label="静默启动"
            onChange={(v) =>
              saveStartup(
                { autostart: startup.autostart, silentStart: v },
                v ? "已开启静默启动（仅开机自启时生效）。" : "已关闭静默启动。",
              )
            }
          />
        </Row>
        {startupMsg && <p role="status" className="px-3.5 py-3 text-xs text-[var(--color-fg-subtle)]">{startupMsg}</p>}
      </Group>
      <DataRootSection />
    </InspectorBody>
  );
}
