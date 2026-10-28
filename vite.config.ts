import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  // Credentials are runtime inputs held by the OS vault, never build inputs.
  envDir: false,
  envPrefix: [],
});
