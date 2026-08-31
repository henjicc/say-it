import { StrictMode, useRef } from "react";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useOrbStroke } from "./useOrbStroke";

let resize: () => void;
let resolutionChanged: () => void;
let rect: DOMRect;
const observe = vi.fn();
const disconnect = vi.fn();
const removeListener = vi.fn();

function Preview() {
  const button = useRef<HTMLButtonElement>(null);
  const width = useOrbStroke(button);
  return <div data-testid="frame"><button ref={button} style={{ borderWidth: width }}>orb</button></div>;
}

describe("orb stroke display adaptation", () => {
  beforeEach(() => {
    rect = new DOMRect(0, 0, 28, 28);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => rect);
    vi.stubGlobal("devicePixelRatio", 2);
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: () => void) { resize = callback; }
      observe = observe;
      disconnect = disconnect;
    });
    vi.stubGlobal("matchMedia", () => ({
      addEventListener: (_: string, callback: () => void) => { resolutionChanged = callback; },
      removeEventListener: removeListener,
    }));
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it("observes the untransformed frame and updates size and density without remounting", () => {
    const view = render(<Preview />);
    const button = view.getByRole("button");
    expect(observe).toHaveBeenCalledWith(view.getByTestId("frame"));
    expect(button.style.borderWidth).toBe("1.5px");
    rect = new DOMRect(0, 0, 72, 72);
    act(() => resize());
    expect(button.style.borderWidth).toBe("2px");
    vi.stubGlobal("devicePixelRatio", 1.75);
    act(() => resolutionChanged());
    expect(Number.parseFloat(button.style.borderWidth) * 1.75).toBeCloseTo(4);
  });

  it("cleans up StrictMode observers and ignores callbacks after disposal", () => {
    const view = render(<StrictMode><Preview /></StrictMode>);
    expect(disconnect).toHaveBeenCalledTimes(1);
    view.unmount();
    expect(disconnect).toHaveBeenCalledTimes(2);
    const removed = removeListener.mock.calls.length;
    act(() => { resize(); resolutionChanged(); });
    expect(removeListener).toHaveBeenCalledTimes(removed);
  });
});
