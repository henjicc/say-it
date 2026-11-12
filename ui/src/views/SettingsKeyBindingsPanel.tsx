import { useMemo } from "react";
import { ArrowRight } from "lucide-react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { shortcutLabel } from "@/features/dictation/hotkeys";
import { shortcutBindingsQuery, shortcutTargetKey, type ShortcutBindingItem } from "@/features/hotkeys/catalog";
import { useCachedQuery } from "@/hooks/useCachedQuery";
import { useUiStore } from "@/store/useUiStore";

/**
 * 全部快捷键的总览：一眼看清哪些键被占用、哪些没生效。
 * 每个快捷键只在用它的功能页里修改，这里点一下跳过去。
 */
export function SettingsKeyBindingsPanel() {
  const { data: items, error: loadError, refresh: load } = useCachedQuery(shortcutBindingsQuery);
  const error = loadError && !items ? `读取快捷键失败：${loadError}` : "";
  const openDictation = useUiStore((state) => state.openDictationShortcutSettings);
  const openAssistant = useUiStore((state) => state.openAssistantSettings);
  const openSubtitles = useUiStore((state) => state.openSubtitleShortcutSettings);

  const groups = useMemo(() => [
    { title: "语音输入", items: items?.filter((item) => item.target.kind === "dictationMain" || item.target.kind === "dictationProfile") ?? [] },
    { title: "助手", items: items?.filter((item) => item.target.kind === "assistant") ?? [] },
    { title: "字幕", items: items?.filter((item) => item.target.kind === "subtitles") ?? [] },
  ], [items]);

  const goToSource = (item: ShortcutBindingItem) => {
    if (item.target.kind === "subtitles") openSubtitles();
    else if (item.target.kind === "assistant") openAssistant(item.target.action);
    else openDictation(item.target.kind === "dictationProfile" ? item.target.profileId : undefined);
  };

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
          {group.items.map((item) => {
            const keys = shortcutLabel(item);
            return (
              <Row
                key={shortcutTargetKey(item.target)}
                // 名称带着「语音输入 · 」这类前缀，分组标题已经说明了属于哪个功能。
                label={item.name.split(" · ").pop()}
                // 注册失败时必须说出来：否则这里照样显示成已绑定，用户按了没反应也无从得知。
                message={item.registrationError && <span role="alert" className="text-[var(--color-err)]">当前未生效：{item.registrationError}</span>}
              >
                <button
                  type="button"
                  aria-label={`修改${item.name}的快捷键`}
                  onClick={() => goToSource(item)}
                  className="flex items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]"
                >
                  <span className={keys ? undefined : "text-[var(--color-fg-subtle)]"}>
                    {keys ? `${keys} · ${item.triggerMode === "pressHold" ? "按住" : "按一下"}` : "未设置"}
                  </span>
                  <ArrowRight className="h-3.5 w-3.5 flex-none" aria-hidden />
                </button>
              </Row>
            );
          })}
        </Group>
      ))}
    </InspectorBody>
  );
}
