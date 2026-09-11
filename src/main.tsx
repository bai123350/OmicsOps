import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import DesktopApp from "./DesktopApp";
import "./styles.css";
import "./desktop-layout.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <DesktopApp />
  </StrictMode>,
);
