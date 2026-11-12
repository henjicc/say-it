import { useEffect, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { CMD, cmd } from "@/lib/tauri";

const HIDDEN = "hidden";

/** 每个原生窗口可以摆出来的状态；键是后端认识的窗口名和状态名。 */
const WINDOWS: { key: string; label: string; hint: string; scenes: [string, string][] }[] = [
  {
    key: "indicator",
    label: "语音输入胶囊",
    hint: "在 Windows 上，智能处理中可以把鼠标移到胶囊上、点一下，看悬停和点击的反馈；Mac 上胶囊不响应鼠标。",
    scenes: [
      ["recording", "录音（波形和逐字文字）"],
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
      ["recording", "录音（音量柱）"],
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
      ["original", "原文逐字出现"],
      ["translated", "原文和译文"],
    ],
  },
  {
    key: "recorder",
    label: "会议录制条",
    hint: "可以拖动、悬停和点停止；点停止只收起录制条，拖到的位置不会保存。再选一次同一项可以重新显示。",
    scenes: [
      ["recording", "录制中"],
      ["micLost", "麦克风断开"],
      ["systemLost", "电脑声音断开"],
    ],
  },
];

/** 用假数据把原生窗口摆到屏幕上看外观和手感；窗口上的操作不触发真实动作。 */
export function NativeWindowsPanel() {
  const [scenes, setScenes] = useState<Record<string, string>>({});
  const [message, setMessage] = useState("");

  // 离开这一页就收起全部测试窗口，交还给真实状态。
  useEffect(() => () => void cmd(CMD.devNativeWindowTestStop).catch(() => {}), []);

  const show = (window: string, scene: string) => {
    setMessage("");
    cmd(CMD.devNativeWindowTest, { window, scene })
      .then(() => setScenes((current) => ({ ...current, [window]: scene })))
      .catch((reason: unknown) => setMessage(String(reason)));
  };

  const hideAll = () => {
    setMessage("");
    cmd(CMD.devNativeWindowTestStop)
      .then(() => setScenes({}))
      .catch((reason: unknown) => setMessage(String(reason)));
  };

  return (
    <InspectorBody>
      <Group>
        {WINDOWS.map((item) => (
          <Row key={item.key} label={item.label} hint={item.hint}>
            <Select
              className="w-64"
              aria-label={`${item.label}的状态`}
              value={scenes[item.key] ?? HIDDEN}
              onChange={(event) => show(item.key, event.target.value)}
            >
              <option value={HIDDEN}>不显示</option>
              {item.scenes.map(([key, label]) => <option key={key} value={key}>{label}</option>)}
            </Select>
          </Row>
        ))}
        <Row
          label="测试窗口"
          hint="测试用的是假数据，不录音、不识别，窗口上的操作不会触发真实功能。开始真实的语音输入、字幕或会议录制时测试自动结束。"
          message={message ? <span role="alert" className="text-[var(--color-err)]">{message}</span> : undefined}
        >
          <Button size="sm" onClick={hideAll}>全部收起</Button>
        </Row>
      </Group>
    </InspectorBody>
  );
}
