import { useEffect, useMemo, useState } from "react";
import { getName, getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { appDataDir } from "@tauri-apps/api/path";
import { Button } from "@/components/ui/Button";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { copyText } from "@/lib/clipboard";

const AUTHOR_NAME = "痕继痕迹";
const AUTHOR_HOME = "https://space.bilibili.com/39337803";

function InfoCard({
  title,
  value,
  hint,
  action,
}: {
  title: string;
  value: React.ReactNode;
  hint?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div className="rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
      <p className="text-xs text-[var(--color-fg-subtle)]">{title}</p>
      <div className="mt-2 text-[15px] font-semibold text-[var(--color-fg)]">{value}</div>
      {hint && <p className="mt-1 text-xs leading-relaxed text-[var(--color-fg-subtle)]">{hint}</p>}
      {action && <div className="mt-3">{action}</div>}
    </div>
  );
}

export function AboutView() {
  const [appName, setAppName] = useState("说吧！");
  const [appVersion, setAppVersion] = useState("读取中...");
  const [tauriVersion, setTauriVersion] = useState("读取中...");
  const [dataDir, setDataDir] = useState("读取中...");
  const [copyMsg, setCopyMsg] = useState("");

  useEffect(() => {
    getName().then(setAppName).catch(() => {});
    getVersion().then(setAppVersion).catch(() => setAppVersion("未知"));
    getTauriVersion().then(setTauriVersion).catch(() => setTauriVersion("未知"));
    appDataDir().then(setDataDir).catch(() => setDataDir("未知"));
  }, []);

  const diagnosticsText = useMemo(
    () =>
      [
        `应用：${appName}`,
        `版本：${appVersion}`,
        `运行时：Tauri ${tauriVersion}`,
        `平台：${navigator.userAgent}`,
        `数据目录：${dataDir}`,
        `作者：${AUTHOR_NAME}`,
        `主页：${AUTHOR_HOME}`,
      ].join("\n"),
    [appName, appVersion, dataDir, tauriVersion],
  );

  const copyWithToast = async (text: string, message: string) => {
    await copyText(text);
    setCopyMsg(message);
    window.clearTimeout((copyWithToast as typeof copyWithToast & { timer?: number }).timer);
    (copyWithToast as typeof copyWithToast & { timer?: number }).timer = window.setTimeout(
      () => setCopyMsg(""),
      1800,
    );
  };

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="关于"
        description="查看作者、版本、隐私说明、支持方式和诊断信息。"
        actions={
          copyMsg ? (
            <span className="text-xs text-[var(--color-fg-subtle)]" role="status">
              {copyMsg}
            </span>
          ) : undefined
        }
      />

      <div className="grid gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(18rem,0.8fr)]">
        <InfoCard
          title="作者"
          value={AUTHOR_NAME}
          hint="项目作者与主页信息。"
          action={
            <div className="flex flex-wrap gap-2">
              <Button size="sm" variant="primary" onClick={() => copyWithToast(AUTHOR_HOME, "已复制作者主页链接")}>
                复制主页链接
              </Button>
            </div>
          }
        />
        <InfoCard
          title="当前版本"
          value={appVersion}
          hint="安装包版本与应用内展示保持一致。"
        />
      </div>

      <SettingsSection title="产品信息">
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <InfoCard title="应用名称" value={appName} />
          <InfoCard title="运行时" value={`Tauri ${tauriVersion}`} />
          <InfoCard title="前端栈" value="React 19 + TypeScript" />
          <InfoCard title="定位" value="桌面端语音 / 听写工具" />
        </div>
      </SettingsSection>

      <SettingsSection title="更新与支持">
        <div className="grid gap-3 lg:grid-cols-2">
          <InfoCard
            title="获取新版"
            value="通过作者主页查看最新动态"
            hint="后续如果接入发布页或自动更新，可以直接扩展到这一块。"
            action={
              <Button size="sm" onClick={() => copyWithToast(AUTHOR_HOME, "已复制更新入口链接")}>
                复制更新入口
              </Button>
            }
          />
          <InfoCard
            title="反馈与联系"
            value="建议、问题反馈、版本信息请优先附上诊断信息"
            hint="这样更方便定位环境差异、版本差异和配置问题。"
            action={
              <Button size="sm" onClick={() => copyWithToast(diagnosticsText, "已复制诊断信息")}>
                复制诊断信息
              </Button>
            }
          />
        </div>
      </SettingsSection>

      <SettingsSection title="隐私与数据">
        <div className="grid gap-3 lg:grid-cols-2">
          <div className="rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
            <p className="text-sm font-medium text-[var(--color-fg)]">数据处理说明</p>
            <ul className="mt-3 flex list-disc flex-col gap-2 pl-5 text-sm leading-relaxed text-[var(--color-fg-muted)]">
              <li>麦克风与音频输入仅在你主动使用语音输入、实时字幕或转写功能时参与处理。</li>
              <li>识别所需的服务配置和偏好设置保存在本地设备。</li>
              <li>当你选择云端识别能力时，相关音频会按对应服务的处理流程发送到云端。</li>
            </ul>
          </div>
          <div className="rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
            <p className="text-sm font-medium text-[var(--color-fg)]">使用建议</p>
            <ul className="mt-3 flex list-disc flex-col gap-2 pl-5 text-sm leading-relaxed text-[var(--color-fg-muted)]">
              <li>共享电脑或演示环境下，建议在设置中检查 API Key 和启动项配置。</li>
              <li>反馈识别问题时，尽量同时提供功能入口、复现步骤和当前版本号。</li>
              <li>如果后续加入更细的日志开关，这里可以继续补充采集范围说明。</li>
            </ul>
          </div>
        </div>
      </SettingsSection>

      <SettingsSection title="诊断信息">
        <div className="rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <p className="text-xs text-[var(--color-fg-subtle)]">应用标识</p>
              <p className="mt-1 font-mono text-sm text-[var(--color-fg)]">com.vibecode.sayit</p>
            </div>
            <div>
              <p className="text-xs text-[var(--color-fg-subtle)]">数据目录</p>
              <p className="mt-1 break-all font-mono text-sm text-[var(--color-fg)]">{dataDir}</p>
            </div>
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            <Button size="sm" onClick={() => copyWithToast(dataDir, "已复制数据目录")}>
              复制数据目录
            </Button>
            <Button size="sm" onClick={() => copyWithToast(diagnosticsText, "已复制诊断信息")}>
              复制完整诊断信息
            </Button>
          </div>
        </div>
      </SettingsSection>
    </div>
  );
}
