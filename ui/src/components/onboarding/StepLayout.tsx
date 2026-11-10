import { ChevronLeft } from "lucide-react";
import { Button } from "@/components/ui/Button";

/** 每一步独立滚动；步骤切换时整个组件重建，不沿用上一页的滚动位置。 */
export function StepBody({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div
      role="region"
      aria-label={label}
      tabIndex={0}
      className="min-h-0 flex-1 overflow-y-auto break-words px-6 py-5 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--accent-ring)]"
    >
      <div className="flex flex-col gap-4">{children}</div>
    </div>
  );
}

/** 第一步没有「上一步」；当前步骤没准备好时主按钮不可用，并在旁边写明原因。 */
export function StepFooter({
  onBack,
  label,
  onNext,
  disabled,
  reason,
}: {
  onBack?: () => void;
  label: string;
  onNext: () => void;
  disabled?: boolean;
  reason?: string;
}) {
  return (
    <div className="flex shrink-0 items-center justify-between gap-4 border-t border-[var(--color-line)] px-6 py-4">
      {onBack ? (
        <Button variant="text" onClick={onBack}>
          <ChevronLeft className="h-4 w-4" aria-hidden />上一步
        </Button>
      ) : <span />}
      <div className="flex min-w-0 items-center gap-3">
        {reason && <p role="status" className="min-w-0 text-xs text-[var(--color-fg-subtle)]">{reason}</p>}
        <Button variant="primary" className="shrink-0" onClick={onNext} disabled={disabled}>{label}</Button>
      </div>
    </div>
  );
}

/** 步骤里的错误，保持可见。 */
export function StepError({ children }: { children: React.ReactNode }) {
  return <p role="alert" className="text-xs text-[var(--color-err)]">{children}</p>;
}
