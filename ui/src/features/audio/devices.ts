import { useCachedQuery, type Query } from "@/hooks/useCachedQuery";
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

export const audioDevicesQuery: Query<AudioDeviceList> = {
  key: "audioDevices",
  load: () => cmd<AudioDeviceList>(CMD.listAudioDevices),
};

/** 系统真实的麦克风/播放设备，多个设置面板共用；读不到时为空列表，调用方退回默认设备选项。 */
export function useAudioDevices() {
  return useCachedQuery(audioDevicesQuery).data ?? EMPTY_LIST;
}
