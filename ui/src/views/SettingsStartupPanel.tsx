import { useEffect, useState } from "react";
import { Card, CardTitle } from "@/components/ui/Card";
import { CheckField } from "@/components/ui/Field";
import { CMD, cmd } from "@/lib/tauri";

interface StartupStatus {
  autostart?: boolean;
  silentStart?: boolean;
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
    <Card>
      <CardTitle>启动设置</CardTitle>
      <div className="mt-3 flex flex-col gap-3">
        <CheckField
          checked={!!startup.autostart}
          onChange={(v) =>
            saveStartup(
              { autostart: v, silentStart: startup.silentStart },
              v ? "已开启开机自启。" : "已关闭开机自启。",
            )
          }
        >
          开机自启（登录系统后自动运行本程序）
        </CheckField>
        <CheckField
          checked={!!startup.silentStart}
          disabled={!startup.autostart}
          onChange={(v) =>
            saveStartup(
              { autostart: startup.autostart, silentStart: v },
              v ? "已开启静默启动（仅开机自启时生效）。" : "已关闭静默启动。",
            )
          }
        >
          静默启动（仅开机自启时生效：启动后不弹出窗口，直接驻留托盘）
        </CheckField>
      </div>
      {startupMsg && <p className="mt-2 text-xs text-white/50">{startupMsg}</p>}
    </Card>
  );
}
