import { useEffect, useState } from "react";
import { CMD, cmd } from "@/lib/tauri";

export interface AudioDeviceInfo {
  name: string;
  isDefault: boolean;
}

export interface AudioDeviceList {
  inputs: AudioDeviceInfo[];
  outputs: AudioDeviceInfo[];
}

const EMPTY_LIST: AudioDeviceList = { inputs: [], outputs: [] };

let cachedDevices: AudioDeviceList | null = null;

/** 枚举系统真实的麦克风/播放设备，结果按模块缓存，多个设置面板共用同一份。 */
export function useAudioDevices() {
  const [devices, setDevices] = useState<AudioDeviceList>(cachedDevices ?? EMPTY_LIST);

  useEffect(() => {
    if (cachedDevices) return;
    cmd<AudioDeviceList>(CMD.listAudioDevices)
      .then((list) => {
        cachedDevices = list;
        setDevices(list);
      })
      .catch(() => {
        /* 保留空列表，调用方会退回默认设备选项 */
      });
  }, []);

  return devices;
}
