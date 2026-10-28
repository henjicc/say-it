import { beforeAll, describe, expect, it, vi } from "vitest";
import builtinModels from "../../../../shared/asr-models.json";
import { loadModelCatalog, optionsForScene, ocrOptionsForScene } from "./modelRegistry";

vi.mock("@/lib/tauri", () => ({
  CMD: { getModelCatalog: "get_model_catalog" },
  cmd: vi.fn(async () => ({
    version: 1,
    defaultRealtimeModel: "fun-asr-realtime-2026-02-28",
    defaultFileModel: "fun-asr-flash-2026-06-15",
    models: [
      ...builtinModels,
      {
        id: "local-plugin-fixture",
        label: "本地插件模型",
        providerId: "local-plugin",
        category: "realtime",
        protocol: "local-sherpa-online",
        supportsVocabulary: false,
        supportsAlignmentTimestamps: false,
        emitsPartialResults: true,
        scenes: ["dictationRealtime"],
        isDefaultRealtime: false,
        isDefaultFile: false,
      },
      {
        id: "translation-fixture",
        label: "插件翻译模型",
        providerId: "plugin-translation",
        category: "translation",
        protocol: "plugin",
        supportsVocabulary: false,
        supportsAlignmentTimestamps: false,
        emitsPartialResults: false,
        scenes: ["subtitleTranslation"],
        isDefaultRealtime: false,
        isDefaultFile: false,
      },
    ],
    providers: {
      profiles: [
        { id: "local-ppocr", kind: "builtin-ppocr", displayName: "内置 PP-OCRv6 Tiny", capabilities: ["ocr"], enabled: true },
        { id: "apple-speech", kind: "builtin", displayName: "Apple", capabilities: ["asr"], enabled: true },
        { id: "volcengine", kind: "sdk", displayName: "火山引擎", capabilities: ["asr"], enabled: true },
        { id: "bailian", kind: "sdk", displayName: "阿里云百炼", capabilities: ["asr"], enabled: true },
        { id: "siliconflow", kind: "sdk", displayName: "硅基流动", capabilities: ["asr"], enabled: true },
        { id: "llm-groq", kind: "llm", displayName: "Groq", capabilities: ["asr"], enabled: true },
        { id: "local-plugin", kind: "plugin", displayName: "本地模型插件", capabilities: ["asr"], enabled: true },
      ],
    },
  })),
}));

describe("ASR model labels", () => {
  beforeAll(async () => {
    await loadModelCatalog();
  });

  it("内置 PP-OCR 只生成一个本地 OCR 模型选项", () => {
    const options = ocrOptionsForScene("activeAppContext");
    expect(options).toEqual([{
      value: "local-ppocr-v6-tiny", label: "PP-OCRv6 Tiny（内置）",
      providerId: "local-ppocr", remote: false,
    }]);
  });

  it("marks every ASR option as realtime or non-realtime", () => {
    const options = [
      ...optionsForScene("dictationRealtime"),
      ...optionsForScene("dictationFile"),
    ];
    expect(options.length).toBeGreaterThan(0);
    expect(options.every((option) => /（(?:实时|非实时)）$/u.test(option.label))).toBe(true);
    expect(options.find((option) => option.value === "apple-speech-transcriber-live")?.label)
      .toBe("Apple 系统本地识别（实时）");
    expect(options.find((option) => option.value === "seedasr-2.0-realtime")?.label)
      .toBe("火山引擎 SeedASR 2.0（实时）");
    expect(options.filter((option) => option.filterProviderId === "local").map((option) => option.value))
      .toEqual(["apple-speech-transcriber-live", "local-plugin-fixture"]);
    expect(options.filter((option) => option.filterProviderId === "local").every((option) => option.filterProviderLabel === "本地"))
      .toBe(true);
  });

  it("does not add ASR timing labels to non-ASR catalog entries", () => {
    expect(optionsForScene("subtitleTranslation")[0].label).toBe("插件翻译模型");
  });
});
