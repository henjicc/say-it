import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { adoptTranscriptionJob, normalizeParams } from "@/features/transcription/controller";
import { useFilePick } from "@/features/transcription/filePicker";
import { CMD, cmd, type MeetingRecordingFile, type MeetingStatus } from "@/lib/tauri";
import { useTranscriptionStore } from "@/store/useTranscriptionStore";

function formatElapsed(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = String(Math.floor((total % 3600) / 60)).padStart(2, "0");
  const seconds = String(total % 60).padStart(2, "0");
  return hours ? `${hours}:${minutes}:${seconds}` : `${minutes}:${seconds}`;
}

/**
 * 同时录麦克风和系统声音；停止后录音交给「文件」识别。
 * 简单模式没有转写的子导航，不显示跳到「识别」设置的那一行。
 */
export function MeetingPanel({ simple = false }: { simple?: boolean } = {}) {
  const diarizationEnabled = useTranscriptionStore((s) => s.params.diarizationEnabled);
  const setTab = useTranscriptionStore((s) => s.setTab);
  const setSelectedFile = useTranscriptionStore((s) => s.setSelectedFile);
  const [status, setStatus] = useState<MeetingStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  /** 不是错误的提示，例如录音已保存但没有自动识别的原因。 */
  const [notice, setNotice] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const { message: fileMessage, loadFileInfo } = useFilePick(setSelectedFile);

  const refresh = () => cmd<MeetingStatus>(CMD.getMeetingStatus).then(setStatus);
  useEffect(() => {
    refresh().catch((reason: unknown) => setMessage(String(reason)));
  }, []);

  // 录制中每秒刷新计时，同时拉一次状态，一路断开时及时提示。
  const recording = status?.recording ?? false;
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => {
      setNow(Date.now());
      refresh().catch(() => {});
    }, 1000);
    return () => window.clearInterval(timer);
  }, [recording]);

  const start = async () => {
    setBusy(true);
    setMessage("");
    setNotice("");
    try {
      setStatus(await cmd<MeetingStatus>(CMD.meetingStart));
      setNow(Date.now());
    } catch (reason) {
      setMessage(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    setMessage("");
    setNotice("");
    try {
      // 识别由后端在保存录音后直接开始，停止后马上关掉主窗口也不会落下。
      const file = await cmd<MeetingRecordingFile>(CMD.meetingStop, {
        params: normalizeParams(useTranscriptionStore.getState().params),
      });
      setStatus({ recording: false, startedAtMs: null, warning: null });
      if (!file.jobId) {
        // 已有识别在进行（不打断它、也不换掉它的文件）或没能开始识别。
        setNotice(`${file.warning ? `${file.warning}。` : ""}录音已保存：${file.path}。可以稍后在「文件」里选择它识别。`);
        return;
      }
      if (file.warning) setNotice(`${file.warning}。`);
      await loadFileInfo(file.path);
      await adoptTranscriptionJob(file.jobId);
      if (!file.warning) setTab("transcribe");
    } catch (reason) {
      setMessage(String(reason));
      await refresh().catch(() => {});
    } finally {
      setBusy(false);
    }
  };

  const error = message || fileMessage;
  const info = notice || status?.warning || "";
  const rowMessage = error
    ? <span role="alert" className="text-[var(--color-err)]">{error}</span>
    : info
      ? <span role="status" className="text-[var(--color-fg-muted)]">{info}</span>
      : undefined;
  return (
    <Group>
      <Row
        label="录制会议"
        hint="同时录下麦克风和电脑播放的声音，停止后自动识别，可以再生成纪要。用扬声器外放时麦克风会把电脑声音再录一遍，建议戴耳机。"
        message={rowMessage}
      >
        {recording && status?.startedAtMs && (
          <span role="timer" aria-label="已录制时长" className="font-mono text-[13px] tabular-nums text-[var(--color-fg-muted)]">
            {formatElapsed(now - status.startedAtMs)}
          </span>
        )}
        {recording ? (
          <Button size="sm" variant="danger" disabled={busy} onClick={() => void stop()}>停止录制</Button>
        ) : (
          <Button size="sm" variant="primary" disabled={busy || !status} onClick={() => void start()}>开始录制</Button>
        )}
      </Row>
      {!simple && (
        <Row label="区分说话人">
          <Button size="sm" variant="text" onClick={() => setTab("settings")}>{diarizationEnabled ? "已开启" : "未开启"}</Button>
        </Row>
      )}
      <Row label="会议录音">
        <Button size="sm" variant="text" onClick={() => void cmd(CMD.openMeetingRecordings).catch((reason: unknown) => setMessage(String(reason)))}>打开文件夹</Button>
      </Row>
    </Group>
  );
}
