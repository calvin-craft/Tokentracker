import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Widget } from "./Widget";
import { SettingsView } from "./SettingsView";
import "./styles.css";

declare global {
  interface Window {
    /** Set by the Rust side for the settings window. */
    __TT_VIEW__?: string;
  }
}

const view = window.__TT_VIEW__ === "settings" ? "settings" : "widget";
document.body.dataset.view = view;

createRoot(document.getElementById("root")!).render(
  <StrictMode>{view === "settings" ? <SettingsView /> : <Widget />}</StrictMode>,
);
