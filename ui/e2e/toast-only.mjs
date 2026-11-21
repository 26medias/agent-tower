// Runs only the checks of e2e/toast-checks.mjs on the mock page, in a headless Chromium (1280 x 800). It is the quick way to
// repeat them. The full run is `node e2e/mock-page.mjs`, which calls the same checks last.
// Usage: pnpm dev (in another shell), then: E2E_URL=http://127.0.0.1:1420 node e2e/toast-only.mjs
// Env: E2E_URL, E2E_CHROME (path of a chromium or chrome binary), E2E_TOAST_SHOTS (folder for the screenshots).
import { chromium } from "playwright-core";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { toastChecks } from "./toast-checks.mjs";

const url = process.env.E2E_URL ?? "http://127.0.0.1:1420";
const shots = process.env.E2E_TOAST_SHOTS ?? "/tmp/agent-tower-review/toast";
mkdirSync(shots, { recursive: true });

function findChrome() {
    if (process.env.E2E_CHROME) return process.env.E2E_CHROME;
    const cache = join(homedir(), ".cache", "ms-playwright");
    if (!existsSync(cache)) return undefined;
    for (const d of readdirSync(cache).filter((n) => /^chromium-\d+$/.test(n)).sort().reverse()) {
        const p = join(cache, d, "chrome-linux64", "chrome");
        if (existsSync(p)) return p;
    }
    return undefined;
}

const results = [];
function check(name, ok, detail = "") {
    results.push({ name, ok });
    console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  (" + detail + ")" : ""}`);
}

const browser = await chromium.launch({ headless: true, executablePath: findChrome() });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
try {
    await page.goto(url);
    await page.waitForSelector(".storey.on");
    await toastChecks({ page, check, url, shots });
} catch (e) {
    check("no script error", false, e.message.split("\n")[0]);
}
await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`${results.length - failed.length}/${results.length} checks passed`);
process.exit(failed.length === 0 ? 0 : 1);
