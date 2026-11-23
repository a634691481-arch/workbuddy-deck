import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { defineElement } from "@lordicon/element";

import App from "./App.vue";
import "./style.css";
import "./lib/prefs"; // 显示偏好（主题/圆角）在挂载前应用，避免首屏闪错主题
import { hasHostWindow } from "./lib/tauri-env";

// 注册 <lord-icon> 自定义元素（服务端/无 DOM 时是空操作）。
defineElement();

createApp(App).mount("#app");

// 启动画面握手：挂载后再等两帧，确保首屏真的画出来了再放行主窗口，
// 否则会「主窗口先白后黑」地闪一下。
//
// 只有「有真实宿主窗口」时才调：嵌入面板（WorkBuddy 页面内 iframe）里
// shim 伪造了 __TAURI_INTERNALS__，isTauri() 同样为 true，而后端 app_ready
// 会 show() + set_focus() 主窗口——点一下面板图标就把独立窗口弹到前台。
if (hasHostWindow()) {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      void invoke("app_ready").catch(() => {});
    }),
  );
}
