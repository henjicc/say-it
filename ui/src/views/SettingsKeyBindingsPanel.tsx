import { useMemo, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";
import type { ShortcutTriggerMode } from "@/features/dictation/hotkeys";
import {
  clearShortcutBinding,
  shortcutBindingsQuery,
  shortcutTargetKey,
  updateShortcutBinding,
  type ShortcutBindingItem,
} from "@/features/hotkeys/catalog";
import { reportShortcutConflict } from "@/features/hotkeys/conflictFeedback";
import { fetchQuery, useCachedQuery } from "@/hooks/useCachedQuery";

/** 一个快捷键：这里改和在功能页里改是同一份，改完各处一起更新。 */
function BindingRow({ item }: { item: ShortcutBindingItem }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const run = async (operation: (current: ShortcutBindingItem) => Promise<ShortcutBindingItem[]>) => {
    setBusy(true);
    setError("");
    try {
      await operation(item);
    } catch (reason) {
      if (!reportShortcutConflict(reason)) setError(String(reason));
      await fetchQuery(shortcutBindingsQuery, true).catch(() => undefined);
    } finally {
      setBusy(false);
    }
  };
  // 注册失败时必须说出来：否则这里照样显示成已绑定，用户按了没反应也无从得知。
  const message = error || (item.registrationError && `当前未生效：${item.registrationError}`);
  return (
    <Row
      // 名称带着「语音输入 · 」这类前缀，分组标题已经说明了属于哪个功能。
      label={item.name.split(" · ").pop()}
      message={message && <span role="alert" className="text-[var(--color-err)]">{message}</span>}
    >
      <div className="flex items-center gap-2">
        {item.keyCode && (
          <Select
            className="w-24"
            aria-label={`${item.name}的触发方式`}
            value={item.triggerMode}
            disabled={busy}
            onChange={(event) => void run((current) => updateShortcutBinding(current, current, event.target.value as ShortcutTriggerMode))}
          >
            <option value="toggle">按一下</option>
            <option value="pressHold">按住</option>
          </Select>
        )}
        <div className={ROW_CONTROL_W}>
          <ShortcutRecorder
            value={item}
            disabled={busy}
            ariaLabel={`${item.name}的快捷键`}
            onChange={(shortcut) => void run((current) => updateShortcutBinding(current, shortcut, current.triggerMode))}
            onClear={() => void run(clearShortcutBinding)}
          />
        </div>
      </div>
    </Row>
  );
}

/** 全部快捷键的总览：一眼看清哪些键被占用、哪些没生效，也可以直接在这里改。 */
export function SettingsKeyBindingsPanel() {
  const { data: items, error: loadError, refresh: load } = useCachedQuery(shortcutBindingsQuery);
  const error = loadError && !items ? `读取快捷键失败：${loadError}` : "";

  const groups = useMemo(() => [
    { title: "语音输入", items: items?.filter((item) => item.target.kind === "dictationMain" || item.target.kind === "dictationProfile") ?? [] },
    { title: "助手", items: items?.filter((item) => item.target.kind === "assistant") ?? [] },
    { title: "字幕", items: items?.filter((item) => item.target.kind === "subtitles") ?? [] },
  ], [items]);

  if (error) {
    return (
      <Group>
        <Row label="读取快捷键失败" message={<span role="alert" className="text-[var(--color-err)]">{error}</span>}>
          <Button size="sm" onClick={() => void load()}>重新读取</Button>
        </Row>
      </Group>
    );
  }

  return (
    <InspectorBody>
      {groups.filter((group) => group.items.length > 0).map((group) => (
        <Group key={group.title} title={group.title}>
          {group.items.map((item) => <BindingRow key={shortcutTargetKey(item.target)} item={item} />)}
        </Group>
      ))}
    </InspectorBody>
  );
}
