/** 是否运行在 Tauri 宿主内。浏览器里 `@tauri-apps/api` 的 window 方法会崩，必须先判断。 */
export function isTauri(): boolean {
  return typeof (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== "undefined";
}
