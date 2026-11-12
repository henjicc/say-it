import { describe, expect, it } from "vitest";
import { speakerText } from "@/features/transcription/subtitles";
import type { TranscriptionResult } from "@/store/useTranscriptionStore";

const sentence = (text: string, speakerId?: number) => ({ beginTime: 0, endTime: 0, text, speakerId, words: [] });

describe("speakerText", () => {
  it("合并同一说话人连续的句子并标出说话人", () => {
    const result: TranscriptionResult = {
      transcripts: [{ text: "", sentences: [sentence("先说预算。", 0), sentence("下周定。", 0), sentence("好的。", 1)] }],
    };
    expect(speakerText(result)).toBe("说话人 0：先说预算。下周定。\n\n说话人 1：好的。");
  });

  it("没有说话人信息时与纯文本一致", () => {
    const result: TranscriptionResult = { transcripts: [{ text: "全文", sentences: [sentence("全文")] }] };
    expect(speakerText(result)).toBe("全文");
  });
});
