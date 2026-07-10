import { useEffect, useState } from "react";
import { Copy, Eye, EyeOff } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { Input, Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, cmd } from "@/lib/tauri";
import { TRANSLATION_MODEL_NONE } from "@/features/translation/models";
import { useSubtitleStore } from "@/store/useSubtitleStore";

interface ObsOverlayStatus {
  ready: boolean;
  connected: boolean;
  url: string;
  installed: boolean;
  sourceName?: string;
  error?: string;
}

interface ObsConnectionStatus {
  obsVersion: string;
  websocketVersion: string;
  browserSourceAvailable: boolean;
  scenes: { name: string }[];
  canvasWidth: number;
  canvasHeight: number;
}

const defaultStatus: ObsOverlayStatus = { ready: false, connected: false, url: "", installed: false };
const PASSWORD_MASK = "•".repeat(16);

export function ObsOverlayPanel() {
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("4455");
  const [password, setPassword] = useState("");
  const [savedPassword, setSavedPassword] = useState("");
  const [hasSavedPassword, setHasSavedPassword] = useState(false);
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [passwordDirty, setPasswordDirty] = useState(false);
  const [passwordEditing, setPasswordEditing] = useState(false);
  const [overlay, setOverlay] = useState<ObsOverlayStatus>(defaultStatus);
  const [connection, setConnection] = useState<ObsConnectionStatus | null>(null);
  const [sceneName, setSceneName] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const subtitlePrefs = useSubtitleStore((state) => state.prefs);

  const connectionArgs = () => ({
    host: host.trim(),
    port: Math.max(1, Math.min(65535, Number(port) || 4455)),
    ...(passwordDirty ? { password } : {}),
  });

  const passwordInputValue = passwordDirty || passwordEditing
    ? password
    : hasSavedPassword
      ? passwordVisible
        ? savedPassword
        : PASSWORD_MASK
      : "";

  const commitSavedPassword = () => {
    if (passwordDirty) {
      setSavedPassword(password);
      setHasSavedPassword(!!password);
    }
    setPassword("");
    setPasswordDirty(false);
    setPasswordEditing(false);
    setPasswordVisible(false);
  };

  const refreshOverlay = async () => {
    const status = await cmd<ObsOverlayStatus>(CMD.getObsOverlayStatus);
    setOverlay(status);
    if (status.error) setError(status.error);
  };

  useEffect(() => {
    refreshOverlay().catch((reason) => setError(`读取 OBS 字幕服务状态失败：${String(reason)}`));
    cmd<{ host: string; port: number; hasPassword: boolean }>(CMD.getObsConnectionSettings)
      .then((settings) => {
        setHost(settings.host || "127.0.0.1");
        setPort(String(settings.port || 4455));
        setHasSavedPassword(settings.hasPassword);
      })
      .catch((reason) => setError(`读取 OBS 连接设置失败：${String(reason)}`));
  }, []);

  const togglePasswordVisibility = async () => {
    if (passwordDirty) {
      setPasswordVisible((current) => !current);
      return;
    }
    if (!passwordVisible && hasSavedPassword && !savedPassword) {
      try {
        setSavedPassword(await cmd<string>(CMD.getObsPassword));
        setPasswordEditing(false);
        setPasswordVisible(true);
      } catch (reason) {
        setError(`读取 OBS 密码失败：${String(reason)}`);
      }
      return;
    }
    setPasswordEditing(false);
    setPasswordVisible((current) => !current);
  };

  const connect = async () => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const status = await cmd<ObsConnectionStatus>(CMD.connectObs, {
        request: connectionArgs(),
      });
      setConnection(status);
      commitSavedPassword();
      setSceneName((current) => current || status.scenes[0]?.name || "");
      if (!status.browserSourceAvailable) {
        setError("OBS 已连接，但未检测到 Browser Source，无法自动安装字幕源。");
      } else {
        setMessage(`已连接 OBS ${status.obsVersion}（obs-websocket ${status.websocketVersion}）。`);
      }
    } catch (reason) {
      setConnection(null);
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    if (!sceneName) {
      setError("请先选择 OBS 场景。");
      return;
    }
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const canvasWidth = connection?.canvasWidth || 1920;
      const canvasHeight = connection?.canvasHeight || 1080;
      const fontSize = canvasHeight * (subtitlePrefs.fontSizePercent / 100) * 1.8;
      const effectiveLines = subtitlePrefs.mode === "replace" ? 1 : subtitlePrefs.lineCount;
      const translationRows =
        subtitlePrefs.translationModel !== TRANSLATION_MODEL_NONE && subtitlePrefs.translationLayout === "bilingual"
          ? 2
          : 1;
      const sourceWidth = Math.round(canvasWidth * (subtitlePrefs.widthPercent / 100));
      const sourceHeight = Math.ceil(
        fontSize * 1.38 * effectiveLines * translationRows + 20 * translationRows + (translationRows > 1 ? 10 : 0),
      );
      const next = await cmd<ObsOverlayStatus>(CMD.installObsOverlay, {
        request: {
          ...connectionArgs(),
          sceneName,
          sourceWidth,
          sourceHeight,
        },
      });
      setOverlay(next);
      commitSavedPassword();
      setMessage("OBS 字幕源已按当前字幕框尺寸更新，并放置在画布底部。 ");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const uninstall = async () => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const next = await cmd<ObsOverlayStatus>(CMD.uninstallObsOverlay, {
        request: connectionArgs(),
      });
      setOverlay(next);
      commitSavedPassword();
      setMessage("已删除说吧！创建的 OBS 字幕源。 ");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const copyUrl = async () => {
    try {
      await navigator.clipboard.writeText(overlay.url);
      setMessage("字幕源 URL 已复制。 ");
    } catch {
      setError("复制失败，请手动复制下方 URL。 ");
    }
  };

  return (
    <div className="flex flex-col gap-7">
      <SettingsSection title="本地字幕服务">
        <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          字幕页面仅监听本机，OBS 通过 Browser Source 读取。字体、颜色和背景跟随“字幕样式”；位置、缩放、裁切和层级在 OBS 中调整。
        </p>
        <FormGrid columns={1}>
          <Field layout="row" label="字幕源 URL">
            <div className="relative">
              <Input className="pr-12" readOnly value={overlay.url} placeholder="正在启动本地字幕服务…" />
              <button
                type="button"
                className="absolute right-2 top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-[var(--radius-sm)] text-[var(--color-fg-subtle)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)] disabled:cursor-not-allowed disabled:opacity-40"
                onClick={copyUrl}
                disabled={!overlay.url}
                aria-label="复制字幕源 URL"
                title="复制字幕源 URL"
              >
                <Copy className="h-4 w-4" strokeWidth={1.8} aria-hidden />
              </button>
            </div>
          </Field>
        </FormGrid>
        {!overlay.ready && !overlay.error && <p className="text-xs text-[var(--color-fg-subtle)]">本地字幕服务正在启动…</p>}
      </SettingsSection>

      <SettingsSection title="连接 OBS" right={<Button size="sm" variant="primary" onClick={connect} disabled={busy}>连接 OBS</Button>}>
        <FormGrid>
          <Field layout="row" label="地址">
            <Input value={host} onChange={(event) => setHost(event.target.value)} placeholder="127.0.0.1" disabled={busy} />
          </Field>
          <Field layout="row" label="端口">
            <Input value={port} inputMode="numeric" onChange={(event) => setPort(event.target.value)} placeholder="4455" disabled={busy} />
          </Field>
          <Field layout="row" label="密码">
            <div className="relative">
              <Input
                type={passwordVisible ? "text" : "password"}
                value={passwordInputValue}
                onFocus={() => {
                  if (hasSavedPassword && !passwordVisible && !passwordDirty) {
                    setPassword("");
                    setPasswordEditing(true);
                  }
                }}
                onBlur={() => {
                  if (!passwordDirty) setPasswordEditing(false);
                }}
                onChange={(event) => {
                  setPassword(event.target.value);
                  setPasswordDirty(true);
                  setPasswordEditing(true);
                }}
                placeholder={hasSavedPassword ? "输入新密码可覆盖当前配置" : "未启用认证可留空"}
                className="pr-11"
                disabled={busy}
              />
              <button
                type="button"
                aria-label={passwordVisible ? "隐藏密码" : "显示密码"}
                title={passwordVisible ? "隐藏密码" : "显示密码"}
                onClick={togglePasswordVisibility}
                disabled={busy || (!hasSavedPassword && !passwordDirty)}
                className="absolute right-2 top-1/2 grid h-8 w-8 -translate-y-1/2 place-items-center rounded-[var(--radius-md)] text-[var(--color-fg-subtle)] transition-colors hover:bg-[var(--color-surface-strong)] hover:text-[var(--color-fg)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)] disabled:cursor-not-allowed disabled:opacity-35"
              >
                {passwordVisible ? <EyeOff className="h-4 w-4" aria-hidden /> : <Eye className="h-4 w-4" aria-hidden />}
              </button>
            </div>
          </Field>
          <Field layout="row" label="安装场景">
            <Select value={sceneName} onChange={(event) => setSceneName(event.target.value)} disabled={busy || !connection?.browserSourceAvailable}>
              {!connection?.scenes.length && <option value="">请先连接 OBS</option>}
              {connection?.scenes.map((scene) => <option key={scene.name} value={scene.name}>{scene.name}</option>)}
            </Select>
          </Field>
        </FormGrid>
        <div className="flex flex-wrap gap-2">
          <Button variant="primary" onClick={install} disabled={busy || !connection?.browserSourceAvailable || !overlay.ready}>
            {overlay.installed ? "更新字幕源" : "安装字幕源"}
          </Button>
          <Button variant="danger" onClick={uninstall} disabled={busy || !overlay.installed}>卸载字幕源</Button>
        </div>
        <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
          自动安装会读取并匹配 OBS 当前画布尺寸。手动添加 Browser Source 时，建议设置为画布尺寸；无法确认时可先使用 1920×1080，并保持透明背景。
          {overlay.installed && overlay.sourceName ? ` 当前受管理的源：${overlay.sourceName}。` : ""}
        </p>
      </SettingsSection>

      {(message || error) && (
        <p className={error ? "whitespace-pre-line text-sm text-[#ff8589]" : "text-sm text-[var(--color-good)]"} role={error ? "alert" : "status"}>
          {error || message}
        </p>
      )}
    </div>
  );
}
