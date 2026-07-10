import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { Input, Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, cmd } from "@/lib/tauri";

interface ObsOverlayStatus {
  ready: boolean;
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
}

const defaultStatus: ObsOverlayStatus = { ready: false, url: "", installed: false };

export function ObsOverlayPanel() {
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("4455");
  const [password, setPassword] = useState("");
  const [overlay, setOverlay] = useState<ObsOverlayStatus>(defaultStatus);
  const [connection, setConnection] = useState<ObsConnectionStatus | null>(null);
  const [sceneName, setSceneName] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const connectionArgs = () => ({
    host: host.trim(),
    port: Math.max(1, Math.min(65535, Number(port) || 4455)),
    password,
  });

  const refreshOverlay = async () => {
    const status = await cmd<ObsOverlayStatus>(CMD.getObsOverlayStatus);
    setOverlay(status);
    if (status.error) setError(status.error);
  };

  useEffect(() => {
    refreshOverlay().catch((reason) => setError(`读取 OBS 字幕服务状态失败：${String(reason)}`));
  }, []);

  const connect = async () => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const status = await cmd<ObsConnectionStatus>(CMD.connectObs, {
        request: connectionArgs(),
      });
      setConnection(status);
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
      const next = await cmd<ObsOverlayStatus>(CMD.installObsOverlay, {
        request: {
          ...connectionArgs(),
          sceneName,
        },
      });
      setOverlay(next);
      setMessage("OBS 字幕源已安装。后续请在 OBS 中拖拽、缩放和调整图层。 ");
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
            <div className="flex gap-2">
              <Input readOnly value={overlay.url} placeholder="正在启动本地字幕服务…" />
              <Button size="sm" onClick={copyUrl} disabled={!overlay.url}>复制</Button>
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
          <Field layout="row" label="WebSocket 密码">
            <Input type="password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder="仅用于本次连接，不会保存" disabled={busy} />
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
          未能自动安装时，可在 OBS 添加 Browser Source，粘贴上方 URL，设置为 1280×360，保持透明背景。
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
