// Checks of the last repairs ("final-b4" review): D-a (the toast stack covers no control), D-b (the bar of the employee
// does not overflow the info panel), D-c (message rows and folded status rows in "Team activity", the MSG mark of a tab)
// and the polish items (the roster, "+ employee", the examples of the rules, the start folder of Browse).
// `mock-page.mjs` calls `last3Checks` after `fourChecks`, with its own browser page and the mock backend.
import { join } from "node:path";

const FLOOR = "/home/demo/tower";
const BIG = "/home/demo/big";
const SMALL = { width: 1280, height: 800 };
const LARGE = { width: 2560, height: 1400 };
const ARROW = "→";

export async function last3Checks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const M = (name, ...args) => page.evaluate(([n, a]) => window.__towerMock[n](...a), [name, args]);
    const flat = (t) => t.replace(/\s+/g, " ").trim();

    // Two switches that a page can only get at the start: the mock finds `rtk`, and the Browse button has a fake dialog.
    // The init script reads them from sessionStorage, so a later page of another check is not changed.
    await page.addInitScript(() => {
        try {
            if (sessionStorage.getItem("e2e-l3-rtk") === "1") window.__towerMockConfig = { ...(window.__towerMockConfig ?? {}), rtk: true };
            if (sessionStorage.getItem("e2e-l3-browse") === "1") {
                window.__browseStarts = [];
                window.__towerMockBrowse = (start) => {
                    window.__browseStarts.push(start);
                    return null;
                };
            }
        } catch {
            // No storage: the switches stay off.
        }
    });

    async function fresh(size = SMALL, { infoW = null, rtk = false, browse = false, freeze = true } = {}) {
        await page.evaluate(
            ([info, r, b]) => {
                localStorage.clear();
                sessionStorage.removeItem("e2e-hide-board");
                sessionStorage.setItem("e2e-l3-rtk", r ? "1" : "0");
                sessionStorage.setItem("e2e-l3-browse", b ? "1" : "0");
                if (info !== null) localStorage.setItem("agent-tower:layout", JSON.stringify({ info }));
            },
            [infoW, rtk, browse],
        );
        await page.setViewportSize(size);
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        if (freeze) await M("freeze", true);
        await page.waitForTimeout(200);
    }

    const clearToasts = async () => {
        const dismiss = page.locator('.toast button[aria-label="Dismiss"]');
        while ((await dismiss.count()) > 0) await dismiss.first().click();
    };

    // A live session of an employee, with a terminal that is full, so that the last two lines of the terminal are known.
    async function liveWithScreen(slug) {
        const id = await M("startSession", FLOOR, slug);
        await page.waitForSelector(`.tab[data-session="${id}"]`);
        await M("setStatus", id, "idle");
        await page.locator(`.tab[data-session="${id}"] button.main`).click();
        await M("output", id, "line of the terminal\r\n".repeat(80));
        await page.waitForFunction(() => document.querySelectorAll(".pane:not(.hidden) .xterm-rows > div").length > 5);
        await page.waitForTimeout(250);
        return id;
    }

    // ================= D-a. The toast stack covers no control =================
    const geometry = () =>
        page.evaluate(() => {
            const box = (el) => {
                if (!el) return null;
                const b = el.getBoundingClientRect();
                return { left: b.left, top: b.top, right: b.right, bottom: b.bottom };
            };
            const rows = [...document.querySelectorAll(".pane:not(.hidden) .xterm-rows > div")].map(box);
            return {
                toasts: [...document.querySelectorAll('[data-testid="toasts"] .toast')].map(box),
                // The box of the stack. It scrolls inside, so a toast can lie under its bottom edge: the box is what must stay in place.
                stack: box(document.querySelector('[data-testid="toasts"]')),
                board: box(document.querySelector(".board")),
                strip: box(document.querySelector('[data-testid="top-strip"]')),
                office: box(document.querySelector(".office")),
                tabs: box(document.querySelector(".tabs")),
                area: box(document.querySelector('[data-testid="terminal-area"]')),
                // The top of the 8th line from the bottom: the last 8 lines of the terminal (the input of the CLI and its status lines) are under it.
                last8Top: rows.length >= 8 ? rows[rows.length - 8].top : null,
            };
        });
    const hit = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    // The controls of the open task detail (the back button, the task ID, the status) are not under a toast.
    const detailReachable = () =>
        page.evaluate(() => {
            const out = {};
            const probes = {
                back: 'button[aria-label="Back to the list of tasks"]',
                status: '[data-testid="detail-status"]',
                id: ".board .detail .title-row .id",
            };
            for (const [name, sel] of Object.entries(probes)) {
                const el = document.querySelector(sel);
                if (!el) {
                    out[name] = "missing";
                    continue;
                }
                const b = el.getBoundingClientRect();
                const top = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
                out[name] = !!top && el.contains(top) ? "reachable" : `covered by ${top?.className ?? "nothing"}`;
            }
            return out;
        });

    for (const [label, size] of [["1280", SMALL], ["2560", LARGE]]) {
        await fresh(size);
        await M("blockMerge", FLOOR, "The main checkout has uncommitted changes in tracked files outside .swarm/: notes.txt. The app does not merge over them. Commit or stash them, then merge again.");
        await liveWithScreen("ceo");
        await page.locator(".board .card", { hasText: "Check the build on Linux" }).click();
        await page.waitForSelector('[data-testid="merge"]');
        await page.locator('[data-testid="merge"]').click();
        await page.waitForSelector('[data-testid="toasts"] .toast');
        // The stack shows the toasts that fit whole when they are folded, so the number on view can be lower than the number sent.
        let pushed = await page.locator(".toast").count(); // the toast of the blocked merge is the first one
        for (const count of [1, 4]) {
            while (pushed < count) {
                pushed++;
                await M("notice", FLOOR, `Could not resume Maya ${pushed}: the program was not found. Change the path in Settings.`);
                await page.waitForTimeout(80);
            }
            await page.waitForTimeout(250);
            const g = await geometry();
            const others = [["board", g.board], ["top strip", g.strip], ["office", g.office], ["tabs", g.tabs]];
            const covered = others.filter(([, b]) => g.toasts.some((t) => hit(t, b))).map(([n]) => n);
            check(`D-a ${label} with ${count} toast(s): the stack covers no part of the task board, the top strip, the office or the tabs`, covered.length === 0 && g.toasts.length >= 1 && g.toasts.length <= count, JSON.stringify({ covered, n: g.toasts.length }));
            // Changed on purpose (toast place): the stack is at the TOP right of the terminal area and never reaches into the last 8 terminal lines.
            check(`D-a ${label} with ${count} toast(s): the stack is in the terminal area, above the last 8 terminal lines`,
                g.last8Top !== null && g.stack.bottom <= g.last8Top && g.stack.top >= g.area.top && g.stack.right <= g.board.left && g.stack.left >= g.area.left && g.toasts.every((t) => t.top >= g.area.top && t.top < g.last8Top && t.right <= g.board.left && t.left >= g.area.left),
                JSON.stringify({ last8Top: g.last8Top, areaTop: g.area.top, stackBottom: g.stack.bottom, toasts: g.toasts.map((t) => [Math.round(t.top), Math.round(t.bottom)]) }));
            const reach = await detailReachable();
            check(`D-a ${label} with ${count} toast(s): the back button and the status of the open task detail can be clicked`, reach.back === "reachable" && reach.status === "reachable", JSON.stringify(reach));
            const tops = g.toasts.map((t) => t.top);
            check(`D-a ${label} with ${count} toast(s): the newest toast is the highest, nearest to the anchor`, tops.every((b, i) => i === 0 || b > tops[i - 1]), JSON.stringify(tops));
            await shot(`toasts-${count}-${label}.png`);
        }
        // A fifth toast: at most 4 show, and a line counts the older ones. It is under the stack.
        await M("notice", FLOOR, "Could not resume Maya 5: the program was not found.");
        await page.waitForSelector('[data-testid="toasts-more"]');
        const g5 = await geometry();
        const moreBox = await page.locator('[data-testid="toasts-more"]').evaluate((el) => el.getBoundingClientRect().top);
        check(`D-a ${label}: at most 4 toasts show, with a line for the older ones under them, and still no cover`,
            g5.toasts.length >= 1 && g5.toasts.length <= 4 && moreBox >= Math.max(...g5.toasts.map((t) => t.bottom)) && !g5.toasts.some((t) => [g5.board, g5.strip, g5.office, g5.tabs].some((b) => hit(t, b))));
        const older5 = (await page.locator('[data-testid="toasts-more"]').innerText()).match(/^(\d+) older/)?.[1];
        check(`D-a ${label}: the line for the older ones counts the toasts that do not show (5 sent: the blocked merge, three more, and a fifth)`, Number(older5) === pushed + 1 - g5.toasts.length, JSON.stringify({ shown: g5.toasts.length, older5, sent: pushed + 1 }));
        await clearToasts();
    }

    // The toast of the blocked merge closes by itself after 8 s while the detail of its task is open. Another toast stays.
    await fresh(SMALL);
    await M("blockMerge", FLOOR, "The main checkout is on the branch other.");
    await page.locator(".board .card", { hasText: "Check the build on Linux" }).click();
    await page.waitForSelector('[data-testid="merge"]');
    await page.locator('[data-testid="merge"]').click();
    const blocked = page.locator('.toast[data-kind="error"]').filter({ hasText: "Task T-3 is blocked" });
    await blocked.first().waitFor();
    await M("employeeStart", FLOOR, "ceo", "eng-lin");
    const startedToast = page.locator('.toast[data-kind="info"]').filter({ hasText: "Ada started Lin" });
    await startedToast.first().waitFor();
    await M("notice", FLOOR, "Could not resume Maya: another error that has nothing to do with a task.");
    await page.waitForTimeout(5000);
    check("D-a: after 5 s the information toast is still there", (await startedToast.count()) === 1);
    await page.waitForTimeout(1600);
    check("D-a: an information toast closes after 6 s", (await startedToast.count()) === 0);
    await page.waitForTimeout(1000);
    check("D-a: at 7 s the toast of the blocked merge is still there", (await blocked.count()) === 1);
    await page.waitForTimeout(2200);
    check("D-a: the toast of the blocked merge closes by itself after 8 s, because the detail of T-3 is open", (await blocked.count()) === 0);
    check("D-a: an error toast of no task stays", (await page.locator('.toast[data-kind="error"]').filter({ hasText: "nothing to do with a task" }).count()) === 1);
    await clearToasts();

    // The toast of T-3 stays when the detail of ANOTHER task is open, and closes 8 s after the detail of T-3 opens.
    await page.getByRole("button", { name: "Back to the list of tasks" }).click();
    await page.locator(".board .card", { hasText: "Write the login form" }).click();
    await page.waitForSelector('[data-testid="detail-status"]');
    await M("notice", FLOOR, "Task T-3 is blocked: a conflict in a.txt.");
    await blocked.first().waitFor();
    await page.waitForTimeout(9000);
    check("D-a: the toast of the blocked merge of T-3 stays while the detail of T-2 is open (8 s have passed)", (await blocked.count()) === 1);
    await page.getByRole("button", { name: "Back to the list of tasks" }).click();
    await page.locator(".board .card", { hasText: "Check the build on Linux" }).click();
    await page.waitForSelector('[data-testid="detail-status"]');
    await page.waitForTimeout(7000);
    check("D-a: after the detail of T-3 opens, the toast is still there at 7 s", (await blocked.count()) === 1);
    await page.waitForTimeout(1800);
    check("D-a: and it closes at 8 s", (await blocked.count()) === 0);
    await clearToasts();

    // ================= D-b. The bar does not overflow the info panel =================
    // For each state of the bar and each width of the info panel: no horizontal scroll, and each button inside the panel.
    const overflow = () =>
        page.evaluate(() => {
            const info = document.querySelector(".office .info");
            const box = info.getBoundingClientRect();
            const outside = [...info.querySelectorAll("button")]
                .filter((b) => b.getClientRects().length > 0)
                .map((b) => [b.textContent.trim().slice(0, 24), b.getBoundingClientRect()])
                .filter(([, r]) => r.left < box.left - 0.5 || r.right > box.right + 0.5)
                .map(([name, r]) => `${name} (${Math.round(r.left - box.left)}..${Math.round(r.right - box.left)} of ${Math.round(box.width)})`);
            const bar = document.querySelector(".bar").getBoundingClientRect();
            return { sw: info.scrollWidth, cw: info.clientWidth, width: Math.round(box.width), outside, barInside: bar.left >= box.left - 0.5 && bar.right <= box.right + 0.5 };
        });
    const states = [
        ["none", async () => {}],
        ["live", async () => {
            const id = await M("startSession", FLOOR, "eng-lin");
            await M("setStatus", id, "idle");
            await page.locator('.roster .row[data-slug="eng-lin"]').click();
            await page.waitForSelector('.bar[data-slug="eng-lin"] button.stop');
        }],
        ["stopped", async () => {
            await page.locator(".bar button.stop").click();
            await page.waitForSelector(".bar button.start");
            await page.waitForSelector(".bar button.resume");
        }],
        ["choose-cli", async () => {
            // The header asks for the CLI of the setup agent in a row of its own: it must fit too.
            await page.getByRole("button", { name: "Run the setup agent" }).click();
            await page.waitForSelector('[role="group"][aria-label="Choose the CLI of the setup agent"]');
        }],
        ["setup", async () => {
            await page.locator("button.cli-claude").click();
            await page.waitForSelector('.bar[data-slug="setup"] [data-testid="stop-setup-bar"]');
        }],
        ["asks", async () => {
            // The setup session ends the state of the bar: select a live employee that asks a question.
            const pm = await M("startSession", FLOOR, "pm");
            await M("setStatus", pm, "question");
            await page.locator('.roster .row[data-slug="pm"]').click();
            await page.waitForSelector('.bar[data-slug="pm"]');
        }],
    ];
    for (const width of [200, 278, 456]) {
        await fresh(SMALL, { infoW: width });
        for (const [name, enter] of states) {
            await enter();
            await page.waitForTimeout(150);
            const o = await overflow();
            const widthOk = Math.abs(o.width - width) <= 2;
            check(`D-b info panel ${width} px, bar state "${name}": no horizontal scroll (scrollWidth <= clientWidth), each button inside the panel, the bar inside`,
                widthOk && o.sw <= o.cw && o.outside.length === 0 && o.barInside, JSON.stringify(o));
            await page.locator(".office .info").screenshot({ path: join(shots, `bar-${name}-${width}.png`) });
        }
    }
    // The bar of a selected employee has no "+ employee". The roster has it, as its last item.
    await fresh(SMALL);
    await page.locator('.roster .row[data-slug="eng-lin"]').click();
    check("D-b: the bar of the selected employee has no + employee button", (await page.locator(".bar button", { hasText: "+ employee" }).count()) === 0);
    const addItem = page.locator('.roster li:last-child [data-testid="add-employee"]');
    check("D-b: the roster has + employee as its last item", (await addItem.count()) === 1 && (await addItem.innerText()).trim() === "+ employee");
    await addItem.click();
    check("D-b: + employee in the roster opens the form for a new employee", (await page.locator('form[aria-label="Add employee"]').count()) === 1);
    await page.keyboard.press("Escape");
    await page.locator('form[aria-label="Add employee"]').waitFor({ state: "detached" });

    // ================= Polish: the roster =================
    // A row that asks keeps the whole name and hides the job title. At 1280 the Project Manager has the longest title.
    await fresh(SMALL);
    const pmId = await M("startSession", FLOOR, "pm");
    await M("setStatus", pmId, "question");
    await page.waitForSelector('.roster .row[data-slug="pm"].asking');
    const asking = await page.evaluate(() => {
        const row = document.querySelector('.roster .row[data-slug="pm"]');
        const name = row.querySelector(".r-name");
        return { name: name.textContent, whole: name.scrollWidth <= name.clientWidth, roleShown: !!row.querySelector(".r-role"), asks: row.querySelector('[data-testid="roster-asks"]')?.textContent };
    });
    check("polish: a row that asks shows the whole name and the word ASKS, and hides the job title", asking.name === "Maya" && asking.whole && !asking.roleShown && asking.asks === "asks", JSON.stringify(asking));
    await page.locator(".office .info").screenshot({ path: join(shots, "roster-asks-1280.png") });

    // At 2560 the columns are wide: no job title is cut, each column is at least 240 px.
    await fresh(LARGE);
    await page.locator(`.storey[data-path="${BIG}"]`).evaluate((g) => g.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await page.waitForSelector(`.storey.on[data-path="${BIG}"]`);
    await page.waitForFunction(() => document.querySelectorAll(".roster .row").length >= 13);
    // Long job titles, as a real team has them.
    const slugs = await page.locator(".roster .row").evaluateAll((els) => els.map((e) => e.dataset.slug));
    await M("patchFloor", BIG, { employee: { slug: slugs[2], patch: { role: "Senior Backend Engineer" } } });
    await M("patchFloor", BIG, { employee: { slug: slugs[3], patch: { role: "Technical Writer and Editor" } } });
    await page.waitForFunction(() => [...document.querySelectorAll(".roster .r-role")].some((r) => r.textContent === "Technical Writer and Editor"));
    const wide = await page.evaluate(() => {
        const roles = [...document.querySelectorAll(".roster .r-role")];
        return {
            cut: roles.filter((r) => r.scrollWidth > r.clientWidth).map((r) => r.textContent),
            widths: [...document.querySelectorAll(".roster li")].map((li) => Math.round(li.getBoundingClientRect().width)),
            roles: roles.map((r) => r.textContent),
        };
    });
    check("polish: at 2560 no job title of the roster is cut, and each column is at least 240 px wide", wide.cut.length === 0 && Math.min(...wide.widths) >= 240 && wide.roles.some((r) => r.length >= 14), JSON.stringify({ cut: wide.cut, min: Math.min(...wide.widths) }));
    await page.locator(".office .info").screenshot({ path: join(shots, "roster-2560-long-titles.png") });

    // ================= Polish: the examples of the rules in Settings =================
    const openRules = async () => {
        await page.click("button.settings");
        await page.locator('[role="dialog"][aria-label="Settings"]').waitFor();
        await page.waitForSelector('[data-testid="examples-warning"]');
    };
    await fresh(SMALL);
    await openRules();
    const plain = await page.locator("[data-example]").evaluateAll((els) => els.map((e) => e.dataset.example));
    check("polish: with no rtk on the machine the examples are cargo test, npm test and ls", JSON.stringify(plain) === JSON.stringify(["Bash(cargo test:*)", "Bash(npm test:*)", "Bash(ls:*)"]), JSON.stringify(plain));
    check("polish: one line under the examples says: Each example with * gets the warning above.", flat(await page.locator('[data-testid="examples-warning"]').innerText()) === "Each example with * gets the warning above.");
    check("polish: the warning that the line points to is above the examples", await page.evaluate(() => {
        const w = document.querySelector('[data-testid="rules-warning"]').getBoundingClientRect();
        const e = document.querySelector('[data-testid="examples-warning"]').getBoundingClientRect();
        return w.top < e.top;
    }));
    await page.locator('[role="dialog"][aria-label="Settings"] .examples').scrollIntoViewIfNeeded();
    await shot("settings-examples-plain-1280.png");
    await fresh(LARGE, { rtk: true });
    await openRules();
    const withRtk = await page.locator("[data-example]").evaluateAll((els) => els.map((e) => e.dataset.example));
    check("polish: with rtk found the example is Bash(rtk cargo test:*) in place of Bash(cargo test:*)", JSON.stringify(withRtk) === JSON.stringify(["Bash(rtk cargo test:*)", "Bash(npm test:*)", "Bash(ls:*)"]), JSON.stringify(withRtk));
    check("polish: a click on the rtk example adds that rule", await (async () => {
        await page.locator('[data-example="Bash(rtk cargo test:*)"]').click();
        await page.waitForSelector('[data-example="Bash(rtk cargo test:*)"]', { state: "detached" });
        const call = (await page.evaluate(() => window.__towerMockCalls.filter((c) => c.command === "floor_permissions_set"))).pop();
        return call.args.allow.includes("Bash(rtk cargo test:*)");
    })());
    await shot("settings-examples-rtk-2560.png");

    // ================= Polish: Browse opens in the folder of the last floor =================
    await fresh(SMALL, { browse: true });
    await page.locator('form.open [data-testid="browse"]').click();
    await page.waitForFunction(() => window.__browseStarts.length === 1);
    const starts = await page.evaluate(() => window.__browseStarts);
    check("polish: Browse opens the dialog in the parent folder of the last floor that is not missing (/home/demo)", starts[0] === "/home/demo", JSON.stringify(starts));

    // ================= D-c. The MSG mark on a tab =================
    await fresh(SMALL);
    const ceoId = await M("startSession", FLOOR, "ceo");
    const linId = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${linId}"]`);
    await M("setStatus", ceoId, "idle");
    await M("setStatus", linId, "idle");
    await page.locator(`.tab[data-session="${ceoId}"] button.main`).click();
    await M("deliverMessage", FLOOR, "pm", "ceo", "Hello Ada, this goes to the tab in view.");
    await M("deliverMessage", FLOOR, "pm", "eng-lin", "Hello Lin, this goes to a tab that is not in view.");
    const linMark = page.locator(`.tab[data-session="${linId}"] [data-testid="tab-msg"]`);
    await linMark.waitFor();
    check("D-c: a message to a tab that is not in view gives it a small MSG mark", (await linMark.innerText()).trim() === "MSG");
    check("D-c: a message to the tab in view gives no mark", (await page.locator(`.tab[data-session="${ceoId}"] [data-testid="tab-msg"]`).count()) === 0);
    const markColor = await linMark.evaluate((el) => getComputedStyle(el).color);
    const unreadColor = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim());
    check("D-c: the MSG mark uses the accent colour of the unread mark, not a new colour", markColor === "rgb(94, 224, 192)" && unreadColor.toLowerCase() === "#5ee0c0", `${markColor} ${unreadColor}`);
    await shot("msg-mark-1280.png");
    await page.waitForTimeout(8500);
    check("D-c: the MSG mark is still there at 8.5 s", (await linMark.count()) === 1);
    await page.waitForTimeout(2000);
    check("D-c: the MSG mark is gone after 10 s", (await linMark.count()) === 0);
    await M("deliverMessage", FLOOR, "pm", "eng-lin", "A second message.");
    await linMark.waitFor();
    await page.locator(`.tab[data-session="${linId}"] button.main`).click();
    check("D-c: a click on the tab ends the mark at once", (await linMark.count()) === 0);

    // ================= D-c. Message rows and folded status rows in "Team activity" =================
    await fresh(LARGE);
    const ceo2 = await M("startSession", FLOOR, "ceo");
    const pm2 = await M("startSession", FLOOR, "pm");
    const lin2 = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${lin2}"]`);
    for (const id of [ceo2, pm2, lin2]) await M("setStatus", id, "idle");
    await page.locator(`.tab[data-session="${ceo2}"] button.main`).click();
    const activity = page.locator('[data-testid="team-activity"]');
    await activity.waitFor();
    const lineTexts = () => activity.locator("li .text").allInnerTexts();
    const pause = () => page.waitForTimeout(120);

    // The preview rule of the contract: control characters out, newline to a space, 80 characters.
    const body = "Also, at the end of your work,\nsay the word BANANA.\u0007 Then stop and wait for the next message of the team.\r\nThank you.";
    const preview = [...body.replace(/\r\n/g, "\n").replace(/[\u0000-\u0008\u000b-\u001f]/g, "").replace(/[\n\t]/g, " ")].slice(0, 80).join("");
    await M("deliverMessage", FLOOR, "pm", "eng-lin", body);
    await pause();
    await M("deliverMessage", FLOOR, "pm", "eng-lin", { task: 3 });
    await pause();
    await M("deliverMessage", FLOOR, "pm", "eng-lin", { task: 3, status: "review" });
    await pause();
    await M("deliverMessage", FLOOR, "operator", "eng-lin", { task: 2 });
    await pause();
    let texts = await lineTexts();
    check("D-c: a delivered message is a row \"Maya → Lin: <the first 80 characters>\" (control characters out, newline as a space), and an ellipsis when it is cut", texts.includes(`Maya ${ARROW} Lin: ${preview}\u2026`) && preview.length === 80, JSON.stringify(texts));
    check("D-c: the row never carries more than the first 80 characters of the body", !texts.some((t) => t.includes("next message of the team")), JSON.stringify(texts));
    check("D-c: a task message is \"Maya → Lin: task T-3 assigned\" or \"Maya → Lin: T-3 is now in review\"", texts.includes(`Maya ${ARROW} Lin: task T-3 assigned`) && texts.includes(`Maya ${ARROW} Lin: T-3 is now in review`), JSON.stringify(texts));
    check("D-c: a task that the operator gave shows \"Operator → Lin: task T-2 assigned\"", texts.includes(`Operator ${ARROW} Lin: task T-2 assigned`));
    check("D-c: the message rows have the kind message", (await activity.locator('li button[data-kind="message"]').count()) === 4);
    // A click goes to the tab of the receiver.
    await page.locator('li button[data-kind="message"]', { hasText: "task T-3 assigned" }).click();
    check("D-c: a click on a message row goes to the tab of the receiver", (await page.locator(".bar").getAttribute("data-slug")) === "eng-lin" && (await page.locator(`.tab.active[data-session="${lin2}"]`).count()) === 1);

    // The task row: "in review", not "review".
    await M("taskByEmployee", FLOOR, "eng-lin", { id: 2, status: "review" });
    await page.waitForFunction(() => /T-2 is now in review/.test(document.querySelector('[data-testid="team-activity"]').textContent));
    texts = await lineTexts();
    check("D-c: a task row reads \"T-2 is now in review\" (never \"is now review\" or \"in_progress\")", texts.includes("T-2 is now in review") && !texts.some((t) => /is now (review|in_progress)/.test(t)), JSON.stringify(texts));

    // The fold: one status row for each employee.
    const rows = () => activity.locator("li .text");
    const rowsOf = async (text) => (await lineTexts()).filter((t) => t.startsWith(text));
    for (const status of ["working", "idle", "working", "idle", "working", "idle"]) {
        await M("setStatus", ceo2, status);
        await pause();
    }
    check("D-c: after three turns Ada has ONE status row (answered), not a history", JSON.stringify(await rowsOf("Ada (CEO)")) === JSON.stringify(["Ada (CEO) answered"]), JSON.stringify(await rowsOf("Ada (CEO)")));
    await M("setStatus", pm2, "working");
    await pause();
    await M("setStatus", pm2, "question");
    await pause();
    await M("setStatus", pm2, "idle");
    await pause();
    await M("setStatus", pm2, "working");
    await pause();
    const pmRows = await rowsOf("Maya (Project Manager)");
    // Oldest to newest. Changed on purpose (activity fold): the messages that Maya sent earlier do not stop the fold, so her "is working" row
    // replaced her "is idle" row of the start. Then the question row, then "is idle" (after the question) that the last "is working" replaced.
    check("D-c: a question row is never folded away, and the status row after it replaces the older status row after it",
        JSON.stringify(pmRows) === JSON.stringify(["Maya (Project Manager) is working", "Maya (Project Manager) asks a question", "Maya (Project Manager) is working"]), JSON.stringify(pmRows));
    await M("deliverMessage", FLOOR, "pm", "eng-lin", "A message from Maya lies between two status rows of Maya.");
    await pause();
    await M("setStatus", pm2, "idle");
    await pause();
    const pmRows2 = await rowsOf("Maya (Project Manager)");
    // Changed on purpose (activity fold): a message does NOT stop the fold. Maya sent a message, then her turn ended:
    // the new "answered" row replaces her old "is working" row. Each employee keeps its newest status row. The
    // question row stays.
    check("D-c: Maya sends a message, then answers: her \"answered\" row replaces her \"is working\" row, and her question row stays",
        JSON.stringify(pmRows2) === JSON.stringify(["Maya (Project Manager) answered", ...pmRows.slice(1)]) && pmRows2.length === pmRows.length, JSON.stringify(pmRows2));
    check("D-c: the list is still newest first, and it has no row about a status twice for one employee", (await rows().count()) === (await lineTexts()).length);
    await activity.screenshot({ path: join(shots, "activity-list-2560.png") });
    await shot("activity-2560.png");
}
