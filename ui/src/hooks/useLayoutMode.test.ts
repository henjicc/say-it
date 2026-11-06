import { describe, expect, it } from "vitest";
import { nextLayoutMode } from "./useLayoutMode";

describe("nextLayoutMode", () => {
  it("switches by window width with a buffer zone that keeps the current mode", () => {
    expect(nextLayoutMode("pro", 719)).toBe("simple");
    expect(nextLayoutMode("pro", 760)).toBe("pro");
    expect(nextLayoutMode("simple", 760)).toBe("simple");
    expect(nextLayoutMode("simple", 800)).toBe("pro");
  });
});
