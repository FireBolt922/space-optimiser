/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and must see its own errors.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
    // The About screen imports the repository LICENSE.
    fs: { allow: [".", "../.."] },
  },
  build: {
    target: "es2022",
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/test/**", "src/vite-env.d.ts", "src/main.tsx"],
      reporter: ["text", "text-summary", "lcov"],
      reportsDirectory: "coverage",
      thresholds: { lines: 80, functions: 80, branches: 70, statements: 80 },
    },
  },
});
