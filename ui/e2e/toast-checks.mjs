// Checks of the toast place and the three small items of the last review ("final-b5"):
//   - the toast stack at the TOP right of the terminal area, the long toast with "more", the toasts under a modal,
//     the timer of an information toast that waits for a modal, and the error of a refused action inside each modal;
//   - one status row for each employee in "Team activity";
//   - "resume" in the bar after a click on the roster row, the character, the name tag and the tab;
//   - the main button of the office header at the minimum width of the info panel (200 px).
// `mock-page.mjs` calls `toastChecks` after `last3Checks`, with its own browser page and the mock backend.
import { join } from "node:path";

const FLOOR = "/home/demo/tower";
const SMALL = { width: 1280, height: 800 };
const LARGE = { width: 2560, height: 1400 };
const ARROW = "→";

const LONG =
    "Could not start Maya: the program claude was not found on this machine. Set its path in Settings, Programs, then try again. " +
    "If the path is right, check that the file can run and that your user may read it.";

export async function toastChecks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const M = (name, ...args) => page.evaluate(([n, a]) => window.__towerMock[n](...a), [name, args]);
    const flat = (t) => t.replace(/\s+/g, " ").trim();

    async function fresh(size = SMALL, { infoW = null } = {}) {
        await page.evaluate(
            ([info]) => {
                localStorage.clear();
                sessionStorage.removeItem("e2e-hide-board");
                sessionStorage.setItem("e2e-l3-rtk", "0");
                sessionStorage.setItem("e2e-l3-browse", "0");
                if (info !== null) localStorage.setItem("agent-tower:layout", JSON.stringify({ info }));
            },
            [infoW],
        );
        await page.setViewportSize(size);
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        await M("freeze", true);
        await page.waitForTimeout(200);
    }

    const clearToasts = async () => {
        const dismiss = page.locator('.toast button[aria-label="Dismiss"]');
        while ((await dismiss.count()) > 0) await dismiss.first().click();
    };

    // A live session with a terminal that is full, so that the last 8 rows of the terminal are known.
    async function liveWithScreen(slug) {
        const id = await M("startSession", FLOOR, slug);
        await page.waitForSelector(`.tab[data-session="${id}"]`);
        await M("setStatus", id, "idle");
        await page.locator(`.tab[data-session="${id}"] button.main`).click();
        await M("output", id, "line of the terminal\r\n".repeat(80));
        await page.waitForFunction(() => document.querySelectorAll(".pane:not(.hidden) .xterm-rows > div").length > 8);
        await page.waitForTimeout(250);
        return id;
    }

    const notice = async (text) => {
        const before = await page.locator(".toast").count();
        await M("notice", FLOOR, text);
        await page.waitForFunction((n) => document.querySelectorAll(".toast").length > n || document.querySelector('[data-testid="toasts-more"]'), before);
        await page.waitForTimeout(60);
    };

    // The box of an element, and the part of a toast that shows (the stack scrolls inside its own box).
    const geometry = () =>
        page.evaluate(() => {
            const box = (el) => {
                if (!el) return null;
                const b = el.getBoundingClientRect();
                return { left: b.left, top: b.top, right: b.right, bottom: b.bottom };
            };
            const stack = document.querySelector('[data-testid="toasts"]');
            const sb = box(stack);
            const rows = [...document.querySelectorAll(".pane:not(.hidden) .xterm-rows > div")].map(box);
            const toasts = [...stack.querySelectorAll(".toast")].map((el) => {
                const b = box(el);
                const text = el.querySelector(".text");
                const lh = parseFloat(getComputedStyle(text).lineHeight);
                return {
                    ...b,
                    shows: { left: b.left, right: b.right, top: Math.max(b.top, sb.top), bottom: Math.min(b.bottom, sb.bottom) },
                    text: text.textContent,
                    title: text.getAttribute("title"),
                    lines: Math.round(text.clientHeight / lh),
                    more: el.querySelector('[data-testid="toast-more"]')?.textContent.trim() ?? null,
                };
            });
            return {
                stack: sb,
                toasts,
                board: box(document.querySelector(".board")),
                detail: box(document.querySelector(".board .detail")),
                strip: box(document.querySelector('[data-testid="top-strip"]')),
                office: box(document.querySelector(".office")),
                tabs: box(document.querySelector(".tabs")),
                area: box(document.querySelector('[data-testid="terminal-area"]')),
                last8Top: rows.length >= 8 ? rows[rows.length - 8].top : null,
            };
        });
    const hit = (a, b) => a && b && a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;

    // ================= The stack at the top right of the terminal area =================
    for (const [label, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        await liveWithScreen("ceo");
        await page.locator(".board .card", { hasText: "Check the build on Linux" }).click();
        await page.waitForSelector('[data-testid="detail-status"]');
        // 1 toast, then 4 toasts. The oldest (0) and the third (2) are long.
        const texts = [`${LONG} (1)`, "Could not resume Eli: a short text.", `${LONG} (3)`, "Could not resume Sam: another short text."];
        for (const count of [1, 4]) {
            while ((await page.locator(".toast").count()) < count) await notice(texts[await page.locator(".toast").count()]);
            await page.waitForTimeout(250);
            const g = await geometry();
            const others = [["task board", g.board], ["task detail", g.detail], ["top strip", g.strip], ["office", g.office], ["tabs", g.tabs]];
            const covered = others.filter(([, b]) => g.toasts.some((t) => hit(t.shows, b))).map(([n]) => n);
            check(`T ${label} with ${count} toast(s): no toast covers the strip, the office, the tabs, the board or the task detail`, covered.length === 0 && g.toasts.length === count, JSON.stringify({ covered, n: g.toasts.length }));
            check(`T ${label} with ${count} toast(s): the stack is 6 px under the top of the terminal area and 10 px inside the right edge of the centre column`,
                Math.abs(g.stack.top - (g.area.top + 6)) <= 1 && Math.abs(g.toasts[0].right - (g.board.left - 10)) <= 1 && g.toasts[0].top >= g.area.top + 5,
                JSON.stringify({ stackTop: g.stack.top, areaTop: g.area.top, right: g.toasts[0].right, boardLeft: g.board.left }));
            check(`T ${label} with ${count} toast(s): the stack is inside the terminal area and above its last 8 rows`,
                g.last8Top !== null && g.stack.bottom <= g.last8Top && g.stack.left >= g.area.left && g.stack.right <= g.board.left && g.toasts.every((t) => t.shows.bottom <= g.last8Top),
                JSON.stringify({ stackBottom: g.stack.bottom, last8Top: g.last8Top }));
            const tops = g.toasts.map((t) => t.top);
            check(`T ${label} with ${count} toast(s): the newest toast is at the top`, g.toasts[0].text.startsWith(texts[count - 1]) && tops.every((v, i) => i === 0 || v > tops[i - 1]), JSON.stringify(g.toasts.map((t) => [t.text.slice(-12), Math.round(t.top)])));
            const longOnes = g.toasts.filter((t) => t.text.length > 100);
            check(`T ${label} with ${count} toast(s): a long toast shows 3 lines and a "more" control, a short one shows no control, the full text is in the title`,
                longOnes.length === (count === 1 ? 1 : 2) && longOnes.every((t) => t.lines === 3 && t.more === "more" && t.title === t.text) &&
                    g.toasts.filter((t) => t.text.length <= 100).every((t) => t.more === null && t.title === t.text),
                JSON.stringify(g.toasts.map((t) => [t.lines, t.more])));
            await shot(`t-${count}-toasts-${label}.png`);
        }
        // "more" shows the full text in the toast, and "less" cuts it again.
        const longToast = page.locator(".toast", { hasText: "(3)" });
        await longToast.locator('[data-testid="toast-more"]').click({ timeout: 3000 }).catch(() => {});
        const opened = await longToast.locator(".text").evaluate((el) => ({ full: el.scrollHeight <= el.clientHeight + 1, lines: Math.round(el.clientHeight / parseFloat(getComputedStyle(el).lineHeight)), text: el.textContent }));
        check(`T ${label}: "more" shows the full text of a long toast, and the control says "less"`, opened.full && opened.lines > 3 && opened.text.startsWith(texts[2]) && (await longToast.locator('[data-testid="toast-more"]').innerText({ timeout: 3000 }).catch(() => "")) === "less", JSON.stringify(opened));
        const afterOpen = await geometry();
        check(`T ${label}: with a toast open the stack still stays above the last 8 rows and covers no part of the page`,
            afterOpen.stack.bottom <= afterOpen.last8Top && ![afterOpen.board, afterOpen.strip, afterOpen.office, afterOpen.tabs].some((b) => hit(afterOpen.stack, b)), JSON.stringify({ stackBottom: afterOpen.stack.bottom, last8Top: afterOpen.last8Top }));
        await shot(`t-4-toasts-one-open-${label}.png`);
        await longToast.locator('[data-testid="toast-more"]').click({ timeout: 3000 }).catch(() => {});
        check(`T ${label}: "less" cuts the text to 3 lines again`, (await longToast.locator(".text").evaluate((el) => Math.round(el.clientHeight / parseFloat(getComputedStyle(el).lineHeight)))) === 3);
        // A fifth toast: the line for the older ones is under the stack, inside the same box.
        await notice("Could not resume Zed: a fifth text.");
        const g5 = await geometry();
        const moreTop = await page.locator('[data-testid="toasts-more"]').evaluate((el) => el.getBoundingClientRect().top);
        check(`T ${label}: at most 4 toasts show and the line for the older ones is under them`, g5.toasts.length === 4 && moreTop >= Math.max(...g5.toasts.map((t) => t.top)), JSON.stringify({ n: g5.toasts.length, moreTop }));
        await clearToasts();
    }

    // The 8-rows limit comes from the rows that are measured. With no terminal on view the limit is the area less 130 px.
    await fresh(SMALL);
    await notice(`${LONG} (a)`);
    await notice(`${LONG} (b)`);
    await notice(`${LONG} (c)`);
    const noRows = await geometry();
    check("T: with no terminal rows the stack stays above the bottom of the terminal area less 130 px, and it scrolls inside", noRows.last8Top === null && noRows.stack.bottom <= noRows.area.bottom - 130 + 1, JSON.stringify({ stackBottom: noRows.stack.bottom, areaBottom: noRows.area.bottom }));
    await clearToasts();

    // ================= A toast under a modal =================
    // Each control of the modal (button, input, select, textarea) is on top at its centre, with 2 toasts that lie under the modal.
    const controlsOnTop = (rootSel) =>
        page.evaluate((sel) => {
            const root = document.querySelector(sel);
            const controls = [...root.querySelectorAll("button, input, select, textarea, a[href]")].filter((el) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden");
            const bad = [];
            for (const el of controls) {
                el.scrollIntoView({ block: "center" });
                const b = el.getBoundingClientRect();
                const top = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
                if (!top || !root.contains(top)) bad.push(`${el.tagName}${el.name ? "[" + el.name + "]" : ""} ${(el.textContent || el.getAttribute("aria-label") || "").trim().slice(0, 20)} -> ${top?.className ?? "nothing"}`);
            }
            const toasts = document.querySelector('[data-testid="toasts"]');
            const backdrop = root.closest(".backdrop");
            return {
                n: controls.length,
                bad,
                z: [Number(getComputedStyle(toasts).zIndex), Number(getComputedStyle(backdrop).zIndex)],
                inert: toasts.inert === true,
                hidden: toasts.getAttribute("aria-hidden"),
            };
        }, rootSel);
    const modalCase = async (label, open, rootSel, min) => {
        await notice(`${LONG} (m1)`);
        await notice("Could not resume Eli: a short text under a modal.");
        await open();
        await page.locator(rootSel).waitFor();
        await page.waitForTimeout(150);
        const r = await controlsOnTop(rootSel);
        check(`T ${label}: every control of the modal (${r.n}) is on top at its centre, and no toast is there`, r.n >= min && r.bad.length === 0, JSON.stringify(r.bad));
        check(`T ${label}: the toasts are under the backdrop (z-index ${r.z[0]} < ${r.z[1]}) and inert`, r.z[0] < r.z[1] && r.inert && r.hidden === "true", JSON.stringify(r));
        await page.keyboard.press("Escape");
        await page.locator(rootSel).waitFor({ state: "detached" });
        await clearToasts();
    };
    for (const [tag, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        await liveWithScreen("ceo");
        await modalCase(`${tag} employee form`, () => page.click('[data-testid="add-employee"]'), 'form[aria-label="Add employee"]', 8);
        // Settings, scrolled to Programs, with an override so that "reset" and "test" show.
        await page.click("button.settings");
        const settings = '[role="dialog"][aria-label="Settings"]';
        await page.locator(settings).waitFor();
        await page.locator('[data-program="claude"] input').fill("/opt/my/claude");
        await page.locator('[data-program="claude"] button[type="submit"]').click();
        await page.locator('[data-program="claude"] button.reset').waitFor();
        await page.keyboard.press("Escape");
        await page.locator(settings).waitFor({ state: "detached" });
        await modalCase(
            `${tag} settings (Programs)`,
            async () => {
                await page.click("button.settings");
                await page.locator(settings).waitFor();
                await page.evaluate(() => document.querySelector('[data-section="programs"]').scrollIntoView({ block: "start" }));
            },
            settings,
            20,
        );
        await page.locator(".board .card", { hasText: "Check the build on Linux" }).click();
        await page.waitForSelector('[data-testid="open-report"]');
        await modalCase(`${tag} task reader`, () => page.click('[data-testid="open-report"]'), '[data-testid="reader"]', 1);
        await shot(`t-after-reader-${tag}.png`);
    }

    // Screenshots with a toast under the Settings panel (Programs) and under the employee form.
    for (const [tag, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        await liveWithScreen("ceo");
        await page.click("button.settings");
        await page.locator('[role="dialog"][aria-label="Settings"]').waitFor();
        await page.locator('[data-program="claude"] input').fill("/opt/my/claude");
        await page.locator('[data-program="claude"] button[type="submit"]').click();
        await page.locator('[data-program="claude"] button.reset').waitFor();
        await page.keyboard.press("Escape");
        await page.locator('[role="dialog"][aria-label="Settings"]').waitFor({ state: "detached" });
        await notice(`${LONG} (s1)`);
        await notice("Could not resume Eli: a short text.");
        await page.click("button.settings");
        await page.locator('[role="dialog"][aria-label="Settings"]').waitFor();
        await page.evaluate(() => document.querySelector('[data-section="programs"]').scrollIntoView({ block: "start" }));
        await page.waitForTimeout(200);
        await shot(`t-toast-settings-programs-${tag}.png`);
        await page.keyboard.press("Escape");
        await page.click('[data-testid="add-employee"]');
        await page.locator('form[aria-label="Add employee"]').waitFor();
        await page.waitForTimeout(200);
        await shot(`t-toast-employee-form-${tag}.png`);
        await page.keyboard.press("Escape");
    }

    // ================= An information toast waits for the modal =================
    await fresh(SMALL);
    await M("employeeStart", FLOOR, "ceo", "eng-lin");
    const started = page.locator('.toast[data-kind="info"]').filter({ hasText: "Ada started Lin" });
    await started.first().waitFor();
    const born = Date.now();
    await page.waitForTimeout(4000 - (Date.now() - born));
    await page.click('[data-testid="add-employee"]');
    await page.locator('form[aria-label="Add employee"]').waitFor();
    await page.waitForTimeout(5000);
    check("T: an information toast that came 4 s before a modal opened is still there 5 s after the modal opened", (await started.count()) === 1);
    await page.keyboard.press("Escape");
    const closed = Date.now();
    await page.locator('form[aria-label="Add employee"]').waitFor({ state: "detached" });
    await page.waitForTimeout(1000);
    check("T: it is still there 1 s after the modal closed", (await started.count()) === 1);
    await started.first().waitFor({ state: "detached", timeout: 6000 }).catch(() => {});
    const left = Date.now() - closed;
    check("T: it goes about 2 s after the modal closed (the 2 s that were left)", left >= 1500 && left <= 2800, `${left} ms`);
    // A toast that comes while a modal is open does not start its timer until the modal closes.
    await page.click('[data-testid="add-employee"]');
    await page.locator('form[aria-label="Add employee"]').waitFor();
    await M("employeeStart", FLOOR, "ceo", "eng-sam");
    const startedSam = page.locator('.toast[data-kind="info"]').filter({ hasText: "Ada started Sam" });
    await startedSam.first().waitFor({ state: "attached" });
    await page.waitForTimeout(7000);
    check("T: an information toast that comes while a modal is open is still there after 7 s", (await startedSam.count()) === 1);
    await page.keyboard.press("Escape");
    const closed2 = Date.now();
    await startedSam.first().waitFor({ state: "detached", timeout: 9000 }).catch(() => {});
    const left2 = Date.now() - closed2;
    check("T: and it closes about 6 s after the modal closed", left2 >= 5400 && left2 <= 6800, `${left2} ms`);
    // An error toast stays as before.
    await M("notice", FLOOR, "Could not resume Maya: an error that stays.");
    await page.waitForSelector('.toast[data-kind="error"]');
    await page.click('[data-testid="add-employee"]');
    await page.locator('form[aria-label="Add employee"]').waitFor();
    await page.keyboard.press("Escape");
    check("T: an error toast is still there after a modal", (await page.locator('.toast[data-kind="error"]').count()) === 1);
    await clearToasts();

    // ================= A refused action inside each modal shows its error inside the modal =================
    const REFUSED = "The file cannot be written: the folder is read-only.";
    const inModal = (rootSel, testid) =>
        page.evaluate(
            ([root, id]) => {
                const el = document.querySelector(`[data-testid="${id}"]`);
                return { there: !!el, inside: !!el && !!document.querySelector(root)?.contains(el), role: el?.getAttribute("role"), text: el?.textContent ?? "", toast: [...document.querySelectorAll(".toast .text")].map((t) => t.textContent) };
            },
            [rootSel, testid],
        );
    const refused = (name, r) => r.there && r.inside && r.role === "alert" && r.text.includes(REFUSED) && r.toast.some((t) => t.includes(REFUSED));
    await fresh(SMALL);
    // Employee form.
    await page.click('[data-testid="add-employee"]');
    const formSel = 'form[aria-label="Add employee"]';
    await page.locator(formSel).waitFor();
    await page.fill(`${formSel} input[name="name"]`, "Zed");
    await M("failNext", "employee_save", REFUSED);
    await page.click(`${formSel} button[type="submit"]`);
    await page.locator('[data-testid="action-error-employee"]').waitFor({ timeout: 3000 }).catch(() => {});
    let r = await inModal(formSel, "action-error-employee");
    const aboveButtons = await page.evaluate(() => (document.querySelector('[data-testid="action-error-employee"]')?.getBoundingClientRect().bottom ?? 1e9) <= document.querySelector("form .buttons").getBoundingClientRect().top + 1);
    check("T refused employee save: the form shows the error in a line with role alert, above its buttons, and the form stays open", refused("employee", r) && aboveButtons && (await page.locator(formSel).count()) === 1, JSON.stringify(r));
    await shot("t-employee-form-error.png");
    await page.keyboard.press("Escape");
    await page.locator(formSel).waitFor({ state: "detached" });
    await clearToasts();
    check("T: the inline error of a modal clears when the modal closes", (await page.locator('[data-testid="action-error-employee"]').count()) === 0);

    // Settings: each action.
    await page.click("button.settings");
    const settings = '[role="dialog"][aria-label="Settings"]';
    await page.locator(settings).waitFor();
    const settingsAction = async (label, testid, command, act) => {
        // The toasts are inert while the panel is open, so they stay. Their text is the same: a toast shows once.
        await M("failNext", command, REFUSED);
        await act();
        await page.locator(`[data-testid="${testid}"]`).waitFor({ timeout: 3000 }).catch(() => {});
        const res = await inModal(settings, testid);
        check(`T refused ${label}: Settings shows the error inside the panel (role alert), and a toast has the same text`, refused(label, res), JSON.stringify(res));
    };
    await settingsAction("OpenAI key", "action-error-keys", "settings_set", async () => {
        await page.fill("#settings-openai-key", "sk-not-a-real-key");
        await page.click('.keyform button[type="submit"]');
    });
    await settingsAction("logo image model", "action-error-image", "settings_set", () => page.selectOption('select[name="imageModel"]', { index: 1 }));
    await settingsAction("notification switch", "action-error-interface", "settings_set", () => page.click('input[name="notifyOnQuestion"]'));
    await settingsAction("custom model", "action-error-models:claude", "models_set_custom", async () => {
        await page.fill('[data-kind="claude"] input[type="text"]', "my-model-x");
        await page.click('[data-kind="claude"] button.add');
    });
    await settingsAction("floor defaults", "action-error-defaults", "floor_defaults_set", async () => {
        await page.selectOption('select[name="defaultCli"]', "codex");
        await page.click('form[aria-label="Floor defaults"] button[type="submit"]');
    });
    await settingsAction("program path", "action-error-program:node", "programs_set", async () => {
        await page.fill('[data-program="node"] input', "/opt/my/node");
        await page.click('[data-program="node"] button[type="submit"]');
    });
    // A program that has an override: "reset" is refused.
    await page.fill('[data-program="codex"] input', "/opt/my/codex");
    await page.click('[data-program="codex"] button[type="submit"]');
    await page.locator('[data-program="codex"] button.reset').waitFor();
    await settingsAction("program reset", "action-error-program:codex", "programs_set", () => page.click('[data-program="codex"] button.reset'));
    // The rules: the error was inline before. It stays a line of the form, with the text of the server.
    await M("failNext", "floor_permissions_set", REFUSED);
    await page.fill('input[name="rule"]', "Bash(ls:*)");
    await page.click('form.add button[type="submit"]');
    await page.locator('[data-testid="rule-error"]').waitFor();
    const ruleErr = await inModal(settings, "rule-error");
    check("T refused rule: the error shows in the Settings panel (role alert), as before", ruleErr.there && ruleErr.inside && ruleErr.role === "alert" && ruleErr.text.includes(REFUSED), JSON.stringify(ruleErr));
    await page.evaluate(() => document.querySelector('[data-testid="action-error-keys"]')?.scrollIntoView({ block: "center" }));
    await shot("t-settings-refused.png");
    await page.keyboard.press("Escape");
    await page.locator(settings).waitFor({ state: "detached" });
    await clearToasts();
    check("T: the inline errors of Settings clear when the panel closes", (await page.locator('[data-testid^="action-error-"]').count()) === 0);
    // The task reader: it has no action. A report or a diff that cannot be read shows inside the reader.
    await page.locator(".board .card", { hasText: "Write the login form" }).click();
    await page.waitForSelector('[data-testid="show-diff"]');
    await M("failNext", "task_diff", REFUSED);
    await page.click('[data-testid="show-diff"]');
    await page.locator('[data-testid="reader"] .error').waitFor({ timeout: 3000 }).catch(() => {});
    const diffErr = await page.evaluate(() => document.querySelector('[data-testid="reader"] .error')?.textContent ?? null);
    check("T the task reader: a diff that cannot be read shows its error inside the reader", diffErr !== null && diffErr.includes(REFUSED), String(diffErr));
    await page.keyboard.press("Escape");
    await clearToasts();

    // ================= One current status row for each employee in "Team activity" =================
    await fresh(LARGE);
    const ceoId = await M("startSession", FLOOR, "ceo");
    const pmId = await M("startSession", FLOOR, "pm");
    const linId = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${linId}"]`);
    for (const id of [ceoId, pmId, linId]) await M("setStatus", id, "idle");
    const activity = page.locator('[data-testid="team-activity"]');
    const lineTexts = () => activity.locator("li .text").allInnerTexts();
    const pause = () => page.waitForTimeout(120);
    // The order of the live run "dc-activity": Maya works and answers, her message to Lin is delivered, Lin's message to Maya,
    // Lin answers, Maya works again and answers again.
    await M("setStatus", pmId, "working");
    await pause();
    await M("setStatus", pmId, "idle");
    await pause();
    await M("deliverMessage", FLOOR, "pm", "eng-lin", "Also, at the end of your work, say the word MANGO.");
    await pause();
    await M("setStatus", linId, "working");
    await pause();
    await M("deliverMessage", FLOOR, "eng-lin", "pm", "MANGO");
    await pause();
    await M("setStatus", linId, "idle");
    await pause();
    await M("setStatus", pmId, "working");
    await pause();
    await M("setStatus", pmId, "idle");
    await pause();
    const all = await lineTexts();
    const mayaRows = all.filter((t) => t.startsWith("Maya (Project Manager)") && !t.includes(ARROW));
    const linRows = all.filter((t) => t.startsWith("Lin (Engineer)") && !t.includes(ARROW));
    check("T activity: in the live-run order Maya has ONE status row (answered), not two", JSON.stringify(mayaRows) === JSON.stringify(["Maya (Project Manager) answered"]), JSON.stringify(all));
    check("T activity: Lin sent a message, then answered: no \"answered\" row for Lin, and no old \"is working\" row", linRows.length === 0 && all.some((t) => t === `Lin (Engineer) ${ARROW} Maya (Project Manager): MANGO` || t.includes("MANGO")), JSON.stringify(all));
    check("T activity: the two message rows stay", all.filter((t) => t.includes(ARROW)).length === 2, JSON.stringify(all));
    await activity.screenshot({ path: join(shots, "t-activity-list-2560.png") });
    await page.setViewportSize(SMALL);
    await page.waitForTimeout(300);
    await page.locator(".office .info").screenshot({ path: join(shots, "t-activity-list-1280.png") });

    // ================= "resume" in the bar =================
    // A stopped session that can resume (Lin, from "an earlier run"): the same bar after a click on the roster row, the character, the
    // name tag and the tab, and after another session started.
    await fresh(SMALL);
    const resumeShows = () =>
        page.evaluate(() => {
            const b = document.querySelector(".bar button.resume");
            if (!b) return "no resume button";
            const r = b.getBoundingClientRect();
            const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
            return !b.disabled && (top === b || b.contains(top)) && document.querySelector(".bar").getAttribute("data-slug") === "eng-lin" ? "shows" : `covered or disabled: ${top?.className}`;
        });
    await page.locator('button.row[data-slug="eng-lin"]').click();
    check("T resume: a click on the roster row of Lin (a stopped session that can resume) shows start and resume in the bar", (await resumeShows()) === "shows", await resumeShows());
    await page.locator('button.row[data-slug="pm"]').click();
    await page.locator('g.emp[data-slug="eng-lin"] .hit').click({ force: true });
    check("T resume: a click on the character shows resume", (await resumeShows()) === "shows");
    await page.locator('button.row[data-slug="pm"]').click();
    await page.locator('g.tag[data-slug="eng-lin"]').click({ force: true });
    check("T resume: a click on the name tag shows resume", (await resumeShows()) === "shows");
    // A session of Lin that ran in this run and stopped (it has a tab).
    await page.locator('button.row[data-slug="pm"]').click();
    const lin = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${lin}"]`);
    await M("setStatus", lin, "working");
    await M("setStatus", lin, "idle");
    await page.locator(`.tab[data-session="${lin}"] button.main`).click();
    await page.locator(".bar button.stop").click();
    await page.waitForSelector(".bar button.resume", { timeout: 3000 }).catch(() => {});
    check("T resume: after Lin stops in this run, the bar shows resume (tab active)", (await resumeShows()) === "shows");
    await page.locator('button.row[data-slug="pm"]').click();
    await page.locator('button.row[data-slug="eng-lin"]').click();
    check("T resume: a click on the roster row of Lin after the stop shows resume", (await resumeShows()) === "shows");
    await page.locator('button.row[data-slug="pm"]').click();
    await page.locator(`.tab[data-session="${lin}"] button.main`).click();
    check("T resume: a click on the tab of the stopped Lin shows resume", (await resumeShows()) === "shows");
    await M("startSession", FLOOR, "pm");
    await M("setDescription", FLOOR, "A new description makes the floor load again.");
    await page.waitForTimeout(400);
    await page.locator('button.row[data-slug="eng-lin"]').click();
    check("T resume: after another session started and the floor loaded again, a click on the roster row still shows resume", (await resumeShows()) === "shows");

    // ================= The main button of the office header at 200 px =================
    for (const resumable of [false, true]) {
        await fresh(SMALL, { infoW: 200 });
        if (resumable) await M("setResumable", FLOOR, ["ceo", "pm"]);
        await page.waitForTimeout(300);
        const cut = await page.evaluate(() => {
            const b = document.querySelector(".office .buttons .main");
            const info = document.querySelector(".office .info").getBoundingClientRect();
            const r = b.getBoundingClientRect();
            const range = document.createRange();
            range.selectNodeContents(b);
            const t = range.getBoundingClientRect();
            const one = parseFloat(getComputedStyle(b).lineHeight) || 18;
            return {
                text: b.textContent.trim(),
                clipX: b.scrollWidth > b.clientWidth + 1,
                clipY: b.scrollHeight > b.clientHeight + 1,
                textInside: t.left >= r.left - 0.5 && t.right <= r.right + 0.5 && t.top >= r.top - 0.5 && t.bottom <= r.bottom + 0.5,
                inPanel: r.left >= info.left - 0.5 && r.right <= info.right + 0.5,
                ellipsis: getComputedStyle(b).textOverflow === "ellipsis",
                lines: Math.round(t.height / one),
                width: Math.round(info.width),
            };
        });
        check(`T header: at 200 px the main button (${resumable ? "resume" : "setup"}) shows its whole text, wrapped onto 2 lines, with no ellipsis`,
            cut.width === 200 && !cut.clipX && !cut.clipY && cut.textInside && cut.inPanel && !cut.ellipsis && cut.lines === 2, JSON.stringify(cut));
        await page.locator(".office .info").screenshot({ path: join(shots, `t-header-200-${resumable ? "resume" : "setup"}.png`) });
    }
}
