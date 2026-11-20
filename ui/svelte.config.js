// Tauri has no Node.js server, so we use adapter-static with a fallback to index.html (SPA mode).
// See: https://v2.tauri.app/start/frontend/sveltekit/
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
    preprocess: vitePreprocess(),
    kit: {
        adapter: adapter({
            fallback: "index.html",
        }),
    },
};

export default config;
