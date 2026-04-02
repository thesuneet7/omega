import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  // Use relative asset URLs so packaged Electron builds can load JS/CSS from file://
  base: "./",
  plugins: [react()],
  server: {
    port: 5174,
    strictPort: true
  }
});
