import { HistoryView } from "@/views/HistoryView";

export function HistoryPage() {
  return (
    <main className="min-w-0 flex-1 overflow-y-auto bg-[var(--color-panel)] px-8 pt-5 pb-8">
      <div className="mx-auto max-w-[920px]"><HistoryView /></div>
    </main>
  );
}
