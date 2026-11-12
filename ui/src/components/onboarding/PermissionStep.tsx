import { useEffect, useState } from "react";
import { CheckCircle2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Meter } from "@/components/ui/Meter";
import { micLevelFraction, useMicLevel } from "@/features/audio/useMicLevel";
import { PRIVACY_ROWS, useSetupPermissions } from "@/features/onboarding/permissions";
import { CMD, cmd } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { MicRow } from "@/views/dictation/DictationSections";
import { StepBody, StepError, StepFooter } from "./StepLayout";

function ReadyMark({ label }: { label: string }) {
  return <CheckCircle2 role="img" className="h-4 w-4 flex-none text-[var(--color-ok)]" aria-label={label} />;
}

/**
 * 麦克风被系统拒绝（macOS 授权、Windows 隐私设置）后只能去系统设置里打开。Windows 上被拒时采集
 * 照常启动、只是录到静音，所以不能只在启动出错时才查；从系统设置切回来时也要重查。
 */
function useMicPermissionDenied(startError: string) {
  const [denied, setDenied] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    setError("");
    let active = true;
    const query = () => {
      void cmd<boolean>(CMD.microphonePermissionDenied)
        .then((value) => active && setDenied(value))
        .catch((reason) => console.error("读取麦克风权限状态失败", reason));
    };
    query();
    window.addEventListener("focus", query);
    return () => {
      active = false;
      window.removeEventListener("focus", query);
    };
  }, [startError]);

  const openSettings = () => {
    setError("");
    void cmd(CMD.openPrivacySettings, { kind: "microphone" }).catch((reason) => setError(String(reason)));
  };
  return { denied, error, openSettings };
}

/**
 * 权限只做检测与提示，不拦下一步：没有麦克风、被别的软件占用、读不到权限状态都不是用户
 * 在这里能解决的，而首次引导走不完整个软件就打不开。
 */
export function PermissionStep({ onNext }: { onNext: () => void }) {
  const micDeviceId = useDictPrefs((state) => state.prefs.micDeviceId);
  const { level, startError } = useMicLevel(micDeviceId);
  const permissions = useSetupPermissions();
  const mic = useMicPermissionDenied(startError);

  return (
    <>
      <StepBody label="权限">
        <Group>
          <MicRow />
          <Row
            label="说句话试试"
            message={(mic.denied || startError) && (
              <span role="alert" className="text-[var(--color-err)]">
                {mic.denied ? `麦克风权限已被拒绝${mic.error && `：${mic.error}`}` : `无法使用麦克风：${startError}`}
              </span>
            )}
          >
            {mic.denied ? (
              <Button size="sm" onClick={mic.openSettings}>打开系统设置</Button>
            ) : (
              <div className={`${ROW_CONTROL_W} flex items-center gap-2`}>
                <Meter value={micLevelFraction(level)} label="麦克风电平" kind="level" className="flex-1" />
                {level > 0 && !startError && <ReadyMark label="麦克风可以使用" />}
              </div>
            )}
          </Row>
        </Group>
        {permissions.needed && (
          <Group>
            <Row
              label="文字输入"
              hint="需要辅助功能权限，才能把识别出的文字输入其他软件。"
              message={permissions.errors.permissions && <StepError>{permissions.errors.permissions}</StepError>}
            >
              {permissions.textInputReady ? (
                <ReadyMark label="已授权" />
              ) : (
                <Button size="sm" disabled={permissions.requesting} onClick={() => void permissions.requestTextInput()}>授予权限</Button>
              )}
            </Row>
            {PRIVACY_ROWS.map((row) => {
              const check = permissions.check(row.id);
              if (!check) return null;
              const ready = check.status === "ready";
              const error = permissions.errors[row.id];
              return (
                <Row
                  key={row.id}
                  label={row.label}
                  hint={row.hint}
                  message={error ? <StepError>{error}</StepError> : !ready && row.pending}
                >
                  {ready ? (
                    <ReadyMark label="已授权" />
                  ) : (
                    <Button size="sm" onClick={() => permissions.openSettings(row.id, row.kind)}>打开系统设置</Button>
                  )}
                </Row>
              );
            })}
          </Group>
        )}
      </StepBody>
      <StepFooter label="下一步" onNext={onNext} />
    </>
  );
}
