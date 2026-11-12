import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { fetchQuery, useCachedQuery, type Query } from "./useCachedQuery";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function Show({ query }: { query: Query<string> }) {
  const { data } = useCachedQuery(query);
  return <p>{data ?? "未读到"}</p>;
}

describe("useCachedQuery", () => {
  afterEach(cleanup);

  it("再次打开时先显示上次读到的结果，同时在后台刷新", async () => {
    const reads = [deferred<string>(), deferred<string>()];
    let call = 0;
    const query: Query<string> = { key: "remount", load: () => reads[call++].promise };
    const first = render(<Show query={query} />);
    expect(screen.getByText("未读到")).toBeInTheDocument();
    await act(async () => reads[0].resolve("旧结果"));
    first.unmount();

    render(<Show query={query} />);
    expect(screen.getByText("旧结果")).toBeInTheDocument();
    await act(async () => reads[1].resolve("新结果"));
    expect(screen.getByText("新结果")).toBeInTheDocument();
  });

  it("数据变过后重新读的结果不会被更早发起、更晚返回的读取覆盖", async () => {
    const older = deferred<string>();
    const newer = deferred<string>();
    const query: Query<string> = { key: "order", load: () => older.promise };
    render(<Show query={query} />);
    void fetchQuery({ ...query, load: () => newer.promise }, true);
    await act(async () => newer.resolve("新结果"));
    await act(async () => older.resolve("旧结果"));
    expect(screen.getByText("新结果")).toBeInTheDocument();
  });
});
