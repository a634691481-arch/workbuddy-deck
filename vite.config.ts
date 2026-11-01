import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

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
  // Tauri 固定端口：dev 模式用它，构建时用相对路径。
  server: { port: 1420, strictPort: true },
  clearScreen: false,
});
