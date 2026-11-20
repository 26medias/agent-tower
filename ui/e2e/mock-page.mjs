// End-to-end check of the page with the mock backend, in a headless Chromium (1280 x 800).
// Usage: pnpm dev (in another shell), then: pnpm e2e
// Env: E2E_URL (default http://127.0.0.1:1420), E2E_SHOT (screenshot path; the 5 and 13 employee
//      screenshots go next to it with the suffixes -5 and -13),
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
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
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

// ---- helpers for the building and the office ----

const emp = (slug) => page.locator(`.emp[data-slug="${slug}"]`);
const selectEmp = (slug) => emp(slug).locator(".hit").click();
const bar = page.locator(".bar");
const calls = (command) => page.evaluate((c) => window.__towerMockCalls.filter((x) => x.command === c), command);

// A click goes to a point on the front wall of a storey, through the real hit test of the page.
// (A click on the centre of the box would hit the storey above, which covers the box.)
async function storeyPoint(path) {
    return page.evaluate((path) => {
        const svg = document.querySelector("svg.building");
        const g = document.querySelector(`.storey[data-path="${path}"]`);
        const n = Number(svg.dataset.storeys);
        const i = Number(g.dataset.index);
        const p = svg.createSVGPoint();
        p.x = 1100;
        p.y = (n - 1 - i) * 300 + 700;
        const q = p.matrixTransform(svg.getScreenCTM());
        return { x: q.x, y: q.y };
    }, path);
}
async function clickStorey(path) {
    const pt = await storeyPoint(path);
    await page.mouse.click(pt.x, pt.y);
}

// The dim of a storey is a dark overlay polygon (fill-opacity). WebKitGTK does not draw a CSS
// `filter` on SVG children, so the checks read the overlay, never a filter.
const shade = (path) =>
    page.evaluate((p) => {
        const el = document.querySelector(`.storey[data-path="${p}"] polygon.shade`);
        return el ? Number(getComputedStyle(el).fillOpacity) : null;
    }, path);
const approx = (a, b) => a !== null && Math.abs(a - b) < 0.02;

// Feet of each character in SVG units, read from the sprite images in the DOM.
const anchors = () =>
    page.evaluate(() =>
        [...document.querySelectorAll(".emp")].map((g) => {
            const img = g.querySelector("image.sprite");
            const x = Number(img.getAttribute("x"));
            const y = Number(img.getAttribute("y"));
            const w = Number(img.getAttribute("width"));
            const h = Number(img.getAttribute("height"));
            return { slug: g.dataset.slug, x: x + w / 2, y: y + h };
        }),
    );

// The floor diamond of the room art (numbers of prototype/building.html), solved for (u, v).
function insideDiamond({ x, y }) {
    const back = [768, 345], right = [1430, 645], left = [105, 645];
    const ax = right[0] - back[0], ay = right[1] - back[1], bx = left[0] - back[0], by = left[1] - back[1];
    const det = ax * by - ay * bx;
    const dx = x - back[0], dy = y - back[1];
    const u = (dx * by - dy * bx) / det, v = (ax * dy - ay * dx) / det;
    return u >= 0 && u <= 1 && v >= 0 && v <= 1;
}

async function openDebug() {
    if ((await page.locator("details.debug-area[open]").count()) === 0) await page.click("details.debug-area > summary");
}

try {
    await page.goto(url);

    // 1. The building and the office.
    await page.waitForSelector(".storey.on");
    check("the open floor is the first floor, Demo Tower", (await page.locator(".storey.on").getAttribute("data-path")) === "/home/demo/tower");
    check("the building has one storey for each floor (4 in the mock list)", (await page.locator(".storey").count()) === 4);
    check(
        "the storey count matches the floor list and the viewBox height",
        (await page.locator(".floors .list li").count()) === 4 &&
            (await page.locator("svg.building").getAttribute("viewBox")) === "0 0 1536 1924",
    );
    check(
        "the first floor is at the bottom (each next storey is higher)",
        await page.evaluate(() => {
            const ys = [...document.querySelectorAll(".storey")].map((g) => g.querySelector("image").getBoundingClientRect().top);
            return ys.every((y, i) => i === 0 || y < ys[i - 1]);
        }),
    );
    check(
        "a missing floor is marked and cannot be opened",
        (await page.locator('.storey.missing[data-path="/home/demo/gone"]').getAttribute("aria-disabled")) === "true",
    );
    check(
        "the office header shows the name, the path and the description",
        (await page.locator(".office .name").innerText()) === "Demo Tower" &&
            (await page.locator(".office .path").innerText()) === "/home/demo/tower" &&
            (await page.locator(".office .description").innerText()).includes("demo project"),
    );
    check(
        "the office has one desk and one character for each employee (4)",
        (await page.locator(".emp").count()) === 4 &&
            (await page.locator('.emp image[href$="desk-back.webp"]').count()) === 4 &&
            (await page.locator(".emp image.sprite").count()) === 4,
    );
    const byIndex = await page.evaluate(() =>
        [...document.querySelectorAll(".emp")].sort((a, b) => a.dataset.index - b.dataset.index).map((g) => g.dataset.slug),
    );
    check(
        "employees are placed in hierarchy order (root, reports, depth first, ties by name)",
        byIndex.join(",") === "ceo,pm,eng-lin,eng-sam",
        byIndex.join(","),
    );
    check(
        "CEO and PM get sprite 0 and 1",
        (await emp("ceo").locator("image.sprite").getAttribute("href")) === "/art/agent-0.webp" &&
            (await emp("pm").locator("image.sprite").getAttribute("href")) === "/art/agent-1.webp",
    );
    check("a character without a live session has the status stopped", (await emp("ceo").getAttribute("data-status")) === "stopped");
    const spriteStyle = (slug) =>
        emp(slug)
            .locator("image.sprite")
            .evaluate((el) => ({ opacity: Number(getComputedStyle(el).opacity), filter: getComputedStyle(el).filter }));
    const stoppedStyle = await spriteStyle("ceo");
    check(
        "a stopped character is drawn with a plain opacity of 0.45 and no CSS filter",
        approx(stoppedStyle.opacity, 0.45) && stoppedStyle.filter === "none",
        JSON.stringify(stoppedStyle),
    );
    await page.waitForTimeout(350); // The overlay fades with a short transition after the first paint.
    const dims = { open: await shade("/home/demo/tower"), dim: await shade("/home/demo/second"), missing: await shade("/home/demo/gone") };
    check(
        "storey overlays: none on the open floor, dim on the others, stronger on the missing floor",
        approx(dims.open, 0) && approx(dims.dim, 0.5) && dims.missing >= 0.7,
        JSON.stringify(dims),
    );
    check(
        "no storey and no overlay uses a CSS filter, and the overlay is a visible dark fill that takes no pointer events",
        await page.evaluate(() =>
            [...document.querySelectorAll(".storey, .storey *")].every((el) => getComputedStyle(el).filter === "none") &&
            [...document.querySelectorAll("polygon.shade")].length === 4 &&
            [...document.querySelectorAll("polygon.shade")].every((el) => {
                const cs = getComputedStyle(el);
                return cs.fill !== "none" && cs.pointerEvents === "none" && el.tagName === "polygon";
            }),
        ),
    );
    const hoverAt = await storeyPoint("/home/demo/second");
    await page.mouse.move(hoverAt.x, hoverAt.y);
    await page.waitForTimeout(350);
    const hovered = await shade("/home/demo/second");
    check("hover makes the overlay of a storey lighter (0.25)", approx(hovered, 0.25), String(hovered));
    await page.mouse.move(2, 2);
    await page.waitForTimeout(350);
    check("the overlay is back to dim when the pointer leaves", approx(await shade("/home/demo/second"), 0.5));
    check("the name tag shows the name", (await page.locator('.tag[data-slug="ceo"] .tag-name').textContent()) === "Ada");
    check("mock badge shown", await page.locator(".mock").isVisible());
    check("the debug area is collapsed", (await page.locator("details.debug-area[open]").count()) === 0);
    const box1280 = { office: await page.locator(".office").boundingBox(), area: await page.locator(".area").boundingBox() };
    check("at 1280 x 800 the office is at most 42% of the height", box1280.office.height <= 800 * 0.42 + 1, `office ${box1280.office.height}`);
    check("at 1280 x 800 the terminal area has at least half of the height", box1280.area.height >= 400, `area ${box1280.area.height}`);

    // 1b. A click on a storey opens that floor (through the real hit test).
    await clickStorey("/home/demo/second");
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check(
        "a click on a storey opens that floor",
        (await page.locator(".office .name").innerText()) === "Second Floor" && (await page.locator(".emp").count()) === 1,
    );
    const goneOpens = async () => (await calls("floor_open")).filter((c) => c.args.path === "/home/demo/gone").length;
    const goneBefore = await goneOpens();
    await clickStorey("/home/demo/gone");
    await page.waitForTimeout(150);
    check("a click on the missing storey does nothing", (await goneOpens()) === goneBefore);
    await clickStorey("/home/demo/tower");
    await page.waitForSelector('.storey.on[data-path="/home/demo/tower"]');
    check("a click on the first storey (the bottom one) opens it", (await page.locator(".emp").count()) === 4);

    // 1c. 13 employees: every character anchor is inside the floor diamond, and every character can be clicked.
    await clickStorey("/home/demo/big");
    await page.waitForSelector('.storey.on[data-path="/home/demo/big"]');
    await page.waitForFunction(() => document.querySelectorAll(".emp").length === 13);
    const a13 = await anchors();
    check(
        "the big office has 13 characters and 13 desks",
        a13.length === 13 && (await page.locator('.emp image[href$="desk-back.webp"]').count()) === 13,
    );
    const outside = a13.filter((a) => !insideDiamond(a));
    check(
        "with 13 employees every character anchor is inside the floor diamond",
        outside.length === 0,
        outside.map((a) => `${a.slug}@${a.x.toFixed(0)},${a.y.toFixed(0)}`).join(" "),
    );
    check("the 13 anchors are all different", new Set(a13.map((a) => `${Math.round(a.x)},${Math.round(a.y)}`)).size === 13);
    const covered = await page.evaluate(() =>
        [...document.querySelectorAll(".emp")]
            .filter((g) => {
                const r = g.querySelector(".hit").getBoundingClientRect();
                const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
                const owner = hit?.closest(".emp, .tag");
                return !owner || owner.dataset.slug !== g.dataset.slug;
            })
            .map((g) => g.dataset.slug),
    );
    check("with 13 employees the centre of each character reaches that character (or its tag)", covered.length === 0, covered.join(","));
    const textH = await page.evaluate(() => document.querySelector(".tag .tag-name").getBoundingClientRect().height);
    check("with 13 employees the name text is readable (at least 8 px tall on the screen)", textH >= 8, `text box height ${textH.toFixed(1)} px`);
    check("a long name in the tag is cut", (await page.locator('.tag[data-slug="des-2"] .tag-name').textContent()).endsWith("…"));
    await page.screenshot({ path: shot.replace(/\.png$/, "-13.png") });
    await clickStorey("/home/demo/tower");
    await page.waitForSelector('.storey.on[data-path="/home/demo/tower"]');
    await page.waitForFunction(() => document.querySelectorAll(".emp").length === 4);

    // 2. Select Ada, start her: tab and terminal.
    await selectEmp("ceo");
    const barText = [await bar.locator(".name").innerText(), await bar.locator(".role").innerText(), await bar.locator(".cli").textContent(), await bar.locator(".model").innerText()];
    check("a click on a character shows the action bar of that employee", barText.join("|") === "Ada|CEO|claude|haiku", barText.join("|"));
    check("a click on a character without a session makes no tab", (await page.locator(".tab").count()) === 0);
    await bar.locator("button.start").click();
    await page.waitForSelector(".tab.active");
    await page.waitForSelector(".pane:not(.hidden) .xterm");
    check("a tab and a terminal exist after start", (await page.locator(".tab").count()) === 1);
    await waitScreen("Agent Tower mock terminal");
    check("scrollback (banner) is written", (await screen()).includes("Agent Tower mock terminal"));
    await waitScreen("channel online ✓ café");
    check("live output with a UTF-8 character split over two chunks is correct", (await screen()).includes("channel online ✓ café"));

    // 3. Typing reaches the mock echo, including UTF-8.
    await page.locator(".pane:not(.hidden) .xterm-helper-textarea").focus();
    await page.keyboard.type("héllo €");
    await waitScreen("héllo €");
    check("typed text is echoed", (await screen()).includes("héllo €"));
    await page.keyboard.press("Enter");
    await waitScreen('you typed: "héllo €"');
    check("Enter reaches the mock (line returned with UTF-8 intact)", (await screen()).includes('you typed: "héllo €"'));
    await page.keyboard.press("ArrowUp");
    await waitScreen("^[[A");
    check("an arrow key goes through as ESC [ A", (await screen()).includes("^[[A"));

    // 4. Status dot, and the same status on the character.
    await page.waitForSelector(".tab.active .dot[data-status=idle]");
    check(
        "tab shows a status dot that follows session-changed (starting -> idle)",
        (await page.locator(".tab.active .dot").getAttribute("data-status")) === "idle",
    );
    check(
        "the character of a live session is drawn at full opacity",
        approx((await spriteStyle("ceo")).opacity, 1),
        JSON.stringify(await spriteStyle("ceo")),
    );
    check(
        "the character of a live session shows a live status (not stopped)",
        (await emp("ceo").getAttribute("data-status")) !== "stopped",
        (await emp("ceo").getAttribute("data-status")) ?? "",
    );

    // 5. Draft flag, refusal, "message waiting" mark, clear_draft, and a paste that goes through.
    await page.keyboard.type("draft text");
    await page.waitForSelector(".tab.active .mark.draft");
    check('typed text sets the "draft" mark', (await page.locator(".tab.active .mark.draft").count()) === 1);
    await openDebug();
    await page.fill("#debug-paste", "hello from debug paste");
    await page.click(".debug button[type=submit]");
    await page.waitForSelector(".debug .result.refused");
    const refusal = await page.locator(".debug .result").innerText();
    check("debug paste shows the refusal text", refusal === "REFUSED: draft", refusal);
    await page.waitForSelector(".tab.active .mark.waiting");
    const waitTitle = (await page.locator(".tab.active .mark.waiting").getAttribute("title")) ?? "";
    check(
        '"message waiting" mark appears, with the agreed tooltip',
        waitTitle === "A message waits. Click to clear your draft flag so it can be delivered.",
        waitTitle,
    );
    await page.click(".tab.active .mark.waiting");
    await page.waitForSelector(".tab.active .mark.waiting", { state: "detached" });
    check("click on the mark calls clear_draft (mark goes away)", (await page.locator(".tab.active .mark.waiting").count()) === 0 && (await calls("clear_draft")).length >= 1);
    await waitScreen("[held message delivered]");
    check("the held message was delivered after clear_draft", (await screen()).includes("[held message delivered]"));
    await page.fill("#debug-paste", "second paste");
    await page.click(".debug button[type=submit]");
    await page.waitForSelector(".debug .result.ok");
    await waitScreen("[paste] second paste");
    check("debug paste in a clean idle session goes through", (await screen()).includes("[paste] second paste"));

    // The "draft" mark is clickable too (the flag can be wrong).
    await page.locator(".pane:not(.hidden) .xterm-helper-textarea").focus();
    await page.keyboard.type("zz");
    await page.waitForSelector(".tab.active .mark.draft");
    const draftTitle = (await page.locator(".tab.active .mark.draft").getAttribute("title")) ?? "";
    check("draft mark has its own tooltip", draftTitle.includes("Click to clear the flag"), draftTitle);
    const mark = await page.locator(".tab.active .mark.draft").evaluate((el) => el.tagName);
    check("draft mark is a button", mark === "BUTTON");
    await page.click(".tab.active .mark.draft");
    await page.waitForSelector(".tab.active .mark.draft", { state: "detached" });
    check("a click on the draft mark clears the flag (mark goes away on the event)", (await page.locator(".tab.active .mark.draft").count()) === 0);

    // 6. Second session; switching tabs loses no output; a click on a character picks its tab.
    await selectEmp("pm");
    await bar.locator("button.start").click();
    await page.waitForSelector(".tab:nth-child(2).active");
    await waitScreen("Agent Tower mock terminal");
    await page.locator(".tab", { hasText: "Ada" }).locator("button.main").click();
    await waitScreen('you typed: "héllo €"');
    check("switching back to the first tab keeps its output", (await page.locator(".pane").count()) === 2);
    await selectEmp("pm");
    check("a click on a character of a live session makes its tab active (Maya)", (await page.locator(".tab.active").innerText()).includes("Maya"));
    await selectEmp("ceo");
    await waitScreen('you typed: "héllo €"');
    check("a click on another live character switches the tab and the terminal (Ada)", (await page.locator(".tab.active").innerText()).includes("Ada"));
    check(
        "the action bar follows the selection (Ada, stop offered)",
        (await bar.locator(".name").innerText()) === "Ada" && (await bar.locator("button.stop").count()) === 1,
    );

    // 7. Refit on resize.
    const rowsBefore = await page.locator(".pane:not(.hidden) .xterm-rows > div").count();
    await page.setViewportSize({ width: 1000, height: 560 });
    await page.waitForTimeout(500);
    const rowsAfter = await page.locator(".pane:not(.hidden) .xterm-rows > div").count();
    check("terminal refits when the window shrinks", rowsAfter < rowsBefore, `rows ${rowsBefore} -> ${rowsAfter}`);
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.waitForTimeout(400);

    const allCalls = await page.evaluate(() => window.__towerMockCalls);
    const resizes = allCalls.filter((c) => c.command === "pty_resize");
    check(
        "pty_resize was called with the new size after the window shrank",
        resizes.some((r) => r.args.rows === rowsAfter),
        `rows after = ${rowsAfter}; sent = ${resizes.map((r) => r.args.rows + "x" + r.args.cols).join(" ")}`,
    );
    const attaches = allCalls.filter((c) => c.command === "session_attach");
    check(
        "session_attach was called once for each session",
        attaches.length === 2 && new Set(attaches.map((c) => c.args.sessionId)).size === 2,
        `attaches = ${attaches.length}`,
    );
    const start = allCalls.find((c) => c.command === "session_start");
    check(
        "session_start got numeric rows and cols and a null firstMessage",
        typeof start?.args.rows === "number" && typeof start?.args.cols === "number" && start.args.firstMessage === null,
        JSON.stringify(start?.args),
    );
    const writes = allCalls.filter((c) => c.command === "pty_write").map((c) => Buffer.from(c.args.data, "base64"));
    check(
        "pty_write carried the UTF-8 bytes of the typed text",
        Buffer.concat(writes).toString("utf8").includes("héllo €\r"),
    );

    // 7b. A start that is rejected: toast, no tab, bar back to normal. Then it works.
    const tabsBefore = await page.locator(".tab").count();
    const panesBefore = await page.locator(".pane").count();
    await selectEmp("eng-sam");
    await bar.locator("button.start").click();
    await page.waitForSelector(".toast");
    const toastText = await page.locator(".toast .text").first().innerText();
    check("a rejected start shows its error text in a toast", toastText.includes("cannot start eng-sam"), toastText);
    const toastBox = await page.locator(".toast").first().boundingBox();
    const officeBox = await page.locator(".office").boundingBox();
    const debugBox = await page.locator(".debug-area").boundingBox();
    check(
        "the toast does not cover the office or the debug paste bar",
        toastBox.y + toastBox.height <= officeBox.y + 1 && toastBox.y + toastBox.height < debugBox.y,
        `toast y=${toastBox.y} h=${toastBox.height}, office y=${officeBox.y}, debug y=${debugBox.y}`,
    );
    check(
        "after a rejected start: no new tab, no new terminal, no session state on the bar",
        (await page.locator(".tab").count()) === tabsBefore &&
            (await page.locator(".pane").count()) === panesBefore &&
            (await bar.locator(".state").innerText()) === "no session",
    );
    check("after a rejected start the start button is enabled again", await bar.locator("button.start").isEnabled());
    await bar.locator("button.start").click();
    await page.waitForSelector(".tab:has-text('Sam')");
    check("the second start works and opens a tab", (await page.locator(".tab").count()) === tabsBefore + 1);
    await page.locator(".tab", { hasText: "Ada" }).locator("button.main").click();
    await waitScreen('you typed: "héllo €"');

    // 8. Stop, tab stays (stopped), resume offered.
    await selectEmp("pm");
    await bar.locator("button.stop").click();
    await page.waitForSelector(".bar button.resume");
    check("stopped employee offers resume", (await page.locator(".bar button.resume").count()) === 1);
    check("a stopped character has the status stopped again", (await emp("pm").getAttribute("data-status")) === "stopped");
    const tabsBeforeResume = await page.locator(".tab").count();
    await bar.locator("button.resume").click();
    await page.waitForFunction(() => [...document.querySelectorAll(".toast .text")].some((t) => t.textContent.includes("never had a turn")));
    check(
        "a rejected resume shows its error text in a toast",
        (await page.locator(".toast .text").allInnerTexts()).some((t) => t.includes("never had a turn")),
    );
    check(
        "after a rejected resume: same tabs, the bar still offers start and resume",
        (await page.locator(".tab").count()) === tabsBeforeResume &&
            (await bar.locator("button.resume").isEnabled()) &&
            (await bar.locator("button.start").isEnabled()),
    );
    await selectEmp("eng-lin");
    await bar.locator("button.resume").click();
    await page.waitForSelector(".tab:has-text('Lin')");
    check("resume of a stopped employee opens a tab", (await page.locator(".tab:has-text('Lin')").count()) === 1);

    // 9. Add employee form, edit and remove from the bar.
    await page.click(".bar button.add");
    await page.fill('input[name="name"]', "Zed Zebra");
    check("slug is made from the name", (await page.locator(".slug code").innerText()) === "zed-zebra");
    await page.selectOption('select[name="cli"]', "codex");
    check(
        "model placeholder follows the CLI",
        (await page.locator('input[name="model"]').getAttribute("placeholder")) === "gpt-6-luna",
    );
    await page.click('form[aria-label="Add employee"] button.primary');
    await page.waitForSelector('.emp[data-slug="zed-zebra"]');
    check(
        "new employee appears in the office (5 desks, 5 characters)",
        (await page.locator(".emp").count()) === 5 && (await page.locator('.emp image[href$="desk-back.webp"]').count()) === 5,
    );
    check("with 5 employees every character anchor is inside the floor diamond", (await anchors()).every(insideDiamond));
    const sprite = await emp("zed-zebra").locator("image.sprite").getAttribute("href");
    check("another role gets a sprite from 2 to 5", /agent-[2-5]\.webp$/.test(sprite ?? ""), sprite ?? "");
    while ((await page.locator(".toast button").count()) > 0) await page.locator(".toast button").first().click();
    if ((await page.locator("details.debug-area[open]").count()) > 0) await page.click("details.debug-area > summary");
    await page.waitForTimeout(100);
    const officeBox2 = await page.locator(".office").boundingBox();
    const areaBox2 = await page.locator(".area").boundingBox();
    check(
        "the terminal keeps at least half of the height with 5 employees",
        officeBox2.height <= 337 && areaBox2.height >= 400,
        `office ${officeBox2.height}, terminal area ${areaBox2.height}`,
    );
    await page.waitForTimeout(300);
    await page.screenshot({ path: shot.replace(/\.png$/, "-5.png") });
    await selectEmp("zed-zebra");
    await bar.locator("button.edit").click();
    await page.waitForSelector('form[aria-label="Edit employee"]');
    check("edit opens the form with the employee", (await page.locator('input[name="name"]').inputValue()) === "Zed Zebra");
    await page.keyboard.press("Escape");
    await bar.locator("button.remove").click();
    await bar.locator("button.remove").click();
    await page.waitForSelector('.emp[data-slug="zed-zebra"]', { state: "detached" });
    check("remove (two clicks) takes the employee away", (await page.locator(".emp").count()) === 4);

    // 10. NO_FLOOR flow.
    await page.fill("#floor-path", "/home/demo/new-project");
    await page.click(".open button.primary");
    await page.waitForSelector(".create");
    check("NO_FLOOR offers create", (await page.locator(".create").innerText()).includes("/home/demo/new-project"));
    await page.click(".create button.primary");
    await page.waitForSelector('.storey.on[data-path="/home/demo/new-project"]');
    check("create opens the new floor with 2 employees", (await page.locator(".emp").count()) === 2);
    check("the building grew by one storey", (await page.locator(".storey").count()) === 5);
    check(
        "a new floor offers the setup agent",
        (await page.locator("button.setup.primary").count()) === 1 && (await page.locator('.office .hint:has-text("setup agent")').count()) === 1,
    );

    // 10b. The floor is keyed by FloorView.path, not by the typed text.
    await page.fill("#floor-path", "~/tower/../second/");
    await page.click(".open button.primary");
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check(
        "a typed path with ~, .. and a trailing slash opens the canonical floor",
        (await page.locator(".office .path").innerText()) === "/home/demo/second",
    );
    check(
        "the building has no second storey for the typed text",
        (await page.locator('.storey[data-path="/home/demo/second"]').count()) === 1 &&
            (await page.locator('.storey[data-path*="~"]').count()) === 0,
    );
    await selectEmp("ceo");
    await bar.locator("button.start").click();
    await page.waitForSelector(".tab.active .dot[data-status=idle]");
    check(
        "session-changed events with the canonical path reach the open floor (tab dot goes to idle)",
        (await page.locator(".tab.active .dot").getAttribute("data-status")) === "idle",
    );
    const startCall = (await calls("session_start")).pop();
    check("session_start used the canonical path", startCall.args.path === "/home/demo/second", startCall.args.path);
    const openCountBefore = (await calls("floor_open")).length;
    await page.click(".bar button.add");
    await page.fill('input[name="name"]', "Cy");
    await page.click('form[aria-label="Add employee"] button.primary');
    await page.waitForSelector('.emp[data-slug="cy"]');
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "floor_open").length > n, openCountBefore);
    check(
        "floor-changed with the canonical path makes the UI reload the open floor",
        (await calls("floor_open")).length > openCountBefore,
    );
    await page.fill("#floor-path", "/home/demo/link");
    await page.click(".open button.primary");
    await page.waitForSelector('.storey.on[data-path="/home/demo/tower"]');
    check("a symlink path opens the real floor", (await page.locator(".office .path").innerText()) === "/home/demo/tower");
    await page.waitForSelector(".tab:has-text('Ada')");

    // 11. Error toast, no dialog.
    await page.fill("#floor-path", "");
    await page.fill("#floor-path", "   ");
    await page.click(".open button.primary");
    await page.waitForSelector(".toast");
    check("an error shows a toast", (await page.locator(".toast").count()) >= 1);

    // 12. Logo: a floor without a logo, "New logo", the NO_KEY path, then the busy state and a logo.
    await clickStorey("/home/demo/second");
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check(
        "a floor without a logo shows the placeholder sticker with the first letter",
        (await page.locator('.storey[data-path="/home/demo/second"] .plain').count()) === 1 &&
            (await page.locator('.storey[data-path="/home/demo/second"] .letter').textContent()) === "S",
    );
    check("a floor with a logo shows the logo image on the wall", (await page.locator('.storey[data-path="/home/demo/tower"] image.logo').count()) === 1);
    check("the header shows no logo image when there is no logo", (await page.locator(".office .logo img").count()) === 0);
    await page.click("button.newlogo");
    check("New logo shows a field with the stored prompt (empty here)", (await page.locator("#logo-prompt").inputValue()) === "");
    await page.fill("#logo-prompt", "A small blue boat.");
    await page.click(".logoform button.primary");
    await page.waitForSelector('[data-error="logo"]');
    const noKeyText = await page.locator('[data-error="logo"]').innerText();
    check(
        "without a key the error starts with NO_KEY: and the busy state ends",
        noKeyText.startsWith("NO_KEY:") && (await page.locator(".office .logo.busy").count()) === 0,
        noKeyText,
    );
    check("the NO_KEY error shows the OpenAI key field", await page.locator("#openai-key").isVisible());
    check("the key field is a password field", (await page.locator("#openai-key").getAttribute("type")) === "password");
    // The NO_KEY text is the error of the call. It goes away when the operator leaves the floor.
    await clickStorey("/home/demo/tower");
    await page.waitForSelector('.storey.on[data-path="/home/demo/tower"]');
    await clickStorey("/home/demo/second");
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check(
        "a NO_KEY call error is gone after the operator opens another floor and comes back",
        (await page.locator('[data-error="logo"]').count()) === 0 && (await page.locator("#openai-key").count()) === 0,
    );
    await page.click("button.newlogo");
    check("the prompt that was sent is stored (the field shows it)", (await page.locator("#logo-prompt").inputValue()) === "A small blue boat.");
    await page.click(".logoform button.primary");
    await page.waitForSelector('[data-error="logo"]');
    check("generate without a key shows NO_KEY again", (await page.locator("#openai-key").isVisible()));
    const gen1 = (await calls("logo_generate"))[0];
    check("logo_generate got the path and the prompt", gen1.args.path === "/home/demo/second" && gen1.args.prompt === "A small blue boat.", JSON.stringify(gen1.args));
    await page.fill("#openai-key", "sk-test-not-a-real-key");
    await page.click(".keyform button.primary");
    await page.waitForSelector(".office .logo.busy");
    check("saving the key starts the generation again (busy state)", (await page.locator(".office .logo.busy").count()) === 1);
    const setCall = (await calls("settings_set"))[0];
    check("settings_set sent the key and no model change", setCall.args.openaiKey === "sk-test-not-a-real-key" && setCall.args.imageModel === null, JSON.stringify(setCall.args));
    await page.waitForSelector(".office .logo img");
    check(
        "after the busy state the header shows a logo that decodes",
        (await page.locator(".office .logo.busy").count()) === 0 && (await page.locator(".office .logo img").evaluate((i) => i.complete && i.naturalWidth > 0)),
    );
    await page.waitForSelector('.storey[data-path="/home/demo/second"] image.logo');
    check("the building sticker of that floor changed to the logo", (await page.locator('.storey[data-path="/home/demo/second"] .plain').count()) === 0);
    check("the NO_KEY error and the key field are gone after the key is saved", (await page.locator('[data-error="logo"]').count()) === 0 && (await page.locator("#openai-key").count()) === 0);
    // A second "New logo", now that a key is set: busy, then a new image.
    const logoBefore = await page.locator(".office .logo img").getAttribute("src");
    await page.waitForTimeout(20);
    await page.click("button.newlogo");
    check("New logo field holds the stored prompt", (await page.locator("#logo-prompt").inputValue()) === "A small blue boat.");
    await page.fill("#logo-prompt", "");
    await page.click(".logoform button.primary");
    await page.waitForSelector(".office .logo.busy");
    check("the New logo button is disabled while a logo is busy", await page.locator("button.newlogo").isDisabled());
    await page.waitForSelector(".office .logo:not(.busy) img");
    const logoAfter = await page.locator(".office .logo img").getAttribute("src");
    check("the second logo is a new image", logoBefore !== logoAfter);
    const gen2 = (await calls("logo_generate")).pop();
    check("an emptied prompt field sends the empty text (remove the stored prompt)", gen2.args.prompt === "", JSON.stringify(gen2.args));
    // The stored prompt is gone now. An empty field that had no value means "no change" (null).
    await page.click("button.newlogo");
    check("after the removal the New logo field is empty", (await page.locator("#logo-prompt").inputValue()) === "");
    await page.click(".logoform button.primary");
    await page.waitForSelector(".office .logo.busy");
    const gen3 = (await calls("logo_generate")).pop();
    check("an empty field with no stored prompt sends null (no change)", gen3.args.prompt === null, JSON.stringify(gen3.args));
    await page.waitForSelector(".office .logo:not(.busy) img");

    // 13. The setup agent: asks for the CLI, makes a "Setup" tab, and the floor changes.
    const empBefore = await page.locator(".emp").count();
    await page.click("button.setup");
    check(
        "Run the setup agent asks for Claude or Codex",
        (await page.locator("button.cli-claude").count()) === 1 && (await page.locator("button.cli-codex").count()) === 1,
    );
    await page.click("button.cli-codex");
    await page.waitForSelector('.tab:has-text("Setup")');
    check('the setup button creates a "Setup" tab', (await page.locator(".tab:has-text('Setup')").count()) === 1);
    check("the Setup tab is the active tab", (await page.locator(".tab.active").innerText()).includes("Setup"));
    await waitScreen("Agent Tower mock setup agent");
    check("the setup terminal shows the output of the setup agent", (await screen()).includes("Agent Tower mock setup agent"));
    const setupCall = (await calls("setup_start"))[0];
    check(
        "setup_start got the path, the cli and a size",
        setupCall.args.path === "/home/demo/second" && setupCall.args.cli === "codex" && typeof setupCall.args.rows === "number",
        JSON.stringify(setupCall.args),
    );
    check(
        "while the setup session is live the button is Stop the setup and it is enabled",
        (await page.locator("button.setup").innerText()) === "Stop the setup" && (await page.locator("button.setup").isEnabled()),
    );
    await page.waitForFunction((n) => document.querySelectorAll(".emp").length > n, empBefore, { timeout: 9000 });
    check("when the setup adds an employee (floor-changed) the office gets one more desk and character", (await page.locator(".emp").count()) === empBefore + 1);
    await page.waitForSelector('.tab.active .dot[data-status="idle"]', { timeout: 5000 });
    check(
        "the Setup tab dot follows the status (idle at the end of the turn)",
        (await page.locator(".tab.active .dot").getAttribute("data-status")) === "idle",
    );
    check("the setup tab has no resume button (no employee bar for it)", (await page.locator(".tab button.resume").count()) === 0);
    const stopsBefore = (await calls("session_stop")).length;
    await page.click("button.setup");
    await page.waitForSelector('.tab:has-text("Setup") .dot[data-status="stopped"]');
    const setupStops = (await calls("session_stop")).slice(stopsBefore);
    const setupTab = await page.locator('.tab:has-text("Setup")').count();
    check(
        "the button Stop the setup calls session_stop once, and the Setup tab goes to stopped",
        setupStops.length === 1 && typeof setupStops[0].args.sessionId === "string" && setupTab === 1,
        JSON.stringify(setupStops.map((c) => c.args)),
    );
    await page.waitForSelector("button.setup:not(.stop-setup)");
    check(
        "after the stop the button offers Run the setup agent again",
        (await page.locator("button.setup").innerText()) === "Run the setup agent" && (await page.locator("button.stop-setup").count()) === 0,
    );

    // 14. A floor can be forgotten from the floor list, also a missing one.
    await page.click("details.floors > summary");
    const forget = page.locator('.floors li[data-path="/home/demo/gone"] button');
    await forget.click();
    await forget.click();
    await page.waitForFunction(() => document.querySelectorAll(".storey").length === 4);
    check("forget (two clicks) removes the missing floor from the building", (await page.locator('.storey[data-path="/home/demo/gone"]').count()) === 0);
    await clickStorey("/home/demo/tower");
    await page.waitForSelector('.storey.on[data-path="/home/demo/tower"]');
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
