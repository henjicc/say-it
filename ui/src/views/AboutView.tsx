import { useEffect, useRef, useState } from "react";
import { getName, getVersion } from "@tauri-apps/api/app";
import { Group, Row } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { ClearIcon } from "@/components/ui/icons";
import { Modal } from "@/components/ui/Modal";
import { Pill } from "@/components/ui/Pill";
import { Switch } from "@/components/ui/Switch";
import { cmd, CMD } from "@/lib/tauri";
import { useDeveloperStore } from "@/store/useDeveloperStore";
import appIcon from "../../../src-tauri/icons/source/icon.svg";

const AUTHOR_NAME = "痕继痕迹";
const AUTHOR_HOME = "https://space.bilibili.com/39337803";
/** 连点图标这么多次打开开发者模式；从第 3 次起提示还差几次。 */
const DEVELOPER_TAPS = 7;
const DEVELOPER_HINT_FROM = 3;
/** 两次点击间隔超过这个时间就重新计数。 */
const TAP_RESET_MS = 1500;

export function AboutDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [appName, setAppName] = useState("说吧！");
  const [appVersion, setAppVersion] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const developer = useDeveloperStore((s) => s.enabled);
  const setDeveloper = useDeveloperStore((s) => s.setEnabled);
  const taps = useRef({ count: 0, at: 0 });
  const iconRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    taps.current = { count: 0, at: 0 };
    setMessage("");
    setError("");
    getName().then(setAppName).catch(() => {});
    getVersion().then(setAppVersion).catch(() => setAppVersion("未知版本"));
  }, [open]);

  const tapIcon = () => {
    if (developer) return;
    const now = Date.now();
    const count = now - taps.current.at > TAP_RESET_MS ? 1 : taps.current.count + 1;
    taps.current = { count, at: now };
    if (count < DEVELOPER_HINT_FROM) return;
    if (count < DEVELOPER_TAPS) {
      setMessage(`再点 ${DEVELOPER_TAPS - count} 次开启开发者模式`);
      return;
    }
    taps.current = { count: 0, at: 0 };
    setError("");
    setDeveloper(true)
      .then(() => setMessage("已开启开发者模式，侧栏多了「开发者」页"))
      .catch((reason) => {
        setMessage("");
        setError(`开启开发者模式失败：${String(reason)}`);
      });
  };

  const turnOffDeveloper = () => {
    setError("");
    setDeveloper(false)
      .then(() => {
        setMessage("已关闭开发者模式");
        // 开关所在的行随之消失，焦点交给图标，键盘用户不会落到页面最外层。
        iconRef.current?.focus();
      })
      .catch((reason) => setError(`关闭开发者模式失败：${String(reason)}`));
  };

  const openAuthorHome = () => {
    setError("");
    cmd(CMD.openExternalLink, { url: AUTHOR_HOME }).catch((reason) => {
      setError(`打开主页失败：${String(reason)}`);
    });
  };

  const prerelease = appVersion.includes("-");

  return (
    <Modal
      open={open}
      onClose={onClose}
      scope="container"
      ariaLabel="关于说吧"
      className="max-w-[340px]"
    >
      <div className="relative flex flex-col items-center px-6 pt-7 pb-6">
        <IconButton size="sm" variant="ghost" label="关闭" onClick={onClose} className="absolute top-3 right-3">
          <ClearIcon />
        </IconButton>
        {/* 图标是可点的：连点打开开发者模式。源图（1024）四周各留 100 的透明边，按 1024/824 放大，
            让圆角块正好铺满按钮；只用变换放大，不裁切，保留图标自身的圆角形状。 */}
        <button
          ref={iconRef}
          type="button"
          onClick={tapIcon}
          aria-label={`${appName} 图标`}
          className="block h-20 w-20 rounded-[18px] p-0 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)] focus-visible:ring-offset-4 focus-visible:ring-offset-[var(--color-overlay)]"
        >
          <img src={appIcon} alt="" draggable={false} className="pointer-events-none block h-20 w-20 max-w-none scale-[1.243] select-none" />
        </button>

        <h2 className="mt-4 text-xl font-semibold text-[var(--color-fg)]">{appName}</h2>
        <p className="mt-1 flex items-center gap-2 text-[13px] text-[var(--color-fg-muted)]">
          <span className="tabular-nums">{appVersion ? `版本 ${appVersion}` : " "}</span>
          {prerelease && (
            <Pill className="px-2 py-px text-[11px]">测试版</Pill>
          )}
        </p>
        {error ? (
          <p role="alert" className="mt-2 min-h-[18px] text-center text-xs text-[var(--color-err)]">{error}</p>
        ) : (
          <p role="status" aria-live="polite" className="mt-2 min-h-[18px] text-xs text-[var(--color-fg-subtle)]">
            {message}
          </p>
        )}

        <Group className="mt-3 w-full">
          <Row label="作者">
            <button
              type="button"
              onClick={openAuthorHome}
              className="-mr-2 rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-accent-text)] hover:bg-[var(--color-surface-hover)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]"
            >
              {AUTHOR_NAME}
            </button>
          </Row>
          {developer && !import.meta.env.DEV && (
            <Row label="开发者模式" controlId="about-developer-mode">
              <Switch id="about-developer-mode" checked onChange={turnOffDeveloper} label="开发者模式" />
            </Row>
          )}
        </Group>
      </div>
    </Modal>
  );
}
