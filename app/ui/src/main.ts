import { invoke } from "@tauri-apps/api/core";
import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

// No browser context menu (Back, Refresh, Inspect...) anywhere in the app; text fields keep
// theirs (cut / copy / paste). Components open their own menus in their handlers.
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  if (!t?.closest("input, textarea, [contenteditable]")) e.preventDefault();
});

// Startup timing (logged only with FARBA_STARTUP_LOG, see startup_mark in the backend).
void invoke("startup_mark", { stage: "script" });

const app = mount(App, { target: document.getElementById("app")! });

export default app;
