import { describe, expect, it } from "vitest";
import { audioPrice, tokenPrice } from "./pricing";

describe("model price labels", () => {
  it("formats per-hour, estimated, free, tiered, and sub-cent prices", () => {
    expect(audioPrice({ billing: "audio-duration", currency: "CNY", perSecond: 0.00033 })).toEqual({ text: "¥1.19/时" });
    expect(audioPrice({ billing: "audio-duration", currency: "CNY", perSecond: 0 })).toEqual({ text: "免费" });
    expect(audioPrice({
      billing: "tokens",
      currency: "CNY",
      inputPerMillionTokens: 0.8,
      outputPerMillionTokens: 2.7,
      audioTokenRate: { inputTokensPerSecond: 12.5, outputTokensPerSecond: 3, source: "test" },
    })).toEqual({ text: "约 ¥0.065/时" });
    expect(audioPrice(undefined)).toBeUndefined();
    expect(tokenPrice({ currency: "USD", inputPerMillionTokens: 0.075, outputPerMillionTokens: 0.3 }))
      .toEqual({ text: "$0.075 / $0.3", perMillionTokens: true });
    expect(tokenPrice({
      currency: "CNY",
      inputPerMillionTokens: 2,
      outputPerMillionTokens: 8,
      tiers: [{ inputPerMillionTokens: 6, outputPerMillionTokens: 24, aboveInputTokens: 256000 }],
    }).text).toBe("¥2 / ¥8 起");
  });
});
