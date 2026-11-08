import { useProviderStore, type ProviderProfile } from "@/store/useProviderStore";

const providerQueues = new Map<string, Promise<unknown>>();

/** 同一供应商的配置写入与模型列表刷新按顺序执行，避免整份写回的字段互相覆盖。 */
export function enqueueProviderTask<T>(providerId: string, task: () => Promise<T>): Promise<T> {
  const operation = (providerQueues.get(providerId) ?? Promise.resolve()).then(task);
  providerQueues.set(providerId, operation.then(() => undefined, () => undefined));
  return operation;
}

/** 写入时读取最新配置，再按它算出这次要写的字段。 */
export function useProviderConfigQueue(providerId: string) {
  const updateConfig = useProviderStore((state) => state.updateConfig);
  return (build: (latest: ProviderProfile) => Record<string, unknown>): Promise<ProviderProfile> => enqueueProviderTask(providerId, () => {
    const latest = useProviderStore.getState().profiles.find((profile) => profile.id === providerId);
    if (!latest) throw new Error("供应商不存在");
    return updateConfig(providerId, build(latest));
  });
}
