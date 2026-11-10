import { CheckCircle2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Meter } from "@/components/ui/Meter";
import { micLevelFraction, useMicLevel } from "@/features/audio/useMicLevel";
import { useTextInputPermission } from "@/features/onboarding/permissions";
import { useDictPrefs } from "@/store/useDictPrefs";
import { MicRow } from "@/views/dictation/DictationSections";
import { StepBody, StepError, StepFooter } from "./StepLayout";

function ReadyMark({ label }: { label: string }) {
  return <CheckCircle2 role="img" className="h-4 w-4 flex-none text-[var(--color-ok)]" aria-label={label} />;
}

/**
 * 权限只做检测与提示，不拦下一步：没有麦克风、被别的软件占用、读不到权限状态都不是用户
 * 在这里能解决的，而首次引导走不完整个软件就打不开。
 */
export function PermissionStep({ onNext }: { onNext: () => void }) {
  const micDeviceId = useDictPrefs((state) => state.prefs.micDeviceId);
  const { level, startError } = useMicLevel(micDeviceId);
  const textInput = useTextInputPermission();

  return (
    <>
      <StepBody label="权限">
        <Group>
          <MicRow />
          <Row
            label="说句话试试"
            message={startError && <span role="alert" className="text-[var(--color-err)]">无法使用麦克风：{startError}</span>}
          >
            <div className={`${ROW_CONTROL_W} flex items-center gap-2`}>
              <Meter value={micLevelFraction(level)} label="麦克风电平" kind="level" className="flex-1" />
              {level > 0 && !startError && <ReadyMark label="麦克风可以使用" />}
            </div>
          </Row>
        </Group>
        {textInput.needed && (
          <Group>
            <Row
              label="文字输入"
              hint="需要辅助功能权限，才能把识别出的文字输入其他软件。"
              message={textInput.error && <StepError>{textInput.error}</StepError>}
            >
              {textInput.ready ? (
                <ReadyMark label="已授权" />
              ) : (
                <Button size="sm" disabled={textInput.requesting} onClick={() => void textInput.request()}>授予权限</Button>
              )}
            </Row>
          </Group>
        )}
      </StepBody>
      <StepFooter label="下一步" onNext={onNext} />
    </>
  );
}
