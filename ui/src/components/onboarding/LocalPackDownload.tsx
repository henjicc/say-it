import { useEffect } from "react";
import { CheckCircle2, Download } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Meter } from "@/components/ui/Meter";
import { useLocalModelProgress, useLocalModels } from "@/features/models/localModels";
import { StepError } from "./StepLayout";

export const formatSize = (bytes: number) =>
  bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;

/** 下载这一步要用的官方模型包，并显示进度。 */
export function LocalPackDownload({ ids }: { ids: readonly string[] }) {
  const { load, install, loadError, packsError } = useLocalModels();
  const { ready, downloading, busyElsewhere, unavailable, installError, total, done } = useLocalModelProgress(ids);

  useEffect(() => {
    void load();
  }, [load]);

  if (ready) {
    return <p className="flex items-center gap-1.5 text-xs text-[var(--color-ok)]"><CheckCircle2 className="h-3.5 w-3.5" aria-hidden />模型已下载</p>;
  }
  if (downloading) {
    return (
      <div className="flex items-center gap-3">
        <Meter value={total ? done / total : 0} label="模型下载进度" className="flex-1" />
        {total > 0 && <span className="shrink-0 text-xs tabular-nums text-[var(--color-fg-subtle)]">{formatSize(done)} / {formatSize(total)}</span>}
      </div>
    );
  }
  if (unavailable) return <StepError>官方模型列表里暂时没有这个模型，请选别的。</StepError>;
  return (
    <div className="flex flex-col items-start gap-2">
      <Button size="sm" disabled={busyElsewhere} onClick={() => void install(ids)}>
        <Download className="h-3.5 w-3.5" aria-hidden />
        {installError ? "重新下载" : total ? `下载模型（${formatSize(total)}）` : "下载模型"}
      </Button>
      {busyElsewhere && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">另一个模型正在下载，下完后才能开始</p>}
      {(installError || loadError || packsError) && <StepError>{installError || loadError || packsError}</StepError>}
    </div>
  );
}
