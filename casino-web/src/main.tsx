import React, { Suspense, lazy } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./base.css";
import "./noir.css";
import "./overrides.css";

// 같은 개인 링크에 ?view=stocks 를 붙이면 증권 화면(HTS)을 연다. 카지노만 쓰는 사람은 증권 코드를 받지 않는다.
const StockApp = lazy(() => import("./stocks/StockApp"));
const stocks = new URLSearchParams(window.location.search).get("view") === "stocks";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {stocks ? (
      <Suspense fallback={null}>
        <StockApp />
      </Suspense>
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
