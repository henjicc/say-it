import { beforeEach, describe, expect, it } from "vitest";
import { MAIN_NAV_ITEMS } from "@/components/shell/navConfig";
import { useUiStore } from "@/store/useUiStore";

describe("首页与智能助手导航", () => {
  beforeEach(() => useUiStore.setState({ view: "home", assistantTab: "smart" }));

  it("uses home as the default primary destination", () => {
    expect(MAIN_NAV_ITEMS[0]).toMatchObject({ view: "home", label: "首页" });
    expect(useUiStore.getState().view).toBe("home");
  });

  it("opens the exact intelligent assistant feature from a deep link", () => {
    useUiStore.getState().openAssistantSettings("translateSpeech");
    expect(useUiStore.getState()).toMatchObject({ view: "assistant", assistantTab: "translateSpeech" });
  });
});
