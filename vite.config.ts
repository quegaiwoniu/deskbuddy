import { defineConfig } from "vite";
import path from "node:path";

export default defineConfig({
  // 公开角色包与品牌资源位于 assets/，构建时复制到 dist
  publicDir: "assets",
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "index.html"),
        settings: path.resolve(__dirname, "settings.html"),
      },
    },
  },
});
