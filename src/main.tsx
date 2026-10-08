import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
// 新 UI 设计层（Kinetic Dark）：必须在 index.css 之后引入，
// 使同优先级的 .kb-* 规则稳定胜出，--seed-* 多主题令牌保持遗留层身份。
import "./kinetic.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
