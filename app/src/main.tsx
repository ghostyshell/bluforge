import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// macOS draws the window buttons over the webview (titleBarStyle: Overlay),
// so the header needs room for them.
if (navigator.userAgent.includes("Mac")) {
  document.documentElement.dataset.os = "mac";
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
