import { describe, expect, it } from "vitest";
import { proNavGroups } from "./navConfig";

describe("开发者入口", () => {
  it("只在开发者模式下出现", () => {
    expect(proNavGroups(true).flat().map((item) => item.view)).toContain("developer");
    expect(proNavGroups(false).flat().map((item) => item.view)).not.toContain("developer");
  });
});
