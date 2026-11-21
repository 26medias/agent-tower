// Checks of the last repairs ("final-b2" review): N1 (the top strip), N2 (the toast), N3 (the texts, the
// chip, the zoom of the tags) and the look items (one row of buttons, the info panel without a scroll, the
// list "Team activity"). `mock-page.mjs` calls `lastChecks` after `repairChecks`, with its own browser page.
// The mock page (the in-memory backend) runs here: `window.__towerMock` freezes the automatic status changes
// and sets a status, so that each check has a fixed state.
import { join } from "node:path";

const FLOOR = "/home/demo/tower";
const BIG = "/home/demo/big";
const SMALL = { width: 1280, height: 800 };
const LARGE = { width: 2560, height: 1400 };

export async function lastChecks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const M = (name, ...args) => page.evaluate(([n, a]) => window.__towerMock[n](...a), [name, args]);
    const rgb = (css) => (css.match(/\d+(\.\d+)?/g) ?? []).slice(0, 3).map(Number);
    const isAmber = (css) => {
        const [r, g, b] = rgb(css);
        return r >= 240 && g >= 180 && g <= 235 && b >= 90 && b <= 175;
    };
    const flat = (t) => t.replace(/\s+/g, " ").trim();
    const clearToasts = async () => {
        const dismiss = page.locator('.toast button[aria-label="Dismiss"]');
        while ((await dismiss.count()) > 0) await dismiss.first().click();
    };
    const attention = page.locator('[data-testid="attention-bar"]');

    async function fresh(size = SMALL) {
        await page.evaluate(() => localStorage.clear());
        await page.setViewportSize(size);
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        await M("freeze", true);
        await page.waitForTimeout(150);
    }

    // The boxes that must not move: the strip, the office, the room, the tabs, the terminal area and the live pane.
    const metrics = () =>
        page.evaluate(() => {
            const r = (sel) => {
                const e = document.querySelector(sel);
                if (!e) return null;
                const b = e.getBoundingClientRect();
                return [b.x, b.y, b.width, b.height].map((n) => Math.round(n * 10) / 10);
            };
            return {
                strip: r('[data-testid="top-strip"]'),
                office: r(".office"),
                room: r(".office .room"),
                info: r(".office .info"),
                tabs: r(".tabs"),
                area: r(".area"),
                pane: r(".pane:not(.hidden)"),
            };
        });

    // ================= N1. The top strip: nothing below it moves =================
    for (const [label, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        const ceo = await M("startSession", FLOOR, "ceo");
        await page.waitForSelector(`.tab[data-session="${ceo}"]`);
        await M("setStatus", ceo, "idle");
        await page.waitForTimeout(250);
        const none = await metrics();
        check(`N1 ${label}: with nothing to say the strip is a row of 30 px at the top and shows the name and the path`,
            none.strip[1] === 0 && none.strip[3] === 30 && Math.abs(none.strip[1] + none.strip[3] - none.office[1]) <= 0.5 &&
                (await page.locator('[data-testid="top-strip"] h2.name').innerText()) === "Demo Tower" &&
                (await page.locator('[data-testid="top-strip"] .path').innerText()) === FLOOR, JSON.stringify(none.strip));
        await shot(`n1-${label}-normal.png`);

        // One question.
        await M("setStatus", ceo, "question");
        await attention.waitFor();
        await page.waitForTimeout(200);
        const one = await metrics();
        check(`N1 ${label}: a question does not move the office, the room, the tabs and the terminal (same boxes)`, JSON.stringify(one) === JSON.stringify(none), JSON.stringify({ none, one }));
        check(`N1 ${label}: the strip is amber with a question`, isAmber(await page.locator('[data-testid="top-strip"]').evaluate((e) => getComputedStyle(e).backgroundColor)));
        await shot(`n1-${label}-question.png`);

        // Two questions on two floors (Big Office was never opened).
        const pia = await M("startSession", BIG, "pm");
        await M("setStatus", pia, "question");
        await page.waitForFunction(() => document.querySelector('[data-testid="attention-bar"]')?.dataset.count === "2");
        await page.waitForTimeout(200);
        const two = await metrics();
        check(`N1 ${label}: two questions on two floors do not move anything either`, JSON.stringify(two) === JSON.stringify(none), JSON.stringify({ none, two }));
        await shot(`n1-${label}-two-questions.png`);

        // The gone banner, with the questions, then without.
        await M("goneFloor", FLOOR);
        await page.locator('[data-testid="gone-banner"]').waitFor();
        await page.waitForTimeout(250);
        const both = await metrics();
        check(`N1 ${label}: the gone banner and two questions together do not move anything (the strip stays 30 px)`, JSON.stringify(both) === JSON.stringify(none), JSON.stringify({ none, both }));
        await M("setStatus", ceo, "idle");
        await M("setStatus", pia, "idle");
        await page.waitForFunction(() => !document.querySelector('[data-testid="attention-bar"]'));
        await page.waitForTimeout(250);
        const gone = await metrics();
        check(`N1 ${label}: the gone banner alone does not move anything`, JSON.stringify(gone) === JSON.stringify(none), JSON.stringify({ none, gone }));
        await shot(`n1-${label}-gone.png`);

        // N2. A toast of another floor starts under the strip, and covers no button of the strip.
        await clearToasts();
        await M("setStatus", ceo, "question");
        await attention.waitFor();
        await M("notice", BIG, "The folder of the floor Big Office is gone: /home/demo/big.", "floor_gone");
        await M("notice", FLOOR, "Some notice.", "task_blocked");
        await page.waitForFunction(() => document.querySelectorAll(".toast").length === 2);
        await page.waitForTimeout(300);
        const over = await page.evaluate(() => {
            const rect = (e) => e.getBoundingClientRect();
            const hit = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
            const strip = rect(document.querySelector('[data-testid="top-strip"]'));
            const toasts = [...document.querySelectorAll(".toast")].map(rect);
            const buttons = [...document.querySelectorAll('[data-testid="top-strip"] button')].map((b) => [b.textContent.trim(), rect(b)]);
            return {
                under: toasts.every((t) => t.top >= strip.bottom),
                covers: buttons.filter(([, b]) => toasts.some((t) => hit(t, b))).map(([n]) => n),
                names: buttons.map(([n]) => n),
                stripBottom: strip.bottom,
                toastTop: Math.min(...toasts.map((t) => t.top)),
            };
        });
        check(`N2 ${label}: the toast stack starts under the strip and covers none of the strip buttons (${over.names.join(", ")})`,
            over.under && over.covers.length === 0 && over.names.includes("Forget this floor") && over.names.includes("Check again") && over.names.includes("Open"), JSON.stringify(over));
        await shot(`n2-${label}-toast.png`);
        await clearToasts();
    }

    // ================= N3. Texts =================
    await fresh();
    // A question on a floor that the page never opened: the bar names the employee and the role, not the slug.
    const ada = await M("startSession", FLOOR, "ceo");
    await M("setStatus", ada, "question");
    const pia = await M("startSession", BIG, "pm");
    await M("setStatus", pia, "question");
    await page.waitForFunction(() => document.querySelector('[data-testid="attention-bar"]')?.dataset.count === "2");
    await page.waitForFunction(() => /Next: Ada \(CEO\)/.test(document.querySelector('[data-testid="attention-bar"]').textContent));
    await attention.getByRole("button", { name: "Open" }).click();
    await page.waitForFunction(() => /Next: Pia \(Project Manager\) on Big Office/.test(document.querySelector('[data-testid="attention-bar"]')?.textContent ?? ""), null, { timeout: 4000 }).catch(() => {});
    const nextText = flat(await attention.innerText());
    check("N3: a question on a floor that was never opened names the employee and the role (Pia (Project Manager) on Big Office), not the slug",
        /Next: Pia \(Project Manager\) on Big Office/.test(nextText) && !/\bpm\b/.test(nextText), nextText);
    await page.waitForTimeout(200);
    await M("setStatus", ada, "idle");
    await page.waitForFunction(() => document.querySelector('[data-testid="attention-bar"]')?.dataset.count === "1");
    const oneText = flat(await attention.innerText());
    check("N3: one question on another floor says: Pia (Project Manager) asks a question on Big Office", /Pia \(Project Manager\) asks a question on Big Office/.test(oneText), oneText);
    check("N3: the list of the floor was read once with floor_open (the call does not change the open floor)",
        (await page.evaluate(() => window.__towerMockCalls.filter((c) => c.command === "floor_open" && c.args.path === "/home/demo/big").length)) === 1 &&
            (await page.locator('[data-testid="top-strip"] h2.name').innerText()) === "Demo Tower");

    // The tag chip: the name never touches the chip, and the chip is inside the tag.
    await fresh(SMALL);
    await M("patchFloor", BIG, { employee: { slug: "pm", patch: { name: "WMWMWMWMWMWM" } } });
    await page.locator(`.storey[data-path="${BIG}"]`).evaluate((g) => g.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await page.waitForSelector(`.storey.on[data-path="${BIG}"]`);
    await page.waitForSelector(".tag .chip");
    const chips = await page.evaluate(() => {
        const out = [];
        for (const tag of document.querySelectorAll(".tag")) {
            const chip = tag.querySelector(".chip");
            if (!chip) continue;
            const name = tag.querySelector(".tag-name").getBBox();
            const chipRect = chip.querySelector("rect");
            const cx = Number(chipRect.getAttribute("x"));
            const cw = Number(chipRect.getAttribute("width"));
            const box = tag.querySelector("rect.box");
            const bx = Number(box.getAttribute("x"));
            const bw = Number(box.getAttribute("width"));
            out.push({ chip: chip.dataset.chip, text: tag.querySelector(".tag-name").textContent, gap: cx - (name.x + name.width), inside: cx + cw <= bx + bw - 4 && cx >= bx });
        }
        return out;
    });
    check("N3: the CEO and the PM chips have their own space: the name ends before the chip starts (a gap of at least 2 units), and the chip is inside the tag",
        chips.length === 2 && chips.every((c) => c.gap >= 2 && c.inside), JSON.stringify(chips));

    // The zoom of the room makes the tags grow, up to their natural size (14 px).
    await fresh(SMALL);
    await M("addEmployees", FLOOR, 21);
    await page.waitForFunction(() => document.querySelectorAll(".emp").length === 25);
    await page.waitForTimeout(200);
    const tagPx = () =>
        page.evaluate(() => {
            const svg = document.querySelector(".room svg");
            const g = document.querySelector(".tag");
            const scale = Number(/scale\(([\d.]+)\)/.exec(g.getAttribute("transform"))[1]);
            return Math.round(26 * scale * svg.getScreenCTM().a * 100) / 100;
        });
    const px1 = await tagPx();
    await page.getByRole("button", { name: "Zoom in" }).click();
    await page.waitForTimeout(100);
    const px14 = await tagPx();
    await page.getByRole("button", { name: "Zoom in" }).click();
    await page.waitForTimeout(100);
    const px2 = await tagPx();
    await shot("n3-zoom-1280.png");
    check("N3: with 25 employees the tag text is at least 10 px with the whole room", px1 >= 9.9, String(px1));
    check("N3: one step of zoom (x1.4) makes the tag text bigger, up to the natural size of 14 px", px14 > px1 + 2 && px14 <= 14.2, JSON.stringify({ px1, px14 }));
    check("N3: more zoom does not make the text smaller, and it stays near the natural size while the desks are small", px2 >= px14 - 0.2 && px2 <= 20, JSON.stringify({ px14, px2 }));
    await page.getByRole("button", { name: "Show the whole room" }).click();

    // The toast "answered", with an Open button that goes to the tab.
    await fresh();
    const a = await M("startSession", FLOOR, "ceo");
    const l = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${l}"]`);
    await M("setStatus", a, "idle");
    await M("setStatus", l, "idle");
    await page.locator(`.tab[data-session="${l}"] button.main`).click();
    await page.locator(`.pane[data-session="${l}"] .xterm`).click();
    await page.keyboard.type("hello");
    await page.locator(`.tab[data-session="${a}"] button.main`).click();
    await M("setStatus", l, "working");
    await M("setStatus", l, "idle");
    const answered = page.locator('.toast[data-kind="info"]').filter({ hasText: "Lin (Engineer) answered" });
    await answered.waitFor();
    check("N3: the toast says: Lin (Engineer) answered, with no jargon, and has an Open button",
        flat(await answered.locator(".text").innerText()) === "Lin (Engineer) answered" && (await answered.getByRole("button", { name: "Open" }).count()) === 1);
    await answered.getByRole("button", { name: "Open" }).click();
    await page.waitForFunction((id) => document.querySelector(`.tab[data-session="${id}"].active`), l);
    check("N3: the Open button of the toast makes the tab of Lin active and closes the toast",
        (await page.locator('.bar').getAttribute("data-slug")) === "eng-lin" && (await page.locator(".toast").filter({ hasText: "answered" }).count()) === 0);

    // ================= The office header: one row of buttons =================
    await fresh(SMALL);
    const rowsOf = () =>
        page.evaluate(() => {
            const row = document.querySelector(".office .buttons");
            const ys = [...row.children].filter((c) => c.getBoundingClientRect().height > 0).map((c) => Math.round(c.getBoundingClientRect().y));
            return { ys: [...new Set(ys)], height: row.getBoundingClientRect().height, visibleButtons: [...row.querySelectorAll("button")].filter((b) => b.getBoundingClientRect().height > 0).map((b) => b.textContent.trim()) };
        });
    const plain = await rowsOf();
    check("N3 header: the buttons of the office header are one row with one main button and a More menu", plain.ys.length === 1 && plain.height <= 40 && plain.visibleButtons.length === 2 && plain.visibleButtons[1].startsWith("More"), JSON.stringify(plain));
    await M("patchFloor", FLOOR, { defaults: { cli: "codex" } });
    await M("setResumable", FLOOR, ["ceo", "pm"]);
    await page.locator('[data-testid="resume-team"]').waitFor();
    await page.waitForTimeout(150);
    const full = await rowsOf();
    check("N3 header: with a default CLI and a team to resume it is still one row (the main button is the resume)",
        full.ys.length === 1 && full.height <= 40 && full.visibleButtons.length === 2 && /^Resume the last team/.test(full.visibleButtons[0]), JSON.stringify(full));
    const menu = page.locator('[data-testid="more-menu"]');
    check("N3 header: the menu is closed at first", await menu.isHidden());
    await page.click('[data-testid="more"]');
    const items = await menu.locator("button").allInnerTexts();
    check("N3 header: the menu holds the setup agent, the other CLI and the new logo", (await menu.isVisible()) && items.length === 3 && /setup agent/.test(items[0]) && /CLI/.test(items[1]) && /logo/i.test(items[2]), JSON.stringify(items));
    await shot("n3-menu-1280.png");
    await page.keyboard.press("Escape");
    check("N3 header: Escape closes the menu", await menu.isHidden());
    await page.click('[data-testid="more"]');
    await page.mouse.click(700, 600);
    check("N3 header: a click outside closes the menu", await menu.isHidden());
    await page.click('[data-testid="more"]');
    await menu.locator("button.newlogo").click();
    check("N3 header: an item runs and closes the menu (New logo opens its field)", (await menu.isHidden()) && (await page.locator("#logo-prompt").count()) === 1);

    // The info panel needs no scroll for 4 and for 8 employees at 1280 x 800, and the roster has two columns.
    await fresh(SMALL);
    const fits = async () =>
        page.evaluate(() => {
            const info = document.querySelector(".office .info");
            const xs = [...new Set([...document.querySelectorAll(".roster .row")].map((r) => Math.round(r.getBoundingClientRect().x)))];
            return { sh: info.scrollHeight, ch: info.clientHeight, columns: xs.length };
        });
    await page.locator('.emp[data-slug="ceo"] .hit').click();
    const f4 = await fits();
    check("N3 info: 4 employees and a selected one: the info panel needs no scroll at 1280 x 800", f4.sh <= f4.ch + 1 && f4.columns === 2, JSON.stringify(f4));
    await M("addEmployees", FLOOR, 4);
    await page.waitForFunction(() => document.querySelectorAll(".roster .row").length === 8);
    await page.waitForTimeout(200);
    const f8 = await fits();
    check("N3 info: 8 employees: the info panel needs no scroll at 1280 x 800 (the roster has two columns)", f8.sh <= f8.ch + 1 && f8.columns === 2, JSON.stringify(f8));
    await shot("n3-info-8emp-1280.png");
    check("N3 info: the list Team activity is hidden at 1280 x 800 (no room)", (await page.locator('[data-testid="team-activity"]').count()) === 0);

    // The description shows two lines, and "more" shows all of it.
    await M("setDescription", FLOOR, "A demo project with a CEO, a project manager and many engineers. It builds a small web shop with a cart, a checkout and a page for the orders. The team works in small steps, and each step has a review.");
    await page.waitForFunction(() => document.querySelector('[data-testid="description-more"]'));
    const clamp = await page.evaluate(() => {
        const d = document.querySelector('[data-testid="description"]');
        const lh = parseFloat(getComputedStyle(d).lineHeight);
        return { h: d.clientHeight, sh: d.scrollHeight, lines: Math.round(d.clientHeight / lh) };
    });
    check("N3 info: a long description shows two lines and a more control", clamp.lines === 2 && clamp.sh > clamp.h, JSON.stringify(clamp));
    await page.click('[data-testid="description-more"]');
    const open = await page.evaluate(() => document.querySelector('[data-testid="description"]').clientHeight);
    check("N3 info: more shows all the text, and less folds it again", open > clamp.h + 8 && (await page.locator('[data-testid="description-more"]').innerText()) === "less");
    await page.click('[data-testid="description-more"]');
    check("N3 info: less folds it", (await page.evaluate(() => document.querySelector('[data-testid="description"]').clientHeight)) === clamp.h);
    await M("setDescription", FLOOR, "A demo project.");
    await page.waitForTimeout(150);
    check("N3 info: a short description has no more control", (await page.locator('[data-testid="description-more"]').count()) === 0);

    // ================= Team activity at 2560 x 1400 =================
    await fresh(LARGE);
    const ceoId = await M("startSession", FLOOR, "ceo");
    const pmId = await M("startSession", FLOOR, "pm");
    await page.waitForSelector(`.tab[data-session="${pmId}"]`);
    await M("setStatus", ceoId, "idle");
    await M("setStatus", pmId, "idle");
    const activity = page.locator('[data-testid="team-activity"]');
    await activity.waitFor();
    const lineTexts = () => activity.locator("li .text").allInnerTexts();
    // A short pause after each event: two events in the same millisecond have no fixed order in
    // the list, and real events are seconds apart.
    const pause = () => page.waitForTimeout(120);
    await M("setStatus", ceoId, "working");
    await pause();
    await M("setStatus", ceoId, "idle");
    await pause();
    await M("setStatus", pmId, "question");
    await pause();
    await M("taskByEmployee", FLOOR, "pm", { create: "Plan the demo" });
    await pause();
    await M("employeeStart", FLOOR, "ceo", "eng-lin");
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="team-activity"] li').length >= 4 && /made T-6/.test(document.querySelector('[data-testid="team-activity"]').textContent) && /Ada started Lin/.test(document.querySelector('[data-testid="team-activity"]').textContent));
    const texts = await lineTexts();
    check("N3 activity: the list is there at 2560 x 1400: started, a new task, a question and an answer; the newest first",
        texts.slice(0, 2).includes("Ada started Lin") && texts.slice(0, 2).includes("Maya made T-6: Plan the demo") &&
            texts.includes("Maya (Project Manager) asks a question") && texts.includes("Ada (CEO) answered") &&
            texts.indexOf("Maya (Project Manager) asks a question") < texts.indexOf("Ada (CEO) answered") &&
            !texts.includes("Lin (Engineer) started"), JSON.stringify(texts));
    const times = await activity.locator("li time").allInnerTexts();
    check("N3 activity: each line has its time (HH:MM)", times.length === texts.length && times.every((t) => /^\d\d:\d\d$/.test(t)), JSON.stringify(times));
    // A question on a floor that was never opened gets its line before the employees of that floor are known.
    // The line names the employee when it is drawn, so it never shows the slug after the floor is open.
    const piaId = await M("startSession", BIG, "pm");
    await M("setStatus", piaId, "question");
    // The page reads the employees of Big Office with floor_open (that is a call of its own). Open the floor after it.
    await page.waitForFunction(() => window.__towerMockCalls.some((c) => c.command === "floor_open" && c.args.path === "/home/demo/big"));
    await page.locator(`.storey[data-path="${BIG}"]`).evaluate((g) => g.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await page.waitForSelector(`.storey.on[data-path="${BIG}"]`);
    // One row is enough: the question row replaced the "started" status row of the same employee (the fold rule).
    await page.waitForFunction(() => /asks a question/.test(document.querySelector('[data-testid="team-activity"]')?.textContent ?? ""));
    const bigTexts = await lineTexts();
    check("N3 activity: the lines of a floor that was never opened name the employee (Pia (Project Manager)), not the slug (pm)",
        bigTexts.includes("Pia (Project Manager) asks a question") && !bigTexts.some((t) => /^pm /.test(t)), JSON.stringify(bigTexts));
    await page.locator(`.storey[data-path="${FLOOR}"]`).evaluate((g) => g.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await page.waitForSelector(`.storey.on[data-path="${FLOOR}"]`);
    await page.waitForFunction(() => /Ada started Lin/.test(document.querySelector('[data-testid="team-activity"]')?.textContent ?? ""));
    const colours = await activity.locator("li .line").evaluateAll((els) => els.map((e) => [e.dataset.kind, getComputedStyle(e).color]));
    check("N3 activity: only the question line is amber", colours.filter(([k]) => k === "question").every(([, c]) => isAmber(c)) && colours.filter(([k]) => k !== "question").every(([, c]) => !isAmber(c)) && colours.some(([k]) => k === "question"), JSON.stringify(colours));
    await shot("n3-activity-2560.png");
    await activity.locator("li .line", { hasText: "Maya made T-6" }).click();
    await page.waitForFunction(() => document.querySelector(".detail")?.innerText.includes("Plan the demo"), null, { timeout: 5000 }).catch(() => {});
    check("N3 activity: a click on a task line opens the task", (await page.locator(".detail").innerText()).includes("Plan the demo"));
    await page.getByRole("button", { name: /Back/ }).click();
    await activity.locator("li .line", { hasText: "Maya (Project Manager) asks a question" }).click();
    await page.waitForFunction((id) => document.querySelector(`.tab[data-session="${id}"].active`), pmId);
    check("N3 activity: a click on a session line goes to the tab of that employee", (await page.locator(".bar").getAttribute("data-slug")) === "pm");
    // At most 30 lines.
    // Changed on purpose (last repair): a new status row of an employee replaces its old one, so status changes cannot fill
    // the list. Each new task makes a row that is never folded away.
    for (let i = 0; i < 32; i++) {
        await M("taskByEmployee", FLOOR, "pm", { create: `Filler task ${i}` });
        await page.waitForTimeout(40);
    }
    await page.waitForFunction(() => /made T-38/.test(document.querySelector('[data-testid="team-activity"]').textContent));
    await page.waitForTimeout(150);
    check("N3 activity: at most 30 lines", (await activity.locator("li").count()) === 30);
    // A small window hides the list.
    await page.setViewportSize(SMALL);
    await page.waitForTimeout(300);
    check("N3 activity: a small window (1280 x 800) hides the list", (await page.locator('[data-testid="team-activity"]').count()) === 0);
    await page.setViewportSize(LARGE);
    await page.waitForTimeout(300);
    check("N3 activity: a large window shows it again", (await page.locator('[data-testid="team-activity"]').count()) === 1);

    // ================= The board with 50 tasks: scroll, filter, search, compact =================
    for (const [label, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        for (let i = 0; i < 46; i++) {
            const id = await M("taskByEmployee", FLOOR, "pm", { create: `Task number ${i + 6} with a title that is a bit long to wrap` });
            if (i % 3 === 0) await M("taskByEmployee", FLOOR, "pm", { id, assign: i % 2 ? "eng-lin" : "eng-sam" });
        }
        await page.waitForFunction(() => document.querySelectorAll(".board .card").length >= 40);
        await page.waitForTimeout(300);
        const body = () =>
            page.evaluate(() => {
                const b = document.querySelector(".board .body");
                const f = document.querySelector('[data-testid="board-filter"]');
                return { sh: b.scrollHeight, ch: b.clientHeight, sw: b.scrollWidth, cw: b.clientWidth, filterTop: Math.round(f.getBoundingClientRect().top), bodyTop: Math.round(b.getBoundingClientRect().top), pageScroll: document.scrollingElement.scrollHeight - document.scrollingElement.clientHeight };
            });
        const b0 = await body();
        check(`Board ${label}: 50 tasks scroll inside the board, the page does not scroll, and there is no sideways scroll`, b0.sh > b0.ch && b0.sw <= b0.cw + 1 && b0.pageScroll <= 0, JSON.stringify(b0));
        await page.locator(".board .body").evaluate((e) => e.scrollTo(0, 600));
        await page.waitForTimeout(100);
        const b1 = await body();
        check(`Board ${label}: the filter row stays at the top of the board while the list scrolls`, b1.filterTop === b1.bodyTop && Math.abs(b1.filterTop - b0.bodyTop) <= 1, JSON.stringify({ b0, b1 }));
        await page.locator('[data-testid="filter-assignee"]').selectOption("eng-lin");
        await page.waitForTimeout(100);
        const lin = await page.locator(".board .card").evaluateAll((els) => els.map((e) => e.querySelector(".c-who")?.textContent));
        check(`Board ${label}: the filter by assignee shows only the cards of Lin and says how many`, lin.length > 3 && lin.every((n) => n === "Lin") && /of \d+ tasks/.test(await page.locator('[data-testid="filter-note"]').innerText()), JSON.stringify(lin.slice(0, 5)));
        await page.locator('[data-testid="filter-assignee"]').selectOption("");
        await page.fill('[data-testid="filter-query"]', "number 31");
        await page.waitForTimeout(100);
        const found = await page.locator(".board .card .c-title").allInnerTexts();
        check(`Board ${label}: the search keeps only the title with the words (and says 1 of N)`, found.length === 1 && /number 31 /.test(found[0]) && /^1 of 51 tasks/.test(await page.locator('[data-testid="filter-note"]').innerText()), JSON.stringify(found));
        await page.fill('[data-testid="filter-query"]', "");
        await page.locator('[data-testid="filter-compact"]').check();
        await page.waitForTimeout(150);
        const rows = await page.evaluate(() => {
            const ul = document.querySelector(".board ul.cards");
            const cards = [...document.querySelectorAll(".board .card")].map((c) => c.getBoundingClientRect());
            const u = ul.getBoundingClientRect();
            return { n: cards.length, widths: [...new Set(cards.map((c) => Math.round(c.width)))], maxH: Math.max(...cards.map((c) => c.height)), ul: Math.round(u.width), overflow: ul.scrollWidth - ul.clientWidth, someTitleCut: [...document.querySelectorAll(".board .c-title.one")].some((t) => t.scrollWidth > t.clientWidth) };
        });
        check(`Board ${label}: compact rows are one line each, as wide as the list, and a long title is cut (no sideways overflow)`,
            rows.n >= 40 && rows.widths.length === 1 && rows.widths[0] === rows.ul && rows.maxH <= 30 && rows.overflow <= 1 && rows.someTitleCut, JSON.stringify(rows));
        await shot(`board-${label}-compact.png`);
        await page.locator('[data-testid="filter-compact"]').uncheck();
    }

    // ================= The proposal box for old rules, at both sizes =================
    for (const [label, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        await M("setProposed", FLOOR, ["Bash(cargo test:*)", "Bash(npm test:*)"]);
        await page.click("button.settings");
        const sec = page.locator('section[data-section="permissions"]');
        await sec.locator('[data-testid="rules-proposal"]').waitFor();
        await sec.locator("h3").scrollIntoViewIfNeeded();
        const box = await page.evaluate(() => {
            const p = document.querySelector('[data-testid="rules-proposal"]');
            const modal = p.closest('[role="dialog"]') ?? document.querySelector(".panel");
            const pr = p.getBoundingClientRect();
            const mr = modal.getBoundingClientRect();
            const btn = [...p.querySelectorAll("button")].find((b) => b.textContent.trim() === "Use these rules");
            return { inside: pr.left >= mr.left && pr.right <= mr.right, overflow: p.scrollWidth - p.clientWidth, rules: [...p.querySelectorAll("code, li, .rule")].length, btn: !!btn };
        });
        check(`Proposal ${label}: the box for old rules is inside the panel, has no sideways overflow and has its button`, box.inside && box.overflow <= 1 && box.btn, JSON.stringify(box));
        await shot(`settings-proposal-${label}.png`);
        await page.keyboard.press("Escape");
    }
}
