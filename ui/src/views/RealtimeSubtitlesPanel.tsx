import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Select } from "@/components/ui/Input";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import {
  toggleSubtitles,
  syncSubtitleIndicator,
  showSubtitlePreview,
  hideSubtitlePreview,
  applyObsOutputRouting,
} from "@/features/subtitles/controller";
import { useSubtitleStore } from "@/store/useSubtitleStore";
import { SubtitleGeneralPanel } from "@/views/SubtitleGeneralPanel";
import { SubtitleStylePanel } from "@/views/SubtitleStylePanel";
import { SubtitleTranslationPanel } from "@/views/SubtitleTranslationPanel";
import { ObsOverlayPanel } from "@/views/ObsOverlayPanel";

type TabKey = "general" | "style" | "translation" | "obs";

const TABS: TabItem<TabKey>[] = [
  { key: "general", label: "通用设置" },
  { key: "style", label: "字幕样式" },
  { key: "translation", label: "字幕翻译" },
  { key: "obs", label: "OBS 接入" },
];

export function RealtimeSubtitlesPanel() {
  const [tab, setTab] = useState<TabKey>("general");
  const [previewOpen, setPreviewOpen] = useState(false);
  const running = useSubtitleStore((s) => s.running);
  const prefs = useSubtitleStore((s) => s.prefs);
  const patch = useSubtitleStore((s) => s.patch);

  // 开关状态放在这一层（而不是某个 tab 内部），这样切通用设置/字幕样式/字幕翻译
  // 之间的任意 tab 时预览都不会中断，方便边看效果边调整各处设置；
  // 真正运行、或预览开着时，持续把最新设置同步到悬浮窗。
  useEffect(() => {
    if (running || previewOpen) syncSubtitleIndicator(prefs);
  }, [prefs, running, previewOpen]);

  // 预览开关的显示/隐藏生命周期：打开时在桌面实际位置模拟播放示例内容；
  // 关闭、真正开始字幕、或离开实时字幕这个页面（本组件卸载）时都自动收起。
  useEffect(() => {
    if (running || !previewOpen) return undefined;
    // 只在开关/运行状态变化时触发一次；样式跟随交给上面那个 effect。
    showSubtitlePreview(prefs);
    return () => {
      hideSubtitlePreview();
    };
  }, [previewOpen, running]);

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="实时字幕"
        description="持续识别语音并在屏幕上显示字幕，适合会议、网课、视频和临时转写。"
        actions={
          <>
            <Select
              value={prefs.obsOutputEnabled ? "obs" : "desktop"}
              onChange={(event) => {
                patch({ obsOutputEnabled: event.target.value === "obs" });
                void applyObsOutputRouting();
              }}
              // 与右侧两个 h-10 按钮等高对齐；Select 默认高度是表单控件的 --control-h。
              className="w-36 [&>button]:min-h-0 [&>button]:h-10 [&>button]:py-0"
              title="选择字幕输出位置。输出到 OBS 时需要先在“OBS 接入”里连接并安装字幕源；OBS 未就绪会自动回落到桌面悬浮窗。"
            >
              <option value="desktop">输出到桌面</option>
              <option value="obs">输出到 OBS</option>
            </Select>
            <Button
              variant="ghost"
              aria-pressed={previewOpen}
              disabled={running}
              onClick={() => setPreviewOpen(!previewOpen)}
              className={cn(
                previewOpen && "border-[var(--accent-ring)] bg-[var(--accent-soft)] text-[var(--color-accent)]",
              )}
              title="打开后按当前样式模拟播放示例内容（含滚动/替换动画，开启翻译时同步演示译文），不会启动麦克风识别、也不产生真实翻译请求；离开实时字幕页面会自动关闭。"
            >
              {previewOpen ? "正在预览" : "字幕预览"}
            </Button>
            <Button variant={running ? "danger" : "primary"} onClick={toggleSubtitles}>
              {running ? "停止字幕" : "开始字幕"}
            </Button>
          </>
        }
      />

      <Tabs<TabKey> tabs={TABS} active={tab} onChange={setTab} />

      {tab === "general" && <SubtitleGeneralPanel />}
      {tab === "style" && <SubtitleStylePanel />}
      {tab === "translation" && <SubtitleTranslationPanel />}
      {tab === "obs" && <ObsOverlayPanel />}
    </div>
  );
}
