import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Discord Activity는 /.proxy/ prefix로 프록시됨
// 개발 시에는 백엔드(8802)로 직접 프록시
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/activity/api": {
        target: "http://localhost:8802",
        changeOrigin: true,
      },
      "/activity/ws": {
        target: "ws://localhost:8802",
        ws: true,
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist",
  },
  // Discord proxy는 /.proxy/ prefix를 붙이므로 base는 /로 유지
  base: "/",
});
