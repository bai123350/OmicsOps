import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import DesktopApp from "./DesktopApp";
import { AppearanceProvider } from "./use-appearance";
import "./styles.css";
import "./desktop-layout.css";
import "./appearance.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppearanceProvider>
      <DesktopApp />
    </AppearanceProvider>
  </StrictMode>,
);
