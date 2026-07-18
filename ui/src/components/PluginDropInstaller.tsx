import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { CircleCheck, CircleX, LoaderCircle, TriangleAlert } from "lucide-react";
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

type InstallState =
  | { status: "idle" }
  | { status: "installing" }
  | { status: "success" }
  | { status: "error"; message: string; allowUntrusted: boolean };

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
  const [installState, setInstallState] = useState<InstallState>({ status: "idle" });
  const installing = installState.status === "installing";

  const close = () => {
    if (installing) return;
    finishCurrent();
  };

  useEffect(() => {
    let active = true;
    setPreview(undefined);
    setPreviewError("");
    setTrustReason("");
    setInstallState({ status: "idle" });
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
    setInstallState({ status: "installing" });
    setTrustReason("");
    try {
      await installPluginPackage(sourcePath, {
        allowUnsigned: allowUntrusted,
        trustSigningKey: allowUntrusted,
        expectedArchiveSha256: preview?.archiveSha256,
      });
      setInstallState({ status: "success" });
      setView("settings");
      setSettingsTab("plugins");
      void emitEvent(EVT.pluginRegistryChanged).catch((error) => {
        console.error("刷新插件目录失败", error);
      });
    } catch (error) {
      if (!allowUntrusted && requiresExplicitTrust(error)) {
        setInstallState({ status: "idle" });
        setTrustReason(String(error));
      } else {
        setInstallState({ status: "error", message: String(error), allowUntrusted });
      }
    }
  };

  const packageName = preview?.name || "所选插件";
  const installLabel = preview?.packageKind === "model-pack" ? "安装模型包" : "安装插件";
  const successDescription = preview?.packageKind === "model-pack"
    ? `${packageName} 已安装。关闭后可在插件管理中查看模型状态并启用。`
    : `${packageName} 已安装并加载。关闭后可在插件管理中配置或启用。`;

  return (
    <>
      {dragActive && (
        <div className="pointer-events-none fixed inset-0 z-[calc(var(--z-modal)-1)] grid place-items-center bg-[var(--accent-soft)] p-8">
          <div className="rounded-[var(--radius-xl)] border border-dashed border-[var(--color-accent)] bg-[var(--color-overlay)] px-7 py-5 text-center shadow-[var(--shadow-popover)]">
            <p className="text-base font-medium text-[var(--color-fg)]">松开以安装说吧插件</p>
            <p className="mt-1 text-sm text-[var(--color-fg-subtle)]">支持 .sayit 插件包和模型包</p>
          </div>
        </div>
      )}
      {sourcePath && (
        <Modal
          open
          onClose={close}
          title={trustReason ? "确认插件来源" : "安装插件"}
          showCloseButton={false}
          className="max-w-[460px]"
        >
          <div className="p-5">
            {installState.status === "success" ? (
              <div role="status" aria-live="polite" className="flex items-start gap-3">
                <CircleCheck className="mt-0.5 h-8 w-8 flex-none text-[var(--color-ok)]" aria-hidden />
                <div className="min-w-0">
                  <h4 className="text-base font-semibold text-[var(--color-fg)]">安装完成</h4>
                  <p className="mt-1 break-words text-sm leading-relaxed text-[var(--color-fg-muted)]">{successDescription}</p>
                </div>
              </div>
            ) : installState.status === "error" ? (
              <div role="alert">
                <div className="flex items-start gap-3">
                  <CircleX className="mt-0.5 h-8 w-8 flex-none text-[var(--color-err)]" aria-hidden />
                  <div className="min-w-0">
                    <h4 className="text-base font-semibold text-[var(--color-fg)]">安装失败</h4>
                    <p className="mt-1 text-sm leading-relaxed text-[var(--color-fg-muted)]">请检查错误信息后重新安装。</p>
                  </div>
                </div>
                <p className="mt-4 max-h-32 overflow-y-auto break-words rounded-[var(--radius-md)] bg-[color-mix(in_srgb,var(--color-err)_12%,transparent)] px-3 py-2.5 text-xs leading-relaxed text-[var(--color-err)]">
                  {installState.message}
                </p>
              </div>
            ) : installing ? (
              <div role="status" aria-live="polite" className="flex items-start gap-3">
                <LoaderCircle className="mt-0.5 h-7 w-7 flex-none animate-spin text-[var(--color-accent-light)]" aria-hidden />
                <div className="min-w-0">
                  <h4 className="text-sm font-semibold text-[var(--color-fg)]">正在安装</h4>
                  <p className="mt-1 break-words text-sm leading-relaxed text-[var(--color-fg-muted)]">正在校验并安装 {packageName}，请不要关闭窗口。</p>
                </div>
              </div>
            ) : previewError ? (
              <div role="alert">
                <div className="flex items-start gap-3">
                  <CircleX className="mt-0.5 h-8 w-8 flex-none text-[var(--color-err)]" aria-hidden />
                  <div className="min-w-0">
                    <h4 className="text-base font-semibold text-[var(--color-fg)]">无法安装插件</h4>
                    <p className="mt-1 text-sm leading-relaxed text-[var(--color-fg-muted)]">文件未通过校验，请确认这是有效的 .sayit 文件。</p>
                  </div>
                </div>
                <p className="mt-4 max-h-32 overflow-y-auto break-words rounded-[var(--radius-md)] bg-[color-mix(in_srgb,var(--color-err)_12%,transparent)] px-3 py-2.5 text-xs leading-relaxed text-[var(--color-err)]">
                  {previewError}
                </p>
              </div>
            ) : trustReason ? (
              <div role="alert">
                <div className="flex items-start gap-3">
                  <TriangleAlert className="mt-0.5 h-7 w-7 flex-none text-[var(--color-warn)]" aria-hidden />
                  <div className="min-w-0">
                    <h4 className="text-sm font-semibold text-[var(--color-fg)]">发布者尚未受信任</h4>
                    <p className="mt-1 text-sm leading-relaxed text-[var(--color-fg-muted)]">
                      仅在确认插件来源可靠时继续。安装后，插件将获得清单中声明的权限。
                    </p>
                  </div>
                </div>
                <p className="mt-4 break-words rounded-[var(--radius-md)] bg-[color-mix(in_srgb,var(--color-warn)_10%,transparent)] px-3 py-2.5 text-xs leading-relaxed text-[var(--color-warn)]">
                  {trustReason}
                </p>
              </div>
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
                <p className="mt-3 text-xs leading-relaxed text-[var(--color-fg-subtle)]">安装后，同 ID 的现有版本将被替换。</p>
              </>
            ) : (
              <div role="status" aria-live="polite" className="flex items-center gap-3 py-1">
                <LoaderCircle className="h-5 w-5 animate-spin text-[var(--color-accent-light)]" aria-hidden />
                <p className="text-sm text-[var(--color-fg-muted)]">正在校验插件清单、文件完整性与签名…</p>
              </div>
            )}
            <div className="mt-6 flex justify-end gap-2">
              {installState.status === "success" ? (
                <Button size="sm" variant="primary" autoFocus onClick={close}>完成</Button>
              ) : installState.status === "error" ? (
                <>
                  <Button size="sm" onClick={close}>关闭</Button>
                  <Button size="sm" variant="primary" autoFocus onClick={() => void install(installState.allowUntrusted)}>重新安装</Button>
                </>
              ) : previewError ? (
                <Button size="sm" autoFocus onClick={close}>关闭</Button>
              ) : installing ? (
                <Button size="sm" variant="primary" disabled>
                  <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden />
                  正在安装
                </Button>
              ) : (
                <>
                  <Button size="sm" autoFocus onClick={close}>取消</Button>
                  {preview && (
                    <Button size="sm" variant={trustReason ? "danger" : "primary"} onClick={() => void install(Boolean(trustReason))}>
                      {trustReason ? "信任并安装" : installLabel}
                    </Button>
                  )}
                </>
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
