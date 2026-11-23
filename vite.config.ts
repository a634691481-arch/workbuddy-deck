import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf-8")) as { version: string };

export default defineConfig({
  // <lord-icon> 是 Lordicon 的自定义元素，交给浏览器原生解析，不当 Vue 组件。
  plugins: [
    vue({
      template: { compilerOptions: { isCustomElement: (tag) => tag === "lord-icon" } },
    }),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  define: {
    // 前端展示的版本号统一来自 package.json（启动动画 / 关于页不再各写一份）
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  // Tauri 固定端口：dev 模式用它，构建时用相对路径。
  // 显式绑 IPv4 回环：本地桥（deck_http）请求 127.0.0.1:1420，
  // 默认只监听 ::1 时 curl / reqwest 都连不上。
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  clearScreen: false,
});
