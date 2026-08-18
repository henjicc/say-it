import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: { alias: { "@": `${root}ui/src` } },
  test: {
    environment: "jsdom",
    setupFiles: ["ui/src/test/setup.ts"],
    include: ["ui/src/**/*.test.{ts,tsx}"],
    clearMocks: true,
  },
});
