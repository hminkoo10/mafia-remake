import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readdirSync } from "node:fs";

const dealerClips = readdirSync(new URL("./public/dealers/", import.meta.url))
  .filter((file) => /^[a-z0-9-]+-(idle|deal|flip)\.(webm|mp4)$/.test(file));

// 봇의 Activity 서버가 /casino/ 아래에서 이 페이지를 서비스한다.
// 개발 시에는 API/WS를 로컬 봇(2053)으로 프록시한다.
export default defineConfig({
  plugins: [react()],
  define: { __DEALER_CLIPS__: JSON.stringify(dealerClips) },
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
