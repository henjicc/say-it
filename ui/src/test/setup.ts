import "@testing-library/jest-dom/vitest";

// jsdom 不实现 ResizeObserver，而 WebView2 里它是常规可用的 API。
// 补一个不会触发回调的空实现，让依赖它的组件能在测试里正常挂载。
if (!("ResizeObserver" in globalThis)) {
  class ResizeObserverStub implements ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  globalThis.ResizeObserver = ResizeObserverStub;
}
