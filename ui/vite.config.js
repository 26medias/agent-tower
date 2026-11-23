import { defineConfig } from "vitest/config";
import { sveltekit } from "@sveltejs/kit/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;
// The version of the app, for the Settings panel. It is the version of `ui/package.json`, which
// has the same number as `Cargo.toml` and `src-tauri/tauri.conf.json`.
const appVersion = process.env.npm_package_version ?? "dev";

// https://vite.dev/config/
export default defineConfig(() => ({
    plugins: [sveltekit()],
    define: { __APP_VERSION__: JSON.stringify(appVersion) },

    test: {
        include: ["src/**/*.test.ts"],
        environment: "node",
    },

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
