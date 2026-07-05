import { useEffect, useState } from "react";
import { getName, getVersion } from "@tauri-apps/api/app";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { cmd, CMD } from "@/lib/tauri";
import appIcon from "../../../src-tauri/icons/icon.png";

const AUTHOR_NAME = "痕继痕迹";
const AUTHOR_HOME = "https://space.bilibili.com/39337803";

export function AboutDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [appName, setAppName] = useState("说吧！");
  const [appVersion, setAppVersion] = useState("读取中...");
  const [openError, setOpenError] = useState("");

  useEffect(() => {
    if (!open) return;
    getName().then(setAppName).catch(() => {});
    getVersion().then(setAppVersion).catch(() => setAppVersion("未知版本"));
  }, [open]);

  const openAuthorHome = () => {
    setOpenError("");
    cmd(CMD.openExternalLink, { url: AUTHOR_HOME }).catch((error) => {
      setOpenError(String(error));
    });
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      scope="container"
      ariaLabel="关于说吧"
      className="max-w-[460px] rounded-[var(--radius-xl)]"
    >
      <div className="p-5">
        <div className="flex items-center gap-4">
          <img src={appIcon} alt={appName} className="h-20 w-20 rounded-[var(--radius-xl)]" />

          <div className="flex min-h-20 min-w-0 flex-1 flex-col justify-center">
            <h2 className="truncate text-2xl font-semibold leading-8 text-[var(--color-fg)]">
              {appName}
            </h2>
            <p className="truncate text-sm leading-6 text-[var(--color-fg-muted)]">
              作者：{AUTHOR_NAME}
            </p>
            <p className="truncate text-sm leading-6 text-[var(--color-fg-subtle)]">
              版本：{appVersion}
            </p>
          </div>
        </div>

        {openError && (
          <p className="mt-3 text-xs text-[var(--color-err)]">打开主页失败：{openError}</p>
        )}

        <div className="mt-6 flex justify-end gap-2">
          <Button variant="primary" onClick={openAuthorHome}>
            关注作者
          </Button>
          <Button onClick={onClose}>关闭</Button>
        </div>
      </div>
    </Modal>
  );
}
