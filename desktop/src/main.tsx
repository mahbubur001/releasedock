import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles/app.css";
import { invoke } from "@tauri-apps/api/core";

// Report UI errors to the app log (stderr); the window has no visible console.
const report = (message: string) => { invoke("log_ui_error", { message }).catch(() => {}); };
window.addEventListener("error", (e) => report(`${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report(`Unhandled rejection: ${String(e.reason?.stack ?? e.reason)}`));
report("UI started");
import "./styles/extra.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
