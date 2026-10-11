import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { tauriEngine } from "./engine";
import { applyTheme, loadTheme, resolveTheme } from "./theme";
import "./styles/tokens.css";
import "./styles/app.css";

// Set the saved theme before the first paint, so a light choice never flashes dark.
applyTheme(resolveTheme(loadTheme()));

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App engine={tauriEngine} />
  </StrictMode>,
);
