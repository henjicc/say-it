import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Search, X } from "lucide-react";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/cn";
import type { SettingsTabKey } from "@/store/useUiStore";

export interface SettingsSearchItem {
  id: string;
  label: string;
  tab: SettingsTabKey;
  tabLabel: string;
  section: string;
  targetText: string;
  keywords: string;
}

export const SETTINGS_SEARCH_ITEMS: SettingsSearchItem[] = [
  { id: "asr-providers", label: "ASR 供应商", tab: "model", tabLabel: "模型", section: "识别模型与密钥", targetText: "ASR 供应商", keywords: "语音识别 听写 api key 密钥 模型" },
  { id: "ocr-providers", label: "OCR 供应商", tab: "model", tabLabel: "模型", section: "识别模型与密钥", targetText: "OCR 供应商", keywords: "文字识别 截图 api key 密钥 模型" },
  { id: "translation-providers", label: "翻译供应商", tab: "model", tabLabel: "模型", section: "识别模型与密钥", targetText: "翻译供应商", keywords: "翻译 api key 密钥 模型" },
  { id: "llm-providers", label: "大语言模型", tab: "model", tabLabel: "模型", section: "模型供应商", targetText: "大语言模型", keywords: "llm ai 智能处理 api key 密钥 默认模型" },
  { id: "plugins", label: "插件管理", tab: "plugins", tabLabel: "插件", section: "安装、启用与卸载", targetText: "插件管理", keywords: "sayit 模型包 安装 扫描 启用 卸载" },
  { id: "input-device", label: "输入设备", tab: "audio", tabLabel: "音频", section: "麦克风保活", targetText: "输入设备", keywords: "麦克风 录音设备 音频输入" },
  { id: "microphone-keepalive", label: "麦克风保活", tab: "audio", tabLabel: "音频", section: "输入设备", targetText: "麦克风保活", keywords: "麦克风 设备 关闭 延迟 秒" },
  { id: "audio-cues", label: "提示音", tab: "audio", tabLabel: "音频", section: "开始与结束提示", targetText: "提示音", keywords: "声音 开始提示音 结束提示音 自定义音效" },
  { id: "autostart", label: "开机自启", tab: "general", tabLabel: "通用", section: "启动设置", targetText: "开机自启", keywords: "登录系统 自动启动 开机启动" },
  { id: "silent-start", label: "静默启动", tab: "general", tabLabel: "通用", section: "启动设置", targetText: "静默启动", keywords: "托盘 不弹窗口 后台启动" },
  { id: "data-root", label: "数据目录", tab: "general", tabLabel: "通用", section: "存储位置", targetText: "数据目录", keywords: "存储位置 迁移 恢复默认 模型目录 插件目录" },
  { id: "setup", label: "使用引导与环境状态", tab: "general", tabLabel: "通用", section: "使用引导", targetText: "使用引导", keywords: "首次使用 环境检查 权限 引导" },
  { id: "tone", label: "整体色调", tab: "general", tabLabel: "通用", section: "外观", targetText: "整体色调", keywords: "暗色 亮色 主题 明暗" },
  { id: "accent", label: "强调色", tab: "general", tabLabel: "通用", section: "外观", targetText: "强调色", keywords: "主题色 按钮 选中项 焦点 滑块 颜色" },
  { id: "background", label: "背景基色", tab: "general", tabLabel: "通用", section: "外观", targetText: "背景色", keywords: "背景 柔和背景 自定义 主题 颜色" },
  { id: "glass", label: "全局系统毛玻璃", tab: "general", tabLabel: "通用", section: "外观", targetText: "全局系统毛玻璃", keywords: "透明 模糊 材质 底色强度 边框强度" },
  { id: "history", label: "本地历史", tab: "general", tabLabel: "通用", section: "历史与学习", targetText: "本地历史", keywords: "保存历史 保留天数 排除应用 个性化纠错 学习记忆 清理数据" },
  { id: "key-bindings", label: "快捷键集中管理", tab: "keys", tabLabel: "按键", section: "全部快捷键", targetText: "集中管理", keywords: "按键 热键 绑定 单击 长按 清除" },
  { id: "model-compare", label: "模型对比", tab: "compare", tabLabel: "对比", section: "录音或音频文件", targetText: "开始对比", keywords: "识别效果 多模型 并排比较 上传音频 录音" },
  { id: "diagnostics", label: "诊断日志", tab: "advanced", tabLabel: "高级", section: "日志与诊断包", targetText: "诊断日志", keywords: "详细元数据 正文日志 日志目录 清空 导出诊断包" },
  { id: "silence-disconnect", label: "静音断流", tab: "advanced", tabLabel: "高级", section: "连接与阈值", targetText: "静音断流", keywords: "静音 阈值 断开 api 实时字幕 语音输入" },
  { id: "loudness-denoise", label: "响度与降噪", tab: "advanced", tabLabel: "高级", section: "音频调校", targetText: "响度与降噪", keywords: "lufs rnnoise vad 峰值 目标响度 降噪强度" },
  { id: "equalizer", label: "均衡器（高低频）", tab: "advanced", tabLabel: "高级", section: "音频调校", targetText: "均衡器（高低频）", keywords: "eq 低频 高频 增益 厚度 亮度" },
  { id: "audio-lab", label: "录音试听与波形", tab: "advanced", tabLabel: "高级", section: "音频调校", targetText: "录音试听与波形", keywords: "ab 试听 原始 处理后 波形 音频" },
];

function normalize(value: string) {
  return value.trim().toLocaleLowerCase().replace(/\s+/g, " ");
}

export function filterSettings(query: string) {
  const terms = normalize(query).split(" ").filter(Boolean);
  if (terms.length === 0) return [];

  return SETTINGS_SEARCH_ITEMS
    .filter((item) => {
      const haystack = normalize(`${item.label} ${item.tabLabel} ${item.section} ${item.keywords}`);
      return terms.every((term) => haystack.includes(term));
    })
    .sort((left, right) => {
      const value = normalize(query);
      const leftLabel = normalize(left.label);
      const rightLabel = normalize(right.label);
      const leftRank = leftLabel === value ? 0 : leftLabel.startsWith(value) ? 1 : leftLabel.includes(value) ? 2 : 3;
      const rightRank = rightLabel === value ? 0 : rightLabel.startsWith(value) ? 1 : rightLabel.includes(value) ? 2 : 3;
      return leftRank - rightRank;
    });
}

export function SettingsSearch({ onSelect }: { onSelect: (item: SettingsSearchItem) => void }) {
  const listboxId = useId().replace(/:/g, "");
  const rootRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const results = useMemo(() => filterSettings(query), [query]);

  useEffect(() => {
    if (!open) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [open]);

  const choose = (item: SettingsSearchItem) => {
    setQuery(item.label);
    setOpen(false);
    onSelect(item);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      setOpen(false);
      return;
    }
    if (results.length === 0) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((current) => event.key === "ArrowDown"
        ? (current + 1) % results.length
        : (current - 1 + results.length) % results.length);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      choose(results[Math.min(activeIndex, results.length - 1)]);
    }
  };

  const showResults = open && query.trim().length > 0;

  return (
    <div ref={rootRef} className="relative w-full max-w-[760px]">
      <Search className="pointer-events-none absolute left-4 top-1/2 z-10 h-4 w-4 -translate-y-1/2 text-[var(--color-fg-faint)]" strokeWidth={1.8} aria-hidden />
      <Input
        type="search"
        value={query}
        role="combobox"
        aria-label="搜索设置项"
        aria-autocomplete="list"
        aria-controls={listboxId}
        aria-expanded={showResults}
        aria-activedescendant={showResults && results[activeIndex] ? `${listboxId}-${results[activeIndex].id}` : undefined}
        placeholder="搜索设置项，例如“开机自启”“提示音”“强调色”"
        className="pl-11 pr-11"
        autoComplete="off"
        spellCheck={false}
        onFocus={() => setOpen(true)}
        onChange={(event) => {
          setQuery(event.target.value);
          setActiveIndex(0);
          setOpen(true);
        }}
        onKeyDown={handleKeyDown}
      />
      {query && (
        <button
          type="button"
          aria-label="清空设置搜索"
          className="absolute right-2 top-1/2 grid h-[var(--control-h-sm)] w-[var(--control-h-sm)] -translate-y-1/2 place-items-center rounded-[var(--radius-md)] text-[var(--color-fg-faint)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg-muted)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]"
          onClick={() => {
            setQuery("");
            setOpen(false);
          }}
        >
          <X className="h-4 w-4" strokeWidth={1.8} aria-hidden />
        </button>
      )}

      {showResults && (
        <div
          id={listboxId}
          role="listbox"
          aria-label="设置搜索结果"
          className="absolute left-0 right-0 top-[calc(100%+0.5rem)] z-[var(--z-popover)] max-h-[min(420px,52vh)] overflow-y-auto rounded-[var(--radius-lg)] border border-[var(--color-line-strong)] bg-[var(--color-overlay)] p-1.5 shadow-[var(--shadow-popover)]"
        >
          {results.length === 0 ? (
            <p className="px-3 py-3 text-sm text-[var(--color-fg-subtle)]">没有匹配的设置项</p>
          ) : results.map((item, index) => (
            <button
              key={item.id}
              id={`${listboxId}-${item.id}`}
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              className={cn(
                "flex w-full items-center justify-between gap-4 rounded-[var(--radius-md)] px-3 py-2.5 text-left transition-colors",
                index === activeIndex
                  ? "bg-[var(--accent-soft)] text-[var(--color-fg)]"
                  : "text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]",
              )}
              onPointerMove={() => setActiveIndex(index)}
              onClick={() => choose(item)}
            >
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium">{item.label}</span>
                <span className="mt-0.5 block truncate text-xs text-[var(--color-fg-subtle)]">{item.section}</span>
              </span>
              <span className="shrink-0 rounded-[var(--radius-pill)] border border-[var(--color-line)] px-2 py-0.5 text-[11px] text-[var(--color-fg-subtle)]">{item.tabLabel}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
