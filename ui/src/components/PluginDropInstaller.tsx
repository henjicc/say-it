import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { firstAcceptedPath, PLUGIN_PACKAGE_DROP_ROUTE } from "@/features/fileDrop/routes";
import { installPluginPackage, requiresExplicitTrust } from "@/features/plugins/pluginInstaller";

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

export function PluginDropInstaller() {
  const [dragActive, setDragActive] = useState(false);
  const [sourcePath, setSourcePath] = useState<string>();
  const [requiresTrust, setRequiresTrust] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [message, setMessage] = useState("");

  const close = () => {
    if (installing) return;
    setSourcePath(undefined);
    setRequiresTrust(false);
    setMessage("");
  };

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
          if (pluginPath) setSourcePath(pluginPath);
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
      });
      setMessage("插件已安装并加载。");
    } catch (error) {
      if (!allowUntrusted && requiresExplicitTrust(error)) {
        setRequiresTrust(true);
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
      <Modal
        open={Boolean(sourcePath)}
        onClose={close}
        title={requiresTrust ? "确认信任插件来源" : "安装插件"}
        className="max-w-[460px]"
      >
        <div className="p-5">
          {requiresTrust ? (
            <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
              此插件未签名或其签名密钥尚未受信任。仅在确认来源可靠时继续，安装后该插件可访问其声明的权限。
            </p>
          ) : (
            <>
              <p className="text-sm text-[var(--color-fg-subtle)]">即将安装以下 .sayit 插件包：</p>
              <p className="mt-2 break-all text-sm font-medium text-[var(--color-fg)]">{sourcePath && fileName(sourcePath)}</p>
              <p className="mt-3 text-xs leading-relaxed text-[var(--color-fg-subtle)]">安装时会校验插件包和签名；如安装同 ID 的新版本，会直接替换当前插件。</p>
            </>
          )}
          {message && <p className={message.startsWith("安装失败") ? "mt-4 text-sm text-[var(--color-err)]" : "mt-4 text-sm text-[var(--color-ok)]"}>{message}</p>}
          <div className="mt-6 flex justify-end gap-2">
            <Button size="sm" onClick={close} disabled={installing}>{message && !message.startsWith("安装失败") ? "关闭" : "取消"}</Button>
            {(!message || message.startsWith("安装失败")) && (
              <Button size="sm" variant={requiresTrust ? "danger" : "primary"} disabled={installing} onClick={() => void install(requiresTrust)}>
                {installing ? "正在安装..." : requiresTrust ? "信任并安装" : "确认安装"}
              </Button>
            )}
          </div>
        </div>
      </Modal>
    </>
  );
}
