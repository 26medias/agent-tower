import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
    plugins: [sveltekit()],

    // Vite options for Tauri, applied in `tauri dev` and `tauri build`.
    //
    // 1. Do not hide Rust errors.
    clearScreen: false,
    // 2. Tauri expects a fixed port. Fail if the port is busy.
    server: {
        port: 1420,
        strictPort: true,
        host: host || "127.0.0.1",
        hmr: host
            ? {
                  protocol: "ws",
                  host,
                  port: 1421,
              }
            : undefined,
        watch: {
            // 3. Do not watch src-tauri.
            ignored: ["**/src-tauri/**"],
        },
    },
}));
