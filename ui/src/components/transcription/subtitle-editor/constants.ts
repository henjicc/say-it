export const BASE_PX_PER_SEC = 60;
export const MIN_CUE_MS = 100;
export const NUDGE_MS = 100;
/** 默认间隙合并阈值：小于该间隔的相邻字幕间隙视为切分产生的碎片停顿，默认合并。 */
export const DEFAULT_GAP_MERGE_MS = 500;
export const SNAP_DISTANCE_PX = 8;
export const MAX_UNDO_HISTORY = 100;
export const RATE_OPTIONS = [0.75, 1, 1.25, 1.5];
export const TIMELINE_ZOOM_LEVELS = [0.5, 0.75, 1, 1.5, 2, 3];
export const WAVEFORM_ZOOM_LEVELS = [0.5, 0.75, 1, 1.5, 2, 3];
export const TIMELINE_HEIGHT = 118;
export const WAVEFORM_HEIGHT = 52;
export const WAVEFORM_TOP = 24;
export const WAVEFORM_PADDING = 7;
export const CUE_LANE_TOP = 82;
export const CUE_LANE_HEIGHT = 26;
export const CUE_BLOCK_TOP = 81;
export const MIN_WAVEFORM_BUCKETS = 240;
export const MAX_WAVEFORM_BUCKETS = 6000;
/** 播放头与媒体时钟漂移超过该值视为主动 seek 等真实跳变，直接硬对齐。 */
export const PLAYHEAD_HARD_SNAP_MS = 300;
/** 每帧允许用于追平漂移的速度占比：0.15 表示播放头最快以 0.85x/1.15x 的速度缓慢校准，视觉不可察觉。 */
export const PLAYHEAD_MAX_CORRECTION_RATIO = 0.15;
