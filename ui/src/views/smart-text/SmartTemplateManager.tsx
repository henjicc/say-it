import { memo, useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  Archive,
  ArchiveRestore,
  GripVertical,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { IconButton } from "@/components/ui/IconButton";
import { Modal } from "@/components/ui/Modal";
import { Tabs } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import {
  MAX_SMART_TEXT_TEMPLATES,
  defaultSmartTextTemplates,
  useDictPrefs,
  type DeletedSmartTextTemplate,
  type SmartTextTemplate,
} from "@/store/useDictPrefs";

type ManagerTab = "templates" | "trash";
type ConfirmAction = "delete-templates" | "purge-trash";
type Notice = { tone: "ok" | "err"; text: string };

const deletedAtFormatter = new Intl.DateTimeFormat("zh-CN", {
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

function formatDeletedAt(timestamp: number): string {
  return Number.isFinite(timestamp) ? deletedAtFormatter.format(timestamp) : "未知时间";
}

function selectionAfterToggle(current: Set<string>, id: string): Set<string> {
  const next = new Set(current);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

const sortableAccessibility = {
  screenReaderInstructions: {
    draggable: "按空格键或回车键抓取模板，使用上下方向键调整位置，再次按空格键或回车键放下，按 Esc 取消。",
  },
  announcements: {
    onDragStart: () => "已抓取模板。",
    onDragOver: ({ over }: { over: { id: string | number } | null }) =>
      over ? "模板位置已更新。" : "模板已离开可排序区域。",
    onDragEnd: ({ over }: { over: { id: string | number } | null }) =>
      over ? "模板排序完成。" : "模板位置未改变。",
    onDragCancel: () => "已取消模板排序。",
  },
};

const SortableTemplateRow = memo(function SortableTemplateRow({
  template,
  selected,
  isActive,
  busy,
  onToggle,
}: {
  template: SmartTextTemplate;
  selected: boolean;
  isActive: boolean;
  busy: boolean;
  onToggle: (id: string) => void;
}) {
  const {
    attributes,
    listeners,
    setActivatorNodeRef,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({
    id: template.id,
    disabled: busy,
    transition: {
      duration: 180,
      easing: "cubic-bezier(0.22, 1, 0.36, 1)",
    },
  });
  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 2 : undefined,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        "smart-template-sortable-row relative flex min-h-[66px] items-center gap-3 border-b border-[var(--color-line)] px-3 py-2.5 last:border-b-0",
        selected && "bg-[var(--accent-soft)]",
        isDragging && "bg-[var(--color-overlay)] shadow-[var(--shadow-popover)]",
      )}
    >
      <Checkbox
        checked={selected}
        disabled={busy}
        aria-label={`选择模板“${template.name}”`}
        onChange={() => onToggle(template.id)}
      />
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-medium text-[var(--color-fg)]" title={template.name}>
            {template.name || "未命名模板"}
          </span>
          {isActive && (
            <span className="shrink-0 rounded-[var(--radius-pill)] bg-[var(--accent-soft-strong)] px-2 py-0.5 text-[11px] text-[var(--color-accent-light)]">
              当前
            </span>
          )}
        </div>
        <p className="mt-1 truncate text-xs text-[var(--color-fg-faint)]" title={template.prompt}>
          {template.prompt.replace(/\s+/g, " ").trim() || "尚未填写提示词"}
        </p>
      </div>
      <button
        ref={setActivatorNodeRef}
        type="button"
        disabled={busy}
        {...attributes}
        {...listeners}
        aria-label={`拖动“${template.name}”调整顺序`}
        title="拖动调整顺序；也可按空格键抓取后使用方向键移动"
        className={cn(
          "flex h-[var(--control-h-sm)] w-[var(--control-h-sm)] shrink-0 touch-none items-center justify-center rounded-[var(--radius-md)] border border-transparent text-[var(--color-fg-faint)] transition-colors duration-[var(--dur-fast)] hover:border-[var(--color-line)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg-muted)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)] disabled:cursor-not-allowed disabled:opacity-40",
          isDragging ? "cursor-grabbing" : "cursor-grab",
        )}
      >
        <GripVertical className="h-4 w-4" strokeWidth={1.8} aria-hidden />
      </button>
    </div>
  );
});

export function SmartTemplateManager({
  open,
  onClose,
  onNotice,
}: {
  open: boolean;
  onClose: () => void;
  onNotice?: (notice: Notice) => void;
}) {
  const prefs = useDictPrefs((state) => state.prefs);
  const patch = useDictPrefs((state) => state.patch);
  const templates = prefs.smartTemplates;
  const trash = prefs.smartTemplateTrash;
  const [tab, setTab] = useState<ManagerTab>("templates");
  const [orderedTemplates, setOrderedTemplates] = useState(templates);
  const [selectedTemplateIds, setSelectedTemplateIds] = useState<Set<string>>(new Set());
  const [selectedTrashIds, setSelectedTrashIds] = useState<Set<string>>(new Set());
  const [confirmAction, setConfirmAction] = useState<ConfirmAction>();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice>();
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const sortableTemplateIds = useMemo(
    () => orderedTemplates.map((template) => template.id),
    [orderedTemplates],
  );

  const missingDefaultTemplates = useMemo(
    () => defaultSmartTextTemplates().filter(
      (template) => !templates.some((current) => current.id === template.id),
    ),
    [templates],
  );

  useEffect(() => setOrderedTemplates(templates), [templates]);

  useEffect(() => {
    setSelectedTemplateIds((current) => new Set(
      [...current].filter((id) => templates.some((template) => template.id === id)),
    ));
  }, [templates]);

  useEffect(() => {
    setSelectedTrashIds((current) => new Set(
      [...current].filter((id) => trash.some((entry) => entry.recoveryId === id)),
    ));
  }, [trash]);

  useEffect(() => {
    if (open) return;
    setSelectedTemplateIds(new Set());
    setSelectedTrashIds(new Set());
    setConfirmAction(undefined);
    setNotice(undefined);
  }, [open]);

  const report = (nextNotice: Notice, share = false) => {
    setNotice(nextNotice);
    if (share) onNotice?.(nextNotice);
  };

  const persistOrder = async (next: SmartTextTemplate[]) => {
    if (next === orderedTemplates || busy) return;
    const previous = orderedTemplates;
    setOrderedTemplates(next);
    setBusy(true);
    setNotice(undefined);
    try {
      await patch({ smartTemplates: next });
    } catch (error) {
      setOrderedTemplates(previous);
      report({ tone: "err", text: `调整顺序失败：${String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const toggleTemplateSelection = useCallback((id: string) => {
    setSelectedTemplateIds((current) => selectionAfterToggle(current, id));
  }, []);

  const finishSorting = (event: DragEndEvent) => {
    const { active: dragged, over } = event;
    if (!over || dragged.id === over.id) return;
    const sourceIndex = orderedTemplates.findIndex((template) => template.id === dragged.id);
    const targetIndex = orderedTemplates.findIndex((template) => template.id === over.id);
    if (sourceIndex < 0 || targetIndex < 0) return;
    void persistOrder(arrayMove(orderedTemplates, sourceIndex, targetIndex));
  };

  const deleteSelectedTemplates = async () => {
    const currentPrefs = useDictPrefs.getState().prefs;
    const selected = currentPrefs.smartTemplates.filter((template) => selectedTemplateIds.has(template.id));
    if (selected.length === 0) return;
    const remaining = currentPrefs.smartTemplates.filter((template) => !selectedTemplateIds.has(template.id));
    if (remaining.length === 0) {
      report({ tone: "err", text: "至少需要保留一个模板，请取消选择一个模板后再删除。" });
      setConfirmAction(undefined);
      return;
    }

    const deletedEntries: DeletedSmartTextTemplate[] = selected.map((template) => ({
      recoveryId: crypto.randomUUID(),
      template: { ...template },
      deletedAt: Date.now(),
    }));
    const activeRemoved = selectedTemplateIds.has(currentPrefs.smartTemplateId);
    const activeIndex = currentPrefs.smartTemplates.findIndex(
      (template) => template.id === currentPrefs.smartTemplateId,
    );
    const nextActive = activeRemoved
      ? remaining[Math.min(Math.max(activeIndex, 0), remaining.length - 1)]
      : undefined;

    setBusy(true);
    setNotice(undefined);
    try {
      await patch({
        smartTemplates: remaining,
        smartTemplateId: nextActive?.id ?? currentPrefs.smartTemplateId,
        smartTemplateTrash: [...deletedEntries, ...currentPrefs.smartTemplateTrash].slice(
          0,
          MAX_SMART_TEXT_TEMPLATES,
        ),
      });
      setSelectedTemplateIds(new Set());
      setConfirmAction(undefined);
      report({ tone: "ok", text: `已将 ${selected.length} 个模板移入回收站。` }, true);
    } catch (error) {
      report({ tone: "err", text: `批量删除失败：${String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const restoreEntries = async (recoveryIds: Set<string>) => {
    const currentPrefs = useDictPrefs.getState().prefs;
    const entries = currentPrefs.smartTemplateTrash.filter((entry) => recoveryIds.has(entry.recoveryId));
    if (entries.length === 0) return;
    if (currentPrefs.smartTemplates.length + entries.length > MAX_SMART_TEXT_TEMPLATES) {
      report({
        tone: "err",
        text: `最多支持 ${MAX_SMART_TEXT_TEMPLATES} 个模板，请减少恢复数量或先删除不需要的模板。`,
      });
      return;
    }

    const usedIds = new Set(currentPrefs.smartTemplates.map((template) => template.id));
    const restored = entries.map((entry) => {
      if (!usedIds.has(entry.template.id)) {
        usedIds.add(entry.template.id);
        return { ...entry.template };
      }
      const id = crypto.randomUUID();
      usedIds.add(id);
      return { ...entry.template, id, name: `${entry.template.name}（已恢复）` };
    });

    setBusy(true);
    setNotice(undefined);
    try {
      await patch({
        smartTemplates: [...currentPrefs.smartTemplates, ...restored],
        smartTemplateTrash: currentPrefs.smartTemplateTrash.filter(
          (entry) => !recoveryIds.has(entry.recoveryId),
        ),
      });
      setSelectedTrashIds(new Set());
      report({ tone: "ok", text: `已恢复 ${restored.length} 个模板。` }, true);
    } catch (error) {
      report({ tone: "err", text: `恢复模板失败：${String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const restoreDefaultTemplates = async () => {
    if (missingDefaultTemplates.length === 0) {
      report({ tone: "ok", text: "三个内置模板均已保留，无需补回。" });
      return;
    }
    const currentPrefs = useDictPrefs.getState().prefs;
    if (currentPrefs.smartTemplates.length + missingDefaultTemplates.length > MAX_SMART_TEXT_TEMPLATES) {
      report({ tone: "err", text: "模板数量已接近上限，请先删除不需要的模板。" });
      return;
    }
    setBusy(true);
    setNotice(undefined);
    try {
      await patch({ smartTemplates: [...currentPrefs.smartTemplates, ...missingDefaultTemplates] });
      report({ tone: "ok", text: `已补回 ${missingDefaultTemplates.length} 个内置模板。` }, true);
    } catch (error) {
      report({ tone: "err", text: `补回内置模板失败：${String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const purgeSelectedTrash = async () => {
    if (selectedTrashIds.size === 0) return;
    const currentPrefs = useDictPrefs.getState().prefs;
    const nextTrash = currentPrefs.smartTemplateTrash.filter(
      (entry) => !selectedTrashIds.has(entry.recoveryId),
    );
    const removedCount = currentPrefs.smartTemplateTrash.length - nextTrash.length;
    setBusy(true);
    setNotice(undefined);
    try {
      await patch({ smartTemplateTrash: nextTrash });
      setSelectedTrashIds(new Set());
      setConfirmAction(undefined);
      report({ tone: "ok", text: `已永久删除 ${removedCount} 个回收站模板。` }, true);
    } catch (error) {
      report({ tone: "err", text: `清理回收站失败：${String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const templateSelectionIsAll = templates.length > 0 && selectedTemplateIds.size === templates.length;
  const trashSelectionIsAll = trash.length > 0 && selectedTrashIds.size === trash.length;
  const restoreWouldOverflow = templates.length + selectedTrashIds.size > MAX_SMART_TEXT_TEMPLATES;

  return (
    <Modal
      open={open}
      onClose={() => !busy && onClose()}
      title="模板管理"
      className="max-w-[780px]"
    >
      <div className="flex min-h-[560px] flex-col p-5">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--color-line)] pb-4">
          <Tabs
            id="smart-template-manager-tabs"
            ariaLabel="模板管理分类"
            tabs={[
              { key: "templates", label: `现有模板 · ${templates.length}` },
              { key: "trash", label: `回收站 · ${trash.length}` },
            ]}
            active={tab}
            onChange={(nextTab) => {
              setTab(nextTab);
              setConfirmAction(undefined);
              setNotice(undefined);
            }}
          />
          <span className="text-xs text-[var(--color-fg-faint)]">
            最多 {MAX_SMART_TEXT_TEMPLATES} 个模板
          </span>
        </div>

        {notice && (
          <p
            role="status"
            aria-live="polite"
            className={cn(
              "mt-4 rounded-[var(--radius-md)] border px-3 py-2 text-xs",
              notice.tone === "err"
                ? "border-[color-mix(in_srgb,var(--color-rec)_30%,transparent)] bg-[color-mix(in_srgb,var(--color-rec)_10%,transparent)] text-[var(--color-err)]"
                : "border-[color-mix(in_srgb,var(--color-ok)_28%,transparent)] bg-[color-mix(in_srgb,var(--color-ok)_8%,transparent)] text-[var(--color-ok)]",
            )}
          >
            {notice.text}
          </p>
        )}

        {tab === "templates" ? (
          <div
            id="smart-template-manager-tabs-templates-panel"
            role="tabpanel"
            aria-labelledby="smart-template-manager-tabs-templates-tab"
            className="flex min-h-0 flex-1 flex-col"
          >
            <div className="flex flex-wrap items-center justify-between gap-3 py-4">
              <label className="flex cursor-pointer items-center gap-2 text-xs text-[var(--color-fg-subtle)]">
                <Checkbox
                  size="sm"
                  checked={templateSelectionIsAll}
                  disabled={busy || templates.length === 0}
                  onChange={() => setSelectedTemplateIds(
                    templateSelectionIsAll ? new Set() : new Set(templates.map((template) => template.id)),
                  )}
                />
                {selectedTemplateIds.size > 0 ? `已选择 ${selectedTemplateIds.size} 个` : "全选"}
              </label>
              <Button
                size="sm"
                variant="dangerHover"
                disabled={busy || selectedTemplateIds.size === 0}
                onClick={() => {
                  if (selectedTemplateIds.size >= templates.length) {
                    report({ tone: "err", text: "至少需要保留一个模板，请取消选择一个模板后再删除。" });
                    return;
                  }
                  setConfirmAction("delete-templates");
                }}
              >
                <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                删除所选
              </Button>
            </div>

            {confirmAction === "delete-templates" && (
              <div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--color-rec)_30%,transparent)] bg-[color-mix(in_srgb,var(--color-rec)_10%,transparent)] px-3 py-2.5">
                <p className="text-xs text-[var(--color-fg-muted)]">
                  将把选中的 {selectedTemplateIds.size} 个模板移入回收站，之后仍可恢复。
                </p>
                <div className="flex gap-2">
                  <Button size="sm" disabled={busy} onClick={() => setConfirmAction(undefined)}>取消</Button>
                  <Button size="sm" variant="danger" disabled={busy} onClick={() => void deleteSelectedTemplates()}>
                    {busy ? "正在删除..." : `删除 ${selectedTemplateIds.size} 个模板`}
                  </Button>
                </div>
              </div>
            )}

            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              accessibility={sortableAccessibility}
              onDragEnd={finishSorting}
            >
              <SortableContext items={sortableTemplateIds} strategy={verticalListSortingStrategy}>
                <div className="min-h-0 flex-1 overflow-y-auto rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)]">
                  {orderedTemplates.map((template) => (
                    <SortableTemplateRow
                      key={template.id}
                      template={template}
                      selected={selectedTemplateIds.has(template.id)}
                      isActive={template.id === prefs.smartTemplateId}
                      busy={busy}
                      onToggle={toggleTemplateSelection}
                    />
                  ))}
                </div>
              </SortableContext>
            </DndContext>
            <p className="pt-3 text-xs text-[var(--color-fg-faint)]">
              拖动每行右侧的手柄调整模板顺序。
            </p>
          </div>
        ) : (
          <div
            id="smart-template-manager-tabs-trash-panel"
            role="tabpanel"
            aria-labelledby="smart-template-manager-tabs-trash-tab"
            className="flex min-h-0 flex-1 flex-col"
          >
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--color-line)] py-4">
              <div className="min-w-0">
                <h4 className="text-sm font-medium text-[var(--color-fg)]">内置模板</h4>
                <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
                  {missingDefaultTemplates.length > 0
                    ? `发现 ${missingDefaultTemplates.length} 个缺失项，可按当前版本补回。`
                    : "三个内置模板均已保留。"}
                </p>
              </div>
              <Button size="sm" disabled={busy} onClick={() => void restoreDefaultTemplates()}>
                <ArchiveRestore className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                检查并补回
              </Button>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-3 py-4">
              <label className="flex cursor-pointer items-center gap-2 text-xs text-[var(--color-fg-subtle)]">
                <Checkbox
                  size="sm"
                  checked={trashSelectionIsAll}
                  disabled={busy || trash.length === 0}
                  onChange={() => setSelectedTrashIds(
                    trashSelectionIsAll ? new Set() : new Set(trash.map((entry) => entry.recoveryId)),
                  )}
                />
                {selectedTrashIds.size > 0 ? `已选择 ${selectedTrashIds.size} 个` : "全选"}
              </label>
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  title={restoreWouldOverflow ? `最多支持 ${MAX_SMART_TEXT_TEMPLATES} 个模板` : undefined}
                  disabled={busy || selectedTrashIds.size === 0 || restoreWouldOverflow}
                  onClick={() => void restoreEntries(selectedTrashIds)}
                >
                  <RotateCcw className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  恢复所选
                </Button>
                <Button
                  size="sm"
                  variant="dangerHover"
                  disabled={busy || selectedTrashIds.size === 0}
                  onClick={() => setConfirmAction("purge-trash")}
                >
                  <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  永久删除
                </Button>
              </div>
            </div>

            {confirmAction === "purge-trash" && (
              <div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--color-rec)_30%,transparent)] bg-[color-mix(in_srgb,var(--color-rec)_10%,transparent)] px-3 py-2.5">
                <p className="text-xs text-[var(--color-fg-muted)]">
                  永久删除后无法恢复选中的 {selectedTrashIds.size} 个模板。
                </p>
                <div className="flex gap-2">
                  <Button size="sm" disabled={busy} onClick={() => setConfirmAction(undefined)}>取消</Button>
                  <Button size="sm" variant="danger" disabled={busy} onClick={() => void purgeSelectedTrash()}>
                    {busy ? "正在删除..." : "确认永久删除"}
                  </Button>
                </div>
              </div>
            )}

            <div className="min-h-0 flex-1 overflow-y-auto rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)]">
              {trash.length === 0 ? (
                <div className="flex min-h-[230px] flex-col items-center justify-center px-6 text-center">
                  <Archive className="h-8 w-8 text-[var(--color-fg-faint)]" strokeWidth={1.5} aria-hidden />
                  <p className="mt-3 text-sm font-medium text-[var(--color-fg-muted)]">回收站为空</p>
                  <p className="mt-1 max-w-[46ch] text-xs leading-relaxed text-[var(--color-fg-subtle)]">
                    删除的模板会保留在这里，最多保存最近 {MAX_SMART_TEXT_TEMPLATES} 个。
                  </p>
                </div>
              ) : trash.map((entry) => {
                const selected = selectedTrashIds.has(entry.recoveryId);
                return (
                  <div
                    key={entry.recoveryId}
                    className={cn(
                      "flex min-h-[62px] items-center gap-3 border-b border-[var(--color-line)] px-3 py-2.5 last:border-b-0",
                      selected && "bg-[var(--accent-soft)]",
                    )}
                  >
                    <Checkbox
                      checked={selected}
                      disabled={busy}
                      aria-label={`选择已删除模板“${entry.template.name}”`}
                      onChange={() => setSelectedTrashIds((current) => selectionAfterToggle(current, entry.recoveryId))}
                    />
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-[var(--color-fg)]" title={entry.template.name}>
                        {entry.template.name || "未命名模板"}
                      </p>
                      <p className="mt-1 text-xs text-[var(--color-fg-faint)]">
                        删除于 {formatDeletedAt(entry.deletedAt)}
                      </p>
                    </div>
                    <IconButton
                      label={`恢复“${entry.template.name}”`}
                      size="sm"
                      title={templates.length >= MAX_SMART_TEXT_TEMPLATES ? "模板已达到数量上限" : undefined}
                      disabled={busy || templates.length >= MAX_SMART_TEXT_TEMPLATES}
                      onClick={() => void restoreEntries(new Set([entry.recoveryId]))}
                    >
                      <RotateCcw className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                    </IconButton>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
