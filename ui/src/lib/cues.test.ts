import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({ CMD: {}, cmdSilent: vi.fn() }));

const sources: { start: ReturnType<typeof vi.fn>; buffer: unknown }[] = [];
let playCueKind: typeof import("./cues").playCueKind;

describe("WebView cue fallback", () => {
  beforeEach(async () => {
    vi.resetModules();
    sources.length = 0;
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(8) })));
    vi.stubGlobal("OfflineAudioContext", class {
      decodeAudioData = vi.fn(async () => "decoded");
    });
    vi.stubGlobal("AudioContext", class {
      state = "running";
      destination = {};
      createBufferSource() {
        const source = { buffer: null as unknown, connect: vi.fn(), start: vi.fn() };
        sources.push(source);
        return source;
      }
    });
    ({ playCueKind } = await import("./cues"));
  });
  afterEach(() => vi.unstubAllGlobals());

  it("plays the bundled WAV for a built-in cue", async () => {
    playCueKind("drop-up", "start");
    await vi.waitFor(() => expect(sources).toHaveLength(1));
    expect(fetch).toHaveBeenCalledWith("/cues/drop-up.wav");
    expect(sources[0].buffer).toBe("decoded");
    expect(sources[0].start).toHaveBeenCalledOnce();
  });

  it("stays silent for the none preset", async () => {
    playCueKind("none", "start");
    await Promise.resolve();
    expect(fetch).not.toHaveBeenCalled();
  });
});
