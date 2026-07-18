import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { firstAcceptedPath, PLUGIN_PACKAGE_DROP_ROUTE } from "@/features/fileDrop/routes";
import { installPluginPackage, requiresExplicitTrust } from "@/features/plugins/pluginInstaller";
import { CMD, EVT, cmd, emitEvent, on } from "@/lib/tauri";
import { usePluginImportStore } from "@/store/usePluginImportStore";
import { useUiStore } from "@/store/useUiStore";

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

export function PluginDropInstaller() {
  const [dragActive, setDragActive] = useState(false);
  const sourcePath = usePluginImportStore((state) => state.pendingPaths[0]);
  const enqueue = usePluginImportStore((state) => state.enqueue);
  const finishCurrent = usePluginImportStore((state) => state.finishCurrent);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);
  const [preview, setPreview] = useState<PackagePreview>();
  const [previewError, setPreviewError] = useState("");
  const [trustReason, setTrustReason] = useState("");
  const [installing, setInstalling] = useState(false);
  const [message, setMessage] = useState("");

  const close = () => {
    if (installing) return;
    finishCurrent();
  };

  useEffect(() => {
    let active = true;
    setPreview(undefined);
    setPreviewError("");
    setTrustReason("");
    setMessage("");
    if (!sourcePath) return;
    void cmd<PackagePreview>(CMD.previewProviderPlugin, { sourcePath })
      .then((value) => {
        if (active) setPreview(value);
      })
      .catch((error) => {
        if (active) setPreviewError(String(error));
      });
    return () => {
      active = false;
    };
  }, [sourcePath]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const drainPending = () => cmd<string[]>(CMD.takePendingProviderPluginImports)
      .then((paths) => enqueue(paths))
      .catch((error) => console.error("读取待导入说吧包失败", error));
    void drainPending();
    void on(EVT.providerPluginImportRequested, () => {
      void drainPending();
    }).then((stop) => {
      if (disposed) stop();
      else unlisteners.push(stop);
    });
    return () => {
      disposed = true;
      unlisteners.forEach((stop) => stop());
    };
  }, [enqueue]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter") {
          setDragActive(Boolean(firstAcceptedPath(payload.paths, PLUGIN_PACKAGE_DROP_ROUTE)));
          return;
        }
        if (payload.type === "leave") {
          setDragActive(false);
          return;
        }
        if (payload.type === "drop") {
          setDragActive(false);
          const pluginPath = firstAcceptedPath(payload.paths, PLUGIN_PACKAGE_DROP_ROUTE);
          if (pluginPath) enqueue([pluginPath]);
        }
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const install = async (allowUntrusted: boolean) => {
    if (!sourcePath) return;
    setInstalling(true);
    setMessage("");
    try {
      await installPluginPackage(sourcePath, {
        allowUnsigned: allowUntrusted,
        trustSigningKey: allowUntrusted,
        expectedArchiveSha256: preview?.archiveSha256,
      });
      setMessage(preview?.packageKind === "model-pack" ? "模型包已安装并加载。" : "插件已安装并加载。");
      setTrustReason("");
      setView("settings");
      setSettingsTab("plugins");
      await emitEvent(EVT.pluginRegistryChanged);
    } catch (error) {
      if (!allowUntrusted && requiresExplicitTrust(error)) {
        setTrustReason(String(error));
      } else {
        setMessage(`安装失败：${String(error)}`);
      }
    } finally {
      setInstalling(false);
    }
  };

  return (
    <>
      {dragActive && (
        <div className="pointer-events-none fixed inset-0 z-[calc(var(--z-modal)-1)] grid place-items-center bg-[var(--accent-soft)] p-8">
          <div className="rounded-[var(--radius-xl)] border border-dashed border-[var(--color-accent)] bg-[var(--color-overlay)] px-7 py-5 text-center shadow-[var(--shadow-popover)]">
            <p className="text-base font-medium text-[var(--color-fg)]">松开以安装说吧插件</p>
            <p className="mt-1 text-sm text-[var(--color-fg-subtle)]">支持 .sayit 插件包</p>
          </div>
        </div>
      )}
      {sourcePath && (
        <Modal
          open
          onClose={close}
          title={trustReason ? "确认信任扩展包来源" : "导入 .sayit 包"}
          showCloseButton={false}
          className="max-w-[460px]"
        >
          <div className="p-5">
            {previewError ? (
              <>
                <p className="text-sm font-medium text-[var(--color-err)]">无法读取说吧包</p>
                <p className="mt-2 break-words text-sm leading-relaxed text-[var(--color-fg-subtle)]">{previewError}</p>
              </>
            ) : trustReason ? (
              <>
                <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
                  此扩展包未签名，或签名密钥尚未受信任。仅在确认来源可靠时继续；安装后它将获得清单中声明的权限。
                </p>
                <p className="mt-3 break-words text-xs leading-relaxed text-[var(--color-err)]">{trustReason}</p>
              </>
            ) : preview ? (
              <>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="rounded-[var(--radius-sm)] bg-[var(--accent-soft)] px-2 py-1 text-xs text-[var(--color-accent-light)]">
                    {preview.packageKind === "model-pack" ? "本地模型包" : "在线插件"}
                  </span>
                  <p className="text-sm font-medium text-[var(--color-fg)]">{preview.name}</p>
                  <span className="text-xs text-[var(--color-fg-subtle)]">v{preview.version}</span>
                </div>
                <dl className="mt-4 grid grid-cols-[76px_1fr] gap-x-3 gap-y-2 text-sm">
                  <dt className="text-[var(--color-fg-subtle)]">能力</dt>
                  <dd className="text-[var(--color-fg)]">{preview.capabilities.map(capabilityLabel).join("、") || "无"}</dd>
                  <dt className="text-[var(--color-fg-subtle)]">模型</dt>
                  <dd className="text-[var(--color-fg)]">{preview.modelLabels.join("、") || "无显式模型"}</dd>
                  <dt className="text-[var(--color-fg-subtle)]">签名</dt>
                  <dd className="text-[var(--color-fg)]">{TRUST_LABEL[preview.trust]}</dd>
                </dl>
                <p className="mt-4 break-all text-xs text-[var(--color-fg-subtle)]">{fileName(sourcePath)}</p>
                <p className="mt-3 text-xs leading-relaxed text-[var(--color-fg-subtle)]">确认后才会安装；同 ID 的已安装版本会被替换。</p>
              </>
            ) : (
              <p className="text-sm text-[var(--color-fg-subtle)]">正在校验包清单、文件完整性与签名…</p>
            )}
            {message && <p className={message.startsWith("安装失败") ? "mt-4 text-sm text-[var(--color-err)]" : "mt-4 text-sm text-[var(--color-ok)]"}>{message}</p>}
            <div className="mt-6 flex justify-end gap-2">
              <Button size="sm" autoFocus onClick={close} disabled={installing}>{message && !message.startsWith("安装失败") ? "关闭" : "取消"}</Button>
              {!previewError && preview && (!message || message.startsWith("安装失败")) && (
                <Button size="sm" variant={trustReason ? "danger" : "primary"} disabled={installing} onClick={() => void install(Boolean(trustReason))}>
                  {installing ? "正在安装..." : trustReason ? "信任并安装" : "确认安装"}
                </Button>
              )}
            </div>
          </div>
        </Modal>
      )}
    </>
  );
}

interface PackagePreview {
  sourcePath: string;
  packageKind: "provider-plugin" | "model-pack";
  name: string;
  version: string;
  capabilities: string[];
  modelLabels: string[];
  trust: "trusted" | "signed-untrusted" | "integrity-only" | "unsigned";
  signingKeyId?: string;
  archiveSha256: string;
}

const TRUST_LABEL: Record<PackagePreview["trust"], string> = {
  trusted: "签名可信",
  "signed-untrusted": "签名有效，发布者密钥尚未受信任",
  "integrity-only": "文件完整性有效，但没有签名",
  unsigned: "未签名",
};

function capabilityLabel(capability: string) {
  return ({ asr: "语音识别", ocr: "文字识别", translation: "翻译", customization: "自定义词表" } as Record<string, string>)[capability] || capability;
}
