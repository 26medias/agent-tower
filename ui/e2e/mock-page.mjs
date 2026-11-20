// End-to-end check of the page with the mock backend, in a headless Chromium.
// Usage: pnpm dev (in another shell), then: pnpm e2e
// Env: E2E_URL (default http://127.0.0.1:1420), E2E_SHOT (screenshot path),
//      E2E_CHROME (path of a chromium or chrome binary; default: the Playwright cache).
import { chromium } from "playwright-core";
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const url = process.env.E2E_URL ?? "http://127.0.0.1:1420";
const shot = process.env.E2E_SHOT ?? "mock-page.png";

function findChrome() {
    if (process.env.E2E_CHROME) return process.env.E2E_CHROME;
    const cache = join(homedir(), ".cache", "ms-playwright");
    if (!existsSync(cache)) return undefined;
    const dirs = readdirSync(cache).filter((d) => /^chromium-\d+$/.test(d)).sort().reverse();
    for (const d of dirs) {
        const p = join(cache, d, "chrome-linux64", "chrome");
        if (existsSync(p)) return p;
    }
    return undefined;
}

const problems = [];
const results = [];
function check(name, ok, detail = "") {
    results.push({ name, ok, detail });
    console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  (" + detail + ")" : ""}`);
}

const browser = await chromium.launch({ headless: true, executablePath: findChrome() });
const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
page.on("console", (m) => {
    if (m.type() === "error" || m.type() === "warning") problems.push(`console ${m.type()}: ${m.text()}`);
});
page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
page.on("dialog", async (d) => {
    problems.push(`browser dialog: ${d.type()} ${d.message()}`);
    await d.dismiss();
});

const screen = () => page.locator(".pane:not(.hidden) .xterm-rows").innerText();
async function waitScreen(text, timeout = 8000) {
    await page.waitForFunction(
        (t) => document.querySelector(".pane:not(.hidden) .xterm-rows")?.textContent?.includes(t),
        text,
        { timeout },
    );
}

try {
    await page.goto(url);

    // 1. Floor and employees.
    await page.waitForSelector(".floors li.open");
    check("floor list shows the open floor", (await page.locator(".floors li.open").innerText()).includes("Demo Tower"));
    check("four employee cards", (await page.locator("article.card").count()) === 4);
    check("mock badge shown", await page.locator(".mock").isVisible());

    // 2. Start Ada: tab and terminal.
    await page.locator('article.card[data-slug="ceo"] button.primary').click();
    await page.waitForSelector(".tab.active");
    await page.waitForSelector(".pane:not(.hidden) .xterm");
    check("a tab and a terminal exist after start", (await page.locator(".tab").count()) === 1);
    await waitScreen("Agent Tower mock terminal");
    check("scrollback (banner) is written", true);
    await waitScreen("channel online ✓ café");
    check("live output with a UTF-8 character split over two chunks is correct", true);

    // 3. Typing reaches the mock echo, including UTF-8.
    await page.locator(".pane:not(.hidden) .xterm-helper-textarea").focus();
    await page.keyboard.type("héllo €");
    await waitScreen("héllo €");
    check("typed text is echoed", true);
    await page.keyboard.press("Enter");
    await waitScreen('you typed: "héllo €"');
    check("Enter reaches the mock (line returned with UTF-8 intact)", true);
    await page.keyboard.press("ArrowUp");
    await waitScreen("^[[A");
    check("an arrow key goes through as ESC [ A", true);

    // 4. Status dot.
    await page.waitForSelector(".tab.active .dot[data-status=idle]");
    check("tab shows a status dot that follows session-changed (starting -> idle)", true);

    // 5. Draft flag, refusal, "message waiting" mark, clear_draft, and a paste that goes through.
    await page.keyboard.type("draft text");
    await page.waitForSelector(".tab.active .mark.draft");
    check('typed text sets the "draft" mark', true);
    await page.fill("#debug-paste", "hello from debug paste");
    await page.click(".debug button[type=submit]");
    await page.waitForSelector(".debug .result.refused");
    const refusal = await page.locator(".debug .result").innerText();
    check("debug paste shows the refusal text", refusal === "REFUSED: draft", refusal);
    await page.waitForSelector(".tab.active .mark.waiting");
    check('"message waiting" mark appears', true);
    await page.click(".tab.active .mark.waiting");
    await page.waitForSelector(".tab.active .mark.waiting", { state: "detached" });
    check("click on the mark calls clear_draft (mark goes away, held message delivered)", true);
    await waitScreen("[held message delivered]");
    await page.fill("#debug-paste", "second paste");
    await page.click(".debug button[type=submit]");
    await page.waitForSelector(".debug .result.ok");
    await waitScreen("[paste] second paste");
    check("debug paste in a clean idle session goes through", true);

    // 6. Second session; switching tabs loses no output.
    await page.locator('article.card[data-slug="pm"] button.primary').click();
    await page.waitForSelector(".tab:nth-child(2).active");
    await waitScreen("Agent Tower mock terminal");
    await page.locator(".tab", { hasText: "Ada" }).locator("button.main").click();
    await waitScreen('you typed: "héllo €"');
    check("switching back to the first tab keeps its output", (await page.locator(".pane").count()) === 2);

    // 7. Refit on resize.
    const rowsBefore = await page.locator(".pane:not(.hidden) .xterm-rows > div").count();
    await page.setViewportSize({ width: 1000, height: 560 });
    await page.waitForTimeout(500);
    const rowsAfter = await page.locator(".pane:not(.hidden) .xterm-rows > div").count();
    check("terminal refits when the window shrinks", rowsAfter < rowsBefore, `rows ${rowsBefore} -> ${rowsAfter}`);
    await page.setViewportSize({ width: 1200, height: 800 });
    await page.waitForTimeout(400);

    const calls = await page.evaluate(() => window.__towerMockCalls);
    const resizes = calls.filter((c) => c.command === "pty_resize");
    check(
        "pty_resize was called with the new size after the window shrank",
        resizes.some((r) => r.args.rows === rowsAfter),
        `rows after = ${rowsAfter}; sent = ${resizes.map((r) => r.args.rows + "x" + r.args.cols).join(" ")}`,
    );
    const attaches = calls.filter((c) => c.command === "session_attach");
    check(
        "session_attach was called once for each session",
        attaches.length === 2 && new Set(attaches.map((c) => c.args.sessionId)).size === 2,
        `attaches = ${attaches.length}`,
    );
    const start = calls.find((c) => c.command === "session_start");
    check(
        "session_start got numeric rows and cols and a null firstMessage",
        typeof start?.args.rows === "number" && typeof start?.args.cols === "number" && start.args.firstMessage === null,
        JSON.stringify(start?.args),
    );
    const writes = calls.filter((c) => c.command === "pty_write").map((c) => Buffer.from(c.args.data, "base64"));
    check(
        "pty_write carried the UTF-8 bytes of the typed text",
        Buffer.concat(writes).toString("utf8").includes("h\u00e9llo \u20ac\r"),
    );

    // 8. Stop, tab stays (stopped), resume offered.
    await page.locator('article.card[data-slug="pm"] button.danger').click();
    await page.waitForSelector('article.card[data-slug="pm"] button:has-text("resume")');
    check("stopped employee offers resume", true);
    await page.locator('article.card[data-slug="eng-lin"] button:has-text("resume")').click();
    await page.waitForSelector(".tab:has-text('Lin')");
    check("resume of a stopped employee opens a tab", true);

    // 9. Add employee form.
    await page.click("button.add");
    await page.fill('input[name="name"]', "Zed Zebra");
    check("slug is made from the name", (await page.locator(".slug code").innerText()) === "zed-zebra");
    await page.selectOption('select[name="cli"]', "codex");
    check(
        "model placeholder follows the CLI",
        (await page.locator('input[name="model"]').getAttribute("placeholder")) === "gpt-6-luna",
    );
    await page.click('form[aria-label="Add employee"] button.primary');
    await page.waitForSelector('article.card[data-slug="zed-zebra"]');
    check("new employee card appears", true);

    // 10. NO_FLOOR flow.
    await page.fill("#floor-path", "/home/demo/new-project");
    await page.click(".open button.primary");
    await page.waitForSelector(".create");
    check("NO_FLOOR offers create", (await page.locator(".create").innerText()).includes("/home/demo/new-project"));
    await page.click(".create button.primary");
    await page.waitForSelector('.floors li.open:has-text("new-project")');
    check("create opens the new floor with 2 employees", (await page.locator("article.card").count()) === 2);

    // 11. Error toast, no dialog.
    await page.fill("#floor-path", "");
    await page.fill("#floor-path", "   ");
    await page.click(".open button.primary");
    await page.waitForSelector(".toast");
    check("an error shows a toast", true);

    await page.locator(".floors li", { hasText: "Demo Tower" }).locator("button.floor").click();
    await page.waitForSelector('.floors li.open:has-text("Demo Tower")');
    await page.waitForSelector(".tab");
    await page.waitForTimeout(600);
    await page.screenshot({ path: shot });
    console.log("screenshot:", shot);
} catch (e) {
    problems.push(`script error: ${e.message}`);
    await page.screenshot({ path: shot.replace(/\.png$/, "-fail.png") }).catch(() => {});
}

await browser.close();
// A toast with the NO_FLOOR text is not a console error. Anything in `problems` is a failure.
check("no console errors, page errors or browser dialogs", problems.length === 0, problems.join(" | "));
const failed = results.filter((r) => !r.ok);
console.log(`${results.length - failed.length}/${results.length} checks passed`);
process.exit(failed.length === 0 ? 0 : 1);
