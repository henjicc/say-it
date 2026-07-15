import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";
import { fileURLToPath } from "url";

const dirname = fileURLToPath(new URL(".", import.meta.url));

// Vite 以 ui/ 为根，同时打包主窗、听写指示器与上下文调试窗三个入口。
// 输出到 ui/dist，对应 tauri.conf.json 的 frontendDist。
export default defineConfig({
  root: "ui",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": resolve(dirname, "ui/src"),
      "~shared": resolve(dirname, "shared"),
    },
  },
  clearScreen: false,
  server: {
    port: 5155,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: resolve(dirname, "ui/index.html"),
        indicator: resolve(dirname, "ui/indicator.html"),
        contextDebug: resolve(dirname, "ui/context-debug.html"),
      },
    },
  },
});
