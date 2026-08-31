/** 同时跟踪布局尺寸和显示器像素密度；动画结束后按最终坐标重新对齐。 */
export function observeOrbGeometry(
  element: HTMLElement,
  measure: () => void,
  animationTarget: HTMLElement | null = element.parentElement,
) {
  let disposed = false;
  const refresh = () => { if (!disposed) measure(); };
  const observer = new ResizeObserver(refresh);
  observer.observe(element);
  animationTarget?.addEventListener("animationend", refresh);
  let resolution: MediaQueryList;
  const watchResolution = () => {
    if (disposed) return;
    resolution?.removeEventListener("change", watchResolution);
    resolution = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    resolution.addEventListener("change", watchResolution);
    refresh();
  };
  watchResolution();
  return () => {
    disposed = true;
    observer.disconnect();
    animationTarget?.removeEventListener("animationend", refresh);
    resolution.removeEventListener("change", watchResolution);
  };
}
