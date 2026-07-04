import { Button } from "@/components/ui/Button";
import { Select } from "@/components/ui/Input";
import { ResultCard } from "@/components/compare/ResultCard";
import { mergedModelOptions, type CompareModelKind } from "@/features/compare/models";
import { COMPARE_COLS, COMPARE_MAX_ROWS, COMPARE_MIN_ROWS, useCompareStore } from "@/store/useCompareStore";

const KIND_LABEL: Record<CompareModelKind, string> = { realtime: "实时", file: "文件" };

export function ModelGrid() {
  const cellModels = useCompareStore((s) => s.prefs.cellModels);
  const setCellModel = useCompareStore((s) => s.setCellModel);
  const addRow = useCompareStore((s) => s.addRow);
  const removeRow = useCompareStore((s) => s.removeRow);
  const cellRuntime = useCompareStore((s) => s.cellRuntime);
  const phase = useCompareStore((s) => s.phase);
  const disabled = phase !== "idle";
  const rows = cellModels.length / COMPARE_COLS;
  const options = mergedModelOptions();

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3">
        {cellModels.map((value, index) => (
          <div key={index} className="flex flex-col gap-2">
            <Select value={value} disabled={disabled} onChange={(e) => setCellModel(index, e.target.value)}>
              <option value="">（未选择模型）</option>
              {options.map((option) => (
                <option key={option.value} value={option.value}>
                  {`${option.label}（${KIND_LABEL[option.kind]}）`}
                </option>
              ))}
            </Select>
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
