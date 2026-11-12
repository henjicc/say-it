import { WindowDragStrip } from "@/components/layout/WindowDragStrip";
import { HistoryView } from "@/views/HistoryView";

export function HistoryPage() {
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <WindowDragStrip />
      <main className="min-h-0 flex-1 overflow-y-auto px-8 pt-5 pb-8">
        <div className="mx-auto max-w-[920px]"><HistoryView /></div>
      </main>
    </div>
  );
}
