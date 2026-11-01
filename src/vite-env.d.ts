/// <reference types="vite/client" />

// Lordicon 自定义元素（@lordicon/element 在 main.ts 中 defineElement 注册）。
// 让 vue-tsc 认识 <lord-icon> 及其属性。
import type { DefineComponent } from "vue";

declare module "vue" {
  interface GlobalComponents {
    "lord-icon": DefineComponent<{
      /** 图标 Lottie JSON 地址（本地或 CDN）。 */
      src?: string;
      /** 触发方式：hover | click | in | loop | loop-on-hover | morph | boomerang | sequence | none。 */
      trigger?: string;
      /** 主/辅色，形如 "primary:#ff3b30;secondary:#8a8a8a"。 */
      colors?: string;
      /** 描边宽度。 */
      stroke?: string;
      /** 单帧跳转（如 "start" / "end"）。 */
      state?: string;
      /** 缩放比例（字符串数字）。 */
      scale?: string;
      /** 加载策略：lazy | interaction | delay:500。 */
      loading?: string;
    }>;
  }
}

export {};
