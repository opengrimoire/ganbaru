import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "path";

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: path.resolve("./src/lib"),
    },
    conditions: ["browser"],
  },
  test: {
    include: ["src/**/*.test.ts"],
    setupFiles: ["./src/test-setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      include: ["src/lib/**/*.ts"],
      exclude: ["**/*.test.ts", "**/*.d.ts"],
    },
  },
});
