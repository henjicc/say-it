import { useEffect, useState } from "react";
import { getName, getVersion } from "@tauri-apps/api/app";
import { cmd, CMD } from "@/lib/tauri";
import appIcon from "../../../src-tauri/icons/icon.png";

const AUTHOR_NAME = "痕继痕迹";
const AUTHOR_HOME = "https://space.bilibili.com/39337803";

export function AboutView() {
  const [appName, setAppName] = useState("说吧！");
  const [appVersion, setAppVersion] = useState("读取中...");
  const [openError, setOpenError] = useState("");

  useEffect(() => {
    getName().then(setAppName).catch(() => {});
    getVersion().then(setAppVersion).catch(() => setAppVersion("未知版本"));
  }, []);

  const openAuthorHome = () => {
    setOpenError("");
    cmd(CMD.openExternalLink, { url: AUTHOR_HOME }).catch((error) => {
      setOpenError(String(error));
    });
  };

  return (
    <div className="flex min-h-[60vh] items-center justify-center">
      <section className="flex w-full max-w-[720px] items-center gap-5 rounded-[calc(var(--radius-xl)+4px)] border border-[var(--color-line)] bg-[var(--color-surface)] px-6 py-6 shadow-[var(--shadow-sm)]">
        <img src={appIcon} alt={appName} className="h-18 w-18 rounded-[var(--radius-xl)]" />

        <div className="min-w-0 flex-1">
          <div className="flex flex-col gap-1">
            <h1 className="text-[28px] font-semibold tracking-tight text-[var(--color-fg)]">
              {appName}
            </h1>
            <p className="text-sm text-[var(--color-fg-subtle)]">版本 {appVersion}</p>
          </div>

          <div className="mt-5 flex flex-col gap-1">
            <p className="text-xs text-[var(--color-fg-subtle)]">作者</p>
            <p className="text-base font-medium text-[var(--color-fg)]">{AUTHOR_NAME}</p>
          </div>

          <button
            type="button"
            onClick={openAuthorHome}
            className="mt-4 inline-flex items-center rounded-[var(--radius-md)] px-0 text-sm text-[var(--color-accent)] transition-colors duration-[var(--dur-fast)] hover:text-[var(--color-accent-light)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]"
          >
            打开作者主页
          </button>

          {openError && (
            <p className="mt-2 text-xs text-[var(--color-err)]">打开主页失败：{openError}</p>
          )}
        </div>
      </section>
    </div>
  );
}
