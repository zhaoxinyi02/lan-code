import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

export default defineConfig({
  root: resolve(__dirname, "installer-src"),
  plugins: [react()],
  publicDir: false,
  build: {
    outDir: resolve(__dirname, "installer-dist"),
    emptyOutDir: true,
  },
});
