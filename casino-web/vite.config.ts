import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 봇의 Activity 서버가 /casino/ 아래에서 이 페이지를 서비스한다.
// 개발 시에는 API/WS를 로컬 봇(2053)으로 프록시한다.
export default defineConfig({
  plugins: [react()],
  base: "/casino/",
  server: {
    port: 5174,
    proxy: {
      "/casino/api": {
        target: "http://localhost:2053",
        changeOrigin: true,
        ws: true,
      },
    },
  },
  build: {
    outDir: "dist",
  },
});
