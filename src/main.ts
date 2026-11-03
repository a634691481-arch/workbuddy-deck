import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { defineElement } from "@lordicon/element";

import App from "./App.vue";
import "./style.css";
import "./lib/prefs"; // 显示偏好（主题/圆角）在挂载前应用，避免首屏闪错主题
import { isTauri } from "./lib/tauri-env";

// 注册 <lord-icon> 自定义元素（服务端/无 DOM 时是空操作）。
defineElement();

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
