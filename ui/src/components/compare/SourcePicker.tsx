import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { FileCard, useFileDrop, useFilePick } from "@/features/transcription/filePicker";
import { useCompareStore, type CompareSourceMode } from "@/store/useCompareStore";

const SOURCE_OPTIONS: { value: CompareSourceMode; label: string }[] = [
  { value: "record", label: "录音" },
  { value: "upload", label: "上传音频文件" },
];

export function SourcePicker() {
  const sourceMode = useCompareStore((s) => s.prefs.sourceMode);
  const patch = useCompareStore((s) => s.patch);
  const phase = useCompareStore((s) => s.phase);
  const selectedFile = useCompareStore((s) => s.selectedFile);
  const setSelectedFile = useCompareStore((s) => s.setSelectedFile);
  const disabled = phase !== "idle";

  const { pickState, message, loadFileInfo, pickFile } = useFilePick(setSelectedFile);
  const dragActive = useFileDrop(loadFileInfo, sourceMode === "upload" && !disabled);

  return (
    <div className="flex flex-col gap-3">
      <div className="inline-flex w-fit gap-1 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] p-1">
        {SOURCE_OPTIONS.map((option) => (
          <Button
            key={option.value}
            type="button"
            size="sm"
            variant={sourceMode === option.value ? "primary" : "ghost"}
            disabled={disabled}
            className={cn(sourceMode !== option.value && "border-transparent bg-transparent")}
            onClick={() => patch({ sourceMode: option.value })}
          >
            {option.label}
          </Button>
        ))}
      </div>

      {sourceMode === "upload" && (
        <FileCard
          file={selectedFile}
          dragActive={dragActive}
          disabled={disabled}
          pickState={pickState}
          message={message}
          onPick={pickFile}
          statusTone="idle"
          statusText={selectedFile ? "已选择文件" : ""}
        />
      )}
    </div>
  );
}
