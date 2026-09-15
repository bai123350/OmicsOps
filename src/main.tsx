import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import DesktopApp from "./DesktopApp";
import { PetCompanion } from "./features/workspace/PetCompanion";
import { AppearanceProvider } from "./use-appearance";
import { PetPreferencesProvider } from "./use-pet-preferences";
import "./styles.css";
import "./desktop-layout.css";
import "./appearance.css";
import "./pet.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppearanceProvider>
      <PetPreferencesProvider>
        <DesktopApp />
        <PetCompanion />
      </PetPreferencesProvider>
    </AppearanceProvider>
  </StrictMode>,
);
