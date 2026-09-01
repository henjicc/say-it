import { Button } from "@/components/ui/Button";
import { ResultCard } from "@/components/compare/ResultCard";
import { mergedModelOptions } from "@/features/compare/models";
import { ModelPicker } from "@/features/models/ModelPicker";
import { COMPARE_COLS, COMPARE_MAX_ROWS, COMPARE_MIN_ROWS, useCompareStore } from "@/store/useCompareStore";

export function ModelGrid() {
  const cellModels = useCompareStore((s) => s.prefs.cellModels);
  const setCellModel = useCompareStore((s) => s.setCellModel);
  const addRow = useCompareStore((s) => s.addRow);
  const removeRow = useCompareStore((s) => s.removeRow);
  const cellRuntime = useCompareStore((s) => s.cellRuntime);
  const phase = useCompareStore((s) => s.phase);
  const disabled = phase !== "idle";
  const rows = cellModels.length / COMPARE_COLS;
  const options = [
    { value: "", label: "不选择模型", providerId: "none", providerLabel: "通用" },
    ...mergedModelOptions(),
  ];

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3">
        {cellModels.map((value, index) => (
          <div key={index} className="flex flex-col gap-2">
            <ModelPicker
              value={value}
              options={options}
              disabled={disabled}
              aria-label={`对比模型 ${index + 1}`}
              panelLabel="选择语音识别模型"
              onChange={(nextValue) => setCellModel(index, nextValue)}
            />
            {value && <ResultCard runtime={cellRuntime[index]} />}
          </div>
        ))}
      </div>
      {!disabled && (
        <div className="flex gap-2">
          <Button size="sm" onClick={addRow} disabled={rows >= COMPARE_MAX_ROWS}>
            + 增加一行
          </Button>
          <Button size="sm" onClick={removeRow} disabled={rows <= COMPARE_MIN_ROWS}>
            - 删除一行
          </Button>
        </div>
      )}
    </div>
  );
}
