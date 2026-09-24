import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { readdirSync } from "node:fs";

const dealerClips = readdirSync(new URL("./public/dealers/", import.meta.url))
  .filter((file) => /^[a-z0-9-]+-(idle|deal|flip|return)\.(webm|mp4)$/.test(file));

// 원본 noir.css는 기기의 '동작 줄이기'가 켜져 있으면 모든 애니메이션을 끈다. 원본 파일은 그대로 두고,
// 빌드할 때 그 규칙만 앱의 연출 끄기(.casino-app.calm)에 묶는다. 연출은 화면의 버튼으로 끄고 켠다.
const NOIR_REDUCED_MOTION =
  "@media(prefers-reduced-motion:reduce){*,*:before,*:after{transition:none!important;animation:none!important;scroll-behavior:auto!important}}";
const CALM_MOTION =
  ".casino-app.calm *,.casino-app.calm *:before,.casino-app.calm *:after{transition:none!important;animation:none!important;scroll-behavior:auto!important}";
function calmMotionSwitch(): Plugin {
  return {
    name: "casino-calm-motion",
    enforce: "pre",
    transform(code, id) {
      if (!id.split("?")[0].endsWith("/src/noir.css")) return null;
      if (!code.includes(NOIR_REDUCED_MOTION)) throw new Error("noir.css의 동작 줄이기 규칙을 찾지 못했습니다. vite.config.ts를 맞춰 주세요.");
      return code.replace(NOIR_REDUCED_MOTION, CALM_MOTION);
    },
  };
}

// 봇의 Activity 서버가 /casino/ 아래에서 이 페이지를 서비스한다.
// 개발 시에는 API/WS를 로컬 봇(2053)으로 프록시한다.
export default defineConfig({
  plugins: [calmMotionSwitch(), react()],
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
