// End-to-end check of the page with the mock backend, in a headless Chromium (1280 x 800).
// Usage: pnpm dev (in another shell), then: pnpm e2e
// Env: E2E_URL (default http://127.0.0.1:1420), E2E_SHOT (screenshot path; the 5 and 13 employee
//      screenshots go next to it with the suffixes -5 and -13),
//      E2E_CHROME (path of a chromium or chrome binary; default: the Playwright cache),
//      E2E_SETTINGS_SHOTS (folder for the screenshots of the settings panel and the employee
//      form; default /tmp/agent-tower-review/settings).
import { chromium } from "playwright-core";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const url = process.env.E2E_URL ?? "http://127.0.0.1:1420";
const shot = process.env.E2E_SHOT ?? "mock-page.png";
const settingsShots = process.env.E2E_SETTINGS_SHOTS ?? "/tmp/agent-tower-review/settings";
mkdirSync(settingsShots, { recursive: true });

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
    check("the name tag shows the job title", (await page.locator('.tag[data-slug="ceo"] .tag-role').textContent()) === "CEO");
    check(
        "the employee list shows the name and the job title of each employee",
        (await page.locator('.roster .row[data-slug="ceo"]').innerText()).replace(/\s+/g, " ").trim() === "Ada CEO" &&
            (await page.locator('.roster .row[data-slug="pm"] .r-role').innerText()) === "Project Manager" &&
            (await page.locator(".roster .row").count()) === (await page.locator(".emp").count()),
    );
    check("mock badge shown", await page.locator(".mock").isVisible());
    check("the debug area is collapsed", (await page.locator("details.debug-area[open]").count()) === 0);
    const box1280 = { office: await page.locator(".office").boundingBox(), area: await page.locator(".area").boundingBox() };
    // The old fixed rules ("at most 42%") became the default sizes. The operator can change them now (section 16).
    check("at 1280 x 800 the default office height is 42% of the window (336 px)", Math.abs(box1280.office.height - 336) <= 1, `office ${box1280.office.height}`);
    check("at 1280 x 800 the default sizes give the terminal area at least half of the height", box1280.area.height >= 400, `area ${box1280.area.height}`);
    check("the bigger office toggle is gone", (await page.locator("button.size").count()) === 0 && (await page.getByText("bigger office").count()) === 0);

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
    check("with 13 employees the tags have no job title line", (await page.locator(".tag .tag-role").count()) === 0);
    check("with 13 employees the employee list has 13 rows", (await page.locator(".roster .row").count()) === 13);
    await page.locator('.roster .row[data-slug="des-2"]').click();
    check(
        "a click on a row of the employee list selects that employee",
        (await page.locator(".bar").getAttribute("data-slug")) === "des-2" &&
            (await page.locator('.emp[data-slug="des-2"]').getAttribute("aria-pressed")) === "true",
    );
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
    // The model select shows the list of the selected CLI, and the effort select the efforts of
    // the selected model (docs/ipc-settings.md, section 6).
    const opts = (sel) => page.$$eval(`${sel} option`, (os) => os.map((o) => o.textContent.trim()));
    const val = (sel) => page.locator(sel).inputValue();
    check(
        "the model select of a Claude employee lists the 4 Claude models after the floor default",
        JSON.stringify(await opts('select[name="model"]')) ===
            JSON.stringify(["floor default (CLI default)", "Fable (fable)", "Opus (opus)", "Sonnet (sonnet)", "Haiku (haiku)"]),
        JSON.stringify(await opts('select[name="model"]')),
    );
    check(
        "the effort select with the floor default model is default + the 5 standard efforts",
        JSON.stringify(await opts('select[name="effort"]')) === JSON.stringify(["default", "low", "medium", "high", "xhigh", "max"]),
    );
    await page.selectOption('select[name="model"]', "haiku");
    await page.selectOption('select[name="cli"]', "codex");
    check(
        "a change of CLI resets a model of the other CLI to the floor default",
        (await val('select[name="model"]')) === "" && (await page.locator('select[name="model"] option:checked').innerText()) === "floor default (CLI default)",
    );
    check(
        "the model select shows the model list of the selected CLI (Codex, from the catalog)",
        JSON.stringify(await opts('select[name="model"]')) ===
            JSON.stringify([
                "floor default (CLI default)",
                "GPT-6.2-Sol (gpt-6.2-sol)",
                "GPT-6-Astra (gpt-6-astra)",
                "GPT-6-Luna (gpt-6-luna)",
                "GPT-5.6-Luna (gpt-5.6-luna)",
            ]),
        JSON.stringify(await opts('select[name="model"]')),
    );
    check(
        "a Codex model without ultra has no ultra in the effort list",
        !(await opts('select[name="effort"]')).includes("ultra"),
    );
    await page.selectOption('select[name="model"]', "gpt-6.2-sol");
    check(
        "the effort select shows the efforts of the selected model (ultra for gpt-6.2-sol)",
        JSON.stringify(await opts('select[name="effort"]')) === JSON.stringify(["default", "low", "medium", "high", "xhigh", "max", "ultra"]),
    );
    await page.selectOption('select[name="effort"]', "ultra");
    await page.selectOption('select[name="cli"]', "claude");
    check(
        "a change of CLI also resets an effort that the new model does not have",
        (await val('select[name="model"]')) === "" && (await val('select[name="effort"]')) === "",
    );
    await page.selectOption('select[name="cli"]', "codex");
    await page.selectOption('select[name="model"]', "gpt-6.2-sol");
    await page.selectOption('select[name="effort"]', "ultra");
    await page.selectOption('select[name="model"]', "gpt-6-luna");
    check(
        "a change of model resets an effort that the new model does not have",
        (await val('select[name="effort"]')) === "",
    );
    await page.selectOption('select[name="model"]', "");
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

    // 15. The settings panel, the model lists and the floor defaults.
    const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
    const openSettings = async () => {
        await page.click("button.settings");
        await dialog.waitFor();
    };
    const closeSettings = async () => {
        await page.keyboard.press("Escape");
        await dialog.waitFor({ state: "detached" });
    };
    const since = async (command, n) => (await calls(command)).slice(n);
    const count = async (command) => (await calls(command)).length;

    // A close that does not work is a failed check, not a time-out: the helper then closes the panel
    // with the close button so that the next checks can run.
    const gone = async () => {
        await dialog.waitFor({ state: "detached", timeout: 1500 }).catch(() => {});
        const closed = (await dialog.count()) === 0;
        if (!closed) await page.click('button[aria-label="Close settings"]');
        return closed;
    };
    await page.click("button.settings");
    const opened = await dialog.waitFor({ timeout: 3000 }).then(() => true, () => false);
    check("the Settings button opens the settings panel", opened);
    if (!opened) throw new Error("the settings panel did not open");
    check(
        "the settings panel has the 5 sections",
        JSON.stringify(await page.$$eval('[role="dialog"] section', (s) => s.map((x) => x.dataset.section))) ===
            JSON.stringify(["keys", "image", "models", "defaults", "programs"]),
    );
    await page.keyboard.press("Escape");
    check("Escape closes the settings panel", await gone());
    await openSettings();
    await page.mouse.click(4, 4);
    check("a click on the backdrop closes the settings panel", await gone());
    await openSettings();
    await page.click('button[aria-label="Close settings"]');
    check("the close button closes the settings panel", await gone());
    check("the programs panel is not in the building column any more", (await page.locator("aside .programs").count()) === 0);

    // API keys. A key was saved in the "New logo" flow above.
    await openSettings();
    check("the key state says set (app config)", (await dialog.locator(".state").innerText()) === "set (app config)");
    const setsBefore = await count("settings_set");
    await dialog.locator("button.remove-key").click();
    await page.waitForFunction(() => document.querySelector('[role="dialog"] .state')?.textContent === "not set");
    const removeCall = (await since("settings_set", setsBefore))[0];
    check(
        "remove sends an empty key and no model change, and the state is not set",
        removeCall.args.openaiKey === "" && removeCall.args.imageModel === null && (await dialog.locator("button.remove-key").count()) === 0,
        JSON.stringify(removeCall.args),
    );
    check("the key field is a password field", (await dialog.locator("#settings-openai-key").getAttribute("type")) === "password");
    await dialog.locator("#settings-openai-key").fill("sk-panel-secret-1");
    await dialog.locator(".keyform button.primary").click();
    await page.waitForFunction(() => document.querySelector('[role="dialog"] .state')?.textContent === "set (app config)");
    check(
        "after set the panel shows no key text and the field is empty",
        !(await dialog.innerText()).includes("sk-panel-secret-1") &&
            (await dialog.locator("#settings-openai-key").inputValue()) === "" &&
            (await dialog.locator("button.remove-key").count()) === 1,
    );

    // The logo image model.
    check(
        "the image model select lists the 2 built-in image models",
        JSON.stringify(await opts('select[name="imageModel"]')) === JSON.stringify(["gpt-image-2.6-sunburst", "gpt-image-2.6-flare"]),
    );
    const imagesBefore = await count("settings_set");
    await page.selectOption('select[name="imageModel"]', "gpt-image-2.6-flare");
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "settings_set").length > n, imagesBefore);
    const imageCall = (await since("settings_set", imagesBefore))[0];
    check(
        "a change of the image model sends settings_set with the model and no key",
        imageCall.args.imageModel === "gpt-image-2.6-flare" && imageCall.args.openaiKey === null,
        JSON.stringify(imageCall.args),
    );
    await page.selectOption('select[name="imageModel"]', "gpt-image-2.6-sunburst");

    // The model lists and the custom models.
    const codexList = dialog.locator('.list[data-kind="codex"]');
    const codexInput = codexList.locator("input");
    check("the Codex list shows 4 models and where the list comes from", (await codexList.locator("li").count()) === 4 && (await codexList.locator(".note").innerText()).includes("models_cache.json"));
    check("each model shows its label and its ID", (await codexList.locator('li[data-model="gpt-6-luna"] .text').innerText()) === "GPT-6-Luna (gpt-6-luna)");
    await codexInput.fill("--foo");
    check(
        "a name that starts with a dash cannot be added (the button is off, the reason shows)",
        (await codexList.locator("button.add").isDisabled()) && (await codexList.locator(".problem").count()) === 1,
    );
    await codexInput.fill("gpt-6-luna");
    check("a name that is in the list already cannot be added", await codexList.locator("button.add").isDisabled());
    const customBefore = await count("models_set_custom");
    await codexInput.fill("my-custom-1");
    await codexList.locator("button.add").click();
    await codexList.locator('li[data-model="my-custom-1"]').waitFor();
    const addCall = (await since("models_set_custom", customBefore))[0];
    check(
        "a custom model is added to the Codex list after the others, with a custom tag and a remove button",
        JSON.stringify(addCall.args) === JSON.stringify({ kind: "codex", models: ["my-custom-1"] }) &&
            (await codexList.locator("li").last().getAttribute("data-model")) === "my-custom-1" &&
            (await codexList.locator('li[data-model="my-custom-1"] .tag').innerText()) === "custom" &&
            (await codexInput.inputValue()) === "",
        JSON.stringify(addCall.args),
    );
    check("the built-in models have no remove button", (await codexList.locator("button.remove").count()) === 1);
    await codexList.locator('li[data-model="my-custom-1"] button.remove').click();
    await codexList.locator('li[data-model="my-custom-1"]').waitFor({ state: "detached" });
    const removeModelCall = (await since("models_set_custom", customBefore)).at(-1);
    check(
        "a custom model can be removed (models_set_custom with an empty list)",
        JSON.stringify(removeModelCall.args) === JSON.stringify({ kind: "codex", models: [] }) && (await codexList.locator("li").count()) === 4,
        JSON.stringify(removeModelCall.args),
    );
    const imageList = dialog.locator('.list[data-kind="image"]');
    await imageList.locator("input").fill("img-x");
    await imageList.locator("button.add").click();
    await imageList.locator('li[data-model="img-x"]').waitFor();
    check(
        "an image model can be added in its own list, and then the image select offers it",
        (await opts('select[name="imageModel"]')).includes("img-x"),
    );
    await imageList.locator('li[data-model="img-x"] button.remove').click();
    await imageList.locator('li[data-model="img-x"]').waitFor({ state: "detached" });

    // Programs moved into the panel.
    check("the programs paths are in the settings panel", (await dialog.locator('.programs [data-program="claude"]').count()) === 1);

    // Floor defaults.
    const defaults = dialog.locator('section[data-section="defaults"]');
    const dsel = (name) => defaults.locator(`select[name="${name}"]`);
    check(
        "the floor defaults start as (not set), and the model and effort selects are off with no CLI",
        (await dsel("defaultCli").inputValue()) === "" && (await dsel("defaultModel").isDisabled()) && (await dsel("defaultEffort").isDisabled()),
    );
    await dsel("defaultCli").selectOption("codex");
    check("with a CLI the model select lists that CLI", (await dsel("defaultModel").isEnabled()) && (await defaults.locator('select[name="defaultModel"] option').count()) === 5);
    await dsel("defaultModel").selectOption("gpt-6.2-sol");
    await dsel("defaultEffort").selectOption("ultra");
    await dsel("defaultCli").selectOption("claude");
    check(
        "a change of the default CLI resets a model of the other CLI and its effort",
        (await dsel("defaultModel").inputValue()) === "" && (await dsel("defaultEffort").inputValue()) === "",
    );
    await dsel("defaultCli").selectOption("codex");
    await dsel("defaultModel").selectOption("gpt-6-luna");
    await dsel("defaultEffort").selectOption("high");
    await dsel("defaultPolicy").selectOption("ask_always");
    const defaultsBefore = await count("floor_defaults_set");
    await defaults.locator("button.primary").click();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "floor_defaults_set").length > n, defaultsBefore);
    await page.waitForFunction(() => document.querySelector('[role="dialog"] section[data-section="defaults"] button.primary')?.disabled === true);
    const saveCall = (await since("floor_defaults_set", defaultsBefore))[0];
    check(
        "save sends floor_defaults_set with the path and the four fields",
        JSON.stringify(saveCall.args) ===
            JSON.stringify({ path: "/home/demo/tower", defaults: { cli: "codex", model: "gpt-6-luna", effort: "high", policy: "ask_always" } }),
        JSON.stringify(saveCall.args),
    );
    // Screenshots: the top of the panel, and the whole panel (tall window) with one custom model.
    const claudeList = dialog.locator('.list[data-kind="claude"]');
    await claudeList.locator("input").fill("claude-opus-4-1[1m]");
    await claudeList.locator("button.add").click();
    await claudeList.locator('li[data-model="claude-opus-4-1[1m]"]').waitFor();
    await dialog.evaluate((el) => (el.scrollTop = 0));
    await page.screenshot({ path: join(settingsShots, "settings-panel.png") });
    await page.setViewportSize({ width: 1280, height: 1700 });
    await page.waitForTimeout(150);
    await dialog.screenshot({ path: join(settingsShots, "settings-panel-full.png") });
    await page.setViewportSize({ width: 1024, height: 700 });
    await page.waitForTimeout(150);
    await page.screenshot({ path: join(settingsShots, "settings-panel-1024.png") });
    await page.setViewportSize({ width: 1280, height: 800 });
    await claudeList.locator('li[data-model="claude-opus-4-1[1m]"] button.remove').click();
    await claudeList.locator('li[data-model="claude-opus-4-1[1m]"]').waitFor({ state: "detached" });
    await closeSettings();

    // The saved default shows in the employee form: "floor default (X)" follows the CLI rule.
    await page.click(".bar button.add");
    check(
        "the Claude form shows floor default (CLI default) when the floor default CLI is codex",
        (await page.locator('select[name="model"] option').first().innerText()) === "floor default (CLI default)",
    );
    await page.selectOption('select[name="cli"]', "codex");
    check(
        "the Codex form shows the saved default model in the floor default option",
        (await page.locator('select[name="model"] option').first().innerText()) === "floor default (gpt-6-luna)",
    );
    await page.keyboard.press("Escape");
    await openSettings();
    await dsel("defaultCli").selectOption("");
    check("with the CLI set back to (not set) the model and effort selects are off and empty", (await dsel("defaultModel").isDisabled()) && (await dsel("defaultModel").inputValue()) === "" && (await dsel("defaultEffort").inputValue()) === "");
    await defaults.locator("button.primary").click();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "floor_defaults_set").length > n, defaultsBefore + 1);
    const clearCall = (await since("floor_defaults_set", defaultsBefore)).at(-1);
    check(
        "the CLI (not set) sends null for the model and the effort too (the policy stays)",
        JSON.stringify(clearCall.args.defaults) === JSON.stringify({ cli: null, model: null, effort: null, policy: "ask_always" }),
        JSON.stringify(clearCall.args.defaults),
    );
    await dsel("defaultPolicy").selectOption("");
    await defaults.locator("button.primary").click();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "floor_defaults_set").length > n, defaultsBefore + 2);
    await closeSettings();

    // A stored model that is not in the list stays selected with "(not in the list)".
    await openSettings();
    await codexList.locator("input").fill("my-custom-2");
    await codexList.locator("button.add").click();
    await codexList.locator('li[data-model="my-custom-2"]').waitFor();
    await closeSettings();
    await page.click(".bar button.add");
    await page.fill('input[name="name"]', "Temp Tmp");
    await page.selectOption('select[name="cli"]', "codex");
    check("a custom model is in the model select of the employee form", (await opts('select[name="model"]')).includes("my-custom-2"));
    await page.selectOption('select[name="model"]', "my-custom-2");
    check(
        "a custom model has the 5 standard efforts",
        JSON.stringify(await opts('select[name="effort"]')) === JSON.stringify(["default", "low", "medium", "high", "xhigh", "max"]),
    );
    await page.selectOption('select[name="effort"]', "max");
    await page.screenshot({ path: join(settingsShots, "employee-form.png") });
    await page.click('form[aria-label="Add employee"] button.primary');
    await page.waitForSelector('.emp[data-slug="temp-tmp"]');
    await openSettings();
    await codexList.locator('li[data-model="my-custom-2"] button.remove').click();
    await codexList.locator('li[data-model="my-custom-2"]').waitFor({ state: "detached" });
    await closeSettings();
    await selectEmp("temp-tmp");
    await bar.locator("button.edit").click();
    await page.waitForSelector('form[aria-label="Edit employee"]');
    check(
        "a stored model that left the list stays selected, with the text (not in the list)",
        (await val('select[name="model"]')) === "my-custom-2" &&
            (await page.locator('select[name="model"] option:checked').innerText()) === "my-custom-2 (not in the list)",
    );
    check("the stored effort stays selected", (await val('select[name="effort"]')) === "max");
    const savesBefore = await count("employee_save");
    await page.click('form[aria-label="Edit employee"] button.primary');
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "employee_save").length > n, savesBefore);
    const keepCall = (await since("employee_save", savesBefore))[0];
    check(
        "a save does not change the stored model or effort",
        keepCall.args.employee.model === "my-custom-2" && keepCall.args.employee.effort === "max",
        JSON.stringify(keepCall.args.employee),
    );
    await selectEmp("temp-tmp");
    await bar.locator("button.edit").click();
    await page.waitForSelector('form[aria-label="Edit employee"]');
    await page.selectOption('select[name="cli"]', "claude");
    check("a stored unknown model goes to the floor default when the CLI changes", (await val('select[name="model"]')) === "");
    await page.keyboard.press("Escape");
    await bar.locator("button.remove").click();
    await bar.locator("button.remove").click();
    await page.waitForSelector('.emp[data-slug="temp-tmp"]', { state: "detached" });
    check("the temporary employee is removed again", (await page.locator(".emp").count()) === 4);

    // ================= 16. The handles: resize the containers (Feature A) =================
    const resizeShots = process.env.E2E_RESIZE_SHOTS ?? "/tmp/agent-tower-review/resize";
    mkdirSync(resizeShots, { recursive: true });
    const H_COL = '[role="separator"][aria-label="Resize the building column"]';
    const H_INFO = '[role="separator"][aria-label="Resize the info panel and the room"]';
    const H_OFFICE = '[role="separator"][aria-label="Resize the office"]';
    const sizeOf = async (sel) => {
        const b = await page.locator(sel).first().boundingBox();
        return { w: Math.round(b.width), h: Math.round(b.height), x: Math.round(b.x), y: Math.round(b.y) };
    };
    const colW = async () => (await sizeOf(".column")).w;
    const officeH = async () => (await sizeOf(".office")).h;
    const infoW = async () => (await sizeOf(".office .info")).w;
    async function drag(sel, dx, dy, steps = 8) {
        const b = await page.locator(sel).boundingBox();
        const x = b.x + b.width / 2;
        const y = b.y + b.height / 2;
        await page.mouse.move(x, y);
        await page.mouse.down();
        await page.mouse.move(x + dx, y + dy, { steps });
        await page.mouse.up();
        await page.waitForTimeout(60);
    }
    const key = async (sel, k) => {
        await page.locator(sel).focus();
        await page.keyboard.press(k);
        await page.waitForTimeout(40);
    };
    // The checks of this section were written for a work area of 960 px (before the task board).
    // The board is hidden while they run (its own checks are in section 19).
    await page.addInitScript(() => {
        document.addEventListener("DOMContentLoaded", () => {
            if (sessionStorage.getItem("e2e-hide-board") !== "1") return;
            const style = document.createElement("style");
            style.textContent = ".app{--board-w:0px !important} .board{display:none !important}";
            document.head.appendChild(style);
        });
    });
    await page.evaluate(() => sessionStorage.setItem("e2e-hide-board", "1"));
    await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(resizeShots, "1-default.png") });

    // The three handles: role, orientation, focus, size of the hit area, shown on hover.
    check(
        "there are 3 separators: 2 vertical and 1 horizontal, each focusable and with a label",
        JSON.stringify(
            await page.$$eval('[role="separator"]:not(.board *)', (els) =>
                els.map((e) => [e.getAttribute("aria-orientation"), e.tabIndex, !!e.getAttribute("aria-label")]),
            ),
        ) === JSON.stringify([["vertical", 0, true], ["vertical", 0, true], ["horizontal", 0, true]]),
    );
    const hbox = { col: await sizeOf(H_COL), info: await sizeOf(H_INFO), office: await sizeOf(H_OFFICE) };
    check(
        "each handle is about 6 px wide (the hit area)",
        hbox.col.w === 6 && hbox.info.w === 6 && hbox.office.h === 6,
        JSON.stringify(hbox),
    );
    const lineOpacity = (sel) => page.locator(sel).evaluate((e) => Number(getComputedStyle(e, "::after").opacity));
    check("a handle shows no line when the pointer is away", (await lineOpacity(H_COL)) === 0);
    await page.locator(H_COL).hover();
    await page.waitForTimeout(250);
    check("a handle shows its line on hover", (await lineOpacity(H_COL)) > 0.5);
    await page.mouse.move(700, 600);
    check(
        "a handle has the value, the minimum and the maximum as aria values",
        (await page.locator(H_COL).getAttribute("aria-valuenow")) === "320" &&
            (await page.locator(H_COL).getAttribute("aria-valuemin")) === "160" &&
            (await page.locator(H_COL).getAttribute("aria-valuemax")) === "640",
    );

    // 16a. The building column.
    check("at 1280 x 800 the default column is 320 px", (await colW()) === 320);
    await drag(H_COL, 100, 0);
    check("a drag to the right makes the column 100 px wider", (await colW()) === 420, String(await colW()));
    await drag(H_COL, -900, 0);
    check("a drag far to the left stops at the minimum of 160 px", (await colW()) === 160, String(await colW()));
    await drag(H_COL, 1100, 0);
    check("a drag far to the right stops at 50% of the window (640 px)", (await colW()) === 640, String(await colW()));
    await key(H_COL, "ArrowLeft");
    check("the Left key makes the column 16 px narrower", (await colW()) === 624, String(await colW()));
    await key(H_COL, "ArrowRight");
    await key(H_COL, "ArrowRight");
    check("the Right key makes the column 16 px wider (and stops at the maximum)", (await colW()) === 640, String(await colW()));
    await page.screenshot({ path: join(resizeShots, "2b-wide-column.png") });
    await page.locator(H_COL).dblclick();
    await page.waitForTimeout(60);
    check("a double click on the handle resets the column to 320 px", (await colW()) === 320, String(await colW()));
    await key(H_COL, "ArrowDown");
    check("the Up and Down keys do nothing on a vertical handle", (await colW()) === 320);

    // The pointer stays captured when the drag goes over the room (an SVG) and the terminal area.
    {
        const b = await page.locator(H_COL).boundingBox();
        await page.mouse.move(b.x + 3, b.y + 300);
        await page.mouse.down();
        await page.mouse.move(b.x + 400, b.y + 150, { steps: 6 });
        const captured = await page.locator(H_COL).evaluate((e) => e.hasPointerCapture(1));
        await page.mouse.move(b.x + 100, b.y + 500, { steps: 6 });
        check("a drag uses pointer capture, and goes on over the room and the terminal", captured && (await colW()) === 320 + 97, `captured ${captured}, col ${await colW()}`);
        await page.mouse.up();
        await page.waitForTimeout(60);
    }
    await page.locator(H_COL).dblclick();

    // 16b. The height of the office. A live terminal is open here (Ada), so the fit is visible.
    await selectEmp("ceo");
    await bar.locator("button.start").click();
    await page.waitForSelector(".tab.active .dot[data-status=idle]");
    await page.waitForSelector(".pane:not(.hidden) .xterm-rows > div");
    const termRows = () => page.locator(".pane:not(.hidden) .xterm-rows > div").count();
    await page.waitForTimeout(400);
    check("the default office height is 336 px", (await officeH()) === 336, String(await officeH()));
    await drag(H_OFFICE, 0, 60);
    check("a drag down makes the office 60 px higher", (await officeH()) === 396, String(await officeH()));
    await drag(H_OFFICE, 0, -900);
    check("a drag far up stops at the minimum of 120 px", (await officeH()) === 120, String(await officeH()));
    await page.screenshot({ path: join(resizeShots, "3-small-office.png") });
    await drag(H_OFFICE, 0, 1200);
    const areaH = (await sizeOf(".area")).h;
    check(
        "a drag far down stops where the terminal area keeps 160 px",
        areaH >= 159 && areaH <= 175,
        `office ${await officeH()}, terminal area ${areaH}`,
    );
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(resizeShots, "4-large-office.png") });
    await key(H_OFFICE, "ArrowUp");
    const afterUp = await officeH();
    await key(H_OFFICE, "ArrowDown");
    check("the Up and Down keys move the office handle by 16 px", (await officeH()) - afterUp === 16, `${afterUp} -> ${await officeH()}`);
    await page.locator(H_OFFICE).dblclick();
    await page.waitForTimeout(60);
    check("a double click resets the office to 336 px", (await officeH()) === 336, String(await officeH()));
    await key(H_OFFICE, "ArrowRight");
    check("the Left and Right keys do nothing on a horizontal handle", (await officeH()) === 336);

    // The terminal fits its new size after a drag, and the process gets ONE pty_resize with it.
    await page.waitForTimeout(300);
    const resizeCount = async () => (await calls("pty_resize")).length;
    const before = await resizeCount();
    const rowsBefore16 = await termRows();
    await drag(H_OFFICE, 0, -90, 18);
    await page.waitForTimeout(500);
    const sent = (await calls("pty_resize")).slice(before);
    const rowsNow = await termRows();
    check("the terminal has more rows after the office got smaller", rowsNow > rowsBefore16, `${rowsBefore16} -> ${rowsNow}`);
    check(
        "a drag of 18 steps sends exactly one pty_resize, with the new size",
        sent.length === 1 && sent[0].args.rows === rowsNow,
        `sent ${JSON.stringify(sent.map((c) => c.args))}, rows ${rowsNow}`,
    );
    const fit = await page.evaluate(() => {
        const pane = document.querySelector(".pane:not(.hidden)");
        const screenEl = pane.querySelector(".xterm-screen");
        return { pane: pane.getBoundingClientRect().height, screen: screenEl.getBoundingClientRect().height };
    });
    check("the terminal screen fills its pane after the drag (less than one row of space left)", fit.screen <= fit.pane && fit.pane - fit.screen < 24, JSON.stringify(fit));
    await page.locator(H_OFFICE).dblclick();
    await page.waitForTimeout(500);

    // 16c. The split between the info panel and the room.
    check("the default info panel is 456 px wide at 1280 x 800", (await infoW()) === 456, String(await infoW()));
    await drag(H_INFO, -100, 0);
    check("a drag to the left makes the info panel 100 px narrower", (await infoW()) === 356, String(await infoW()));
    await drag(H_INFO, -900, 0);
    check("a drag far to the left stops at 200 px for the info panel", (await infoW()) === 200, String(await infoW()));
    await page.screenshot({ path: join(resizeShots, "5-narrow-info.png") });
    await drag(H_INFO, 1200, 0);
    const officeWide = (await sizeOf(".office")).w;
    check("a drag far to the right leaves 120 px to the room", (await infoW()) === officeWide - 120 - 1 || (await infoW()) === officeWide - 120, `info ${await infoW()}, office ${officeWide}`);
    await key(H_INFO, "ArrowLeft");
    await key(H_INFO, "ArrowLeft");
    const afterTwo = await infoW();
    await key(H_INFO, "ArrowRight");
    check("the Left and Right keys move the info handle by 16 px", (await infoW()) - afterTwo === 16, `${afterTwo} -> ${await infoW()}`);
    await page.locator(H_INFO).dblclick();
    await page.waitForTimeout(60);
    check("a double click resets the info panel to 456 px", (await infoW()) === 456, String(await infoW()));

    // 16c2. The room follows the height of the office while the operator has not set the info
    // width by hand. Measured before the repair (1280 x 800, office dragged to 585 px): the room
    // picture stayed 503 x 335, because the default info width came from the DEFAULT office height.
    {
        const roomArt = async () => {
            const b = await page.locator(".office .room svg > image").boundingBox();
            return { w: Math.round(b.width), h: Math.round(b.height) };
        };
        await page.locator(H_OFFICE).dblclick();
        await page.waitForTimeout(150);
        const before = await roomArt();
        await drag(H_OFFICE, 0, 130);
        await page.waitForTimeout(150);
        const taller = await roomArt();
        const infoTaller = await infoW();
        check(
            "the room picture is larger after the office is dragged taller",
            taller.w >= before.w + 100 && taller.h >= before.h + 60,
            `before ${JSON.stringify(before)}, office ${await officeH()}, after ${JSON.stringify(taller)}`,
        );
        check(
            "the room picture keeps its 3:2 shape",
            Math.abs(taller.w / taller.h - 1.5) < 0.02,
            JSON.stringify(taller),
        );
        check(
            "the info panel takes the rest, and it keeps at least 300 px",
            infoTaller >= 300 && infoTaller < 456,
            `info ${infoTaller}`,
        );
        await page.screenshot({ path: join(resizeShots, "5b-room-follows-office.png") });
        // A width that the operator sets by hand stays when the office changes.
        await drag(H_INFO, -40, 0);
        const byHand = await infoW();
        await drag(H_OFFICE, 0, -100);
        check(
            "an info width that the operator set by hand stays when the office height changes",
            byHand !== infoTaller && (await infoW()) === byHand,
            `by hand ${byHand}, then ${await infoW()}`,
        );
        await page.locator(H_OFFICE).dblclick();
        await page.locator(H_INFO).dblclick();
        await page.waitForTimeout(100);
        check("the default sizes come back after two double clicks", (await officeH()) === 336 && (await infoW()) === 456, `${await officeH()} ${await infoW()}`);
    }

    // 16d. The building and the room stay inside their containers.
    const inside = (inner, outer, tol = 1) =>
        inner.left >= outer.left - tol && inner.right <= outer.right + tol && inner.top >= outer.top - tol && inner.bottom <= outer.bottom + tol;
    const fitReport = () =>
        page.evaluate(() => {
            const r = (el) => el.getBoundingClientRect();
            const scroller = document.querySelector(".column .scroll");
            const svg = document.querySelector("svg.building");
            const sc = r(scroller);
            const bs = r(svg);
            const room = document.querySelector(".office .room");
            const rc = r(room);
            const art = r(room.querySelector("svg > image"));
            const tagH = document.querySelector(".tag .tag-name")?.getBoundingClientRect().height ?? 0;
            return {
                scrollsY: scroller.scrollHeight > scroller.clientHeight + 1,
                bldW: bs.width,
                buildingInsideX: bs.left >= sc.left - 1 && bs.right <= sc.right + 1,
                buildingInsideY: bs.top >= sc.top - 1 && bs.bottom <= sc.bottom + 1,
                // The art of the room, inside its cell, and centred in the direction that has spare room.
                roomInside: art.left >= rc.left - 1 && art.right <= rc.right + 1 && art.top >= rc.top - 1 && art.bottom <= rc.bottom + 1,
                roomCentred: Math.abs(art.left - rc.left - (rc.right - art.right)) <= 2 && Math.abs(art.top - rc.top - (rc.bottom - art.bottom)) <= 2,
                tagH,
            };
        });
    const sizes16 = [
        { name: "narrow column (160 px)", col: 160, office: 336, info: 456 },
        { name: "default", col: 320, office: 336, info: 456 },
        { name: "wide column (640 px)", col: 640, office: 336, info: 456 },
        { name: "small office (120 px)", col: 320, office: 120, info: 456 },
        { name: "large office (560 px)", col: 320, office: 560, info: 456 },
        { name: "narrow room (info 700 px)", col: 320, office: 336, info: 700 },
    ];
    for (const sz of sizes16) {
        await page.evaluate((v) => localStorage.setItem("agent-tower:layout", JSON.stringify(v)), { col: sz.col, office: sz.office, info: sz.info });
        // Apply with the keyboard-free path: reload the page (the mock state is the same for floor lists).
        await page.reload();
        await page.waitForSelector(".storey.on");
        await page.waitForTimeout(450);
        const rep = await fitReport();
        check(
            `the building and the room stay inside their containers: ${sz.name}`,
            rep.buildingInsideX && (rep.buildingInsideY || rep.scrollsY) && rep.roomInside && rep.roomCentred,
            JSON.stringify(rep),
        );
        if (sz.col === 160) await page.screenshot({ path: join(resizeShots, "2-narrow-column.png") });
        if (sz.office === 120) await page.screenshot({ path: join(resizeShots, "3b-small-office-fit.png") });
        if (sz.office === 560) check("at the large office the name tags are readable (at least 8 px tall)", rep.tagH >= 8, `tag text ${rep.tagH.toFixed(1)} px`);
    }

    // 16e. 13 employees at a large size.
    await page.evaluate(() => localStorage.setItem("agent-tower:layout", JSON.stringify({ col: 260, office: 560, info: 330 })));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await clickStorey("/home/demo/big");
    await page.waitForFunction(() => document.querySelectorAll(".emp").length === 13);
    await page.waitForTimeout(450);
    const rep13 = await fitReport();
    check(
        "13 employees at a large office: the room stays inside and the name text is readable",
        rep13.roomInside && rep13.roomCentred && rep13.tagH >= 8,
        JSON.stringify(rep13),
    );
    await page.screenshot({ path: join(resizeShots, "6-13-employees-large.png") });

    // 16f. Many floors: the building keeps a readable width, and the column scrolls.
    await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
    await page.reload();
    await page.waitForSelector(".storey.on");
    const fourFloors = await fitReport();
    check("with 4 floors the building fits the column without a scroll bar", !fourFloors.scrollsY && fourFloors.buildingInsideY && fourFloors.buildingInsideX, JSON.stringify(fourFloors));
    for (let i = 0; i < 9; i++) {
        await page.fill("#floor-path", `/home/demo/extra-${i}`);
        await page.click(".open button.primary");
        await page.waitForSelector(".create");
        await page.click(".create button.primary");
        await page.waitForSelector(`.storey.on[data-path="/home/demo/extra-${i}"]`);
    }
    // A low window: 13 floors do not fit at a readable size, so the column scrolls.
    await page.setViewportSize({ width: 1280, height: 560 });
    await page.waitForTimeout(400);
    const manyFloors = await fitReport();
    check(
        "with 13 floors the building keeps a readable width (about 200 px), stays inside sideways, and the column scrolls",
        (await page.locator(".storey").count()) === 13 && manyFloors.scrollsY && manyFloors.buildingInsideX && manyFloors.bldW >= 195 && manyFloors.bldW <= 205,
        JSON.stringify(manyFloors),
    );
    await page.screenshot({ path: join(resizeShots, "7-many-floors.png") });
    await page.setViewportSize({ width: 1280, height: 800 });

    // 16g. The sizes survive a reload, and a restored size is clamped to the window.
    await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(300);
    await drag(H_COL, -40, 0);
    await drag(H_OFFICE, 0, -50);
    await drag(H_INFO, 60, 0);
    const mine = { col: await colW(), office: await officeH(), info: await infoW() };
    // The info width before the drag is the default for the REAL office height (286 px): the work
    // area is 1000 px wide, the room box is 1.5 x 286 = 429 px, and the info panel is 571 px.
    // With the drag of 60 px it is 631 px. (Before the repair the default came from the default
    // office height, and the result was 556 px.)
    check("the three sizes changed by the drags", mine.col === 280 && mine.office === 286 && mine.info === 631, JSON.stringify(mine));
    const storedText = await page.evaluate(() => localStorage.getItem("agent-tower:layout"));
    check("the sizes are in localStorage", storedText === JSON.stringify({ col: 280, office: 286, info: 631, board: null, boardCollapsed: null }), String(storedText));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(400);
    const restored = { col: await colW(), office: await officeH(), info: await infoW() };
    check("the three sizes are the same after a reload", JSON.stringify(restored) === JSON.stringify(mine), JSON.stringify(restored));
    await page.evaluate(() => localStorage.setItem("agent-tower:layout", JSON.stringify({ col: 5000, office: 9000, info: 9000 })));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(400);
    const bigRestored = { col: await colW(), area: (await sizeOf(".area")).h, info: await infoW(), office: (await sizeOf(".office")).w };
    check(
        "stored sizes that are too large are clamped at the start (column 50%, terminal area 160 px, room 120 px)",
        bigRestored.col === 640 && bigRestored.area >= 159 && bigRestored.area <= 175 && bigRestored.info === bigRestored.office - 120,
        JSON.stringify(bigRestored),
    );
    await page.evaluate(() => localStorage.setItem("agent-tower:layout", "{this is not json"));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(300);
    check("bad stored text gives the default sizes", (await colW()) === 320 && (await officeH()) === 336 && (await infoW()) === 456);
    // A clamp when the window changes.
    await key(H_COL, "ArrowRight");
    for (let i = 0; i < 10; i++) await page.keyboard.press("ArrowRight");
    await page.waitForTimeout(100);
    check("11 Right keys widen the column by 16 px each", (await colW()) === 320 + 11 * 16, String(await colW()));
    await page.setViewportSize({ width: 800, height: 560 });
    await page.waitForTimeout(400);
    check(
        "when the window gets smaller the column is clamped to 50% of the new width",
        (await colW()) === 400 && (await page.locator(H_COL).getAttribute("aria-valuemax")) === "400",
        String(await colW()),
    );
    await page.evaluate(() => localStorage.setItem("agent-tower:layout", JSON.stringify({ office: 700 })));
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.setViewportSize({ width: 1280, height: 520 });
    await page.waitForTimeout(400);
    const small = (await sizeOf(".area")).h;
    check("when the window gets lower the office shrinks so that the terminal area keeps 160 px", small >= 159 && small <= 175, String(small));
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(300);

    await page.evaluate(() => sessionStorage.removeItem("e2e-hide-board"));
    await page.reload();
    await page.waitForSelector(".storey.on");
    await page.waitForTimeout(300);

    // ================= 17. A loading state when a session starts (Feature B) =================
    const setMock = (config) => page.evaluate((c) => (window.__towerMockConfig = c), config);
    const overlay = (kind) => page.locator(`[data-starting="${kind}"]`);
    const activeOverlayText = async (kind) => (await overlay(kind).innerText()).replace(/\s+/g, " ").trim();
    const elapsedOf = async (kind) => Number((await overlay(kind).locator('[data-role="elapsed"]').innerText()).match(/\d+/)[0]);
    const sid = () => page.locator(".pane:not(.hidden)").getAttribute("data-session");
    await setMock({ startCallMs: 700, startingMs: 600_000 });

    // 17a. Busy buttons, from the click until the command returns.
    await selectEmp("ceo");
    await bar.locator("button.start").click();
    await page.waitForSelector("button.start[aria-busy=true]");
    check(
        'the clicked "start" button is disabled and shows a spinner and "starting..." until the command returns',
        (await bar.locator("button.start").isDisabled()) &&
            (await bar.locator("button.start").innerText()).trim() === "starting..." &&
            (await bar.locator("button.start .spinner").count()) === 1,
        await bar.locator("button.start").innerText(),
    );
    check("no tab exists yet while the command runs", (await page.locator(".tab").count()) === 0);
    await page.waitForSelector(".tab.active");
    const adaId = await sid();

    // 17b. The tab spinner and the centre overlay.
    check(
        "the tab of a starting session shows a spinner in place of the status dot",
        (await page.locator('.tab.active .dot.spin[data-status="starting"]').count()) === 1 &&
            (await page.locator(".tab.active .dot.spin").evaluate((e) => getComputedStyle(e).animationName)) !== "none",
    );
    await overlay("centre").waitFor();
    const centreText = await activeOverlayText("centre");
    check("the centre overlay names the CLI and the employee", centreText.includes("Starting claude for Ada..."), centreText);
    check("the centre overlay has a spinner", (await overlay("centre").locator(".spinner").count()) === 1);
    const e1 = await elapsedOf("centre");
    await page.waitForTimeout(2300);
    const e2 = await elapsedOf("centre");
    check("the elapsed time counts up each second", e2 - e1 >= 2 && e2 - e1 <= 3, `${e1} s -> ${e2} s`);
    check(
        "the centre overlay is in the middle of the terminal pane",
        await page.evaluate(() => {
            const o = document.querySelector('[data-starting="centre"]').getBoundingClientRect();
            const p = document.querySelector(".pane:not(.hidden)").getBoundingClientRect();
            const t = document.querySelector('[data-starting="centre"] .title').getBoundingClientRect();
            return Math.abs(t.left + t.width / 2 - (p.left + p.width / 2)) < 3 && Math.abs(t.top + t.height / 2 - (p.top + p.height / 2)) < p.height * 0.2 && o.width > 0;
        }),
    );
    check("no slow hint before 10 s", (await overlay("centre").locator('[data-role="slow-hint"]').count()) === 0);
    check(
        "the overlay does not block the terminal for input (no pointer events, and the terminal is under the pointer)",
        await page.evaluate(() => {
            const o = document.querySelector('[data-starting="centre"]');
            const r = o.getBoundingClientRect();
            const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
            return getComputedStyle(o).pointerEvents === "none" && !!hit?.closest(".xterm");
        }),
    );
    await page.locator(".pane:not(.hidden) .xterm-helper-textarea").focus();
    check("the terminal can take the focus while the overlay shows", await page.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")));
    await page.screenshot({ path: join(resizeShots, "8-centre-overlay.png") });

    // 17c. Output during the start: the centre overlay turns into the strip.
    await page.evaluate((id) => window.__towerMock.output(id, "Do you trust the files in this folder? (y/n)\r\n"), adaId);
    await overlay("strip").waitFor();
    check("the first output turns the centre overlay into a strip", (await overlay("centre").count()) === 0 && (await overlay("strip").count()) === 1);
    await waitScreen("Do you trust the files");
    const stripBox = await page.evaluate(() => {
        const s = document.querySelector('[data-starting="strip"]').getBoundingClientRect();
        const p = document.querySelector(".pane:not(.hidden)").getBoundingClientRect();
        return { right: p.right - s.right, top: s.top - p.top, width: s.width, paneW: p.width, height: s.height };
    });
    check("the strip is small and at the top right of the pane", stripBox.right >= 0 && stripBox.right < 40 && stripBox.top >= 0 && stripBox.top < 20 && stripBox.width < stripBox.paneW / 2 && stripBox.height < 40, JSON.stringify(stripBox));
    const stripText = await activeOverlayText("strip");
    check('the strip says "starting..." and the seconds', /^starting\.\.\. \d+ s$/.test(stripText), stripText);
    const s1 = await elapsedOf("strip");
    await page.waitForTimeout(1200);
    check("the time in the strip goes on counting (and did not restart)", (await elapsedOf("strip")) > s1 && s1 >= 2, `${s1} -> ${await elapsedOf("strip")}`);
    check("the strip takes no pointer events", (await overlay("strip").evaluate((e) => getComputedStyle(e).pointerEvents)) === "none");
    await page.screenshot({ path: join(resizeShots, "9-strip.png") });

    // 17d. The status changes: both go away, and the tab shows the dot again.
    await page.evaluate((id) => window.__towerMock.leaveStarting(id), adaId);
    await page.waitForSelector('.tab.active .dot[data-status="idle"]');
    check("the strip goes away when the status is not starting any more", (await page.locator("[data-starting]").count()) === 0);
    check("the tab shows the status dot again (no spinner)", (await page.locator(".tab.active .dot.spin").count()) === 0);

    // 17e. A resume that has scrollback output at once: the strip shows from the start.
    await setMock({ startCallMs: 700, startingMs: 600_000, startingText: "Trust this folder? 1. Yes  2. No\r\n" });
    await selectEmp("eng-lin");
    await page.waitForSelector(".bar button.resume");
    await bar.locator("button.resume").click();
    await page.waitForSelector("button.resume[aria-busy=true]");
    check(
        'the clicked "resume" button shows "resuming..." and the "start" button is disabled too',
        (await bar.locator("button.resume").innerText()).trim() === "resuming..." && (await bar.locator("button.start").isDisabled()),
    );
    await page.waitForSelector('.tab.active:has-text("Lin")');
    await overlay("strip").waitFor();
    check(
        "output that is in the scrollback of the attach also gives the strip (no centre overlay)",
        (await overlay("centre").count()) === 0 && (await overlay("strip").count()) === 1,
    );
    await waitScreen("Trust this folder?");
    await page.evaluate((id) => window.__towerMock.leaveStarting(id), await sid());
    await page.waitForSelector("[data-starting]", { state: "detached" });
    check("the strip of the resumed session goes away with the status", (await page.locator("[data-starting]").count()) === 0);

    // 17f. The setup agent button.
    await setMock({ startCallMs: 700, startingMs: 600_000 });
    await page.click("button.setup:not(.stop-setup)");
    await page.click("button.cli-codex");
    await page.waitForSelector("button.setup[aria-busy=true]");
    check(
        'the setup button is disabled and shows "starting..." with a spinner until the command returns',
        (await page.locator("button.setup").isDisabled()) &&
            (await page.locator("button.setup").innerText()).trim() === "starting..." &&
            (await page.locator("button.setup .spinner").count()) === 1 &&
            (await page.locator("button.cli-claude").count()) === 0,
    );
    await page.waitForSelector('.tab.active:has-text("Setup")');
    await overlay("centre").waitFor();
    const setupCentre = await activeOverlayText("centre");
    check("the setup overlay says: Starting codex for Setup...", setupCentre.includes("Starting codex for Setup..."), setupCentre);
    check("while the setup session starts the button offers to stop it", (await page.locator("button.setup").innerText()) === "Stop the setup");
    await page.evaluate((id) => window.__towerMock.leaveStarting(id), await sid());
    await page.waitForSelector("[data-starting]", { state: "detached" });
    await page.click("button.setup");
    await page.waitForSelector("button.setup:not(.stop-setup)");

    // 17g. The slow hint after 10 s and the "still starting" line after 90 s, with a fake clock.
    await setMock({ startCallMs: 0, startingMs: 600_000 });
    await selectEmp("pm");
    await page.waitForSelector(".bar button.start:not([disabled])");
    const stopsBeforeStuck = (await calls("session_stop")).length;
    await bar.locator("button.start").click();
    await page.waitForSelector('.tab.active:has-text("Maya")');
    await overlay("centre").waitFor();
    await page.clock.install();
    // The lines come at the right time, not before: look just before each limit, then just after it.
    await page.clock.fastForward(8_000);
    await page.waitForTimeout(1300); // the page timer ticks each second of real time
    check("at about 9 s there is no slow hint yet", (await page.locator('[data-role="slow-hint"]').count()) === 0, String(await elapsedOf("centre")));
    await page.clock.fastForward(2_500);
    await page.locator('[data-role="slow-hint"]').waitFor({ timeout: 2500 }).catch(() => {});
    check(
        "after 10 s the centre overlay adds the line about a slow load",
        (await page.locator('[data-role="slow-hint"]').count()) === 1 &&
            (await page.locator('[data-role="slow-hint"]').innerText()) ===
                "The CLI loads its tools and the project. In a large project this can take more than one minute." &&
            (await page.locator('[data-role="stuck"]').count()) === 0,
        String(await elapsedOf("centre")),
    );
    await page.clock.fastForward(72_000);
    await page.waitForTimeout(1300);
    check("at about 85 s there is no still-starting line yet", (await page.locator('[data-role="stuck"]').count()) === 0, String(await elapsedOf("centre")));
    await page.clock.fastForward(8_000);
    await page.locator('[data-role="stuck"]').waitFor({ timeout: 2500 }).catch(() => {});
    check(
        "after 90 s the overlay adds: Still starting. If nothing shows, stop the session and look at the program paths in Settings.",
        (await page.locator('[data-role="stuck"]').count()) === 1 &&
            (await page.locator('[data-role="stuck"]').innerText()) === "Still starting. If nothing shows, stop the session and look at the program paths in Settings." &&
            (await elapsedOf("centre")) >= 90,
        String(await elapsedOf("centre")),
    );
    check("the 90 s line takes no automatic action (the session is still starting and live)", (await page.locator('.tab.active .dot[data-status="starting"]').count()) === 1);
    await page.evaluate((id) => window.__towerMock.output(id, "x\r\n"), await sid());
    await overlay("strip").waitFor();
    check(
        "in the strip the stuck line stays and the slow hint is gone",
        (await page.locator('[data-starting="strip"] [data-role="stuck"]').count()) === 1 && (await page.locator('[data-role="slow-hint"]').count()) === 0,
    );
    await page.evaluate((id) => window.__towerMock.leaveStarting(id), await sid());
    await page.waitForSelector("[data-starting]", { state: "detached" });
    check("no session was stopped by the app during the whole wait", (await calls("session_stop")).length === stopsBeforeStuck);

    // 18. A session that an employee starts (the tool employee_start of the real app): the UI did
    // not call session_start. It must show as a tab with a status, show the status on the
    // character and in the list, keep the active tab, and show one toast.
    {
        await setMock({ startCallMs: 0, startingMs: 600_000 });
        const startsBefore = (await calls("session_start")).length;
        const activeBefore = await sid();
        const tabsBefore = await page.locator(".tab").count();
        const target = await page.evaluate(() =>
            [...document.querySelectorAll(".emp")].find((g) => g.dataset.status === "stopped")?.dataset.slug,
        );
        const targetName = await page.evaluate((slug) => document.querySelector(`.tag[data-slug="${slug}"] .tag-name`)?.textContent ?? slug, target);
        const newId = await page.evaluate((slug) => window.__towerMock.employeeStart("/home/demo/tower", "pm", slug), target);
        await page.waitForSelector(`.tab[data-session="${newId}"]`);
        check("a session that an employee started gets a tab", (await page.locator(".tab").count()) === tabsBefore + 1);
        check(
            "the UI made no session_start call for it",
            (await calls("session_start")).length === startsBefore,
        );
        check(
            "the tab shows the status starting",
            (await page.locator(`.tab[data-session="${newId}"] .dot`).getAttribute("data-status")) === "starting",
        );
        check(
            "the character of that employee shows a live status in the room",
            (await emp(target).getAttribute("data-status")) === "starting",
            (await emp(target).getAttribute("data-status")) ?? "",
        );
        const rowStatus = await page.evaluate(
            (slug) => document.querySelector(`.roster .row[data-slug="${slug}"] [data-status]`)?.getAttribute("data-status") ?? "none",
            target,
        );
        check("the employee list shows the same status", rowStatus === "starting", rowStatus);
        check("the active tab did not change", (await sid()) === activeBefore && (await page.locator(".tab.active").getAttribute("data-session")) === activeBefore);
        const toast = page.locator('.toast[data-kind="info"]');
        await toast.first().waitFor();
        check("one toast says who started whom", (await toast.count()) === 1 && (await toast.innerText()).replace(/\s+/g, " ").trim().startsWith(`Maya started ${targetName}`), `target=${JSON.stringify(targetName)} count=${await toast.count()} ` + (await toast.allInnerTexts().then((t) => JSON.stringify(t))));
        check("the notice toast is not an alert", (await toast.getAttribute("role")) === "status");
        await page.evaluate((id) => window.__towerMock.leaveStarting(id), newId);
        await page.waitForSelector(`.tab[data-session="${newId}"] .dot[data-status="idle"]`);
        check("the status of the new tab follows session-changed", true);
        // A click on its tab is the only thing that moves the focus.
        await page.locator(`.tab[data-session="${newId}"]`).click();
        await page.waitForSelector(`.tab.active[data-session="${newId}"]`);
        check("the operator can open the new tab", (await sid()) === newId);
    }


    // ================= 19. The task board (M2 slice 2, docs/ipc-m2.md) =================
    {
        const FLOOR = "/home/demo/tower";
        const boardShots = process.env.E2E_BOARD_SHOTS ?? "/tmp/agent-tower-review/m2b/board";
        mkdirSync(boardShots, { recursive: true });
        const H_BOARD = '[role="separator"][aria-label="Resize the task board"]';
        await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
        await page.setViewportSize({ width: 1280, height: 800 });
        await page.reload();
        await page.waitForSelector(".storey.on");
        await page.waitForSelector('.board .card');
        await page.waitForTimeout(300);
        const boardW = async () => Math.round((await page.locator(".board").boundingBox()).width);
        const workW = async () => Math.round((await page.locator("main.work").boundingBox()).width);
        const groupNames = () => page.$$eval(".board .group .g-name", (els) => els.map((e) => e.textContent.trim()));
        const cardIds = (status) =>
            page.$$eval(`.board .group[data-status="${status}"] .card`, (els) => els.map((e) => Number(e.dataset.task)));
        await page.screenshot({ path: join(boardShots, "1-board.png") });

        // 19a. The default board: 260 px at the right, the terminal keeps its room.
        check("the board is at the right of the work area, 260 px wide", (await boardW()) === 260, String(await boardW()));
        const boxes = await page.evaluate(() => {
            const b = document.querySelector(".board").getBoundingClientRect();
            const w = document.querySelector("main.work").getBoundingClientRect();
            return { boardLeft: b.left, boardRight: b.right, workRight: w.right, win: window.innerWidth };
        });
        check("the board touches the right edge of the window and starts where the work area ends",
            Math.abs(boxes.boardRight - boxes.win) < 1.5 && Math.abs(boxes.boardLeft - boxes.workRight) < 1.5, JSON.stringify(boxes));
        check("at 1280 x 800 the terminal area is at least 480 px wide", (await workW()) >= 480, String(await workW()));
        const termW = await page.locator(".area").boundingBox();
        check("the terminal area is not cut: it is inside the window", termW.x + termW.width <= 1280 - 259, JSON.stringify(termW));
        check("there are 4 separators: the board has one", (await page.$$('[role="separator"]')).length === 4);
        check("the page has no horizontal scroll", await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));

        // 19b. The groups and the cards.
        check("the groups are in order: in progress, review, blocked, to do, done",
            JSON.stringify(await groupNames()) === JSON.stringify(["In progress", "Review", "Blocked", "To do", "Done"]),
            JSON.stringify(await groupNames()));
        check("a group shows its count",
            (await page.locator('.board .group[data-status="done"] .g-count').innerText()) === "1");
        check("done is collapsed at the start: its card is not shown",
            (await cardIds("done")).length === 0 &&
            (await page.locator('.board .group[data-status="done"] .group-head').getAttribute("aria-expanded")) === "false");
        check("the other groups show their cards", (await cardIds("in_progress")).join() === "2" && (await cardIds("review")).join() === "3" &&
            (await cardIds("blocked")).join() === "4" && (await cardIds("todo")).join() === "5");
        const card2 = page.locator('.board .card[data-task="2"]');
        check("a card shows the ID, the title and the name of the assignee",
            (await card2.locator(".c-id").innerText()) === "T-2" &&
            (await card2.locator(".c-title").innerText()) === "Write the login form" &&
            (await card2.locator(".c-who").innerText()) === "Lin",
            (await card2.innerText()).replace(/\n/g, " | "));
        check("a card of a task with no assignee says Unassigned", (await page.locator('.board .card[data-task="5"] .c-who').innerText()) === "Unassigned");
        const colours = await page.$$eval(".board .card", (els) =>
            Object.fromEntries(els.map((e) => [e.dataset.status, getComputedStyle(e).borderLeftColor])));
        check("each status has its own colour on the card",
            new Set(Object.values(colours)).size === 4 && colours.in_progress === "rgb(90, 169, 242)" && colours.review === "rgb(255, 197, 107)" &&
            colours.blocked === "rgb(255, 122, 144)" && colours.todo === "rgb(147, 162, 194)", JSON.stringify(colours));
        await page.locator('.board .group[data-status="done"] .group-head').click();
        check("a click on the done group shows its card", (await cardIds("done")).join() === "1");
        await page.locator('.board .group[data-status="done"] .group-head').click();
        check("a second click collapses it again", (await cardIds("done")).length === 0);
        check("the header counts the open tasks (not done)", (await page.locator(".board .open-count").innerText()) === "4 open");

        // 19c. A click opens the detail panel.
        await page.locator('.board .card[data-task="3"]').click();
        await page.waitForSelector('.board [aria-label="Task detail"] pre.report');
        const detail = page.locator('.board [aria-label="Task detail"]');
        check("a click on a card opens the detail panel and hides the list", (await page.locator(".board .group").count()) === 0 && (await detail.count()) === 1);
        check("the detail shows the ID, the title, the status and the assignee",
            (await detail.locator(".id").innerText()) === "T-3" && (await detail.locator(".title").innerText()) === "Check the build on Linux" &&
            (await detail.locator('[data-testid="detail-status"]').innerText()) === "Review" &&
            (await detail.locator('[data-testid="detail-assignee"]').innerText()) === "Sam");
        check("the detail shows the description", (await detail.locator(".text").innerText()) === "Run the build and report.");
        const eventLines = await detail.locator(".events li").allInnerTexts();
        check("the events have a time, an author and the action",
            eventLines.length === 3 && /^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\n/.test(eventLines[0]) &&
            eventLines[0].includes("Maya made the task") &&
            eventLines[1].includes("Maya gave the task to Sam") && eventLines[2].includes("Sam set the status to Review"),
            JSON.stringify(eventLines));
        check("the report text shows in a scroll box as plain text",
            (await detail.locator("pre.report").innerText()).startsWith("# T-3 report") &&
            (await detail.locator("pre.report").evaluate((e) => getComputedStyle(e).overflow)) === "auto");
        check("the report was read with task_report", (await calls("task_report")).some((c) => c.args.path === FLOOR && c.args.id === 3));
        check("the detail has the controls of the operator: assign, status and note",
            (await detail.locator('select[aria-label="Assign to"]').count()) === 1 &&
            (await detail.locator('select[aria-label="Status"]').count()) === 1 &&
            (await detail.locator('textarea[aria-label="Note"]').count()) === 1);
        await page.screenshot({ path: join(boardShots, "2-detail.png") });

        // 19d. Assign and set the status: the commands get the contract arguments.
        await detail.locator('select[aria-label="Assign to"]').selectOption("eng-lin");
        await detail.getByRole("button", { name: "Assign", exact: true }).click();
        await page.waitForFunction(() => document.querySelector('[data-testid="detail-assignee"]')?.textContent === "Lin");
        const assignCall = (await calls("task_assign")).pop();
        check("assign calls task_assign with path, id and slug", JSON.stringify(assignCall.args) === JSON.stringify({ path: FLOOR, id: 3, slug: "eng-lin" }), JSON.stringify(assignCall.args));
        check("the detail shows the new assignee and an assigned event",
            (await detail.locator(".events li").count()) === 4 && (await detail.locator(".events li").last().innerText()).includes("Operator gave the task to Lin"));
        await detail.locator('select[aria-label="Status"]').selectOption("done");
        await detail.locator('textarea[aria-label="Note"]').fill("Looks good");
        await detail.getByRole("button", { name: "Set the status" }).click();
        await page.waitForFunction(() => document.querySelector('[data-testid="detail-status"]')?.textContent === "Done");
        const updateCall = (await calls("task_update")).pop();
        check("set the status calls task_update with path, id, status and note",
            JSON.stringify(updateCall.args) === JSON.stringify({ path: FLOOR, id: 3, status: "done", note: "Looks good" }), JSON.stringify(updateCall.args));
        check("the note is in the events and the field is empty again",
            (await detail.locator(".events li").last().innerText()).includes("Looks good") && (await detail.locator('textarea[aria-label="Note"]').inputValue()) === "");
        // A note alone keeps the status.
        await detail.locator('textarea[aria-label="Note"]').fill("One more thing");
        await detail.getByRole("button", { name: "Add the note" }).click();
        await page.waitForFunction(() => document.querySelectorAll(".board .events li").length === 6);
        const noteCall = (await calls("task_update")).pop();
        check("a note alone is task_update with the same status",
            noteCall.args.status === "done" && noteCall.args.note === "One more thing", JSON.stringify(noteCall.args));
        check("the button for a note with no status change is off while the note is empty",
            await detail.getByRole("button", { name: "Add the note" }).isDisabled());
        await detail.getByRole("button", { name: /Back/ }).click();
        check("Back shows the list again, with the changed task in the done group",
            (await cardIds("done")).length === 0 && (await page.locator('.board .group[data-status="done"] .g-count').innerText()) === "2" &&
            (await page.locator(".board .open-count").innerText()) === "3 open");

        // 19e. The form makes a task.
        await page.getByRole("button", { name: "+ New" }).click();
        const form = page.locator('.board form[aria-label="New task"]');
        check("the new task button is off while the title is empty", await form.getByRole("button", { name: "Make the task" }).isDisabled());
        await form.locator('input[aria-label="Title of the new task"]').fill("Ship the demo");
        await form.locator('textarea').fill("Write the notes.\nSecond line.");
        await form.getByRole("button", { name: "Make the task" }).click();
        await page.waitForSelector('.board [aria-label="Task detail"]');
        const createCall = (await calls("task_create")).pop();
        check("the form calls task_create with path, title and description",
            JSON.stringify(createCall.args) === JSON.stringify({ path: FLOOR, title: "Ship the demo", description: "Write the notes.\nSecond line." }), JSON.stringify(createCall.args));
        check("the new task opens in the detail panel, in To do, made by the operator, with no assignee",
            (await page.locator('[data-testid="detail-status"]').innerText()) === "To do" &&
            (await page.locator('[data-testid="detail-assignee"]').innerText()) === "Unassigned" &&
            (await page.locator(".board .title").innerText()) === "Ship the demo");
        await page.getByRole("button", { name: /Back/ }).click();
        check("the new task is a card in the To do group", (await cardIds("todo")).join() === "5,6");

        // 19f. The board refreshes on tasks-changed (an employee changes a task).
        const listCalls = (await calls("task_list")).length;
        await page.evaluate((p) => window.__towerMock.taskByEmployee(p, "eng-lin", { id: 2, status: "review", note: "ready" }), FLOOR);
        await page.waitForFunction(() => document.querySelector('.board .group[data-status="review"] .card[data-task="2"]'));
        check("a task that an employee changed moves to its new group with no click", (await cardIds("review")).join() === "2" && (await cardIds("in_progress")).length === 0);
        check("the board read the list again after the event", (await calls("task_list")).length > listCalls);
        const newId = await page.evaluate((p) => window.__towerMock.taskByEmployee(p, "pm", { create: "Employee task" }), FLOOR);
        await page.waitForSelector(`.board .card[data-task="${newId}"]`);
        check("a task that an employee made shows up", (await page.locator(`.board .card[data-task="${newId}"] .c-who`).innerText()) === "Unassigned");
        // The open detail refreshes too.
        await page.locator('.board .card[data-task="2"]').click();
        await page.waitForSelector('.board [aria-label="Task detail"] .events li');
        const eventsBefore = await page.locator(".board .events li").count();
        await page.evaluate((p) => window.__towerMock.taskByEmployee(p, "pm", { id: 2, status: "blocked", note: "waits for the logo" }), FLOOR);
        await page.waitForFunction((n) => document.querySelectorAll(".board .events li").length > n, eventsBefore);
        check("the open detail shows the new event and status",
            (await page.locator('[data-testid="detail-status"]').innerText()) === "Blocked" && (await page.locator(".board .events li").last().innerText()).includes("waits for the logo"));
        await page.screenshot({ path: join(boardShots, "3-refreshed.png") });
        // An event for another floor does not change this board.
        const before2 = (await calls("task_list")).length;
        await page.evaluate(() => window.__towerMock.taskByEmployee("/home/demo/second", "ceo", { create: "Other floor" }));
        await page.waitForTimeout(200);
        check("an event of another floor does not read this floor again", (await calls("task_list")).length === before2);
        await page.getByRole("button", { name: /Back/ }).click();

        // 19g. The resize handle, like the other panes.
        check("the board handle is a vertical separator with a label and 260 as its value",
            (await page.locator(H_BOARD).getAttribute("aria-orientation")) === "vertical" && (await page.locator(H_BOARD).getAttribute("aria-valuenow")) === "260");
        await drag(H_BOARD, -60, 0);
        check("a drag to the left makes the board 60 px wider", (await boardW()) === 320, String(await boardW()));
        check("the terminal area shrinks by the same amount", (await workW()) === 640, String(await workW()));
        await drag(H_BOARD, -900, 0);
        check("a drag far to the left stops where the terminal keeps 480 px", (await boardW()) === 480 && (await workW()) === 480, `${await boardW()} ${await workW()}`);
        await drag(H_BOARD, 900, 0);
        check("a drag far to the right stops at 200 px", (await boardW()) === 200, String(await boardW()));
        await key(H_BOARD, "ArrowLeft");
        check("the Left key widens the board by 16 px", (await boardW()) === 216, String(await boardW()));
        await key(H_BOARD, "ArrowRight");
        check("the Right key narrows it by 16 px", (await boardW()) === 200, String(await boardW()));
        await drag(H_BOARD, -40, 0);
        const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("agent-tower:layout")));
        check("the width is in localStorage like the other sizes", stored.board === 240 && stored.col === null, JSON.stringify(stored));
        await page.reload();
        await page.waitForSelector(".storey.on");
        await page.waitForSelector(".board .card");
        check("the width is the same after a reload", (await boardW()) === 240, String(await boardW()));
        await page.locator(H_BOARD).dblclick();
        check("a double click resets the board to 260 px", (await boardW()) === 260, String(await boardW()));

        // 19h. Collapse: a narrow strip with the number of open tasks.
        const openNow = (await page.locator(".board .open-count").innerText()).replace(" open", "");
        await page.getByRole("button", { name: "Collapse the task board" }).click();
        await page.waitForSelector(".board.collapsed");
        check("the collapsed board is a strip of 36 px", (await boardW()) === 36, String(await boardW()));
        check("the strip shows the count of open tasks", (await page.locator('[data-testid="board-count"]').innerText()) === openNow, `${await page.locator('[data-testid="board-count"]').innerText()} vs ${openNow}`);
        check("the terminal gets the room", (await workW()) === 1280 - 320 - 36, String(await workW()));
        check("the strip has no handle and no cards", (await page.locator(H_BOARD).count()) === 0 && (await page.locator(".board .card").count()) === 0);
        await page.screenshot({ path: join(boardShots, "4-collapsed.png") });
        // The count follows the events while the board is collapsed.
        await page.evaluate((p) => window.__towerMock.taskByEmployee(p, "pm", { create: "While collapsed" }), FLOOR);
        await page.waitForFunction((n) => Number(document.querySelector('[data-testid="board-count"]').textContent) === n + 1, Number(openNow));
        check("the count of the strip goes up when a task is made", true);
        await page.reload();
        await page.waitForSelector(".board.collapsed");
        check("the collapsed state is the same after a reload", (await boardW()) === 36);
        await page.getByRole("button", { name: /Open the task board/ }).click();
        await page.waitForSelector(".board .card");
        check("a click on the strip opens the board again, with its width", (await boardW()) === 260, String(await boardW()));

        // 19i. A narrow window: the board is collapsed at the start, and the terminal keeps 480 px.
        await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
        await page.setViewportSize({ width: 980, height: 700 });
        await page.reload();
        await page.waitForSelector(".storey.on");
        await page.waitForTimeout(300);
        check("below the room for a 260 px board the board starts collapsed", (await page.locator(".board.collapsed").count()) === 1 && (await workW()) >= 480, String(await workW()));
        await page.getByRole("button", { name: /Open the task board/ }).click();
        await page.waitForTimeout(200);
        const narrowOpen = { board: await boardW(), work: await workW() };
        check("an open board in a narrow window is not narrower than 200 px", narrowOpen.board >= 200, JSON.stringify(narrowOpen));
        await page.screenshot({ path: join(boardShots, "5-narrow.png") });
        await page.evaluate(() => localStorage.removeItem("agent-tower:layout"));
        await page.setViewportSize({ width: 1280, height: 800 });
        await page.reload();
        await page.waitForSelector(".storey.on");
        await page.waitForSelector(".board .card");
        check("at 1280 x 800 again the board is open by default", (await boardW()) === 260);
    }

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
