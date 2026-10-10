import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { tauriEngine } from "./engine";
import "./styles/tokens.css";
import "./styles/app.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App engine={tauriEngine} />
  </StrictMode>,
);
