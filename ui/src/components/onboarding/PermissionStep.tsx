import { useEffect, useState } from "react";
import { Folder, Mic } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { micLevelFraction, useMicLevel } from "@/features/audio/useMicLevel";
import { useSetupPermissions } from "@/features/onboarding/permissions";
import { cn } from "@/lib/cn";
import { CMD, cmd, type DataRootStatus } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";

const WAVE_WEIGHTS = [0.42, 0.72, 0.56, 1, 0.78, 0.52, 0.64];

/**
 * 麦克风被系统拒绝（macOS 授权、Windows 隐私设置）后只能去系统设置里打开。
 * Windows 上被拒时采集照常启动、只是录到静音，所以切回窗口时要重查。
 */
function useMicPermissionDenied(startError: string) {
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    setError("");
    let active = true;
    const query = () => {
      void cmd<boolean>(CMD.microphonePermissionDenied)
        .then((value) => active && setDenied(value))
        .catch((reason) => console.error("读取麦克风权限状态失败", reason));
    };
    query();
    window.addEventListener("focus", query);
    return () => {
      active = false;
      window.removeEventListener("focus", query);
    };
  }, [startError]);

  const openSettings = () => {
    setError("");
    void cmd(CMD.openPrivacySettings, { kind: "microphone" }).catch((reason) => setError(String(reason)));
  };
  return { denied, error, openSettings };
}

/**
 * 第 1 步 · 权限与目录准备（严格区分 Windows 与 macOS）：
 * - Windows：仅展示「麦克风实时测音」与「数据存放位置」；
 * - macOS：额外展示 3 项真实系统权限（辅助功能、输入监控、屏幕录制可选）。
 */
export function PermissionStep({
  dataRoot,
  onOpenDirPicker,
}: {
  dataRoot: DataRootStatus | undefined;
  onOpenDirPicker: () => void;
}) {
  const micDeviceId = useDictPrefs((state) => state.prefs.micDeviceId);
  const { level, startError } = useMicLevel(micDeviceId);
  const permissions = useSetupPermissions();
  const mic = useMicPermissionDenied(startError);
  const [micHeard, setMicHeard] = useState(false);

  const fraction = micLevelFraction(level);
  useEffect(() => {
    if (!mic.denied && !startError && fraction > 0.04) {
      setMicHeard(true);
    }
  }, [fraction, mic.denied, startError]);

  const micReady = !mic.denied && !startError && micHeard;
  const activeDir = dataRoot?.configuredRoot ?? "读取中…";

  const macPermCards = [
    {
      id: "permissions",
      title: "辅助功能",
      desc: "把整理好的文字自动输入当前软件",
      ready: permissions.textInputReady,
      required: true,
      pending: "",
      error: permissions.errors.permissions,
      actionLabel: permissions.requesting ? "请求中…" : "授予权限",
      disabled: permissions.requesting,
      onAction: () => void permissions.requestTextInput(),
    },
    {
      id: "inputMonitoring",
      title: "输入监控",
      desc: "支持使用 CapsLock 快捷键触发听写",
      ready: permissions.check("inputMonitoring")?.status === "ready",
      required: true,
      pending: permissions.check("inputMonitoring") && permissions.check("inputMonitoring")?.status !== "ready"
        ? "未授权时 CapsLock 快捷键无法使用"
        : "",
      error: permissions.errors.inputMonitoring,
      actionLabel: "打开系统设置",
      disabled: false,
      onAction: () => permissions.openSettings("inputMonitoring", "input-monitoring"),
    },
    {
      id: "screenRecording",
      title: "屏幕录制（可选）",
      desc: "参考当前窗口文字，专有名词更准",
      ready: permissions.check("screenRecording")?.status === "ready",
      required: false,
      pending: permissions.check("screenRecording") && permissions.check("screenRecording")?.status !== "ready"
        ? "允许后要重启说吧！才生效"
        : "",
      error: permissions.errors.screenRecording,
      actionLabel: "打开系统设置",
      disabled: false,
      onAction: () => permissions.openSettings("screenRecording", "screen-recording"),
    },
  ];

  return (
    <div className="space-y-6" aria-label="权限与目录准备">
      <div className="space-y-2 text-center">
        <h1 className="text-[24px] font-semibold tracking-tight text-[var(--color-fg)]">
          你好，欢迎使用说吧！
        </h1>
        <p className="text-[14px] text-[var(--color-fg-muted)]">
          {permissions.needed
            ? "先完成基础权限与目录设置，很快就能开始语音输入。"
            : "先确认麦克风与文件存放位置，很快就能开始语音输入。"}
        </p>
      </div>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {/* 麦克风实时测音卡 */}
        <div className="flex flex-col justify-between gap-4 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-5 shadow-[var(--shadow-raise)]">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[var(--radius-lg)] bg-[var(--accent-soft)] text-[var(--color-accent-text)]">
                <Mic className="h-[18px] w-[18px]" aria-hidden />
              </div>
              <div>
                <div className="text-[15px] font-semibold text-[var(--color-fg)]">麦克风</div>
                <div className="text-xs text-[var(--color-fg-muted)]">说句话确认正常收音</div>
              </div>
            </div>
            {mic.denied ? (
              <span className="rounded-full bg-[color-mix(in_srgb,var(--color-err)_12%,transparent)] px-2.5 py-0.5 text-xs font-medium text-[var(--color-err)]">
                未授权
              </span>
            ) : micReady ? (
              <span className="rounded-full bg-[color-mix(in_srgb,var(--color-ok)_12%,transparent)] px-2.5 py-0.5 text-xs font-medium text-[var(--color-ok)]">
                已就绪
              </span>
            ) : (
              <span className="text-xs text-[var(--color-fg-faint)]">等待收音</span>
            )}
          </div>

          {mic.denied ? (
            <div className="flex items-center justify-between gap-3 rounded-[var(--radius-lg)] bg-[var(--color-surface-hover)] px-3.5 py-3">
              <span role="alert" className="text-xs text-[var(--color-err)]">
                麦克风权限已被拒绝{mic.error && `：${mic.error}`}
              </span>
              <Button size="sm" variant="primary" onClick={mic.openSettings}>
                打开系统设置
              </Button>
            </div>
          ) : (
            <div className="space-y-2">
              <div className="flex items-center justify-between gap-3 rounded-[var(--radius-lg)] bg-[var(--color-surface-hover)] px-3.5 py-3">
                <div className="flex h-6 items-end gap-1.5" aria-label="麦克风实时音量">
                  {WAVE_WEIGHTS.map((weight, index) => {
                    const pct = Math.max(18, Math.min(100, Math.round(18 + fraction * 82 * weight)));
                    return (
                      <span
                        key={index}
                        className="ob-mic-bar w-1 rounded-full bg-[var(--color-accent)]"
                        style={{ height: `${pct}%` }}
                      />
                    );
                  })}
                </div>
                <span className="text-xs text-[var(--color-fg-muted)]">
                  {micReady ? "收音正常" : "对着麦克风说句话"}
                </span>
              </div>
              {startError && (
                <p role="alert" className="text-xs text-[var(--color-err)]">
                  无法使用麦克风：{startError}
                </p>
              )}
            </div>
          )}
        </div>

        {/* 数据与模型存放目录卡 */}
        <div className="flex flex-col justify-between gap-4 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-5 shadow-[var(--shadow-raise)]">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[var(--radius-lg)] bg-[color-mix(in_srgb,var(--color-ok)_12%,transparent)] text-[var(--color-ok)]">
                <Folder className="h-[18px] w-[18px]" aria-hidden />
              </div>
              <div>
                <div className="text-[15px] font-semibold text-[var(--color-fg)]">数据存放位置</div>
                <div className="text-xs text-[var(--color-fg-muted)]">模型与历史记录均存于此</div>
              </div>
            </div>
            <button
              type="button"
              disabled={!dataRoot || dataRoot.restartRequired}
              onClick={onOpenDirPicker}
              className="text-xs font-medium text-[var(--color-accent-text)] hover:underline disabled:cursor-not-allowed disabled:opacity-45"
            >
              更改
            </button>
          </div>

          <div className="space-y-1.5 rounded-[var(--radius-lg)] bg-[var(--color-surface-hover)] px-3.5 py-2.5">
            <div className="truncate font-mono text-xs text-[var(--color-fg)]" title={activeDir}>
              {activeDir}
            </div>
            <div className="text-[11px] text-[var(--color-fg-muted)]">
              {dataRoot?.restartRequired ? (
                <span className="inline-flex items-center gap-2 text-[var(--color-warn)]">
                  <span>新位置重启后生效</span>
                  <button
                    type="button"
                    onClick={() => void cmd(CMD.restartApp)}
                    className="font-medium underline"
                  >
                    立即重启
                  </button>
                </span>
              ) : (
                "选任意目录或磁盘根目录都会自动新建专属文件夹"
              )}
            </div>
          </div>
        </div>
      </div>

      {permissions.needed ? (
        <div className="space-y-3 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-5 shadow-[var(--shadow-raise)]">
          <div className="flex items-center justify-between gap-2">
            <div className="text-[14px] font-semibold text-[var(--color-fg)]">macOS 系统权限</div>
            <span className="text-xs text-[var(--color-fg-muted)]">授权后回到窗口将自动刷新</span>
          </div>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
            {macPermCards.map((item) => (
              <div
                key={item.id}
                className="flex flex-col justify-between gap-3 rounded-[var(--radius-lg)] bg-[var(--color-surface-hover)] p-3.5"
              >
                <div>
                  <div className="text-[13px] font-medium text-[var(--color-fg)]">{item.title}</div>
                  <div className="mt-0.5 text-xs leading-snug text-[var(--color-fg-muted)]">{item.desc}</div>
                  {item.error ? (
                    <p role="alert" className="mt-1.5 text-[11px] text-[var(--color-err)]">{item.error}</p>
                  ) : (
                    !item.ready && item.pending && (
                      <p className="mt-1.5 text-[11px] text-[var(--color-warn)]">{item.pending}</p>
                    )
                  )}
                </div>
                <div className="flex items-center justify-between">
                  {item.ready ? (
                    <span className="text-xs font-medium text-[var(--color-ok)]">✓ 已授权</span>
                  ) : (
                    <Button
                      size="sm"
                      variant={item.required ? "primary" : "ghost"}
                      disabled={item.disabled}
                      onClick={item.onAction}
                      className={cn("h-7 px-3 text-xs")}
                    >
                      {item.actionLabel}
                    </Button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <p className="text-center text-xs text-[var(--color-fg-muted)]">
          Windows 无需额外系统授权，确认麦克风与目录后即可继续。
        </p>
      )}
    </div>
  );
}
