import { useEffect, useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { SettingsSearch, type SettingsSearchItem } from "@/features/settings/SettingsSearch";
import { SettingsProviderPanel } from "@/views/SettingsProviderPanel";
import { SettingsLlmPanel } from "@/views/SettingsLlmPanel";
import { PluginManagerPanel } from "@/views/PluginManagerPanel";
import { SettingsStartupPanel } from "@/views/SettingsStartupPanel";
import { SettingsMicCuePanel } from "@/views/SettingsMicCuePanel";
import { SettingsAppearancePanel } from "@/views/SettingsAppearancePanel";
import { SettingsComparePanel } from "@/views/SettingsComparePanel";
import { SettingsAdvancedPanel } from "@/views/SettingsAdvancedPanel";
import { SettingsKeyBindingsPanel } from "@/views/SettingsKeyBindingsPanel";
import { SettingsHistoryPanel } from "@/views/SettingsHistoryPanel";
import { SettingsSetupPanel } from "@/views/SettingsSetupPanel";
import { useUiStore, type SettingsTabKey } from "@/store/useUiStore";

const TABS: TabItem<SettingsTabKey>[] = [
  { key: "model", label: "模型" },
  { key: "plugins", label: "插件" },
  { key: "audio", label: "音频" },
  { key: "general", label: "通用" },
  { key: "keys", label: "按键" },
  { key: "compare", label: "对比" },
  { key: "advanced", label: "高级" },
];

export function SettingsView() {
  const tab = useUiStore((state) => state.settingsTab);
  const setTab = useUiStore((state) => state.setSettingsTab);
  const [pendingTarget, setPendingTarget] = useState<SettingsSearchItem | null>(null);

  useEffect(() => {
    if (!pendingTarget || pendingTarget.tab !== tab) return;
    let highlightTimer = 0;
    let secondFrame = 0;
    let highlightedElement: HTMLElement | null = null;
    let cancelled = false;
    const firstFrame = window.requestAnimationFrame(() => {
      secondFrame = window.requestAnimationFrame(() => {
        if (cancelled) return;
        const panel = document.getElementById(`settings-tabs-${tab}-panel`);
        if (!panel) return;
        const targetText = pendingTarget.targetText.trim();
        const candidates = Array.from(panel.querySelectorAll<HTMLElement>("h2, h3, label, p, span, button"));
        const target = candidates.find((element) => element.textContent?.trim() === targetText)
          ?? candidates.find((element) => element.textContent?.trim().includes(targetText));

        let owner = target ?? panel;
        const focusSelector = "input:not([type='hidden']):not(:disabled), textarea:not(:disabled), select:not(:disabled), button:not(:disabled), [tabindex]:not([tabindex='-1'])";
        let focusable = owner.matches(focusSelector) ? owner : undefined;
        while (!focusable && owner !== panel) {
          focusable = owner.querySelector<HTMLElement>(focusSelector) ?? undefined;
          if (!focusable && owner.parentElement) owner = owner.parentElement;
        }

        const highlight = owner === panel ? target ?? panel : owner;
        highlightedElement = highlight;
        highlight.dataset.settingsSearchHit = "true";
        const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
        highlight.scrollIntoView?.({ behavior: reduceMotion ? "auto" : "smooth", block: "center" });
        focusable?.focus({ preventScroll: true });
        highlightTimer = window.setTimeout(() => {
          delete highlight.dataset.settingsSearchHit;
          highlightedElement = null;
          setPendingTarget((current) => current?.id === pendingTarget.id ? null : current);
        }, 1400);
      });
    });

    return () => {
      cancelled = true;
      window.cancelAnimationFrame(firstFrame);
      window.cancelAnimationFrame(secondFrame);
      window.clearTimeout(highlightTimer);
      if (highlightedElement) delete highlightedElement.dataset.settingsSearchHit;
    };
  }, [pendingTarget, tab]);

  const navigateToSetting = (item: SettingsSearchItem) => {
    setPendingTarget({ ...item });
    setTab(item.tab);
  };

  return (
    <div className="flex flex-col gap-7">
      <div className="flex flex-col gap-3">
        <PageHeader title="设置" />
        <SettingsSearch onSelect={navigateToSetting} />
      </div>

      <Tabs<SettingsTabKey>
        id="settings-tabs"
        ariaLabel="设置分类"
        tabs={TABS}
        active={tab}
        onChange={setTab}
      />

      <div
        id={`settings-tabs-${tab}-panel`}
        role="tabpanel"
        aria-labelledby={`settings-tabs-${tab}-tab`}
      >
        {tab === "model" && (
          <div className="flex flex-col gap-7">
            <SettingsProviderPanel />
            <SettingsLlmPanel />
          </div>
        )}
        {tab === "plugins" && <PluginManagerPanel />}
        {tab === "audio" && <SettingsMicCuePanel />}
        {tab === "general" && (
          <div className="flex flex-col gap-7">
            <SettingsStartupPanel />
            <SettingsSetupPanel />
            <SettingsAppearancePanel />
            <SettingsHistoryPanel />
          </div>
        )}
        {tab === "keys" && <SettingsKeyBindingsPanel />}
        {tab === "compare" && <SettingsComparePanel />}
        {tab === "advanced" && <SettingsAdvancedPanel />}
      </div>
    </div>
  );
}
