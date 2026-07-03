import { create } from "zustand";
import type { AlignedResultCue } from "@/features/transcription/subtitles";

export type TranscriptionTab = "transcribe" | "align";
export type TranscriptionStage = "idle" | "uploading" | "recognizing" | "completed" | "error";
export type AlignStage = TranscriptionStage | "aligning";
export type TranscriptionResultView = "text" | "subtitles";

export interface SelectedTranscriptionFile {
  path: string;
  name: string;
  size: number;
}

export interface TranscriptionParams {
  model: string;
  vocabularyId: string;
  languageHints: string[];
  diarizationEnabled: boolean;
  speakerCount: number | null;
}

export interface TranscriptionWord {
  beginTime: number;
  endTime: number;
  text: string;
  punctuation?: string | null;
}

export interface TranscriptionSentence {
  beginTime: number;
  endTime: number;
  text: string;
  sentenceId?: unknown;
  speakerId?: unknown;
  words: TranscriptionWord[];
}

export interface TranscriptionTranscript {
  channelId?: unknown;
  text: string;
  sentences: TranscriptionSentence[];
}

export interface TranscriptionResult {
  durationMs?: number | null;
  transcripts: TranscriptionTranscript[];
}

export interface AlignedLine {
  lineIndex: number;
  text: string;
  beginMs: number;
  endMs: number;
  matchRatio: number;
  interpolated: boolean;
}

/**
 * “识别修正”结果的一个片段：保留文稿某一行的（部分）原文，或一段未被文稿
 * 认领的音频（按词范围给出，前端负责用现有的字幕切分逻辑生成实际文本/时间）。
 */
export type OptimizedSegment =
  | {
      source: "script";
      lineIndex: number;
      text: string;
      beginMs: number;
      endMs: number;
      matchRatio: number;
    }
  | {
      source: "asr";
      wordBegin: number;
      wordEnd: number;
    };

export interface AlignOutput {
  lines: AlignedLine[];
  optimizedSegments: OptimizedSegment[];
}

export type AlignResultView = "script" | "optimized";

export interface AlignRecognitionCache {
  filePath: string;
  paramsKey: string;
  result: TranscriptionResult;
}

export interface TranscriptionEventPayload {
  jobId?: string;
  stage?: string;
  filePath?: string;
  model?: string;
  taskId?: string;
  fileUrl?: string;
  pollCount?: number;
  taskStatus?: string;
  result?: TranscriptionResult;
  message?: string;
  cancelled?: boolean;
}

export const DEFAULT_TRANSCRIPTION_PARAMS: TranscriptionParams = {
  model: "fun-asr",
  vocabularyId: "",
  languageHints: [],
  diarizationEnabled: false,
  speakerCount: null,
};

interface TranscriptionState {
  tab: TranscriptionTab;
  selectedFile: SelectedTranscriptionFile | null;
  params: TranscriptionParams;
  stage: TranscriptionStage;
  resultView: TranscriptionResultView;
  jobId: string;
  taskId: string;
  statusText: string;
  errorMessage: string;
  result: TranscriptionResult | null;
  saveMessage: string;
  alignFile: SelectedTranscriptionFile | null;
  scriptText: string;
  alignStage: AlignStage;
  alignJobId: string;
  alignStatusText: string;
  alignErrorMessage: string;
  alignedLines: AlignedLine[] | null;
  alignOptimizedCues: AlignedResultCue[] | null;
  alignResultView: AlignResultView;
  alignSaveMessage: string;
  alignRecognition: AlignRecognitionCache | null;
  setTab: (tab: TranscriptionTab) => void;
  setSelectedFile: (file: SelectedTranscriptionFile | null) => void;
  setAlignFile: (file: SelectedTranscriptionFile | null) => void;
  setScriptText: (text: string) => void;
  setParams: (params: Partial<TranscriptionParams>) => void;
  replaceParams: (params: TranscriptionParams) => void;
  setRuntime: (patch: Partial<Omit<TranscriptionState, "setTab" | "setSelectedFile" | "setAlignFile" | "setScriptText" | "setParams" | "replaceParams" | "setRuntime" | "resetRuntime">>) => void;
  resetRuntime: () => void;
}

export const useTranscriptionStore = create<TranscriptionState>((set) => ({
  tab: "transcribe",
  selectedFile: null,
  params: DEFAULT_TRANSCRIPTION_PARAMS,
  stage: "idle",
  resultView: "text",
  jobId: "",
  taskId: "",
  statusText: "",
  errorMessage: "",
  result: null,
  saveMessage: "",
  alignFile: null,
  scriptText: "",
  alignStage: "idle",
  alignJobId: "",
  alignStatusText: "",
  alignErrorMessage: "",
  alignedLines: null,
  alignOptimizedCues: null,
  alignResultView: "script",
  alignSaveMessage: "",
  alignRecognition: null,
  setTab: (tab) => set({ tab }),
  setSelectedFile: (selectedFile) => set({ selectedFile }),
  setAlignFile: (alignFile) => set({ alignFile }),
  setScriptText: (scriptText) => set({ scriptText }),
  setParams: (params) => set((state) => ({ params: { ...state.params, ...params } })),
  replaceParams: (params) => set({ params }),
  setRuntime: (patch) => set(patch),
  resetRuntime: () =>
    set({
      stage: "idle",
      jobId: "",
      taskId: "",
      statusText: "",
      errorMessage: "",
      result: null,
      saveMessage: "",
    }),
}));
