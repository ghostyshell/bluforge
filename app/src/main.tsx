import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// macOS draws the window buttons over the webview (titleBarStyle: Overlay),
// so the header needs room for them.
if (navigator.userAgent.includes("Mac")) {
  document.documentElement.dataset.os = "mac";
}

// `npm run dev:mock` loads the browser fixture before React mounts. In a
// release build `import.meta.env.DEV` is false and the import is dropped.
async function boot() {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("mock")) {
    const { installMock } = await import("./mock");
    installMock();
  }
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void boot();
