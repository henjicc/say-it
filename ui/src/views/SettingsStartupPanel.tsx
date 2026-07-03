import { useEffect, useState } from "react";
import { Switch } from "@/components/ui/Switch";
import { SettingsSection } from "@/components/ui/SettingsSection";

import { CMD, cmd } from "@/lib/tauri";

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
  );
}
