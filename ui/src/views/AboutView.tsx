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
      overlayClassName="bg-black/45"
      className="max-w-[400px] rounded-[14px] border-white/10 bg-[#05070b] text-[#f8fafc] shadow-[0_24px_70px_rgba(0,0,0,0.58)]"
    >
      <div className="p-5">
        <div className="flex items-center gap-4">
          <img
            src={appIcon}
            alt={appName}
            className="h-[72px] w-[72px] flex-none rounded-[14px] shadow-[0_10px_28px_rgba(0,0,0,0.38)]"
          />

          <div className="flex min-h-[72px] min-w-0 flex-1 flex-col justify-center">
            <h2 className="truncate text-[23px] font-semibold leading-7 tracking-normal text-[#f8fafc]">
              {appName}
            </h2>
            <p className="truncate text-[13px] leading-[22px] text-[#cbd5e1]">
              作者：{AUTHOR_NAME}
            </p>
            <p className="truncate text-[13px] leading-[22px] text-[#94a3b8]">
              版本：{appVersion}
            </p>
          </div>
        </div>

        {openError && (
          <p className="mt-3 text-xs text-[var(--color-err)]">打开主页失败：{openError}</p>
        )}

        <div className="mt-6 flex justify-end gap-2">
          <Button size="sm" variant="primary" onClick={openAuthorHome}>
            关注作者
          </Button>
          <Button
            size="sm"
            onClick={onClose}
            className="border-white/10 bg-white/5 text-[#e2e8f0] hover:bg-white/10 hover:text-white"
          >
            关闭
          </Button>
        </div>
      </div>
    </Modal>
  );
}
