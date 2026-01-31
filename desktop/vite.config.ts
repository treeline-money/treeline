import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  build: {
    // Tauri apps load locally, so larger chunks are acceptable
    chunkSizeWarningLimit: 1000,
  },
});
