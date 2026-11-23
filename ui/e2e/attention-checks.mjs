// Checks of three features on the mock page (docs/ipc-final.md, "Requests for the operator"; 2026-11-22):
//   1. A tab that the operator selects gives the keyboard focus to its terminal (the xterm helper textarea), unless a modal is
//      open or the operator types in a text field.
//   2. The floor view has two tabs, "Floor" and "Hierarchy". The Hierarchy tab draws the organisation chart.
//   3. A request for the operator shows in the top strip ("Done" clears it), in a toast, in Team activity, in the roster and
//      on the tab of the employee.
// `mock-page.mjs` calls `attentionChecks` last, with its own browser page. This file also runs alone:
//   Usage: pnpm dev (in another shell), then: E2E_URL=http://127.0.0.1:1420 node e2e/attention-checks.mjs
//   Env: E2E_URL, E2E_CHROME (path of a chromium or chrome binary), E2E_ATTENTION_SHOTS (folder for the screenshots).
// The mock has no tools for employees, so the checks send the request through `window.__towerMock.operatorRequest`.
import { chromium } from "playwright-core";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const FLOOR = "/home/demo/tower";
const SECOND = "/home/demo/second";

export async function attentionChecks({ page, check, url, shots }) {
    const M = (name, ...args) => page.evaluate(([n, a]) => window.__towerMock[n](...a), [name, args]);
    const calls = (command) => page.evaluate((c) => window.__towerMockCalls.filter((x) => x.command === c), command);
    const tabOf = (id) => page.locator(`.tab[data-session="${id}"]`);
    const node = (slug) => page.locator(`[data-testid="h-node"][data-slug="${slug}"]`);
    const nodes = page.locator('[data-testid="h-node"]');
    const chart = page.locator('[data-testid="hierarchy"]');
    const strip = page.locator('[data-testid="top-strip"]');
    const requestBar = page.locator('[data-testid="request-bar"]');
    const activity = page.locator('[data-testid="team-activity"]');
    const toasts = page.locator(".toast");

    /** True when the focus is in the xterm helper textarea of this session. It waits up to 1.5 s for the focus to move there. */
    const focusInTerminal = (id) =>
        page
            .waitForFunction(
                (sessionId) => {
                    const a = document.activeElement;
                    return !!a && a.tagName === "TEXTAREA" && !!a.closest(".xterm") && a.closest(".pane")?.dataset.session === sessionId;
                },
                id,
                { timeout: 1500 },
            )
            .then(() => true)
            .catch(() => false);
    /** The element that has the focus now, as a short text. */
    const focusNow = () => page.evaluate(() => (document.activeElement ? `${document.activeElement.tagName}#${document.activeElement.id}.${document.activeElement.className}` : "none"));
    const openFloor = async (path) => {
        if ((await page.locator("details.floors[open]").count()) === 0) await page.locator("details.floors > summary").click();
        await page.locator(`li[data-path="${path}"] [data-testid="floor-open"]`).click();
        await page.waitForFunction((p) => document.querySelector(`li[data-path="${p}"]`)?.classList.contains("open"), path);
    };

    await page.evaluate(() => localStorage.clear());
    await page.setViewportSize({ width: 2560, height: 1400 });
    await page.goto(url);
    await page.waitForSelector(".storey.on");
    await M("freeze", true);

    // ---- 1. The focus ----

    // A modal is open: a tab that appears and becomes active does not take the focus.
    await page.locator('button[aria-label="Settings"]').click();
    await page.locator('[role="dialog"]').waitFor();
    const early = await M("startSession", FLOOR, "ceo");
    await page.waitForSelector(`.tab.active[data-session="${early}"]`);
    await page.waitForTimeout(600);
    check("focus: a modal that is open keeps the focus when a tab becomes active", await page.evaluate(() => !!document.activeElement?.closest('[role="dialog"]')), await focusNow());
    await page.keyboard.press("Escape");
    await page.locator('[role="dialog"]').waitFor({ state: "detached" });

    // A text field that the operator types in keeps the focus when a tab of the open floor becomes active.
    await openFloor(SECOND);
    await page.locator("#floor-path").click();
    await page.keyboard.type("/home/demo/typing");
    const bo = await M("startSession", SECOND, "ceo");
    await page.waitForSelector(`.tab.active[data-session="${bo}"]`);
    await page.waitForTimeout(600);
    check("focus: the open-folder field keeps the focus and its text when a tab appears", (await page.evaluate(() => document.activeElement?.id)) === "floor-path" && (await page.locator("#floor-path").inputValue()) === "/home/demo/typing", await focusNow());

    // The first tab of a floor, after a click on a button: the terminal gets the focus (the pane effect).
    await page.locator("#floor-path").fill("");
    await openFloor(FLOOR);
    const pm = await M("startSession", FLOOR, "pm");
    const ceo = early;
    await page.waitForSelector(`.tab[data-session="${pm}"]`);
    for (const id of [ceo, pm]) await M("setStatus", id, "idle");
    await tabOf(pm).locator("button.main").click();
    check("focus: a click on the tab of Maya puts the focus in her terminal", await focusInTerminal(pm), await focusNow());
    await tabOf(ceo).locator("button.main").click();
    check("focus: a click on the tab of Ada puts the focus in her terminal", await focusInTerminal(ceo), await focusNow());
    // The tab is the active tab already. The focus is in the terminal, then on the button after the click, then back in the terminal.
    await tabOf(ceo).locator("button.main").click();
    check("focus: a click on the tab that is active already gives the focus to its terminal", await focusInTerminal(ceo), await focusNow());
    await page.locator(`.row[data-slug="pm"]`).click();
    check("focus: a click on a roster row opens the tab and gives the focus to its terminal", (await tabOf(pm).getAttribute("class")).includes("active") && (await focusInTerminal(pm)), await focusNow());
    await page.locator(`.emp[data-slug="ceo"] .hit`).click();
    check("focus: a click on the character in the room opens the tab and gives the focus to its terminal", (await tabOf(ceo).getAttribute("class")).includes("active") && (await focusInTerminal(ceo)), await focusNow());
    // The keys go to the terminal: the mock echoes what the operator typed.
    await page.keyboard.type("hello");
    await page.waitForFunction((id) => document.querySelector(`.pane[data-session="${id}"]`)?.textContent.includes("hello"), ceo, { timeout: 3000 });
    check("focus: the operator can type at once (the keys reach the terminal)", true);
    // The "Open" button of the top strip.
    await M("setStatus", pm, "question");
    await page.locator('[data-testid="attention-open"]').click();
    check("focus: the Open button of the top strip gives the focus to the terminal of Maya", await focusInTerminal(pm), await focusNow());
    await M("setStatus", pm, "idle");
    // A toast that goes to a tab (the notice `exchange_stopped` has the button Open, for the employee that was refused).
    await tabOf(pm).locator("button.main").click();
    await M("notice", FLOOR, "The app stopped an exchange between Ada and Maya: 8 messages in 5 minutes.", "exchange_stopped", "ceo");
    await toasts.first().waitFor();
    await toasts.first().locator("button.act").click();
    check("focus: the button of a toast that goes to a tab gives the focus to that terminal", (await tabOf(ceo).getAttribute("class")).includes("active") && (await focusInTerminal(ceo)), await focusNow());
    await page.locator('.toast button[aria-label="Dismiss"]').first().click().catch(() => {});

    // ---- 2. The tabs Floor and Hierarchy ----

    const tabs = page.locator('[data-testid="office-tabs"] [role="tab"]');
    check("hierarchy: the floor view has two tabs, Floor and Hierarchy", (await tabs.count()) === 2 && (await tabs.nth(0).innerText()) === "Floor" && (await tabs.nth(1).innerText()) === "Hierarchy");
    check("hierarchy: Floor is the default tab, and the room shows", (await tabs.nth(0).getAttribute("aria-selected")) === "true" && (await tabs.nth(1).getAttribute("aria-selected")) === "false" && (await page.locator('.emp[data-slug="ceo"]').isVisible()));
    check("hierarchy: the Hierarchy panel is not drawn under the Floor tab", (await chart.count()) === 0);

    // The keyboard: the arrow keys move between the tabs.
    await tabs.nth(0).focus();
    await page.keyboard.press("ArrowRight");
    check("hierarchy: the arrow key selects the Hierarchy tab and moves the focus to it", (await tabs.nth(1).getAttribute("aria-selected")) === "true" && (await page.evaluate(() => document.activeElement?.id)) === "office-tab-hierarchy");
    await page.keyboard.press("ArrowRight");
    check("hierarchy: the arrow key wraps to the Floor tab", (await tabs.nth(0).getAttribute("aria-selected")) === "true");
    await page.keyboard.press("End");
    check("hierarchy: End selects the last tab", (await tabs.nth(1).getAttribute("aria-selected")) === "true");
    check("hierarchy: the chart shows under the Hierarchy tab, and the room is hidden", (await chart.count()) === 1 && !(await page.locator('.emp[data-slug="ceo"]').isVisible()));

    // One node for each employee, a report below its manager.
    const slugs = ["ceo", "pm", "eng-sam", "eng-lin"];
    check("hierarchy: one node for each of the 4 employees", (await nodes.count()) === 4 && (await chart.getAttribute("data-count")) === "4");
    const box = async (slug) => (await node(slug).boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const [bCeo, bPm, bSam, bLin] = [await box("ceo"), await box("pm"), await box("eng-sam"), await box("eng-lin")];
    check("hierarchy: Maya is drawn below Ada, and the engineers below Maya", bPm.y >= bCeo.y + bCeo.height && bSam.y >= bPm.y + bPm.height && bLin.y >= bPm.y + bPm.height, JSON.stringify({ ceo: bCeo.y, pm: bPm.y, sam: bSam.y }));
    check("hierarchy: the two engineers are in one row, side by side", Math.abs(bSam.y - bLin.y) < 2 && Math.abs(bSam.x - bLin.x) > bSam.width / 2);
    check("hierarchy: Maya is centred under Ada, and the engineers are centred under Maya", Math.abs(bPm.x + bPm.width / 2 - (bCeo.x + bCeo.width / 2)) < 3 && Math.abs((bSam.x + bLin.x + bSam.width) / 2 - (bPm.x + bPm.width / 2)) < 3);
    const text = await node("pm").innerText();
    check("hierarchy: a node shows the name, the job title and the status", text.includes("Maya") && text.includes("Project Manager") && /idle/.test(text), JSON.stringify(text));
    check("hierarchy: the status of a node has the colour of the roster (the idle green)", (await node("pm").locator(".n-status").evaluate((el) => getComputedStyle(el).color)) === (await page.locator('.row[data-slug="pm"] .dot').evaluate((el) => getComputedStyle(el).backgroundColor)));
    check("hierarchy: the lines are drawn (the connector pieces of the reports)", (await page.evaluate(() => getComputedStyle(document.querySelector('[data-testid="hierarchy"] li > ul > li'), "::before").content)) !== "none");

    // A click on a node does what a click on the employee in the room does.
    const sam = await M("startSession", FLOOR, "eng-sam");
    await page.waitForSelector(`.tab[data-session="${sam}"]`);
    await node("eng-sam").click();
    check("hierarchy: a click on a node selects the employee and opens its tab", (await tabOf(sam).getAttribute("class")).includes("active") && (await page.locator('.row.selected[data-slug="eng-sam"]').count()) === 1 && (await node("eng-sam").getAttribute("aria-pressed")) === "true");
    check("hierarchy: a click on a node gives the focus to the terminal of the tab", await focusInTerminal(sam), await focusNow());
    await node("eng-lin").click();
    check("hierarchy: a click on a node of an employee with no live session only selects the employee", (await page.locator('.row.selected[data-slug="eng-lin"]').count()) === 1 && (await tabOf(sam).getAttribute("class")).includes("active"));
    // A question marks the node.
    await M("setStatus", pm, "question");
    await node("pm").locator('[data-testid="h-asks"]').waitFor();
    check("hierarchy: a question gives the node an amber mark", (await node("pm").evaluate((el) => getComputedStyle(el).backgroundColor)) === "rgb(255, 197, 107)" && (await node("pm").locator('[data-testid="h-asks"]').textContent()) === "asks", await node("pm").evaluate((el) => getComputedStyle(el).backgroundColor + " / " + el.className) + " / " + (await node("pm").locator('[data-testid="h-asks"]').innerText()));
    await M("setStatus", pm, "idle");

    // 21 employees in 4 levels.
    await M("addEmployees", FLOOR, 17);
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="h-node"]').length === 21);
    await M("patchFloor", FLOOR, { employee: { slug: "gen-10", patch: { manager: "eng-lin" } } });
    await page.waitForFunction(() => document.querySelector('[data-testid="h-node"][data-slug="gen-10"]')?.dataset.level === "3");
    check("hierarchy: 21 employees give 21 nodes", (await nodes.count()) === 21);
    const levels = await page.evaluate(() => {
        const out = {};
        for (const el of document.querySelectorAll('[data-testid="h-node"]')) out[el.dataset.level] = (out[el.dataset.level] ?? 0) + 1;
        return out;
    });
    check("hierarchy: the 21 employees are in 4 levels", JSON.stringify(levels) === JSON.stringify({ 0: 1, 1: 1, 2: 18, 3: 1 }), JSON.stringify(levels));
    const group = page.locator('[data-testid="h-group"]');
    check("hierarchy: the 17 reports with no reports of their own are one block of cards, below Maya", (await group.count()) === 1 && (await group.locator('[data-testid="h-node"]').count()) === 17 && (await group.boundingBox()).y >= (await box("pm")).y + (await box("pm")).height);
    // The chart opens with the first top employee in view, also when it is wider than the area.
    await tabs.nth(0).click();
    await tabs.nth(1).click();
    await chart.waitFor();
    await page.waitForTimeout(200);
    const seen = await page.evaluate(() => {
        const r = document.querySelector('[data-testid="h-node"][data-level="0"]').getBoundingClientRect();
        const c = document.querySelector('[data-testid="hierarchy"]').getBoundingClientRect();
        return { inside: r.left >= c.left && r.right <= c.right && r.top >= c.top && r.bottom <= c.bottom, r: [r.left, r.right], c: [c.left, c.right] };
    });
    check("hierarchy: a wide chart opens with the first top employee in view", seen.inside, JSON.stringify(seen));
    const [bLin2, bDeep] = [await box("eng-lin"), await box("gen-10")];
    check("hierarchy: the fourth level is drawn below its manager", bDeep.y >= bLin2.y + bLin2.height);
    const area = await chart.evaluate((el) => ({ sw: el.scrollWidth, cw: el.clientWidth, sh: el.scrollHeight, ch: el.clientHeight, ox: getComputedStyle(el).overflowX, oy: getComputedStyle(el).overflowY }));
    check("hierarchy: a wide chart scrolls inside its area", area.sw > area.cw && area.ox === "auto" && area.oy === "auto", JSON.stringify(area));
    await chart.evaluate((el) => {
        el.scrollLeft = el.scrollWidth;
        el.scrollTop = el.scrollHeight;
    });
    check("hierarchy: the area scrolls to the right", (await chart.evaluate((el) => el.scrollLeft)) > 0);
    const page2 = await page.evaluate(() => ({ sw: document.scrollingElement.scrollWidth, iw: window.innerWidth, sh: document.scrollingElement.scrollHeight, ih: window.innerHeight, y: window.scrollY, x: window.scrollX }));
    check("hierarchy: the page itself never scrolls", page2.sw <= page2.iw && page2.sh <= page2.ih && page2.x === 0 && page2.y === 0, JSON.stringify(page2));
    await chart.evaluate((el) => (el.scrollLeft = 0));

    // A cycle in the manager fields does not hang the page. The two employees of the cycle are top employees.
    await M("patchFloor", FLOOR, { employee: { slug: "ceo", patch: { manager: "pm" } } });
    await page.waitForFunction(() => document.querySelector('[data-testid="h-node"][data-slug="pm"]')?.dataset.level === "0");
    const tops = await page.evaluate(() => [...document.querySelectorAll('[data-testid="h-node"][data-level="0"]')].map((el) => el.dataset.slug).sort());
    check("hierarchy: a manager cycle gives top employees and loses nobody", JSON.stringify(tops) === JSON.stringify(["ceo", "pm"]) && (await nodes.count()) === 21, JSON.stringify(tops));
    check("hierarchy: the page still answers after a cycle", (await page.evaluate(() => 1 + 1)) === 2);
    await M("patchFloor", FLOOR, { employee: { slug: "ceo", patch: { manager: null } } });
    await page.waitForFunction(() => document.querySelector('[data-testid="h-node"][data-slug="pm"]')?.dataset.level === "1");

    // The tab is kept while the operator works, and the room is back with the Floor tab.
    await node("pm").click();
    check("hierarchy: the Hierarchy tab stays after a click on a node", (await tabs.nth(1).getAttribute("aria-selected")) === "true");

    // One employee.
    await openFloor(SECOND);
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="h-node"]').length === 1);
    check("hierarchy: one employee gives one node and no line", (await nodes.count()) === 1 && (await page.locator('[data-testid="hierarchy"] li > ul').count()) === 0);
    await openFloor(FLOOR);
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="h-node"]').length === 21);

    await tabs.nth(0).click();
    check("hierarchy: a click on Floor shows the room again", (await tabs.nth(0).getAttribute("aria-selected")) === "true" && (await page.locator('.emp[data-slug="ceo"]').isVisible()) && (await chart.count()) === 0);

    // ---- 3. Requests for the operator ----

    // The floor has 21 employees now. A request of Maya.
    const TEXT = "Please run sudo apt install ripgrep. I have no password.";
    const idPm = await M("operatorRequest", FLOOR, "pm", TEXT);
    await requestBar.waitFor();
    const barText = (await requestBar.locator('[data-testid="request-text"]').innerText()).replace(/\s+/g, " ").trim();
    check("request: the top strip shows 'Maya (Project Manager) needs you: <text>'", barText === `Maya (Project Manager) needs you: ${TEXT}`, barText);
    check("request: the top strip is amber, with the buttons Open and Done", (await strip.getAttribute("data-mode")) === "request" && (await strip.evaluate((el) => getComputedStyle(el).backgroundColor)) === "rgb(255, 197, 107)" && (await requestBar.locator("button").allInnerTexts()).join(",") === "Open,Done");
    check("request: the strip keeps its height of one row", (await strip.evaluate((el) => el.offsetHeight)) === 30);
    await toasts.filter({ hasText: "needs you" }).first().waitFor();
    check("request: a toast shows the same words, with the button Open", (await toasts.filter({ hasText: `Maya (Project Manager) needs you: ${TEXT}` }).count()) === 1 && (await toasts.filter({ hasText: "needs you" }).first().locator("button.act").innerText()) === "Open");
    const row = activity.locator('li button[data-kind="request"]');
    check("request: Team activity has the row 'Maya needs the operator: <text>'", (await row.count()) === 1 && (await row.first().locator(".text").innerText()) === `Maya needs the operator: ${TEXT}`, await activity.innerText());
    check("request: the roster row of Maya has the amber mark", (await page.locator('.row.needs[data-slug="pm"] [data-testid="roster-needs"]').count()) === 1 && (await page.locator('.row.needs[data-slug="pm"]').evaluate((el) => getComputedStyle(el).backgroundColor)) === "rgb(255, 197, 107)");
    check("request: the tab of Maya has the amber mark", (await tabOf(pm).locator('[data-testid="tab-needs"]').count()) === 1 && (await tabOf(pm).getAttribute("class")).includes("needs"));
    check("request: the tab of Ada has no mark", (await tabOf(ceo).locator('[data-testid="tab-needs"]').count()) === 0);
    await page.locator('[data-testid="office-tab-hierarchy"]').click();
    check("request: the node of Maya in the hierarchy has the amber mark and the word", (await node("pm").getAttribute("data-needs")) === "true" && (await node("pm").locator('[data-testid="h-needs"]').count()) === 1 && (await node("eng-lin").getAttribute("data-needs")) === "false");
    await page.locator('[data-testid="office-tab-floor"]').click();
    await page.screenshot({ path: join(shots, "request-2560.png") });

    // A click on the row of Team activity goes to the tab of Maya.
    await tabOf(ceo).locator("button.main").click();
    await row.first().click();
    check("request: a click on the activity row goes to the tab of Maya", (await tabOf(pm).getAttribute("class")).includes("active") && (await focusInTerminal(pm)), await focusNow());

    // "Open" goes to the tab. A second request makes the strip say how many, and each one stays reachable.
    const idCeo = await M("operatorRequest", FLOOR, "ceo", "Which license should the project use?");
    await page.waitForFunction(() => document.querySelector('[data-testid="request-bar"]')?.dataset.count === "2");
    const two = (await requestBar.locator('[data-testid="request-text"]').innerText()).replace(/\s+/g, " ");
    check("request: two requests say '2 agents need you' and name the next one (the oldest)", /^2 agents need you\./.test(two) && two.includes("Next: Maya (Project Manager)"), two);
    await requestBar.locator('[data-testid="request-open"]').click();
    check("request: Open goes to the tab of the request that the strip names", (await tabOf(pm).getAttribute("class")).includes("active") && (await focusInTerminal(pm)), await focusNow());
    await tabOf(ceo).locator("button.main").click();
    await requestBar.locator('[data-testid="request-open"]').click();
    check("request: the next click on Open goes to the next request (Ada)", (await tabOf(ceo).getAttribute("class")).includes("active"));

    // "Done" clears the request that the strip shows. The command goes with the path and the ID.
    const before = (await calls("operator_request_clear")).length;
    await requestBar.locator('[data-testid="request-done"]').click();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((x) => x.command === "operator_request_clear").length > n, before);
    const sent = (await calls("operator_request_clear")).at(-1);
    check("request: Done calls operator_request_clear with the path and the ID", sent.args.path === FLOOR && [idPm, idCeo].includes(sent.args.id), JSON.stringify(sent));
    await page.waitForFunction(() => document.querySelector('[data-testid="request-bar"]')?.dataset.count === "1");
    check("request: after Done one request is left, and the strip shows it as one", (await requestBar.locator('[data-testid="request-text"]').innerText()).includes("needs you:") && (await requestBar.getAttribute("data-id")) === String(sent.args.id === idPm ? idCeo : idPm));

    // A refused clear keeps the request and shows an error toast, as the other commands do.
    await M("failNext", "operator_request_clear", "FLOOR_GONE: The folder of this floor is gone: /x.");
    const left = await requestBar.getAttribute("data-id");
    await requestBar.locator('[data-testid="request-done"]').click();
    await page.locator('.toast[data-kind="error"]').first().waitFor();
    check("request: a refused clear keeps the request and shows an error toast", (await requestBar.count()) === 1 && (await requestBar.getAttribute("data-id")) === left && (await page.locator('.toast[data-kind="error"]').count()) >= 1);
    await page.locator('.toast[data-kind="error"] button[aria-label="Dismiss"]').first().click();

    await requestBar.locator('[data-testid="request-done"]').click();
    await requestBar.waitFor({ state: "detached" });
    check("request: Done on the last request removes the row of the strip", (await requestBar.count()) === 0 && (await strip.getAttribute("data-mode")) === "idle");
    check("request: the marks of the roster and of the tabs go away", (await page.locator(".row.needs").count()) === 0 && (await page.locator(".tab.needs").count()) === 0);

    // A long text: one line in the strip, cut with an ellipsis, and all of it in the title.
    const LONG = "Long request. " + "The deploy needs a secret that only the operator knows. ".repeat(9);
    await M("operatorRequest", FLOOR, "eng-sam", LONG.slice(0, 500));
    await requestBar.waitFor();
    const cut = await requestBar.locator('[data-testid="request-text"]').evaluate((el) => ({
        sw: el.scrollWidth,
        cw: el.clientWidth,
        ellipsis: getComputedStyle(el).textOverflow,
        nowrap: getComputedStyle(el).whiteSpace,
        title: el.getAttribute("title"),
        stripH: el.closest('[data-testid="top-strip"]').offsetHeight,
        stripW: el.closest('[data-testid="top-strip"]').scrollWidth - el.closest('[data-testid="top-strip"]').clientWidth,
        buttons: [...el.parentElement.querySelectorAll("button")].map((b) => b.getBoundingClientRect().right <= el.closest('[data-testid="top-strip"]').getBoundingClientRect().right),
    }));
    check("request: a long text is cut with an ellipsis on one line, and the buttons stay in the strip", cut.sw > cut.cw && cut.ellipsis === "ellipsis" && cut.nowrap === "nowrap" && cut.stripH === 30 && cut.stripW <= 0 && cut.buttons.every(Boolean), JSON.stringify(cut));
    check("request: the title has the full text", cut.title === `Sam (Engineer) needs you: ${LONG.slice(0, 500)}`, cut.title?.slice(0, 80));
    // A question and a request together.
    await M("setStatus", sam, "question");
    await page.locator('[data-testid="attention-bar"]').waitFor();
    check("request: a question and a request show side by side in one row", (await strip.getAttribute("data-mode")) === "question" && (await page.locator('[data-testid="attention-bar"]').count()) === 1 && (await requestBar.count()) === 1 && (await strip.evaluate((el) => el.offsetHeight)) === 30 && (await strip.evaluate((el) => el.scrollWidth <= el.clientWidth)));
    await M("setStatus", sam, "idle");
    await requestBar.locator('[data-testid="request-done"]').click();
    await requestBar.waitFor({ state: "detached" });
    // A replaced request: the same employee asks again.
    await M("operatorRequest", FLOOR, "eng-sam", "first");
    await requestBar.waitFor();
    await M("operatorRequest", FLOOR, "eng-sam", "second");
    await page.waitForFunction(() => document.querySelector('[data-testid="request-text"]')?.textContent.includes("second"));
    check("request: a new request of the same employee replaces the old one", (await requestBar.getAttribute("data-count")) === "1" && !(await requestBar.innerText()).includes("first"));
    await requestBar.locator('[data-testid="request-done"]').click();
    await requestBar.waitFor({ state: "detached" });

    // ---- the screenshots at 1280 x 800 ----
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(url);
    await page.waitForSelector(".storey.on");
    await M("freeze", true);
    const a = await M("startSession", FLOOR, "ceo");
    const b = await M("startSession", FLOOR, "pm");
    const c = await M("startSession", FLOOR, "eng-sam");
    await page.waitForSelector(`.tab[data-session="${c}"]`);
    await M("setStatus", a, "working");
    await M("setStatus", b, "idle");
    await M("addEmployees", FLOOR, 17);
    await M("patchFloor", FLOOR, { employee: { slug: "gen-10", patch: { manager: "eng-lin" } } });
    await M("operatorRequest", FLOOR, "pm", "Please run sudo apt install ripgrep. I have no password.");
    await requestBar.waitFor();
    await toasts.first().waitFor();
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(shots, "strip-request-1280.png") });
    // A question beside the request: the question keeps its text, the request is cut.
    await M("setStatus", c, "question");
    await page.locator('[data-testid="attention-bar"]').waitFor();
    await page.screenshot({ path: join(shots, "strip-question-and-request-1280.png") });
    await page.locator('[data-testid="office-tab-hierarchy"]').click();
    await chart.waitFor();
    await page.screenshot({ path: join(shots, "hierarchy-21-1280.png") });
    await M("patchFloor", FLOOR, { employee: { slug: "gen-10", patch: { manager: "pm" } } });
    await page.reload();
    await page.waitForSelector(".storey.on");
}

// ---- standalone run ----

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

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
    const url = process.env.E2E_URL ?? "http://127.0.0.1:1420";
    const shots = process.env.E2E_ATTENTION_SHOTS ?? "/tmp/agent-tower-review/attention";
    mkdirSync(shots, { recursive: true });
    const results = [];
    const check = (name, ok, detail = "") => {
        results.push({ name, ok });
        console.log(`${ok ? "PASS" : "FAIL"}  ${name}${!ok && detail ? "  (" + detail + ")" : ""}`);
    };
    const browser = await chromium.launch({ headless: true, executablePath: findChrome() });
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const problems = [];
    page.on("pageerror", (e) => problems.push(`page error: ${e.message}`));
    page.on("console", (m) => {
        if (m.type() === "error") problems.push(`console error: ${m.text()}`);
    });
    try {
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        await attentionChecks({ page, check, url, shots });
    } catch (e) {
        check("no script error", false, e.message.split("\n").slice(0, 3).join(" | "));
    }
    await browser.close();
    check("no console errors or page errors", problems.length === 0, problems.join(" | "));
    const failed = results.filter((r) => !r.ok);
    console.log(`${results.length - failed.length}/${results.length} checks passed`);
    process.exit(failed.length === 0 ? 0 : 1);
}
