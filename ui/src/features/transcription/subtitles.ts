import type {
  AlignedLine,
  OptimizedSegment,
  TranscriptionResult,
  TranscriptionSentence,
  TranscriptionWord,
} from "@/store/useTranscriptionStore";

export interface SubtitleCue {
  index: number;
  beginMs: number;
  endMs: number;
  text: string;
  speakerId?: string;
}

interface BuildCueOptions {
  maxWidth?: number;
  mergeGapMs?: number;
}

const DEFAULT_MAX_WIDTH = 30;
const DEFAULT_MERGE_GAP_MS = 500;
const MIN_CUE_MS = 300;
const PUNCTUATION_RE = /[，。！？；：、,.!?;:]$/;

function textWidth(text: string) {
  let width = 0;
  let asciiRun = "";
  const flushAscii = () => {
    if (!asciiRun) return;
    width += 2;
    asciiRun = "";
  };
  for (const char of text) {
    if (/\s/.test(char)) {
      flushAscii();
      continue;
    }
    if (/[\u3400-\u9fff]/.test(char)) {
      flushAscii();
      width += 1;
    } else if (/[a-zA-Z0-9]/.test(char)) {
      asciiRun += char;
    } else {
      flushAscii();
      width += 0.5;
    }
  }
  flushAscii();
  return width;
}

function speakerIdOf(sentence: TranscriptionSentence) {
  if (sentence.speakerId === null || sentence.speakerId === undefined) return undefined;
  return String(sentence.speakerId);
}

function wordText(word: TranscriptionWord) {
  return `${word.text || ""}${word.punctuation || ""}`.trim();
}

function joinWordTexts(words: TranscriptionWord[]) {
  return words.map(wordText).filter(Boolean).reduce((acc, text) => {
    if (!acc) return text;
    return /[a-zA-Z0-9]$/.test(acc) && /^[a-zA-Z0-9]/.test(text) ? `${acc} ${text}` : `${acc}${text}`;
  }, "");
}

function sentenceCue(sentence: TranscriptionSentence): Omit<SubtitleCue, "index"> | null {
  const text = sentence.text.trim();
  if (!text) return null;
  return {
    beginMs: Math.max(0, Number(sentence.beginTime) || 0),
    endMs: Math.max(0, Number(sentence.endTime) || 0),
    text,
    speakerId: speakerIdOf(sentence),
  };
}

function cueFromWords(
  words: TranscriptionWord[],
  beginIndex: number,
  endIndex: number,
  speakerId?: string,
): Omit<SubtitleCue, "index"> | null {
  const chunk = words.slice(beginIndex, endIndex + 1);
  const text = joinWordTexts(chunk).trim();
  if (!text) return null;
  const first = chunk[0];
  const last = chunk[chunk.length - 1];
  return {
    beginMs: Math.max(0, Number(first?.beginTime) || 0),
    endMs: Math.max(0, Number(last?.endTime) || 0),
    text,
    speakerId,
  };
}

function splitWords(
  words: TranscriptionWord[],
  beginIndex: number,
  endIndex: number,
  maxWidth: number,
  speakerId?: string,
): Omit<SubtitleCue, "index">[] {
  if (beginIndex > endIndex) return [];
  const text = joinWordTexts(words.slice(beginIndex, endIndex + 1));
  if (textWidth(text) <= maxWidth || beginIndex === endIndex) {
    const cue = cueFromWords(words, beginIndex, endIndex, speakerId);
    return cue ? [cue] : [];
  }

  const target = maxWidth;
  let bestPunctuation = -1;
  let bestPunctuationDistance = Number.POSITIVE_INFINITY;
  for (let i = beginIndex; i < endIndex; i += 1) {
    const leftText = joinWordTexts(words.slice(beginIndex, i + 1));
    const width = textWidth(leftText);
    if (width > maxWidth) break;
    if (!PUNCTUATION_RE.test(wordText(words[i]))) continue;
    const distance = Math.abs(width - target);
    if (distance < bestPunctuationDistance) {
      bestPunctuation = i;
      bestPunctuationDistance = distance;
    }
  }

  let splitIndex = bestPunctuation;
  if (splitIndex < 0) {
    let bestGap = -1;
    for (let i = beginIndex; i < endIndex; i += 1) {
      const leftText = joinWordTexts(words.slice(beginIndex, i + 1));
      if (textWidth(leftText) > maxWidth) break;
      const gap = Math.max(0, (Number(words[i + 1]?.beginTime) || 0) - (Number(words[i]?.endTime) || 0));
      if (gap >= bestGap) {
        bestGap = gap;
        splitIndex = i;
      }
    }
  }

  if (splitIndex < beginIndex) {
    splitIndex = beginIndex;
    for (let i = beginIndex; i < endIndex; i += 1) {
      const leftText = joinWordTexts(words.slice(beginIndex, i + 1));
      if (textWidth(leftText) > maxWidth) break;
      splitIndex = i;
    }
  }

  return [
    ...splitWords(words, beginIndex, splitIndex, maxWidth, speakerId),
    ...splitWords(words, splitIndex + 1, endIndex, maxWidth, speakerId),
  ];
}

function cuesFromSentence(sentence: TranscriptionSentence, maxWidth: number) {
  const base = sentenceCue(sentence);
  if (!base) return [];
  if (textWidth(base.text) <= maxWidth) return [base];
  const words = sentence.words.filter((word) => word.text && word.beginTime !== undefined && word.endTime !== undefined);
  if (words.length === 0) return [base];
  return splitWords(words, 0, words.length - 1, maxWidth, base.speakerId);
}

function canMerge(a: Omit<SubtitleCue, "index">, b: Omit<SubtitleCue, "index">, maxWidth: number, mergeGapMs: number) {
  if (a.speakerId !== b.speakerId) return false;
  if (b.beginMs - a.endMs > mergeGapMs) return false;
  return textWidth(`${a.text}${b.text}`) <= maxWidth;
}

function normalizeTimeline<T extends Omit<SubtitleCue, "index">>(cues: T[]) {
  const sorted = cues
    .filter((cue) => cue.text.trim())
    .sort((a, b) => a.beginMs - b.beginMs || a.endMs - b.endMs);
  const normalized: (T & { index: number })[] = [];
  for (const cue of sorted) {
    const previous = normalized[normalized.length - 1];
    let beginMs = Math.max(0, Math.round(cue.beginMs));
    let endMs = Math.max(0, Math.round(cue.endMs));
    if (previous && beginMs < previous.endMs) beginMs = previous.endMs;
    if (endMs <= beginMs) endMs = beginMs + MIN_CUE_MS;
    normalized.push({
      ...cue,
      index: normalized.length + 1,
      beginMs,
      endMs,
      text: cue.text.trim(),
    });
  }
  return normalized;
}

export function buildCues(result: TranscriptionResult | null, options: BuildCueOptions = {}) {
  const transcript = result?.transcripts?.[0];
  if (!transcript) return [];
  const maxWidth = options.maxWidth || DEFAULT_MAX_WIDTH;
  const mergeGapMs = options.mergeGapMs || DEFAULT_MERGE_GAP_MS;
  const raw = transcript.sentences.flatMap((sentence) => cuesFromSentence(sentence, maxWidth));

  const merged: Omit<SubtitleCue, "index">[] = [];
  for (const cue of raw) {
    const previous = merged[merged.length - 1];
    if (previous && canMerge(previous, cue, maxWidth, mergeGapMs)) {
      previous.endMs = Math.max(previous.endMs, cue.endMs);
      previous.text = `${previous.text}${cue.text}`;
    } else {
      merged.push({ ...cue });
    }
  }
  return normalizeTimeline(merged);
}

export function cuesFromAlignedLines(lines: AlignedLine[]): SubtitleCue[] {
  return lines
    .filter((line) => line.text.trim())
    .map((line, index) => ({
      index: index + 1,
      beginMs: line.beginMs,
      endMs: line.endMs,
      text: line.text,
    }));
}

/** 「识别修正」结果的字幕条目：来自文稿行原文，或未被文稿认领的识别文本。 */
export interface AlignedResultCue extends SubtitleCue {
  source: "script" | "asr";
  lineIndex?: number;
  matchRatio?: number;
}

/**
 * 把后端算好的片段（保留原文的文稿段 + 只给词范围的识别插入段）渲染成字幕：
 * 文稿段直接使用；识别段复用 splitWords 按词范围生成实际文本与时间
 * （与「录音转写」的句级切分同一套逻辑，保证长段落也能正常拆行）。
 */
export function cuesFromOptimizedSegments(
  segments: OptimizedSegment[],
  words: TranscriptionWord[],
): AlignedResultCue[] {
  const raw: Omit<AlignedResultCue, "index">[] = [];
  for (const segment of segments) {
    if (segment.source === "script") {
      raw.push({
        beginMs: segment.beginMs,
        endMs: segment.endMs,
        text: segment.text,
        source: "script",
        lineIndex: segment.lineIndex,
        matchRatio: segment.matchRatio,
      });
      continue;
    }
    for (const cue of splitWords(words, segment.wordBegin, segment.wordEnd, DEFAULT_MAX_WIDTH)) {
      raw.push({ ...cue, source: "asr" });
    }
  }
  return normalizeTimeline(raw);
}

export function formatSrtTime(ms: number) {
  const value = Math.max(0, Math.round(ms));
  const hours = Math.floor(value / 3_600_000);
  const minutes = Math.floor((value % 3_600_000) / 60_000);
  const seconds = Math.floor((value % 60_000) / 1000);
  const milliseconds = value % 1000;
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")},${String(milliseconds).padStart(3, "0")}`;
}

export function cueText(cue: SubtitleCue) {
  return cue.speakerId ? `说话人 ${cue.speakerId}：${cue.text}` : cue.text;
}

export function toSrt(cues: SubtitleCue[]) {
  return `${cues
    .map((cue, index) => [
      String(index + 1),
      `${formatSrtTime(cue.beginMs)} --> ${formatSrtTime(cue.endMs)}`,
      cueText(cue),
    ].join("\r\n"))
    .join("\r\n\r\n")}\r\n`;
}

export function plainText(result: TranscriptionResult | null) {
  return (result?.transcripts || [])
    .map((transcript) => transcript.text || transcript.sentences.map((sentence) => sentence.text).join(""))
    .filter(Boolean)
    .join("\n\n");
}
