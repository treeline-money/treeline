import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [
    svelte({
      emitCss: false, // Inline CSS into JS - required for plugins
    }),
  ],
  build: {
    lib: {
      entry: "src/index.ts",
      formats: ["es"],
      fileName: () => "index.js",
    },
    // Bundle everything including Svelte - each plugin has its own runtime
    outDir: "dist",
    emptyOutDir: true,
    cssCodeSplit: false,
  },
});
