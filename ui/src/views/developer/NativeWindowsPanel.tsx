import { useEffect, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { SectionHeader } from "@/components/ui/SectionHeader";
import { CMD, cmd } from "@/lib/tauri";

/** 每个原生窗口可以摆出来的状态；键是后端认识的窗口名和状态名。测试用假数据，不录音、不识别。 */
const WINDOWS: { key: string; label: string; hint: string; scenes: [string, string][] }[] = [
  {
    key: "indicator",
    label: "语音输入胶囊",
    hint: "在 Windows 上，智能处理中可以把鼠标移到胶囊上、点一下，看悬停和点击的反馈；Mac 上胶囊不响应鼠标。",
    scenes: [
      ["recording", "录音"],
      ["processing", "处理中"],
      ["smartProcessing", "智能处理"],
      ["fallback", "已复制，请手动粘贴"],
      ["notice", "通知"],
    ],
  },
  {
    key: "orb",
    label: "悬浮球",
    hint: "可以悬停、点击、右键和拖动；收起后回到原来的位置。",
    scenes: [
      ["idle", "空闲"],
      ["armed", "待命"],
      ["positioning", "定位中"],
      ["moving", "移动中"],
      ["recording", "录音"],
      ["processing", "处理中"],
      ["smartProcessing", "智能处理"],
      ["success", "完成"],
      ["fallback", "已复制"],
      ["error", "出错"],
      ["cancelled", "已取消"],
      ["busy", "忙碌"],
      ["submitting", "发送中"],
      ["submitted", "已发送"],
    ],
  },
  {
    key: "subtitle",
    label: "实时字幕条",
    hint: "可以悬停看控制条、点锁定和重置、拖动；点关闭只收起字幕条。译文只在字幕设置里开了翻译时显示。",
    scenes: [
      ["original", "原文"],
      ["translated", "原文和译文"],
    ],
  },
  {
    key: "recorder",
    label: "会议录制条",
    hint: "可以拖动、悬停和点停止；点停止只收起录制条，拖到的位置不会保存。",
    scenes: [
      ["recording", "录制中"],
      ["micLost", "麦克风断开"],
      ["systemLost", "电脑声音断开"],
    ],
  },
];

/** 用假数据把原生窗口摆到屏幕上看外观和手感；窗口上的操作不触发真实动作。一次只摆出一个。 */
export function NativeWindowsPanel() {
  const [shown, setShown] = useState<{ window: string; scene: string } | null>(null);
  const [message, setMessage] = useState("");

  // 离开这一页就收起测试窗口，交还给真实状态。
  useEffect(() => () => void cmd(CMD.devNativeWindowTestStop).catch(() => {}), []);

  const hide = () => {
    setMessage("");
    cmd(CMD.devNativeWindowTestStop)
      .then(() => setShown(null))
      .catch((reason: unknown) => setMessage(String(reason)));
  };

  // 点已选中的一项会重新摆一次：通知会自己消失，字幕条和录制条也可能被它们自己的按钮收起。
  const show = (window: string, scene: string) => {
    setMessage("");
    cmd(CMD.devNativeWindowTest, { window, scene })
      .then(() => setShown({ window, scene }))
      .catch((reason: unknown) => {
        setShown(null);
        setMessage(String(reason));
      });
  };

  return (
    <InspectorBody>
      {WINDOWS.map((item) => (
        <section key={item.key} className="flex flex-col gap-2">
          <SectionHeader title={item.label} description={item.hint} />
          <div className="flex flex-wrap gap-2">
            {item.scenes.map(([scene, label]) => {
              const active = shown?.window === item.key && shown.scene === scene;
              return (
                <Button
                  key={scene}
                  size="sm"
                  variant={active ? "primary" : "ghost"}
                  aria-pressed={active}
                  onClick={() => show(item.key, scene)}
                >
                  {label}
                </Button>
              );
            })}
          </div>
        </section>
      ))}
      <div className="flex items-center gap-3">
        <Button size="sm" variant="text" onClick={hide}>收起</Button>
        {message && <span role="alert" className="text-[13px] text-[var(--color-err)]">{message}</span>}
      </div>
    </InspectorBody>
  );
}
