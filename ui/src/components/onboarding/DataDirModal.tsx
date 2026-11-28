import { useEffect, useState } from "react";
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { CUSTOM_DATA_SUBDIR, resolveDataDirPreview } from "@/features/onboarding/setup";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd, type DataRootStatus } from "@/lib/tauri";

/**
 * 引导内的数据目录选择弹窗：支持系统目录选择或手动输入路径（含磁盘根目录如 D:\），
 * 实时预览自动新建的 `SayItData` 专属目录。
 */
export function DataDirModal({
  open,
  status,
  onClose,
  onApplied,
}: {
  open: boolean;
  status: DataRootStatus | undefined;
  onClose: () => void;
  onApplied: (next: DataRootStatus) => void;
}) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    setDraft(status?.configuredRoot ?? "");
    setError("");
    setBusy(false);
  }, [open, status?.configuredRoot]);

  const defaultRoot = status?.defaultRoot ?? "";
  const preview = resolveDataDirPreview(draft, defaultRoot, isMacOS);
  const unchanged = Boolean(
    status && preview.trim().toLowerCase() === status.configuredRoot.trim().toLowerCase(),
  );

  const browse = async () => {
    setError("");
    try {
      const picked = await openFolderDialog({
        directory: true,
        multiple: false,
        title: "选择数据与模型存放目录",
      });
      if (typeof picked === "string" && picked) setDraft(picked);
    } catch (reason) {
      setError(`打开目录选择器失败：${String(reason)}`);
    }
  };

  const confirm = async () => {
    const trimmed = draft.trim();
    if (!trimmed) {
      setError("请选择或输入有效的目录路径");
      return;
    }
    if (unchanged) {
      onClose();
      return;
    }
    setBusy(true);
    setError("");
    try {
      const next = await cmd<DataRootStatus>(CMD.migrateDataRoot, { target: trimmed });
      onApplied(next);
      onClose();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={busy ? undefined : onClose}
      title="选择数据与模型存放目录"
      className="max-w-[460px]"
    >
      <div className="space-y-4 p-5">
        <p className="text-xs leading-relaxed text-[var(--color-fg-muted)]">
          可直接选择磁盘根目录（如 <span className="font-mono text-[var(--color-fg)]">{isMacOS ? "/Volumes/SSD" : "D:\\"}</span>），软件会自动在所选位置新建{" "}
          <span className="font-mono font-medium text-[var(--color-fg)]">{CUSTOM_DATA_SUBDIR}</span> 专属文件夹。
        </p>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <label htmlFor="onboarding-data-dir-input" className="text-xs font-medium text-[var(--color-fg-muted)]">
              目录路径
            </label>
            <div className="flex items-center gap-2">
              {status?.isCustom && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setDraft(status.defaultRoot)}
                  className="inline-flex items-center gap-1 text-xs text-[var(--color-fg-subtle)] hover:text-[var(--color-fg)] disabled:opacity-40"
                >
                  <RotateCcw className="h-3 w-3" aria-hidden />
                  恢复默认
                </button>
              )}
              <Button size="sm" disabled={busy} onClick={() => void browse()}>
                <FolderOpen className="h-3.5 w-3.5" aria-hidden />
                浏览文件夹
              </Button>
            </div>
          </div>
          <Input
            id="onboarding-data-dir-input"
            value={draft}
            disabled={busy}
            onChange={(event) => {
              setDraft(event.target.value);
              setError("");
            }}
            placeholder={isMacOS ? "/Users/username/Documents" : "D:\\"}
            className="font-mono text-xs"
            spellCheck={false}
          />
        </div>

        <div className="rounded-[var(--radius-lg)] bg-[var(--accent-soft)] px-3.5 py-2.5 text-xs">
          <div className="font-medium text-[var(--color-accent-text)]">实际将存放在：</div>
          <div className="mt-1 break-all font-mono text-[var(--color-fg)]">{preview || "—"}</div>
        </div>

        {error && (
          <p role="alert" className="text-xs leading-relaxed text-[var(--color-err)]">
            {error}
          </p>
        )}

        <div className="flex items-center justify-end gap-2 pt-1">
          <Button variant="text" size="sm" disabled={busy} onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" size="sm" disabled={busy || !draft.trim()} onClick={() => void confirm()}>
            {busy ? "正在切换…" : "确定使用此目录"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
