import { createRoot } from "react-dom/client";
import { IndicatorApp } from "./indicator/IndicatorApp";
import "./indicator/indicator.css";

createRoot(document.getElementById("indicator-root")!).render(<IndicatorApp />);
