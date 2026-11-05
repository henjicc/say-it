import { afterEach, describe, expect, it, vi } from "vitest";
// 图标库很大：提前加载，避免用例里的动态导入在高负载下超过默认超时。
import "lucide-react";

afterEach(() => {
  vi.unstubAllEnvs();
  vi.resetModules();
});

async function proNavViews() {
  const { PRO_NAV_GROUPS } = await import("./navConfig");
  return PRO_NAV_GROUPS.flat().map((item) => item.view);
}

describe("开发者入口", () => {
  it("开发状态运行时才出现", async () => {
    vi.stubEnv("DEV", true);
    expect(await proNavViews()).toContain("developer");
  });

  it("发布构建里没有这个入口", async () => {
    vi.stubEnv("DEV", false);
    expect(await proNavViews()).not.toContain("developer");
  });
});
