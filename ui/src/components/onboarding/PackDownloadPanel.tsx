import { useEffect } from "react";
import { Check, Download } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { useLocalModelProgress, useLocalModels } from "@/features/models/localModels";
import type { LocalPackItem } from "@/features/onboarding/setup";

function formatBytes(bytes: number): string {
  return bytes >= 1024 ** 3
    ? `${(bytes / 1024 ** 3).toFixed(1)} GB`
    : `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;
}

/**
 * 套餐内多个模型包的一键下载与实时分包进度面板。
 */
export function PackDownloadPanel({
  items,
  titleIdle = "一键下载并自动配置",
  titleDone = "模型已下载并自动配好",
}: {
  items: readonly LocalPackItem[];
  titleIdle?: string;
  titleDone?: string;
}) {
  const ids = items.map((item) => item.id);
  const { snapshot, packs, downloaded, load, install, loadError, packsError } = useLocalModels();
  const { ready, downloading, busyElsewhere, unavailable, installError } = useLocalModelProgress(ids);

  useEffect(() => {
    void load();
  }, [load]);

  const error = installError || loadError || packsError;

  return (
    <div className="space-y-3 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-raise)]">
      <div className="flex items-center justify-between gap-3">
        <div className="text-[13px] font-medium text-[var(--color-fg)]">
          {ready ? titleDone : downloading ? "正在自动下载模型…" : titleIdle}
        </div>
        {ready ? (
          <span className="inline-flex items-center gap-1 text-xs font-medium text-[var(--color-ok)]">
            <Check className="h-3.5 w-3.5" aria-hidden />
            全部就绪
          </span>
        ) : downloading ? (
          <span className="text-xs font-medium text-[var(--color-accent-text)]">下载中…</span>
        ) : (
          <Button
            size="sm"
            variant="primary"
            disabled={busyElsewhere || unavailable}
            onClick={() => void install(ids)}
            className="h-8 rounded-full px-4 text-xs"
          >
            <Download className="h-3.5 w-3.5" aria-hidden />
            {installError ? "重新下载" : "开始下载"}
          </Button>
        )}
      </div>

      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        {items.map((item) => {
          const plugin = snapshot?.plugins.find((entry) => entry.id === item.id);
          const pack = packs?.find((entry) => entry.id === item.id);
          const isReady = plugin?.modelPack?.state === "ready";
          const totalBytes = pack?.sizeBytes ?? 0;
          const doneBytes = isReady ? totalBytes : (downloaded[item.id] ?? 0);
          const pct = isReady
            ? 100
            : totalBytes > 0
              ? Math.min(99, Math.round((doneBytes / totalBytes) * 100))
              : 0;
          const sizeLabel = totalBytes > 0 ? formatBytes(totalBytes) : item.fallbackSize;

          return (
            <div key={item.id} className="space-y-1.5 rounded-[var(--radius-lg)] bg-[var(--color-surface-hover)] px-3 py-2.5">
              <div className="flex items-center justify-between gap-2 text-xs">
                <span className="truncate text-[var(--color-fg)]">{item.name}</span>
                <span
                  className={
                    isReady
                      ? "shrink-0 font-mono font-medium text-[var(--color-ok)]"
                      : "shrink-0 font-mono text-[var(--color-fg-muted)]"
                  }
                >
                  {isReady ? "完成" : downloading && pct > 0 ? `${pct}%` : sizeLabel}
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-line-strong)]">
                <div
                  className="h-full rounded-full transition-[width] duration-200"
                  style={{
                    width: `${pct}%`,
                    backgroundColor: isReady ? "var(--color-ok)" : "var(--color-accent)",
                  }}
                />
              </div>
            </div>
          );
        })}
      </div>

      {busyElsewhere && (
        <p role="status" className="text-xs text-[var(--color-fg-muted)]">
          另一个模型正在下载，下完后才能开始
        </p>
      )}
      {unavailable && (
        <p role="alert" className="text-xs text-[var(--color-err)]">
          官方模型列表里暂时没有所选模型包，请切换其他方案或改用在线服务。
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--color-err)]">
          {error}
        </p>
      )}
    </div>
  );
}
