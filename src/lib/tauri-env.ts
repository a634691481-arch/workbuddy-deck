/** 是否运行在 Tauri 宿主内。浏览器里 `@tauri-apps/api` 的 window 方法会崩，必须先判断。 */
export function isTauri(): boolean {
  return typeof (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== "undefined";
}

/**
 * 是否运行在 WorkBuddy 页面内的嵌入面板里（iframe + 本地桥 shim）。
 *
 * 面板里 `__TAURI_INTERNALS__` 是 shim 造的，`isTauri()` 仍为 true，
 * 但**没有真实窗口**——窗口控制按钮、`app_ready` 这类都必须关掉。
 */
export function isEmbedded(): boolean {
  return (window as { __WB_DECK_EMBED__?: boolean }).__WB_DECK_EMBED__ === true;
}

/** 需要真实窗口的宿主（独立窗口 / 托盘打开）。 */
export function hasHostWindow(): boolean {
  return isTauri() && !isEmbedded();
}
