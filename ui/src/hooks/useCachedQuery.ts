import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";

/** 一份可缓存的读取：同一个 key 在整个窗口里共用一份结果。 */
export interface Query<T> {
  key: string;
  load: () => Promise<T>;
}

const values = new Map<string, unknown>();
const inflight = new Map<string, Promise<unknown>>();
/** 每个 key 最近一次发起的读取；更早发起、更晚返回的结果不写进缓存。 */
const latest = new Map<string, number>();
let nextRead = 0;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((listener) => listener());
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => void listeners.delete(listener);
};

/**
 * 读取并写入缓存。同一个 key 正在读时共用那一次；`fresh` 为真时（例如数据刚变过）一定重新读，
 * 之前那次的结果作废。
 */
export function fetchQuery<T>(query: Query<T>, fresh = false): Promise<T> {
  const running = inflight.get(query.key);
  if (running && !fresh) return running as Promise<T>;
  const read = ++nextRead;
  latest.set(query.key, read);
  const promise = query.load()
    .then((value) => {
      if (latest.get(query.key) === read) {
        values.set(query.key, value);
        emit();
      }
      return value;
    })
    .finally(() => {
      if (inflight.get(query.key) === promise) inflight.delete(query.key);
    });
  inflight.set(query.key, promise);
  return promise;
}

/** 写操作已经拿到最新结果时直接放进缓存，正在进行的旧读取作废。 */
export function setQueryData<T>(query: Pick<Query<T>, "key">, value: T) {
  latest.set(query.key, ++nextRead);
  values.set(query.key, value);
  emit();
}

/** 只看缓存里的结果，不发起读取；适合自己决定什么时候重新读的组件。 */
export function useQueryData<T>(query: Pick<Query<T>, "key">) {
  return useSyncExternalStore(subscribe, () => values.get(query.key) as T | undefined);
}

/**
 * 页面打开时先显示这个窗口里上次读到的结果，同时在后台重新读一次，有变化再更新。
 * `data` 为空表示还没读到过，不等于读到了空结果：调用方不要在这时显示「暂无」之类的空状态。
 * `refresh` 用在数据刚变过的时候，一定重新读。
 */
export function useCachedQuery<T>(query: Query<T>) {
  const data = useQueryData<T>(query);
  const [error, setError] = useState("");
  const current = useRef(query);
  current.current = query;
  const read = useCallback(async (fresh: boolean) => {
    try {
      await fetchQuery(current.current, fresh);
      setError("");
    } catch (reason) {
      setError(String(reason));
    }
  }, []);
  useEffect(() => {
    void read(false);
  }, [query.key, read]);
  const refresh = useCallback(() => read(true), [read]);
  return { data, error, refresh };
}
