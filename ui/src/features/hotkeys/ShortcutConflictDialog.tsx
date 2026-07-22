import { TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { useShortcutConflictStore } from "./conflictFeedback";

export function ShortcutConflictDialog() {
  const message = useShortcutConflictStore((state) => state.message);
  const close = useShortcutConflictStore((state) => state.close);

  return (
    <Modal
      open={Boolean(message)}
      onClose={close}
      title="快捷键冲突"
      className="max-w-md"
      showCloseButton={false}
    >
      <div className="p-5">
        <div className="flex items-start gap-3">
          <div className="mt-0.5 rounded-full bg-[color-mix(in_srgb,var(--color-warn)_12%,transparent)] p-2 text-[var(--color-warn)]">
            <TriangleAlert className="h-4 w-4" aria-hidden />
          </div>
          <p className="min-w-0 flex-1 text-sm leading-relaxed text-[var(--color-fg-muted)]">{message}</p>
        </div>
        <div className="mt-5 flex justify-end">
          <Button variant="primary" autoFocus onClick={close}>知道了</Button>
        </div>
      </div>
    </Modal>
  );
}
