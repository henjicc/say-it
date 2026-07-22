import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { basename } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, RefreshCw } from "lucide-react";
import { Field } from "@/components/ui/Field";
import { IconButton } from "@/components/ui/IconButton";
import { Select } from "@/components/ui/Input";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import type { RunningApp } from "@/store/useDictPrefs";

const supportsExecutableBrowse = navigator.userAgent.includes("Windows");

function processKey(processName: string) {
  return processName.trim().toLowerCase();
}

function isExeFileName(fileName: string) {
  return (
    fileName.length > 4
    && fileName !== "."
    && fileName !== ".."
    && /\.[eE][xX][eE]$/.test(fileName)
  );
}

function errorDetail(error: unknown) {
  return error instanceof Error ? error.message : String(error || "未知错误");
}

export interface RunningAppSelection {
  processName: string;
  appName: string;
  windowTitle: string | null;
  source: "running" | "file";
}

export function RunningAppPicker({
  value,
  onSelect,
  onClear,
  label = "软件",
  hint,
  placeholder = "请选择软件",
  disabled = false,
  className,
}: {
  value: string;
  onSelect: (selection: RunningAppSelection) => void | Promise<void>;
  onClear?: () => void | Promise<void>;
  label?: React.ReactNode;
  hint?: React.ReactNode;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
}) {
  const controlId = useId();
  const [runningApps, setRunningApps] = useState<RunningApp[]>([]);
  const [loading, setLoading] = useState(false);
  const [picking, setPicking] = useState(false);
  const [message, setMessage] = useState("");
  const pickingRef = useRef(false);

  const loadRunningApps = useCallback(async () => {
    setLoading(true);
    setMessage("");
    try {
      setRunningApps(await cmd<RunningApp[]>(CMD.listRunningApps));
    } catch (error) {
      setMessage(`读取软件列表失败：${errorDetail(error)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadRunningApps();
  }, [loadRunningApps]);

  const uniqueRunningApps = useMemo(() => {
    const seen = new Set<string>();
    return runningApps.filter((app) => {
      const key = processKey(app.processName);
      if (!key || seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }, [runningApps]);

  const selectedRunningApp = uniqueRunningApps.find(
    (app) => processKey(app.processName) === processKey(value),
  );
  const selectedValue = selectedRunningApp?.processName ?? value;

  const commitSelection = async (selection: RunningAppSelection) => {
    setMessage("");
    try {
      await onSelect(selection);
    } catch (error) {
      setMessage(`更新软件选择失败：${errorDetail(error)}`);
    }
  };

  const clearSelection = async () => {
    if (!onClear) return;
    setMessage("");
    try {
      await onClear();
    } catch (error) {
      setMessage(`更新软件选择失败：${errorDetail(error)}`);
    }
  };

  const browseExecutable = async () => {
    if (pickingRef.current) return;
    pickingRef.current = true;
    setPicking(true);
    setMessage("");
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Windows 应用程序", extensions: ["exe"] }],
      });
      if (typeof selected !== "string") return;

      const processName = await basename(selected);
      if (!isExeFileName(processName)) {
        setMessage("请选择 EXE 格式的 Windows 应用程序。");
        return;
      }
      await commitSelection({
        processName,
        appName: processName.slice(0, -4),
        windowTitle: null,
        source: "file",
      });
    } catch (error) {
      setMessage(`选择应用程序失败：${errorDetail(error)}`);
    } finally {
      pickingRef.current = false;
      setPicking(false);
    }
  };

  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <Field
        label={label}
        controlId={controlId}
        hint={hint}
        actions={(
          <>
            {supportsExecutableBrowse && (
              <IconButton
                label="从本地选择 EXE"
                disabled={disabled || picking}
                onClick={() => void browseExecutable()}
              >
                <FolderOpen className="h-4 w-4" strokeWidth={1.8} aria-hidden />
              </IconButton>
            )}
            <IconButton
              label="刷新软件列表"
              disabled={disabled || loading}
              onClick={() => void loadRunningApps()}
            >
              <RefreshCw
                className={cn("h-4 w-4", loading && "animate-spin")}
                strokeWidth={1.8}
                aria-hidden
              />
            </IconButton>
          </>
        )}
      >
        <Select
          id={controlId}
          searchable
          searchPlaceholder="搜索软件…"
          value={selectedValue}
          disabled={disabled}
          onChange={(event) => {
            if (!event.target.value) {
              void clearSelection();
              return;
            }
            const app = uniqueRunningApps.find(
              (candidate) => processKey(candidate.processName) === processKey(event.target.value),
            );
            if (!app) return;
            void commitSelection({
              processName: app.processName,
              appName: app.appName,
              windowTitle: app.windowTitle,
              source: "running",
            });
          }}
        >
          <option value="">{loading && uniqueRunningApps.length === 0 ? "正在读取软件列表…" : placeholder}</option>
          {value && !selectedRunningApp && (
            <option value={value}>{value}（未运行）</option>
          )}
          {uniqueRunningApps.map((app) => (
            <option key={processKey(app.processName)} value={app.processName}>
              {app.windowTitle ? `${app.appName} — ${app.windowTitle}` : app.appName}
            </option>
          ))}
        </Select>
      </Field>
      {message && <p role="alert" className="text-xs text-[var(--color-err)]">{message}</p>}
    </div>
  );
}
