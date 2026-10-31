import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";

import App from "./App.vue";
import "./style.css";
import { isTauri } from "./lib/tauri-env";

createApp(App).mount("#app");

// 启动画面握手：挂载后再等两帧，确保首屏真的画出来了再放行主窗口，
// 否则会「主窗口先白后黑」地闪一下。浏览器预览没有主窗口，直接跳过。
if (isTauri()) {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      void invoke("app_ready").catch(() => {});
    }),
  );
}
