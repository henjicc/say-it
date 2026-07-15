import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ContextDebugApp } from "@/context-debug/ContextDebugApp";
import "@/index.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ContextDebugApp />
  </StrictMode>,
);
