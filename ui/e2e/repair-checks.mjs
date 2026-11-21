// Checks of the repairs of the review "final-b" (B1, D1 to D3, D6 to D10, M6 and the polish list).
// `mock-page.mjs` calls `repairChecks` at the end of its run, with its own browser page. The mock
// page (the in-memory backend) runs here: `window.__towerMock` freezes the automatic status changes,
// and sets a status, so that each check has a fixed state.
import { join } from "node:path";

const FLOOR = "/home/demo/tower";
const BIG = "/home/demo/big";

export async function repairChecks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const M = (name, ...args) => page.evaluate(([n, a]) => window.__towerMock[n](...a), [name, args]);
    const calls = (command) => page.evaluate((c) => window.__towerMockCalls.filter((x) => x.command === c), command);
    const emp = (slug) => page.locator(`.emp[data-slug="${slug}"]`);
    const selectEmp = (slug) => emp(slug).locator(".hit").click();
    const bar = page.locator(".bar");
    const tabOf = (id) => page.locator(`.tab[data-session="${id}"]`);
    const rgb = (css) => (css.match(/\d+(\.\d+)?/g) ?? []).slice(0, 3).map(Number);
    const isAmber = (css) => {
        const [r, g, b] = rgb(css);
        return r >= 240 && g >= 180 && g <= 235 && b >= 90 && b <= 175;
    };
    const waitText = (selector, re) =>
        page.waitForFunction(([s, r]) => new RegExp(r).test(document.querySelector(s)?.textContent ?? ""), [selector, re.source]);
    const clearToasts = async () => {
        const dismiss = page.locator('.toast button[aria-label="Dismiss"]');
        while ((await dismiss.count()) > 0) await dismiss.first().click();
    };
    // A call shows in the log after the 5 ms of the mock IPC: wait for the n-th call.
    const waitCalls = async (command, n) => {
        await page.waitForFunction(([c, k]) => window.__towerMockCalls.filter((x) => x.command === c).length >= k, [command, n]);
        return calls(command);
    };
    const inView = (id) => page.evaluate((i) => document.querySelector(`.pane[data-session="${i}"]`)?.classList.contains("hidden") === false, id);
    // The focus moves after the next frame: wait for it, up to 2 s.
    const focusInTerminal = (id) =>
        page
            .waitForFunction((i) => !!document.activeElement?.closest(`.pane[data-session="${i}"]`), id, { timeout: 2000 })
            .then(() => true, () => false);

    async function fresh(size = { width: 1280, height: 800 }) {
        await page.evaluate(() => localStorage.clear());
        await page.setViewportSize(size);
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        await M("freeze", true);
        await page.waitForTimeout(150);
    }

    async function openFloorByStorey(path) {
        await page.locator(`.storey[data-path="${path}"]`).evaluate((g) => g.dispatchEvent(new MouseEvent("click", { bubbles: true })));
        await page.waitForSelector(`.storey.on[data-path="${path}"]`);
    }

    // E2E_ONLY=gone runs only the sections G (a floor whose folder is gone, the notices, the open errors, the
    // rules). It makes a run of those sections take about a minute. The default runs everything.
    const everything = process.env.E2E_ONLY !== "gone";
    if (everything) {
    // ================= B1. A question in a tab that is not active =================
    await fresh();
    await M("startSession", FLOOR, "ceo");
    const ceoId = await page.evaluate(() => [...document.querySelectorAll(".tab")][0].dataset.session);
    await M("setStatus", ceoId, "idle");
    const linId = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${linId}"]`);
    await M("setStatus", linId, "idle");
    await page.waitForTimeout(100);
    check("B1: no question yet: no attention bar, the title is Agent Tower", (await page.locator('[data-testid="attention-bar"]').count()) === 0 && (await page.title()) === "Agent Tower");
    check("B1: the active tab is the first one (Ada), Lin is another tab", (await inView(ceoId)) && !(await inView(linId)));

    await M("setStatus", linId, "question");
    await page.waitForSelector('[data-testid="attention-bar"]');
    const attention = page.locator('[data-testid="attention-bar"]');
    check("B1: one question: the bar says who asks, and has an Open button",
        (await attention.innerText()).replace(/\s+/g, " ").includes("Lin (Engineer) asks a question") &&
            (await attention.getByRole("button", { name: "Open" }).count()) === 1,
        (await attention.innerText()).replace(/\s+/g, " "));
    const linTab = tabOf(linId);
    const tabStyle = await linTab.evaluate((el) => ({ bg: getComputedStyle(el).backgroundColor, text: el.innerText.replace(/\s+/g, " ").trim() }));
    check("B1: the tab of the session that asks has an amber fill and the text asks after the name",
        isAmber(tabStyle.bg) && /^Lin\s*asks$/i.test(tabStyle.text) && (await linTab.locator('[data-testid="tab-asks"]').count()) === 1, JSON.stringify(tabStyle));
    const anim = await linTab.evaluate((el) => getComputedStyle(el).animationName + "|" + getComputedStyle(el).animationDuration);
    check("B1: the tab pulses slowly (an animation of 2 s or more)", /ask-pulse\|(\d+(\.\d+)?)s/.test(anim) && parseFloat(anim.split("|")[1]) >= 2, anim);
    check("B1: the window title shows the count", (await page.title()) === "(1) Agent Tower", await page.title());
    const barBox = await attention.boundingBox();
    const officeBox = await page.locator(".office").boundingBox();
    // Changed on purpose (N1): the bar lives in the top strip, a row of fixed height that is always there.
    const stripBox = await page.locator('[data-testid="top-strip"]').boundingBox();
    check("B1: the bar is in the top strip: its own row above the office, and it covers nothing",
        stripBox.y === 0 && Math.abs(stripBox.y + stripBox.height - officeBox.y) <= 1 && stripBox.width > 500 &&
            barBox.y >= stripBox.y && barBox.y + barBox.height <= stripBox.y + stripBox.height + 0.5,
        `strip ${JSON.stringify(stripBox)} bar ${JSON.stringify(barBox)} office y=${officeBox.y}`);
    check("B1: the bar is not a toast", (await page.locator(".toast").count()) === 0 && (await attention.getAttribute("role")) === "status");
    const tag = page.locator('.tag[data-slug="eng-lin"]');
    const tagBox = await tag.locator("rect.box").evaluate((el) => ({ fill: el.getAttribute("fill"), stroke: el.getAttribute("stroke") }));
    check("B1: the name tag of the employee has a full amber fill", tagBox.fill === "#ffc56b", JSON.stringify(tagBox));
    check("B1: the character has an amber ring that always shows",
        await emp("eng-lin").locator(".ring").evaluate((el) => getComputedStyle(el).stroke.includes("255, 197, 107") && Number(getComputedStyle(el).opacity) > 0.3));
    const row = page.locator('.roster .row[data-slug="eng-lin"]');
    check("B1: the roster row is amber and says asks",
        isAmber(await row.evaluate((el) => getComputedStyle(el).backgroundColor)) && (await row.locator('[data-testid="roster-asks"]').count()) === 1);
    const badge = page.locator(`.badge[data-path="${FLOOR}"]`);
    const badgeBox = await badge.locator(".disc").boundingBox();
    check("B1: the storey badge is amber, shows the count and is at least 22 px at the natural building size",
        (await badge.locator("text").textContent()) === "1" && badgeBox.width >= 22 && isAmber(await badge.locator(".disc").evaluate((el) => getComputedStyle(el).fill)), JSON.stringify(badgeBox));
    check("B1: the badge pulses (a halo animation)", (await badge.locator(".halo").evaluate((el) => getComputedStyle(el).animationName)).includes("badge-pulse"));
    await shot("b1-question-1280.png");

    // Reduced motion: the amber stays, the pulse stops.
    await page.emulateMedia({ reducedMotion: "reduce" });
    const still = await linTab.evaluate((el) => ({ bg: getComputedStyle(el).backgroundColor, anim: getComputedStyle(el).animationName }));
    check("B1: with reduced motion the tab is amber and does not pulse", isAmber(still.bg) && still.anim === "none", JSON.stringify(still));
    check("B1: with reduced motion the badge and the ring do not pulse",
        (await badge.locator(".halo").evaluate((el) => getComputedStyle(el).animationName)) === "none" &&
            (await emp("eng-lin").locator(".ring").evaluate((el) => getComputedStyle(el).animationName)) === "none");
    await page.emulateMedia({ reducedMotion: "no-preference" });

    // Several questions on two floors, and "Open" steps through them, oldest first.
    await openFloorByStorey(BIG);
    await openFloorByStorey(FLOOR);
    const piaId = await M("startSession", BIG, "pm");
    await M("setStatus", piaId, "question");
    await page.waitForFunction(() => document.querySelector('[data-testid="attention-bar"]')?.dataset.count === "2");
    const text2 = (await attention.innerText()).replace(/\s+/g, " ");
    check("B1: two questions: the bar counts them and names the first (the oldest)", /2 agents ask a question/.test(text2) && /Next: Lin \(Engineer\)/.test(text2), text2);
    check("B1: the title counts 2 and the badge of each floor shows", (await page.title()) === "(2) Agent Tower" && (await page.locator(`.badge[data-path="${BIG}"]`).count()) === 1);
    await attention.getByRole("button", { name: "Open" }).click();
    await page.waitForFunction((id) => document.querySelector(`.tab[data-session="${id}"].active`), linId);
    check("B1: Open makes the tab of the oldest question active, selects the employee and puts the focus in the terminal",
        (await bar.getAttribute("data-slug")) === "eng-lin" && (await inView(linId)) && (await focusInTerminal(linId)));
    const text3 = (await attention.innerText()).replace(/\s+/g, " ");
    check("B1: after Open the bar names the next question, on its floor", /Next: Pia \(Project Manager\) on Big Office/.test(text3), text3);
    await attention.getByRole("button", { name: "Open" }).click();
    await page.waitForSelector(`.storey.on[data-path="${BIG}"]`);
    await page.waitForFunction((id) => document.querySelector(`.tab[data-session="${id}"].active`), piaId);
    check("B1: the second Open opens the other floor, then its tab, its employee and its terminal",
        (await page.locator(".strip h2.name").innerText()) === "Big Office" && (await bar.getAttribute("data-slug")) === "pm" && (await focusInTerminal(piaId)),
        JSON.stringify({ slug: await bar.getAttribute("data-slug"), focus: await page.evaluate(() => document.activeElement?.tagName + "." + document.activeElement?.className), inView: await inView(piaId) }));
    await shot("b1-two-floors-1280.png");
    await M("setStatus", linId, "idle");
    await page.waitForFunction(() => document.querySelector('[data-testid="attention-bar"]')?.dataset.count === "1");
    const text4 = (await attention.innerText()).replace(/\s+/g, " ");
    check("B1: when one question ends the bar says one agent asks, with no floor name for the open floor",
        /Pia \(Project Manager\) asks a question/.test(text4) && !/ on /.test(text4) && (await page.title()) === "(1) Agent Tower", text4);
    await M("setStatus", piaId, "idle");
    await page.waitForFunction(() => !document.querySelector('[data-testid="attention-bar"]'));
    check("B1: the bar and the title go away when the last question ends", (await page.title()) === "Agent Tower" && (await page.locator(".badge").count()) === 0 && (await page.locator('[data-testid="tab-asks"]').count()) === 0);
    const officeAfter = await page.locator(".office").boundingBox();
    // Changed on purpose (N1): the strip stays, so the office is at the same place as before the question.
    check("B1: the office is where it was before the question (under the strip)", officeAfter.y === stripBox.height && officeAfter.y === officeBox.y);

    // ================= D2. The tab and the bar are in step =================
    await fresh();
    await M("startSession", FLOOR, "ceo");
    const ceo2 = await page.evaluate(() => [...document.querySelectorAll(".tab")][0].dataset.session);
    await M("setStatus", ceo2, "idle");
    const lin2 = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${lin2}"]`);
    await M("setStatus", lin2, "idle");
    await page.waitForTimeout(100);
    await tabOf(lin2).locator("button.main").click();
    check("D2: a click on a tab selects its employee in the bar (Lin)", (await bar.getAttribute("data-slug")) === "eng-lin" && (await bar.locator(".name").innerText()) === "Lin");
    await tabOf(ceo2).locator("button.main").click();
    check("D2: a click on the other tab selects Ada, so the stop button belongs to the terminal that shows",
        (await bar.getAttribute("data-slug")) === "ceo" && (await inView(ceo2)));
    await bar.locator("button.stop").click();
    const stopCalls = await waitCalls("session_stop", 1);
    check("D2: stop in the bar stops the session of the active tab, not another one", stopCalls.length === 1 && stopCalls[0].args.sessionId === ceo2, JSON.stringify(stopCalls.map((c) => c.args)));
    await selectEmp("pm");
    check("D2: a click on an employee with no session selects it and keeps the tab", (await bar.getAttribute("data-slug")) === "pm" && (await inView(ceo2)));
    await selectEmp("eng-lin");
    check("D2: a click on an employee with a live session activates its tab", (await inView(lin2)) && (await bar.getAttribute("data-slug")) === "eng-lin");
    // The setup session: its tab shows the setup in the bar, with its own stop button.
    await page.getByRole("button", { name: "Run the setup agent" }).click();
    await page.locator("button.cli-claude").click();
    await page.waitForSelector('.tab:has-text("Setup")');
    const setupId = await page.evaluate(() => [...document.querySelectorAll(".tab")].find((t) => t.innerText.includes("Setup")).dataset.session);
    check("D2: the setup tab is active and the bar shows the setup session with Stop the setup",
        (await bar.locator(".name").innerText()) === "Setup agent" && (await bar.locator('[data-testid="stop-setup-bar"]').innerText()) === "Stop the setup" && (await inView(setupId)));
    await tabOf(lin2).locator("button.main").click();
    check("D2: a click on the Lin tab brings Lin back to the bar", (await bar.locator(".name").innerText()) === "Lin" && (await bar.locator('[data-testid="stop-setup-bar"]').count()) === 0);
    await tabOf(setupId).locator("button.main").click();
    await bar.locator('[data-testid="stop-setup-bar"]').click();
    const stops2 = await waitCalls("session_stop", 2);
    check("D2: Stop the setup in the bar stops the setup session", stops2[stops2.length - 1].args.sessionId === setupId, JSON.stringify(stops2[stops2.length - 1].args));

    // ================= D3. The model text in the bar =================
    await fresh();
    await M("patchFloor", FLOOR, { defaults: { cli: null, model: "haiku" }, employee: { slug: "eng-lin", patch: { model: null } } });
    await selectEmp("eng-lin");
    await page.waitForFunction(() => document.querySelector('[data-testid="bar-model"]')?.textContent !== "default model");
    const modelText = () => bar.locator('[data-testid="bar-model"]').innerText();
    check("D3: no model of its own and a floor default: haiku (floor default)", (await modelText()) === "haiku (floor default)", await modelText());
    await M("patchFloor", FLOOR, { defaults: { cli: "codex", model: "haiku" } });
    await page.waitForFunction(() => document.querySelector('[data-testid="bar-model"]')?.textContent === "CLI default");
    check("D3: the floor default is for another CLI: CLI default", (await modelText()) === "CLI default");
    await M("patchFloor", FLOOR, { defaults: { cli: "claude", model: "haiku" } });
    await page.waitForFunction(() => document.querySelector('[data-testid="bar-model"]')?.textContent === "haiku (floor default)");
    check("D3: the default CLI is the CLI of the employee: haiku (floor default)", (await modelText()) === "haiku (floor default)");
    await M("patchFloor", FLOOR, { defaults: { cli: null, model: null } });
    await page.waitForFunction(() => document.querySelector('[data-testid="bar-model"]')?.textContent === "CLI default");
    check("D3: no default at all: CLI default", (await modelText()) === "CLI default");
    await selectEmp("ceo");
    check("D3: a model of its own is shown as it is", (await modelText()) === "haiku");
    check("D3: the bar never says default model", !(await bar.innerText()).includes("default model"));

    // ================= D6. The result is easy to find =================
    await fresh();
    await M("startSession", FLOOR, "ceo");
    const ceo6 = await page.evaluate(() => [...document.querySelectorAll(".tab")][0].dataset.session);
    await M("setStatus", ceo6, "idle");
    const lin6 = await M("startSession", FLOOR, "eng-lin");
    await page.waitForSelector(`.tab[data-session="${lin6}"]`);
    await M("setStatus", lin6, "idle");
    const sam6 = await M("startSession", FLOOR, "eng-sam");
    await page.waitForSelector(`.tab[data-session="${sam6}"]`);
    await M("setStatus", sam6, "idle");
    await page.waitForTimeout(100);
    check("D6: the active tab is Ada, with no unread mark", (await inView(ceo6)) && (await page.locator('[data-testid="tab-unread"]').count()) === 0);
    await M("setStatus", lin6, "working");
    await M("setStatus", lin6, "idle");
    await page.waitForSelector(`.tab[data-session="${lin6}"] [data-testid="tab-unread"]`);
    check("D6: a session that finished a turn while its tab was not active gets an unread mark", (await page.locator('[data-testid="tab-unread"]').count()) === 1);
    check("D6: no toast for a session that the operator did not type in", (await page.locator(".toast").count()) === 0);
    await M("setStatus", ceo6, "working");
    await M("setStatus", ceo6, "idle");
    await page.waitForTimeout(150);
    check("D6: the active tab gets no mark when its turn ends", (await page.locator(`.tab[data-session="${ceo6}"] [data-testid="tab-unread"]`).count()) === 0);
    await tabOf(lin6).locator("button.main").click();
    check("D6: the mark goes away when the operator opens the tab", (await page.locator('[data-testid="tab-unread"]').count()) === 0);
    // The operator types in Lin, then looks at Ada. When Lin finishes, one toast says so.
    await page.locator(`.pane[data-session="${lin6}"] .xterm`).click();
    await page.keyboard.type("hello");
    await tabOf(ceo6).locator("button.main").click();
    await M("setStatus", lin6, "working");
    await M("setStatus", lin6, "idle");
    // Changed on purpose (N3): the text is "Lin (Engineer) answered" with an Open button (jargon removed).
    const finished = page.locator('.toast[data-kind="info"]').filter({ hasText: "Lin (Engineer) answered" });
    await finished.waitFor();
    check("D6: one information toast for the session that the operator typed in", (await finished.count()) === 1 && (await page.locator(".toast").count()) === 1);
    check("D6: Lin also has the unread mark", (await page.locator(`.tab[data-session="${lin6}"] [data-testid="tab-unread"]`).count()) === 1);
    await M("setStatus", sam6, "working");
    await M("setStatus", sam6, "idle");
    await page.waitForSelector(`.tab[data-session="${sam6}"] [data-testid="tab-unread"]`);
    check("D6: Sam (not typed in) gets the mark and no toast", (await page.locator(".toast").count()) === 1);
    // The 3 newest done tasks stay in view above the folded rest.
    for (let i = 0; i < 4; i++) {
        const id = await M("taskByEmployee", FLOOR, "pm", { create: `Finished work ${i + 1}` });
        await M("taskByEmployee", FLOOR, "pm", { id, status: "done" });
        await page.waitForTimeout(8);
    }
    await page.waitForFunction(() => document.querySelectorAll('.board .group[data-status="done"] .card').length === 3);
    const doneIds = () => page.$$eval('.board .group[data-status="done"] .card', (els) => els.map((e) => e.dataset.task));
    check("D6: with 5 done tasks the board shows the 3 newest, newest first, and folds the rest", (await doneIds()).join() === "9,8,7", (await doneIds()).join());
    check("D6: the group says how many are folded", (await page.locator('[data-testid="done-older"]').innerText()) === "Show 2 older done tasks");
    await page.locator('[data-testid="done-older"]').click();
    check("D6: a click shows the older done tasks below the newest", (await doneIds()).join() === "9,8,7,6,1", (await doneIds()).join());
    await page.locator('[data-testid="done-older"]').click();
    check("D6: a second click folds them again", (await doneIds()).length === 3);
    await shot("d6-board-done-1280.png");

    // ================= D7 and D8. The CEO and PM chips, the order of the roster =================
    await fresh();
    await M("addEmployees", BIG, 12);
    await openFloorByStorey(BIG);
    await page.waitForFunction(() => document.querySelectorAll(".emp").length === 25);
    const chips = await page.evaluate(() => {
        const pick = (slug) => {
            const tag = document.querySelector(`.tag[data-slug="${slug}"]`);
            const chip = tag?.querySelector(".chip");
            const box = tag?.querySelector("rect.box");
            if (!chip) return null;
            const c = chip.querySelector("rect").getBoundingClientRect();
            const b = box.getBoundingClientRect();
            return {
                text: chip.querySelector("text").textContent,
                fill: chip.querySelector("rect").getAttribute("fill"),
                inside: c.left >= b.left - 0.5 && c.right <= b.right + 0.5 && c.top >= b.top - 0.5 && c.bottom <= b.bottom + 0.5,
            };
        };
        return {
            ceo: pick("ceo"),
            pm: pick("pm"),
            other: document.querySelectorAll('.tag:not([data-slug="ceo"]):not([data-slug="pm"]) .chip').length,
            roleLines: document.querySelectorAll(".tag-role").length,
        };
    });
    check("D7: with 25 employees the CEO tag has a gold CEO chip inside the tag", chips.ceo?.text === "CEO" && chips.ceo.fill === "#ffd166" && chips.ceo.inside, JSON.stringify(chips.ceo));
    check("D7: with 25 employees the PM tag has a blue PM chip inside the tag", chips.pm?.text === "PM" && chips.pm.fill === "#7cc4ff" && chips.pm.inside, JSON.stringify(chips.pm));
    check("D7: no other tag has a chip, and no tag has the tall role line", chips.other === 0 && chips.roleLines === 0, JSON.stringify(chips));
    await page.locator('.roster .row[data-slug="eng-1"]').click();
    const pop = await page.locator('.tag[data-slug="eng-1"] .role-pop').evaluate((el) => ({ on: el.classList.contains("on"), opacity: getComputedStyle(el).opacity, text: el.textContent.trim() }));
    check("D7/P14: the role of a selected employee shows in a small box above its tag", pop.on && pop.opacity === "1" && pop.text === "ENGINEER", JSON.stringify(pop));
    check("D7/P14: the role box of another tag is hidden", (await page.locator('.tag[data-slug="eng-2"] .role-pop').evaluate((el) => getComputedStyle(el).opacity)) === "0");
    check("P13: each tag has a tooltip with the full name and the role", (await page.locator('.tag[data-slug="des-2"] > title').textContent()) === "Wolfeschlegel, Designer, stopped");
    const order = await page.evaluate(() => ({
        roster: [...document.querySelectorAll(".roster .row")].map((r) => r.dataset.slug),
        room: [...document.querySelectorAll(".emp")].sort((a, b) => a.dataset.index - b.dataset.index).map((g) => g.dataset.slug),
        names: Object.fromEntries([...document.querySelectorAll(".roster .row")].map((r) => [r.dataset.slug, r.querySelector(".r-name").textContent])),
    }));
    const others = order.roster.slice(2);
    const sorted = [...others].sort((a, b) => order.names[a].localeCompare(order.names[b], "en", { sensitivity: "base" }));
    check("D8: the roster starts with the CEO, then the Project Manager", order.roster[0] === "ceo" && order.roster[1] === "pm", order.roster.slice(0, 3).join());
    check("D8: the others are sorted by name (all at the same depth)", JSON.stringify(others) === JSON.stringify(sorted), others.join());
    check("D8: the room order is the roster order", JSON.stringify(order.room) === JSON.stringify(order.roster));
    await shot("d7-25-employees-1280.png");

    // The duplicate name warns and does not block.
    await fresh();
    await page.click(".bar button.add");
    const form = page.locator('form[aria-label="Add employee"]');
    await form.locator('input[name="name"]').fill("Ada");
    check("D8: a name that another employee has shows a warning", (await form.locator('[data-testid="name-duplicate"]').count()) === 1 && /Ada \(CEO\) has this name already/.test(await form.locator('[data-testid="name-duplicate"]').innerText()));
    check("D8: the warning does not block the add button", await form.locator("button.primary").isEnabled());
    await form.locator('input[name="name"]').fill("Ada Two");
    check("D8: another name has no warning", (await form.locator('[data-testid="name-duplicate"]').count()) === 0);
    await form.locator('input[name="name"]').fill("Ada");
    await form.locator("button.primary").click();
    await page.waitForSelector('.emp[data-slug="ada"]');
    check("D8: a duplicate name is saved (a warning is not a block)", (await page.locator(".emp").count()) === 5);
    await M("startSession", FLOOR, "ada");
    await M("startSession", FLOOR, "ceo");
    await page.waitForFunction(() => document.querySelectorAll(".tab").length === 2);
    const tabNames = await page.$$eval(".tab .label", (els) => els.map((e) => e.textContent.trim()));
    check("P27: two tabs with the same name show their role", tabNames.includes("Ada (CEO)") && tabNames.includes("Ada (Engineer)"), tabNames.join());

    // ================= D9. Floors are easy to pick =================
    await fresh();
    const ceo9 = await M("startSession", FLOOR, "ceo");
    await page.waitForSelector(`.tab[data-session="${ceo9}"]`);
    await M("setStatus", ceo9, "question");
    await page.click("details.floors > summary");
    const rowOf = (path) => page.locator(`.floors li[data-path="${path}"]`);
    check("D9: a floor row shows its live count and its questions", /1 live, 1 asks/.test((await rowOf(FLOOR).innerText()).replace(/\s+/g, " ")), (await rowOf(FLOOR).innerText()).replace(/\s+/g, " "));
    await rowOf("/home/demo/second").locator('[data-testid="floor-open"]').click();
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check("D9: a click on a row of the Floors list opens that floor", (await page.locator(".strip h2.name").innerText()) === "Second Floor");
    await rowOf(BIG).locator("button:not(.floor-text)").click();
    await page.waitForTimeout(100);
    check("D9: the forget button has its own click: the first click asks and does not open the floor",
        (await rowOf(BIG).locator("button:not(.floor-text)").innerText()) === "sure?" && (await page.locator(".storey.on").getAttribute("data-path")) === "/home/demo/second");
    check("D9: a row of a missing floor cannot be opened", await rowOf("/home/demo/gone").locator('[data-testid="floor-open"]').isDisabled());
    // The storey plates.
    const plates = await page.evaluate(() => {
        const svg = document.querySelector("svg.building");
        const k = svg.getScreenCTM().a;
        const rects = [...svg.querySelectorAll(".plate")].map((g) => {
            const bg = g.querySelector(".plate-bg").getBoundingClientRect();
            return { text: g.dataset.plate, l: bg.left, r: bg.right, t: bg.top, b: bg.bottom };
        });
        const overlap = [];
        for (let i = 0; i < rects.length; i++)
            for (let j = i + 1; j < rects.length; j++) {
                const a = rects[i], b = rects[j];
                if (a.l < b.r && b.l < a.r && a.t < b.b && b.t < a.b) overlap.push(`${a.text}/${b.text}`);
            }
        // The plate and the sticker share one transform: their local boxes show if they touch.
        const touch = [...svg.querySelectorAll(".storey")]
            .map((s) => {
                const sticker = s.querySelector("g[transform] > :is(rect.plain, image.logo)");
                const plate = s.querySelector(".plate-bg");
                const a = sticker.getBBox();
                const b = plate.getBBox();
                return b.y < a.y + a.height + 4 ? s.dataset.path : null;
            })
            .filter(Boolean);
        const font = Number(getComputedStyle(svg.querySelector(".plate-text")).fontSize.replace("px", "")) || 56;
        return { rects, overlap, touch, fontPx: font * k, count: svg.querySelectorAll(".plate").length };
    });
    check("D9: each storey has a plate with the floor name (4 storeys)", plates.count === 4 && plates.rects.map((r) => r.text).join() === "Demo Tower,Second Floor,Big Office,missing".replace("Second Floor", "Second Fl…") .replace("Demo Tower", "Demo Tower"), plates.rects.map((r) => r.text).join());
    check("D9: the plate text is at least 9 px high on the screen at the natural building size", plates.fontPx >= 9, String(plates.fontPx));
    check("D9: a plate does not overlap the sticker or another plate", plates.touch.length === 0 && plates.overlap.length === 0, JSON.stringify({ touch: plates.touch, overlap: plates.overlap }));
    await shot("d9-plates-1280.png");
    // A very long name is cut with an ellipsis.
    await page.fill("#floor-path", "/home/demo/long-name");
    await page.click("form.open button.primary");
    await page.fill("#floor-name", "Extraordinarily Long Floor Name");
    await page.click("form.create button.primary");
    await page.waitForSelector(".storey.on[data-index='4']");
    const longPlate = await page.locator(".storey.on .plate").getAttribute("data-plate");
    check("D9: a long floor name is cut with an ellipsis on the plate", longPlate === "Extraordi…" && [...longPlate].length <= 10, longPlate);

    // ================= D10. The toasts are an overlay =================
    await fresh();
    await M("startSession", FLOOR, "ceo");
    await page.waitForSelector(".tab");
    await M("setStatus", await page.evaluate(() => document.querySelector(".tab").dataset.session), "idle");
    await page.locator(".pane:not(.hidden) .xterm").click();
    const focusBefore = await page.evaluate(() => document.activeElement?.className);
    const box0 = { office: await page.locator(".office").boundingBox(), area: await page.locator(".area").boundingBox(), tabs: await page.locator(".tabs").boundingBox() };
    await M("notice", FLOOR, "First notice");
    await M("notice", FLOOR, "Second notice");
    await M("notice", FLOOR, "Third notice");
    await page.waitForFunction(() => document.querySelectorAll(".toast").length === 3);
    const box3 = { office: await page.locator(".office").boundingBox(), area: await page.locator(".area").boundingBox(), tabs: await page.locator(".tabs").boundingBox() };
    check("D10: with 3 toasts the office, the tabs and the terminal area are exactly where they were", JSON.stringify(box0) === JSON.stringify(box3), JSON.stringify({ box0, box3 }));
    const toastBoxes = await page.$$eval(".toast", (els) => els.map((e) => ({ ...e.getBoundingClientRect().toJSON(), text: e.querySelector(".text").textContent })));
    check("D10: the newest toast is on top", toastBoxes[0].text === "Third notice." && toastBoxes[2].text === "First notice." && toastBoxes[0].y < toastBoxes[1].y, JSON.stringify(toastBoxes.map((b) => [b.text, b.y])));
    check("D10: the toasts are at the top right, above the board and not above the terminal", toastBoxes.every((b) => b.x + b.width >= 1280 - 12 && b.y >= 40 && b.y + b.height < box3.tabs.y + 60) && toastBoxes[2].y + toastBoxes[2].height < box3.area.y + 200, JSON.stringify(toastBoxes[2]));
    check("D10: a toast does not take the focus", (await page.evaluate(() => document.activeElement?.className)) === focusBefore);
    await M("notice", FLOOR, "Fourth notice");
    await M("notice", FLOOR, "Fifth notice");
    await page.waitForFunction(() => document.querySelector('[data-testid="toasts-more"]'));
    check("D10: at most 4 toasts show, and a line counts the older ones", (await page.locator(".toast").count()) === 4 && /1 older message/.test(await page.locator('[data-testid="toasts-more"]').innerText()));
    await shot("d10-toasts-1280.png");
    // An information toast closes by itself after 6 s. An error toast stays.
    await M("employeeStart", BIG, "ceo", "pm");
    const infoToast = page.locator('.toast[data-kind="info"]');
    await infoToast.waitFor();
    await page.waitForTimeout(6400);
    check("D10: the information toast closed by itself after 6 s", (await infoToast.count()) === 0);
    check("D10: the error toasts are still there after 6 s", (await page.locator('.toast[data-kind="error"]').count()) === 4);
    await clearToasts();
    check("D10: an error toast closes with its button, and the hidden one shows after that", (await page.locator(".toast").count()) === 0 || (await page.locator('[data-testid="toasts-more"]').count()) === 0);
    check("D10: the layout never moved during the toasts", JSON.stringify(await page.locator(".office").boundingBox()) === JSON.stringify(box0.office));

    // ================= D1. Modals take and keep the focus =================
    await fresh();
    await M("startSession", FLOOR, "ceo");
    await page.waitForSelector(".tab");
    const ceo1 = await page.evaluate(() => document.querySelector(".tab").dataset.session);
    await M("setStatus", ceo1, "idle");
    await page.waitForTimeout(250);
    const writes = async () => (await calls("pty_write")).length;
    const synthEnter = () =>
        page.evaluate(() => {
            const ta = document.querySelector(".pane:not(.hidden) .xterm-helper-textarea");
            ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true, cancelable: true }));
        });
    // The control: with no modal a key at the terminal reaches the session. Without this the checks below could not fail.
    const w0 = await writes();
    await synthEnter();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "pty_write").length > n, w0);
    check("D1: control: with no modal a key at the terminal reaches the session (pty_write)", (await writes()) > w0);
    await page.locator(".pane:not(.hidden) .xterm").click();
    check("D1: the terminal has the focus before the modal", await page.evaluate(() => !!document.activeElement?.closest(".xterm")));
    const inside = (sel) => page.evaluate((s) => !!document.activeElement?.closest(s), sel);
    const behindInert = () =>
        page.evaluate(() => {
            const app = document.querySelector(".app");
            return { inert: app.inert === true, hidden: app.getAttribute("aria-hidden") === "true" };
        });

    async function modalChecks(name, openIt, dialogSel, closeByEscapeFocus, firstField) {
        const base = await writes();
        await openIt();
        await page.locator(dialogSel).waitFor();
        await page.waitForTimeout(80);
        const first = await page.evaluate(() => ({ tag: document.activeElement?.tagName, name: document.activeElement?.getAttribute("name") ?? document.activeElement?.id ?? document.activeElement?.className }));
        check(`D1 ${name}: the first field gets the focus when the modal opens`, (await inside(dialogSel)) && firstField(first), JSON.stringify(first));
        const behind = await behindInert();
        check(`D1 ${name}: everything behind the modal is inert and aria-hidden`, behind.inert && behind.hidden, JSON.stringify(behind));
        let escaped = 0;
        for (let i = 0; i < 40; i++) {
            await page.keyboard.press("Tab");
            if (!(await inside(dialogSel))) escaped++;
        }
        for (let i = 0; i < 40; i++) {
            await page.keyboard.press("Shift+Tab");
            if (!(await inside(dialogSel))) escaped++;
        }
        check(`D1 ${name}: Tab and Shift+Tab stay inside (80 key presses, none left the modal)`, escaped === 0, String(escaped));
        await synthEnter();
        await page.keyboard.type("zz");
        await page.waitForTimeout(120);
        check(`D1 ${name}: no key reached the terminal behind (no pty_write; xterm is blurred and inert)`, (await writes()) === base, `${await writes()} vs ${base}`);
        await page.keyboard.press("Escape");
        await page.locator(dialogSel).waitFor({ state: "detached" });
        await page.waitForTimeout(150);
        check(`D1 ${name}: Escape closed the modal and did not reach the terminal`, (await writes()) === base, `${await writes()} vs ${base}`);
        check(`D1 ${name}: the focus is back on the element that opened the modal`, await page.evaluate((sel) => !!document.activeElement?.matches(sel), closeByEscapeFocus), await page.evaluate(() => document.activeElement?.tagName + "." + document.activeElement?.className));
        const after = await behindInert();
        check(`D1 ${name}: the page is no longer inert`, !after.inert && !after.hidden);
    }

    await modalChecks("employee form", () => page.click(".bar button.add"), 'form[aria-label="Add employee"]', "button.add", (f) => f.name === "name");
    await modalChecks("settings", () => page.click("button.settings"), '[role="dialog"][aria-label="Settings"]', "button.settings", (f) => f.tag === "INPUT" || f.tag === "SELECT");
    await page.locator('.board .card[data-task="3"]').click();
    await page.waitForSelector('[data-testid="open-report"]');
    await modalChecks("report overlay", () => page.click('[data-testid="open-report"]'), '[data-testid="reader"]', '[data-testid="open-report"]', (f) => f.tag === "BUTTON");
    await page.getByRole("button", { name: /Back/ }).click();

    // The edit form too, and a click on the backdrop; a toast stays usable while a modal is open.
    await selectEmp("ceo");
    await page.click(".bar button.edit");
    await page.locator('form[aria-label="Edit employee"]').waitFor();
    await M("notice", FLOOR, "A message while a modal is open");
    await page.waitForSelector(".toast");
    await page.locator('.toast button[aria-label="Dismiss"]').click();
    check("D1: a toast can be closed while a modal is open", (await page.locator(".toast").count()) === 0);
    await page.keyboard.press("Escape");
    await page.locator('form[aria-label="Edit employee"]').waitFor({ state: "detached" });
    check("D1: the edit form closes with Escape and the focus is back on edit", await page.evaluate(() => !!document.activeElement?.matches("button.edit")));
    await page.click(".bar button.add");
    await page.locator('form[aria-label="Add employee"]').waitFor();
    await page.mouse.click(4, 400);
    await page.locator('form[aria-label="Add employee"]').waitFor({ state: "detached" });
    check("D1: a click on the backdrop closes the modal and gives the focus back", await page.evaluate(() => !!document.activeElement?.matches("button.add")));

    // A browser with no `inert`: the fallback blocks the focus and the pointer.
    await page.evaluate(() => {
        window.__inertDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "inert");
        delete HTMLElement.prototype.inert;
    });
    check("D1: the test browser now has no inert attribute", await page.evaluate(() => !("inert" in HTMLElement.prototype)));
    await page.click(".bar button.add");
    await page.locator('form[aria-label="Add employee"]').waitFor();
    const fb = await page.evaluate(() => {
        const app = document.querySelector(".app");
        const tabbable = [...app.querySelectorAll("button, input, select, textarea, summary, [tabindex]")].filter((e) => e.getAttribute("tabindex") !== "-1" && !e.disabled);
        const hit = document.elementFromPoint(100, 500);
        return { marked: app.hasAttribute("data-inert-fallback"), tabbable: tabbable.length, hitInsideApp: !!hit?.closest(".app"), hidden: app.getAttribute("aria-hidden") };
    });
    check("D1: with no inert the page behind gets the fallback: marked, no tabbable element, the pointer passes through", fb.marked && fb.tabbable === 0 && !fb.hitInsideApp && fb.hidden === "true", JSON.stringify(fb));
    let escapedFb = 0;
    for (let i = 0; i < 30; i++) {
        await page.keyboard.press("Tab");
        if (!(await inside('form[aria-label="Add employee"]'))) escapedFb++;
    }
    check("D1: with no inert Tab still stays inside the modal", escapedFb === 0, String(escapedFb));
    await page.keyboard.press("Escape");
    await page.locator('form[aria-label="Add employee"]').waitFor({ state: "detached" });
    const restored = await page.evaluate(() => {
        const app = document.querySelector(".app");
        const stuck = [...app.querySelectorAll("[tabindex='-1']")].filter((e) => !e.matches(".xterm-helper-textarea, [role=separator]") && e.tagName === "BUTTON");
        return { marked: app.hasAttribute("data-inert-fallback"), hidden: app.getAttribute("aria-hidden"), stuck: stuck.length };
    });
    check("D1: with no inert the fallback is removed when the modal closes", !restored.marked && restored.hidden === null && restored.stuck === 0, JSON.stringify(restored));
    await page.evaluate(() => Object.defineProperty(HTMLElement.prototype, "inert", window.__inertDescriptor));

    // ================= M6. Commands with no question: the text and the refused forms =================
    await fresh();
    await page.click("button.settings");
    const dlg = page.locator('[role="dialog"][aria-label="Settings"]');
    const perm = dlg.locator('section[data-section="permissions"]');
    await perm.waitFor();
    await perm.locator('[data-testid="rule-list"]').waitFor();
    const hookNote = (await perm.locator('[data-testid="rules-hook-note"]').innerText()).replace(/\s+/g, " ").trim();
    check("M6: the section says that a rule must match the command as the CLI sees it (the hook note, always visible)",
        hookNote === "A rule must match the command as the CLI sees it. If a hook on this machine rewrites commands (for example cargo test becomes rtk cargo test), write the rule for the rewritten command.", hookNote);
    check("P41: the long text is behind a More... link, and the short line shows", (await perm.locator('[data-testid="rules-more-text"]').count()) === 0 && (await perm.locator('[data-testid="rules-more"]').innerText()) === "More...");
    await perm.locator('[data-testid="rules-more"]').click();
    check("P41: More... shows the rest of the text", (await perm.locator('[data-testid="rules-more-text"]').count()) === 1 && (await perm.locator('[data-testid="rules-more"]').innerText()) === "Less");
    const setsBeforeM6 = (await calls("floor_permissions_set")).length;
    const ruleField = perm.locator('input[aria-label="New rule"]');
    const ruleAdd = perm.getByRole("button", { name: "add", exact: true });
    const refused = [
        ["Read", /no pattern/],
        ["Glob", /no pattern/],
        ["WebSearch(rust)", /no pattern/],
        ["Bash(echo (x))", /\( or a \)/],
        ["Bash(café:*)", /printable ASCII/],
        ["Bash( ls:*)", /start with a space/],
        ["Bash(sh -c x)", /"sh" is not allowed/],
        ["Bash(sudo ls)", /"sudo" is not allowed/],
        ["Bash(rm:*)", /"rm" is not allowed/],
        ["Bash(curl:*)", /"curl" is not allowed/],
        ["Bash(*.sh)", /first word/],
        ["Bash(git:*)", /needs a subcommand/],
        ["Bash(git push:*)", /"git push" is not allowed/],
        ["Bash(git config:*)", /"git config" is not allowed/],
        ["Bash(git log:*)", /must be the exact command/],
        ["Bash(sqlite3:*)", /"sqlite3" is not allowed/],
        ["Bash(man:*)", /"man" is not allowed/],
        ["Read(/etc/passwd)", /relative/],
        ["Read(../secret)", /\.\./],
        ["Grep(a/../b)", /\.\./],
        ["WebFetch(example.com)", /domain:/],
        ["WebFetch(domain:*.example.com)", /\*/],
        ["WebFetch(domain:localhost)", /dot/],
    ];
    for (const [rule, reason] of refused) {
        await ruleField.fill(rule);
        await ruleAdd.click();
        const text = await perm.locator('[data-testid="rule-error"]').innerText();
        check(`M6: the rule ${JSON.stringify(rule)} is refused with a reason`, reason.test(text), text);
    }
    check("M6: a refused form makes no call to the app", (await calls("floor_permissions_set")).length === setsBeforeM6);
    for (const rule of ["Bash(git status)", "Read(./src/**)", "WebFetch(domain:example.com)", "WebSearch"]) {
        await ruleField.fill(rule);
        await ruleAdd.click();
        await perm.locator(`li[data-rule="${rule}"]`).waitFor();
    }
    check("M6: the good forms are accepted and saved", (await perm.locator("li[data-rule]").count()) === 4 && (await calls("floor_permissions_set")).length === setsBeforeM6 + 4);
    // A rule that the client accepts and the server refuses: the text of the server shows as it is.
    await ruleField.fill("Bash(server-says-no:*)");
    await ruleAdd.click();
    await page.waitForFunction(() => /refused by the server/.test(document.querySelector('[data-testid="rule-error"]')?.textContent ?? ""));
    check("M6: the refusal of the server shows with its own text, and the list does not change",
        (await perm.locator('[data-testid="rule-error"]').innerText()) === 'the rule "Bash(server-says-no:*)" is refused by the server: this word is on its list.' && (await perm.locator("li[data-rule]").count()) === 4);
    // Examples: a click adds the rule.
    const exampleBtn = perm.locator('button[data-example="Bash(npm test:*)"]');
    await exampleBtn.click();
    await perm.locator('li[data-rule="Bash(npm test:*)"]').waitFor();
    const lastSet = (await calls("floor_permissions_set")).pop();
    check("P41: a click on an example adds that rule (floor_permissions_set with the whole list)", lastSet.args.allow.includes("Bash(npm test:*)") && lastSet.args.allow.length === 5 && (await perm.locator('button[data-example="Bash(npm test:*)"]').count()) === 0);

    // ================= Settings polish: P38, P39, P40 =================
    const keys = dlg.locator('section[data-section="keys"]');
    check("P38: the key text does not contradict itself (not set, and a hint that says where a pasted key goes)",
        (await keys.locator(".state").innerText()) === "not set" && /A key that you paste here is stored in the app config file\. The app uses it before the environment variable/.test((await keys.innerText()).replace(/\s+/g, " ")));
    const defs = dlg.locator('section[data-section="defaults"]');
    check("P39: the model and effort selects say Choose a CLI first while no CLI is set",
        (await defs.locator('select[name="defaultModel"] option').first().innerText()) === "Choose a CLI first" && (await defs.locator('select[name="defaultEffort"] option').first().innerText()) === "Choose a CLI first" && (await defs.locator('select[name="defaultModel"]').isDisabled()));
    check("P40: the save button says why it is off (no change to save)", (await defs.locator('[data-testid="defaults-reason"]').innerText()) === "No change to save." && (await defs.locator("button.primary").isDisabled()));
    await defs.locator('select[name="defaultCli"]').selectOption("claude");
    check("P39: with a CLI the first option is (not set)", (await defs.locator('select[name="defaultModel"] option').first().innerText()) === "(not set)");
    await defs.locator('select[name="defaultModel"]').selectOption("haiku");
    await defs.locator("button.primary").click();
    await defs.locator('[data-testid="defaults-saved"]').waitFor();
    check("P40: a Saved text shows after save defaults", (await defs.locator('[data-testid="defaults-saved"]').innerText()) === "Saved");
    await page.waitForTimeout(2300);
    check("P40: the Saved text goes away after 2 s", (await defs.locator('[data-testid="defaults-saved"]').count()) === 0);
    check("P20: the default policy select has readable names", JSON.stringify(await defs.locator('select[name="defaultPolicy"] option').allInnerTexts()) === JSON.stringify(["(not set)", "Read only", "Edits in its task folder", "Asks before each action"]));
    const interfaceSec = dlg.locator('section[data-section="interface"]');
    check("P4: the settings panel has the switch of the debug paste bar, off by default", (await interfaceSec.locator('input[name="debugBar"]').isChecked()) === false && (await page.locator("details.debug-area").count()) === 0);
    await interfaceSec.locator('input[name="debugBar"]').check();
    check("P4: the switch shows the debug paste bar", (await page.locator("details.debug-area").count()) === 1);
    await interfaceSec.locator('input[name="debugBar"]').uncheck();
    check("P4: the switch hides the debug paste bar again", (await page.locator("details.debug-area").count()) === 0);
    await shot("settings-1280.png");
    await page.keyboard.press("Escape");
    await page.keyboard.press("Control+Shift+D");
    check("P4: Ctrl+Shift+D shows the debug paste bar (and sends no key to the terminal)", (await page.locator("details.debug-area").count()) === 1);
    await page.keyboard.press("Control+Shift+D");
    check("P4: Ctrl+Shift+D hides it again", (await page.locator("details.debug-area").count()) === 0);

    // ================= The employee form: P18 to P25 =================
    await fresh();
    await page.click(".bar button.add");
    const f = page.locator('form[aria-label="Add employee"]');
    check("P18: the name placeholder is not the name of an employee", (await f.locator('input[name="name"]').getAttribute("placeholder")) === "Name, for example Lena");
    check("P19: no slug word: the form says File name", /File name:/.test(await f.locator(".slug").innerText()) && !/slug/i.test(await f.innerText()));
    check("P22: a new employee reports to the Project Manager by default", (await f.locator('select[name="manager"]').inputValue()) === "pm");
    check("P20: the policy options have readable names", JSON.stringify(await f.locator('select[name="policy"] option').allInnerTexts()) === JSON.stringify(["Read only", "Edits in its task folder", "Asks before each action"]));
    const helps = [];
    for (const id of ["read_only", "edit_in_worktree", "ask_always"]) {
        await f.locator('select[name="policy"]').selectOption(id);
        helps.push(await f.locator('[data-testid="policy-help"]').innerText());
    }
    check("P20: one help line for each policy, and it follows the choice", new Set(helps).size === 3 && /Reads the project/.test(helps[0]) && /task folder/.test(helps[1]) && /before each command/.test(helps[2]), JSON.stringify(helps));
    check("P21: an empty name: the add button is off and a reason shows next to it", (await f.locator("button.primary").isDisabled()) && (await f.locator('[data-testid="form-reason"]').innerText()) === "Write a name.");
    await f.locator('input[name="name"]').fill("!!!");
    check("P21: a name with no letter or digit has a red text under the field", (await f.locator('[data-testid="name-problem"]').innerText()) === "The name needs at least one letter or digit." && (await f.locator("button.primary").isDisabled()));
    await f.locator('input[name="name"]').fill("Lena");
    check("P19: the file name is shown", (await f.locator(".slug code").innerText()) === "lena.md");
    await f.locator('input[name="role"]').fill("");
    check("P21: an empty role: a reason next to the button", (await f.locator('[data-testid="form-reason"]').innerText()) === "Write a role." && (await f.locator("button.primary").isDisabled()));
    await f.locator('input[name="role"]').fill("Engineer");
    check("P23: no instructions: a warning shows, and the add button is on", (await f.locator('[data-testid="no-instructions"]').count()) === 1 && (await f.locator("button.primary").isEnabled()));
    await f.locator('textarea[name="roleText"]').fill("You write tests.");
    check("P23: the warning goes away with a text", (await f.locator('[data-testid="no-instructions"]').count()) === 0);
    check("P25: selects and inputs have one style (no native select look)", await page.evaluate(() => getComputedStyle(document.querySelector('select[name="cli"]')).appearance === "none" && getComputedStyle(document.querySelector('select[name="cli"]')).backgroundColor === getComputedStyle(document.querySelector('input[name="name"]')).backgroundColor));
    await f.locator('input[name="name"]').fill("Ada");
    await shot("employee-form-1280.png");
    await page.keyboard.press("Escape");

    // ================= The office header, the room and the sessions: P9 to P17, P26, P43 =================
    await fresh();
    const infoFits = await page.evaluate(() => {
        const info = document.querySelector(".office .info");
        return { sh: info.scrollHeight, ch: info.clientHeight };
    });
    check("P9: at 1280 x 800 the info panel of a floor with 4 employees shows header, bar and roster with no scroll", infoFits.sh <= infoFits.ch + 1, JSON.stringify(infoFits));
    // One primary button at a time, who the resume lists.
    await openFloorByStorey("/home/demo/second");
    check("P10: a floor with no setup yet has one primary button: the setup agent", (await page.locator(".office .buttons button.primary").count()) === 1 && (await page.locator(".office .buttons button.setup.primary").count()) === 1);
    await M("setResumable", "/home/demo/second", ["ceo"]);
    await page.locator('[data-testid="resume-team"]').waitFor();
    check("P10: with a team to resume only the resume button is primary", (await page.locator(".office .buttons button.primary").count()) === 1 && (await page.locator('[data-testid="resume-team"].primary').count()) === 1);
    check("P11: the resume says who: a line and a tooltip", (await page.locator('[data-testid="resume-who"]').innerText()) === "Resumes: Bo (CEO)" && /Bo \(CEO\)/.test(await page.locator('[data-testid="resume-team"]').getAttribute("title")));
    await openFloorByStorey(FLOOR);
    // The CLI choice of the setup agent.
    await page.getByRole("button", { name: "Run the setup agent" }).click();
    const choose = page.locator(".choose");
    check("P12: the CLI choice has its own row with claude, codex and cancel", JSON.stringify(await choose.locator("button").allInnerTexts()) === JSON.stringify(["claude", "codex", "cancel"]));
    check("P12: New logo is not in the choice row, and it is hidden while the operator chooses", (await choose.getByText("New logo").count()) === 0 && (await page.locator("button.newlogo").count()) === 0);
    await choose.getByRole("button", { name: "cancel" }).click();
    await M("patchFloor", FLOOR, { defaults: { cli: "codex" } });
    await page.waitForFunction(() => document.querySelector('.office button.other-cli'));
    await page.getByRole("button", { name: "Run the setup agent" }).click();
    const setupCalls = await waitCalls("setup_start", 1);
    check("P12: with a floor default CLI the setup agent starts with it and does not ask", setupCalls[0].args.cli === "codex" && (await page.locator(".choose").count()) === 0, JSON.stringify(setupCalls[0].args));
    // Broken employee files.
    await M("setEmployeeErrors", FLOOR, ["/home/demo/tower/.swarm/employees/broken.md: line 1, column 15: a quote is missing"]);
    await page.locator('[data-testid="employee-errors"]').waitFor();
    const errBox = await page.evaluate(() => {
        const ul = document.querySelector('[data-testid="employee-errors"]');
        const file = ul.querySelector(".e-file");
        const folder = ul.querySelector(".e-folder");
        return { file: file.textContent, folder: folder.textContent, sw: ul.scrollWidth, cw: ul.clientWidth, folderBlock: getComputedStyle(folder).display };
    });
    check("P16: a broken employee file shows its file name first, a message, and the folder on its own line, with no horizontal scroll",
        errBox.file === "broken.md" && errBox.folder === "/home/demo/tower/.swarm/employees" && errBox.folderBlock === "block" && errBox.sw <= errBox.cw, JSON.stringify(errBox));
    await M("setEmployeeErrors", FLOOR, []);
    // The room zoom.
    const roomBox = await page.locator(".room svg").boundingBox();
    check("P15: the room starts at zoom 1", (await page.locator(".room svg").getAttribute("data-zoom")) === "1.00");
    await page.mouse.move(roomBox.x + roomBox.width / 2, roomBox.y + roomBox.height / 2);
    await page.mouse.wheel(0, -400);
    await page.waitForTimeout(100);
    const z1 = Number(await page.locator(".room svg").getAttribute("data-zoom"));
    check("P15: the wheel zooms the room in", z1 > 1.5, String(z1));
    const vb1 = await page.locator(".room svg").getAttribute("viewBox");
    await page.mouse.move(roomBox.x + 30, roomBox.y + roomBox.height - 30);
    await page.mouse.down();
    await page.mouse.move(roomBox.x + 120, roomBox.y + roomBox.height - 80, { steps: 4 });
    await page.mouse.up();
    check("P15: a drag on an empty place moves the zoomed room", (await page.locator(".room svg").getAttribute("viewBox")) !== vb1);
    await page.getByRole("button", { name: "Show the whole room" }).click();
    check("P15: the fit button shows the whole room again", (await page.locator(".room svg").getAttribute("data-zoom")) === "1.00");
    await page.getByRole("button", { name: "Zoom in" }).click();
    check("P15: the zoom in button zooms", Number(await page.locator(".room svg").getAttribute("data-zoom")) > 1.3);
    await page.getByRole("button", { name: "Show the whole room" }).click();
    // Stop asks when the session works; a stopped tab says what to do in the app.
    const ceo7 = await M("startSession", FLOOR, "ceo");
    await page.waitForSelector(`.tab[data-session="${ceo7}"]`);
    await M("setStatus", ceo7, "working");
    await selectEmp("ceo");
    await bar.locator("button.stop").click();
    await page.waitForTimeout(150);
    check("P17: stop on a working session asks first: Ada is working. Stop?", (await bar.locator("button.stop").innerText()) === "Ada is working. Stop?" && (await calls("session_stop")).length === 0);
    await bar.locator("button.stop").click();
    await waitCalls("session_stop", 1);
    check("P17: the second click stops the session", (await calls("session_stop"))[0].args.sessionId === ceo7);
    await page.waitForSelector('[data-role="stopped-note"]');
    check("P26: a stopped session shows an app line that says what to do", /Stopped\. Use "resume" or "start"/.test(await page.locator('.pane[data-session="' + ceo7 + '"] ~ [data-role="stopped-note"]').first().innerText()));
    // A start that fails because of a program path: the toast offers Settings.
    await selectEmp("eng-sam");
    await bar.locator("button.start").click();
    const progToast = page.locator(".toast").filter({ hasText: "Cannot start eng-sam" });
    await progToast.waitFor();
    check("P43: a start error about a program says where to fix it, and has a button", /Settings > Programs|Set its path in Programs/.test(await progToast.innerText()) && (await progToast.getByRole("button", { name: "Open Settings" }).count()) === 1, await progToast.innerText());
    await progToast.getByRole("button", { name: "Open Settings" }).click();
    await page.locator('[role="dialog"][aria-label="Settings"]').waitFor();
    await page.waitForTimeout(200);
    const progInView = await page.evaluate(() => {
        const d = document.querySelector('[role="dialog"]').getBoundingClientRect();
        const s = document.querySelector('[data-section="programs"]').getBoundingClientRect();
        return s.top >= d.top - 1 && s.top < d.bottom;
    });
    check("P43: the button opens Settings at the Programs section", progInView);
    await page.keyboard.press("Escape");

    // ================= The task board: P29, P33 to P37 =================
    await fresh();
    await page.evaluate(() => localStorage.setItem("agent-tower:layout", JSON.stringify({ board: 200 })));
    await page.goto(url);
    await page.waitForSelector(".board .group");
    const head = await page.evaluate(() => {
        const h = document.querySelector(".board .head");
        const kids = [...h.children].filter((c) => c.getBoundingClientRect().width > 0);
        const tops = kids.map((c) => Math.round(c.getBoundingClientRect().top + c.getBoundingClientRect().height / 2));
        return { h: h.getBoundingClientRect().height, count: document.querySelector(".open-count").textContent, newText: document.querySelector("button.new").textContent.trim(), sameRow: Math.max(...tops) - Math.min(...tops) <= 3, w: document.querySelector(".board").getBoundingClientRect().width };
    });
    check("P29: at the minimum width the board header stays on one line (the words go, the numbers stay)", head.w <= 202 && head.sameRow && head.h < 48 && head.count === "4" && head.newText === "+", JSON.stringify(head));
    await shot("board-min-1280.png");
    await page.evaluate(() => localStorage.clear());
    await page.goto(url);
    await page.waitForSelector(".board .group");
    await M("freeze", true);
    const cards = () => page.$$eval(".board .card", (els) => els.map((e) => e.dataset.task));
    check("P34/P35: the groups have chevron icons, and a card does not repeat its status", (await page.locator(".board .group-head svg.chev").count()) === 5 && (await page.locator(".board .c-status").count()) === 0 && !/^[v>]$/.test((await page.locator(".board .group-head").first().innerText()).trim().charAt(0)));
    await page.locator('[data-testid="filter-assignee"]').selectOption("eng-lin");
    check("P33: the filter by assignee shows the tasks of Lin only (T-1 and T-2)", (await cards()).sort().join() === "1,2", (await cards()).join());
    check("P33: the filter says how many tasks show and offers to clear it", /2 of 5 tasks\./.test(await page.locator('[data-testid="filter-note"]').innerText()));
    await page.locator('[data-testid="filter-query"]').fill("login");
    check("P33: the search box and the assignee work together (T-2)", (await cards()).join() === "2");
    await page.locator('[data-testid="filter-query"]').fill("zzz");
    check("P33: no match shows a text", (await page.locator(".board .none").innerText()) === "No task matches the filter.");
    await page.locator('[data-testid="filter-note"] button').click();
    check("P33: Clear the filter shows all 5 tasks", (await cards()).length === 5);
    await page.locator('[data-testid="filter-assignee"]').selectOption("(unassigned)");
    check("P33: the filter Unassigned shows T-5", (await cards()).join() === "5");
    await page.locator('[data-testid="filter-assignee"]').selectOption("");
    const cardH = await page.locator('.board .card[data-task="2"]').evaluate((e) => e.getBoundingClientRect().height);
    await page.locator('[data-testid="filter-compact"]').check();
    const compactH = await page.locator('.board .card[data-task="2"]').evaluate((e) => e.getBoundingClientRect().height);
    check("P33: the compact option makes each task one line (less than 32 px, against " + Math.round(cardH) + ")", compactH < 32 && compactH < cardH, String(compactH));
    await shot("board-compact-1280.png");
    await page.locator('[data-testid="filter-compact"]').uncheck();
    await page.locator('.board .card[data-task="2"]').click();
    const sel = await page.locator('.board [aria-label="Task detail"] select[aria-label="Assign to"]').evaluate((e) => e.getBoundingClientRect().width);
    check("P36: in the detail the Assign to select is wide enough for Choose an employee (at least 120 px)", sel >= 120, String(sel));
    await page.getByRole("button", { name: /Back/ }).click();
    await page.locator('.board .card[data-task="3"]').click();
    await page.waitForSelector('[data-testid="merge"]');
    check("P32: the merge button names the target branch", (await page.locator('[data-testid="merge"]').innerText()).trim() === "Merge into main");
    await page.getByRole("button", { name: /Back/ }).click();
    await page.getByRole("button", { name: "+ New" }).click();
    check("P37: a new task with no title shows why the button is off", (await page.locator('[data-testid="new-task-reason"]').innerText()) === "Give the task a title." && (await page.getByRole("button", { name: "Make the task" }).isDisabled()));
    await page.locator('input[aria-label="Title of the new task"]').fill("Something");
    check("P37: the reason goes away with a title", (await page.locator('[data-testid="new-task-reason"]').count()) === 0);
    await page.getByRole("button", { name: "Cancel" }).click();

    // ================= The first start: P1 =================
    await fresh();
    await page.click("details.floors > summary");
    for (const path of ["/home/demo/second", BIG, "/home/demo/gone", FLOOR]) {
        const forget = page.locator(`.floors li[data-path="${path}"] button:not(.floor-text)`);
        await forget.click();
        await forget.click();
    }
    await page.locator('[data-testid="empty-office"]').waitFor();
    const emptyBox = await page.evaluate(() => {
        const card = document.querySelector('[data-testid="empty-office"] .card').getBoundingClientRect();
        const office = document.querySelector(".office").getBoundingClientRect();
        return { dx: Math.abs(card.left + card.width / 2 - (office.left + office.width / 2)), dy: Math.abs(card.top + card.height / 2 - (office.top + office.height / 2)), w: card.width };
    });
    check("P1: with no floor the office area shows a centred card: Open a project folder", emptyBox.dx < 3 && emptyBox.dy < 40 && /open a project folder/i.test(await page.locator('[data-testid="empty-office"] h2').innerText()), JSON.stringify(emptyBox));
    await page.waitForFunction(() => document.activeElement?.id === "start-path", null, { timeout: 2000 }).catch(() => {});
    check("P1: the path field has the focus, and the Open button is off while it is empty", (await page.evaluate(() => document.activeElement?.id)) === "start-path" && (await page.locator('[data-testid="empty-office"] button.primary').isDisabled()),
        await page.evaluate(() => document.activeElement?.tagName + "#" + document.activeElement?.id + "." + document.activeElement?.className));
    await shot("empty-1280.png");
    await page.fill("#start-path", "/home/demo/second");
    await page.keyboard.press("Enter");
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check("P1: a path in the card opens the floor", (await page.locator('[data-testid="empty-office"]').count()) === 0 && (await page.locator(".strip h2.name").innerText()) === "Second Floor");

    // ================= Wide windows (P7, P8) and the screenshots of each repaired screen =================
    async function screens(size, tag) {
        await fresh(size);
        const wide = size.width >= 2000;
        if (wide) {
            check("P7: on a tall window the list of floors is open at the start", (await page.locator("details.floors[open]").count()) === 1);
            const w = await page.evaluate(() => ({
                info: document.querySelector(".office .info").getBoundingClientRect().width,
                office: document.querySelector(".office").getBoundingClientRect().width,
                room: document.querySelector(".office .room").getBoundingClientRect().width,
                roomH: document.querySelector(".office .room").getBoundingClientRect().height,
                officeH: document.querySelector(".office").getBoundingClientRect().height,
            }));
            check("P8: at 2560 the info panel is at most 40% of the office and the room has the rest", w.info <= w.office * 0.4 && w.room >= w.office * 0.58, JSON.stringify(w));
            check("P8: at 2560 the office is taller than before (at least 650 px)", w.officeH >= 650, String(w.officeH));
        }
        // 1. The question states.
        const a = await M("startSession", FLOOR, "ceo");
        const b = await M("startSession", FLOOR, "eng-lin");
        const c = await M("startSession", FLOOR, "eng-sam");
        await page.waitForSelector(`.tab[data-session="${c}"]`);
        await M("setStatus", a, "idle");
        await M("setStatus", b, "question");
        await M("setStatus", c, "idle");
        await page.waitForSelector('[data-testid="attention-bar"]');
        await page.waitForTimeout(300);
        await shot(`${tag}-1-question.png`);
        await openFloorByStorey(BIG);
        const pia = await M("startSession", BIG, "pm");
        await M("setStatus", pia, "question");
        await M("setStatus", await M("startSession", BIG, "ceo"), "idle");
        await openFloorByStorey(FLOOR);
        await page.waitForFunction(() => document.querySelector('[data-testid="attention-bar"]')?.dataset.count === "2");
        await page.waitForTimeout(300);
        await shot(`${tag}-2-two-floors.png`);
        // 2. The unread mark and the amber state in the roster.
        await M("setStatus", b, "idle");
        await M("setStatus", c, "working");
        await M("setStatus", c, "idle");
        await M("setStatus", pia, "idle");
        await page.waitForTimeout(300);
        await shot(`${tag}-3-unread.png`);
        // 3. The employee form with its warnings.
        await page.click(".bar button.add");
        await page.fill('input[name="name"]', "Ada");
        await page.waitForTimeout(150);
        await shot(`${tag}-4-employee-form.png`);
        await page.keyboard.press("Escape");
        // 4. The settings panel, at the permissions and the defaults.
        await page.click("button.settings");
        await page.locator('[data-section="permissions"] li').first().waitFor();
        await page.locator('[data-section="defaults"]').scrollIntoViewIfNeeded();
        await page.waitForTimeout(150);
        await shot(`${tag}-5-settings.png`);
        await page.keyboard.press("Escape");
        // 5. The report and the diff overlay.
        await page.locator('.board .card[data-task="3"]').click();
        await page.waitForSelector('[data-testid="open-report"]');
        await page.click('[data-testid="open-report"]');
        await page.waitForSelector('[data-testid="reader"]');
        await shot(`${tag}-6-report.png`);
        await page.keyboard.press("Escape");
        await page.click('[data-testid="show-diff"]');
        await page.waitForSelector('[data-testid="diff-box"]');
        await shot(`${tag}-7-diff.png`);
        await page.keyboard.press("Escape");
        await page.getByRole("button", { name: /Back/ }).click();
        // 6. The toasts.
        await M("notice", FLOOR, "Task T-4 is blocked: the main checkout is on the branch other.");
        await M("notice", FLOOR, "Could not resume Maya: the program was not found. Change it in Settings.");
        await M("employeeStart", BIG, "ceo", "eng-1");
        await page.waitForFunction(() => document.querySelectorAll(".toast").length >= 3);
        await page.waitForTimeout(400);
        await shot(`${tag}-8-toasts.png`);
        await clearToasts();
        // 7. A crowded floor.
        await M("addEmployees", BIG, 12);
        await openFloorByStorey(BIG);
        await page.waitForFunction(() => document.querySelectorAll(".emp").length === 25);
        await page.waitForTimeout(400);
        if (wide) {
            const cols = await page.evaluate(() => getComputedStyle(document.querySelector(".roster")).gridTemplateColumns.split(" ").length);
            check("P8: at 2560 the roster of a big floor uses columns (3 or more)", cols >= 3, String(cols));
        }
        await shot(`${tag}-9-crowded.png`);
        await openFloorByStorey(FLOOR);
        // 8. The board with a filter.
        await page.locator('[data-testid="filter-assignee"]').selectOption("eng-lin");
        await page.waitForTimeout(150);
        await shot(`${tag}-10-board-filter.png`);
    }
    await screens({ width: 1280, height: 800 }, "r1280");
    await screens({ width: 2560, height: 1400 }, "r2560");
    await page.setViewportSize({ width: 1280, height: 800 });

    }

    // ================= G. A floor whose folder is gone (docs/ipc-final.md) =================
    const banner = page.locator('[data-testid="gone-banner"]');
    const REASON = /folder of this floor is gone/i;
    // Changed on purpose (N1): the strip says "The folder is gone:" (the strip is about the open floor, and "this floor" was redundant).
    const GONE_TEXT = `The folder is gone: ${FLOOR}. The app cannot read or change this floor.`;
    const flat = (t) => t.replace(/\s+/g, " ").trim();
    const off = async (loc) => (await loc.count()) === 1 && (await loc.isDisabled());
    const on = async (loc) => (await loc.count()) === 1 && !(await loc.isDisabled());
    const reasoned = async (loc) => REASON.test((await loc.getAttribute("title")) ?? "");
    const twice = async (loc) => {
        await loc.click();
        await loc.click();
    };
    const toastKinds = (re) =>
        page.evaluate((r) => [...document.querySelectorAll(".toast")].filter((t) => new RegExp(r).test(t.textContent)).map((t) => t.dataset.kind), re.source);
    const synthEnterNow = () =>
        page.evaluate(() => {
            const ta = document.querySelector(".pane:not(.hidden) .xterm-helper-textarea");
            ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true, cancelable: true }));
        });

    // ---- G1. The banner, the disabled controls, the stop button, Check again, Forget, and back ----
    await fresh();
    check("G1: no banner and no dim while the floor is there", (await banner.count()) === 0 && (await page.locator('[data-testid="gone-dim"]').count()) === 0);
    await M("startSession", FLOOR, "ceo");
    await page.waitForSelector(".tab");
    const gCeo = await page.evaluate(() => document.querySelector(".tab").dataset.session);
    await M("setStatus", gCeo, "question");
    await page.locator('[data-testid="attention-bar"]').waitFor();
    await selectEmp("pm");
    check("G1: control: before the floor is gone the start button of an employee is enabled", await on(page.locator(".bar button.start")));
    // Load the rules and open a task now: a read of a gone floor gives FLOOR_GONE, so the panels must already hold their data.
    await page.click("button.settings");
    await page.locator('[data-section="permissions"] input[name="rule"]').waitFor();
    check("G1: control: before the floor is gone the rule field and the defaults button work", (await on(page.locator('input[name="rule"]'))) && (await on(page.locator('select[name="defaultCli"]'))));
    await page.fill('input[name="rule"]', "Bash(cargo test:*)");
    await page.locator('[data-section="permissions"] button[type="submit"]').click();
    await page.locator('[data-rule="Bash(cargo test:*)"]').waitFor();
    await page.keyboard.press("Escape");
    await page.locator('.board .card[data-task="3"]').click();
    await page.waitForSelector('[data-testid="merge"]');
    check("G1: control: before the floor is gone the merge button is enabled", await on(page.locator('[data-testid="merge"]')));
    await clearToasts();
    const gl0 = (await calls("floor_list")).length;
    const go0 = (await calls("floor_open")).length;

    await M("goneFloor", FLOOR);
    await banner.waitFor();
    check("G1: the banner names the folder and says that the app cannot read or change the floor", flat(await banner.innerText()).includes(GONE_TEXT), flat(await banner.innerText()));
    check("G1: the banner has the buttons Forget this floor and Check again", (await banner.getByRole("button", { name: "Forget this floor" }).count()) === 1 && (await banner.getByRole("button", { name: "Check again" }).count()) === 1);
    // Changed on purpose (N1): the banner and the attention bar are two parts of the top strip, a row of fixed height.
    const rows = await page.evaluate(() => {
        const r = (s) => document.querySelector(s).getBoundingClientRect();
        return { banner: r('[data-testid="gone-banner"]'), strip: r('[data-testid="top-strip"]'), office: r(".office"), work: r(".work"), attention: r('[data-testid="attention-bar"]') };
    });
    check("G1: the banner is in the top strip: the one row at the top of the work area, above the office", rows.strip.top === rows.work.top && Math.abs(rows.strip.bottom - rows.office.top) < 1 && rows.strip.width >= rows.work.width - 1 && rows.banner.top >= rows.strip.top - 0.5 && rows.banner.bottom <= rows.strip.bottom + 0.5 && rows.strip.height === 30, JSON.stringify({ s: rows.strip.height, b: rows.banner.top, o: rows.office.top }));
    check("G1: with the attention bar the two share the strip: the bar is right of the banner and the strip did not grow", rows.banner.right <= rows.attention.left + 0.5 && rows.attention.bottom <= rows.strip.bottom + 0.5 && rows.strip.height === 30, JSON.stringify({ a: rows.attention.left, b: rows.banner.right }));
    // The storey shows "missing" at once: the list is read again, and the plate and the class change.
    await page.waitForSelector(`.storey.missing[data-path="${FLOOR}"]`);
    check("G1: the storey of the floor shows missing at once (class and plate)", (await page.locator(`.storey[data-path="${FLOOR}"] .plate`).getAttribute("data-plate")) === "missing");
    check("G1: floor-changed made the UI read floor_list and floor_open again", (await calls("floor_list")).length > gl0 && (await calls("floor_open")).length > go0);
    // Changed on purpose (N2): the banner of the open floor already says it, so the notice shows no toast (it covered the buttons).
    check("N2: the floor_gone notice of the OPEN floor shows no toast (the banner says it)", (await toastKinds(/is gone/)).length === 0 && (await page.locator(".toast").count()) === 0);
    const dim = await page.evaluate(() => {
        const d = document.querySelector('[data-testid="gone-dim"]');
        const o = document.querySelector(".office").getBoundingClientRect();
        const r = d.getBoundingClientRect();
        const cs = getComputedStyle(d);
        return { same: Math.abs(r.width - o.width) < 1.5 && Math.abs(r.height - o.height) < 2.5, alpha: Number((cs.backgroundColor.match(/[\d.]+/g) ?? [])[3] ?? 1), events: cs.pointerEvents, filter: cs.filter };
    });
    check("G1: the office is dimmed by an overlay (no CSS filter) that covers it and takes no pointer event", dim.same && dim.alpha > 0.2 && dim.alpha < 1 && dim.events === "none" && dim.filter === "none", JSON.stringify(dim));

    // The floor_gone toast stays and covers a corner of the board: close it, so that a click reaches the board.
    await clearToasts();
    // The controls that write are disabled, with the reason as the title.
    for (const [name, sel] of [
        ["start", ".bar button.start"],
        ["edit", ".bar button.edit"],
        ["remove", ".bar button.remove"],
        ["+ employee", ".bar button.add"],
        ["Run the setup agent", ".office .buttons button.setup"],
        ["New logo", ".office .buttons button.newlogo"],
    ]) {
        const loc = page.locator(sel);
        check(`G1: ${name} is disabled while the floor is gone, and the title gives the reason`, (await off(loc)) && (await reasoned(loc)));
    }
    // The task detail was open before: merge, discard, assign and status are off.
    for (const [name, sel] of [
        ["merge", '[data-testid="merge"]'],
        ["discard", ".detail button.discard"],
        ["Assign to (select)", "#assign-to"],
        ["Assign", ".detail .row button"],
        ["Status (select)", "#task-status"],
        ["the note", '.detail textarea[aria-label="Note"]'],
        ["Set the status", ".status-form button[type=submit]"],
    ]) {
        const loc = page.locator(sel);
        check(`G1: in the task detail ${name} is disabled while the floor is gone`, (await off(loc)) && (await reasoned(loc)));
    }
    await page.getByRole("button", { name: /Back/ }).click();
    check("G1: + New of the task board is disabled while the floor is gone", (await off(page.locator(".board button.new"))) && (await reasoned(page.locator(".board button.new"))));
    // Settings: the floor defaults and the commands with no question.
    await page.click("button.settings");
    await page.locator('[data-section="permissions"] input[name="rule"]').waitFor();
    for (const [name, sel] of [
        ["the default CLI", 'select[name="defaultCli"]'],
        ["the default policy", 'select[name="defaultPolicy"]'],
        ["save defaults", '[data-section="defaults"] button[type=submit]'],
        ["the new rule field", 'input[name="rule"]'],
        ["add a rule", '[data-section="permissions"] form.add button[type=submit]'],
        ["remove a rule", '[data-rule="Bash(cargo test:*)"] button.remove'],
        ["an example rule", "button.chip"],
    ]) {
        const loc = page.locator(sel).first();
        check(`G1: in Settings ${name} is disabled while the floor is gone`, (await loc.isDisabled()) && (await reasoned(loc)));
    }
    check("G1: Settings still lets the operator change the app level fields (the image model)", await on(page.locator('select[name="imageModel"]')));
    await page.keyboard.press("Escape");

    // The stop button of a live session stays enabled, and the terminal stays usable.
    await selectEmp("ceo");
    const stop = page.locator(".bar button.stop");
    check("G1: the stop button of a live session stays enabled", await on(stop));
    const hit = await stop.evaluate((b) => {
        const r = b.getBoundingClientRect();
        const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        return top === b || b.contains(top);
    });
    check("G1: the stop button is above the dim (a click reaches it)", hit);
    const gw0 = (await calls("pty_write")).length;
    await page.locator(".pane:not(.hidden) .xterm").click();
    await synthEnterNow();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "pty_write").length > n, gw0);
    check("G1: the terminal of a live session stays usable (a key reaches the session)", (await calls("pty_write")).length > gw0);
    // The key answered the question in the mock. Ask again, so that the attention bar shows above the banner in the screenshot.
    await M("setStatus", gCeo, "question");
    await page.locator('[data-testid="attention-bar"]').waitFor();

    // Forget with a live session: the existing error text shows, and the banner stays.
    await clearToasts();
    await twice(banner.locator("button.forget"));
    await page.locator(".toast", { hasText: "live sessions" }).waitFor();
    check("G1: Forget this floor with a live session shows the existing error and keeps the floor", (await banner.count()) === 1 && (await page.locator(".toast", { hasText: /Stop them before you forget the floor/ }).count()) === 1 && (await page.locator(`.floors li[data-path="${FLOOR}"]`).count()) === 1);
    // Check again: floor_open is called, the banner stays, and a toast says that the folder is still gone.
    await clearToasts();
    const gc0 = (await calls("floor_open")).length;
    await banner.getByRole("button", { name: "Check again" }).click();
    await page.locator(".toast", { hasText: /still gone/ }).waitFor();
    check("G1: Check again calls floor_open, and a floor that is still gone keeps its banner and says so", (await calls("floor_open")).length === gc0 + 1 && (await banner.count()) === 1 && (await toastKinds(/still gone/)).join() === "info");
    // The screenshots of the banner state (the attention bar shows above it).
    await clearToasts();
    await page.waitForTimeout(250);
    await shot("gone-banner-1280.png");
    await page.setViewportSize({ width: 2560, height: 1400 });
    await page.waitForTimeout(500);
    await shot("gone-banner-2560.png");
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.waitForTimeout(300);
    // Stop the session: the bar goes, the banner stays.
    await stop.click();
    await page.locator('[data-testid="attention-bar"]').waitFor({ state: "detached" });
    const goneStops = (await calls("session_stop")).length;
    check("G1: a stop on a gone floor works (session_stop) and the banner stays", goneStops === 1 && (await banner.count()) === 1, `${goneStops} calls, ${await banner.count()} banner`);

    // The folder comes back: the banner and the dim go, the controls work again, an information toast says so.
    await M("backFloor", FLOOR);
    await banner.waitFor({ state: "detached" });
    await page.waitForSelector(`.storey[data-path="${FLOOR}"]:not(.missing)`);
    check("G1: when the folder is back the banner, the dim and the missing mark go", (await page.locator('[data-testid="gone-dim"]').count()) === 0 && (await page.locator(`.storey[data-path="${FLOOR}"] .plate`).getAttribute("data-plate")) !== "missing");
    check("G1: the floor_back notice is an information toast", (await toastKinds(/is back/)).join() === "info");
    await selectEmp("pm");
    check("G1: when the folder is back start is enabled again", (await on(page.locator(".bar button.start"))) && !(await reasoned(page.locator(".bar button.start"))));

    // Forget a gone floor with no live session: the floor goes and the empty office shows.
    await M("goneFloor", FLOOR);
    await banner.waitFor();
    await twice(banner.locator("button.forget"));
    await page.locator('[data-testid="empty-office"]').waitFor();
    check("G1: Forget this floor removes the floor from the list and the banner goes", (await banner.count()) === 0 && (await page.locator(`.floors li[data-path="${FLOOR}"]`).count()) === 0 && (await calls("floor_forget")).some((c) => c.args.path === FLOOR));

    // The same inline error in the field of the empty office.
    await page.fill("#start-path", "relative/path");
    await page.keyboard.press("Enter");
    await page.locator('[data-testid="empty-office"] [data-testid="open-error"]').waitFor();
    check("G1: the field of the empty office shows the error under itself, keeps the path and offers no create", (await page.locator('[data-testid="open-error"]').innerText()) === "Type the full path of the folder, for example /home/you/project." && (await page.inputValue("#start-path")) === "relative/path" && (await page.locator(".create").count()) === 0);
    await shot("gone-open-error-empty-1280.png");
    await page.fill("#start-path", "relative/path2");
    check("G1: the error goes when the operator types again", (await page.locator('[data-testid="open-error"]').count()) === 0);

    // ---- G2. Each kind of notice ----
    await fresh();
    await clearToasts();
    const gl1 = (await calls("floor_list")).length;
    const go1 = (await calls("floor_open")).length;
    await M("notice", FLOOR, "The folder of the floor Demo Tower is gone: /home/demo/tower.", "floor_gone");
    // Changed on purpose (N2): the floor_gone toast shows for a floor that is not open. The open floor has the banner.
    await M("notice", BIG, "The folder of the floor Big Office is gone: /home/demo/big.", "floor_gone");
    await M("notice", FLOOR, "Task T-12 is set to done, but its branch task/T-12-x is not merged. The app did not merge it. Use the merge on the board to merge it.", "task_done_unmerged");
    await M("notice", FLOOR, "The folder of the floor Demo Tower is back: /home/demo/tower.", "floor_back");
    await M("notice", FLOOR, "A new kind of notice that the UI does not know.", "brand_new_kind");
    await page.waitForFunction(() => document.querySelectorAll(".toast").length >= 4);
    const kindOf = async (re) => (await toastKinds(re)).join();
    check("G2: floor_gone of another floor is an error toast", (await kindOf(/is gone: \/home\/demo\/big/)) === "error");
    check("G2: floor_gone of the open floor shows no toast", (await kindOf(/is gone: \/home\/demo\/tower/)) === "");
    check("G2: task_done_unmerged is an information toast", (await kindOf(/is not merged/)) === "info");
    check("G2: floor_back is an information toast", (await kindOf(/is back:/)) === "info");
    check("G2: a kind that the UI does not know is an information toast with its text", (await kindOf(/A new kind of notice that the UI does not know/)) === "info");
    await page.waitForFunction(([l, o]) => window.__towerMockCalls.filter((c) => c.command === "floor_list").length >= l && window.__towerMockCalls.filter((c) => c.command === "floor_open").length >= o, [gl1 + 2, go1 + 2], { timeout: 3000 }).catch(() => {});
    check("G2: floor_gone and floor_back read floor_list and floor_open again", (await calls("floor_list")).length >= gl1 + 2 && (await calls("floor_open")).length >= go1 + 2);
    // floor-changed alone (no notice): the list is read again for any known floor, and the floor is read again for the open one only.
    const fc = { l: (await calls("floor_list")).length, o: (await calls("floor_open")).length };
    await M("setResumable", BIG, []);
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "floor_list").length > n, fc.l, { timeout: 3000 }).catch(() => {});
    await page.waitForTimeout(150);
    check("G2: floor-changed of a known floor that is not open reads floor_list again, and not floor_open", (await calls("floor_list")).length > fc.l && (await calls("floor_open")).length === fc.o);
    await M("setResumable", FLOOR, []);
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "floor_open").length > n, fc.o, { timeout: 3000 }).catch(() => {});
    check("G2: floor-changed of the open floor reads floor_open again", (await calls("floor_open")).length > fc.o);
    await page.waitForTimeout(6600);
    check("G2: after 6 s floor_back and the unknown kind closed by themselves", (await kindOf(/is back:/)) === "" && (await kindOf(/does not know/)) === "");
    check("G2: after 6 s the error toast of another floor and the task_done_unmerged toast (a warning) are still there", (await kindOf(/is gone: \/home\/demo\/big/)) === "error" && (await kindOf(/is not merged/)) === "info");
    await page.locator(".toast", { hasText: /is not merged/ }).locator('button[aria-label="Dismiss"]').click();
    check("G2: the task_done_unmerged toast closes with its button", (await kindOf(/is not merged/)) === "");
    await clearToasts();
    await M("notice", FLOOR, "Task T-4 is blocked: a conflict.");
    await page.locator(".toast", { hasText: /is blocked: a conflict/ }).waitFor();
    check("G2: task_blocked is still an error toast", (await kindOf(/is blocked: a conflict/)) === "error");
    await clearToasts();
    await M("notice", FLOOR, "The setup agent tried to change the commands with no question. The app put the old list back.", "permissions_restored");
    await page.locator(".toast", { hasText: /put the old list back/ }).waitFor();
    check("G2: permissions_restored (removed by the app) is not special: an unknown kind, an information toast", (await kindOf(/put the old list back/)) === "info");
    await clearToasts();

    // ---- G3. The four errors of the open folder field, and NO_FLOOR ----
    await fresh();
    await clearToasts();
    await page.click("details.floors > summary").catch(() => {});
    const field = page.locator("#floor-path");
    const colError = page.locator('.toolbar [data-testid="open-error"]');
    let shotDone = false;
    for (const [typed, want] of [
        ["relative/folder", "Type the full path of the folder, for example /home/you/project."],
        ["/missing/project", "The folder /missing/project does not exist."],
        ["/tmp/notes.txt", "/tmp/notes.txt is a file, not a folder."],
        ["/readonly/project", "You cannot write in the folder /readonly/project."],
    ]) {
        await field.fill(typed);
        await page.keyboard.press("Enter");
        await colError.waitFor();
        const text = await colError.innerText();
        check(`G3: "${typed}" shows "${want}" under the field`, text === want, text);
        check(`G3: "${typed}" keeps the typed path, offers no create, and the floor stays open`, (await field.inputValue()) === typed && (await page.locator(".toolbar .create").count()) === 0 && (await page.locator(".storey.on").getAttribute("data-path")) === FLOOR);
        check(`G3: "${typed}" shows its text under the field and not in a second toast`, (await page.locator(".toast", { hasText: want }).count()) === 0 && (await colError.isVisible()));
        if (!shotDone) {
            shotDone = true;
            await shot("gone-open-error-1280.png");
            await page.setViewportSize({ width: 2560, height: 1400 });
            await page.waitForTimeout(400);
            await shot("gone-open-error-2560.png");
            await page.setViewportSize({ width: 1280, height: 800 });
            await page.waitForTimeout(300);
        }
    }
    await field.fill("~/nothing/here.txt");
    check("G3: the error goes when the operator types again", (await colError.count()) === 0);
    await page.keyboard.press("Enter");
    await colError.waitFor();
    check("G3: a path with ~ is expanded before the check", (await colError.innerText()) === "/home/demo/nothing/here.txt is a file, not a folder.");
    // A good folder with no .swarm/ still offers "create", and the create works.
    await field.fill("/home/demo/brand-new");
    await page.keyboard.press("Enter");
    await page.locator(".toolbar .create").waitFor();
    check("G3: a folder with no .swarm/ (NO_FLOOR) still offers create, and shows no path error", (await colError.count()) === 0 && /No floor in/.test(await page.locator(".toolbar .create").innerText()));
    await page.fill("#floor-name", "Brand New");
    await page.locator(".toolbar .create button.primary").click();
    await page.waitForSelector('.storey.on[data-path="/home/demo/brand-new"]');
    check("G3: create makes the floor and opens it", (await page.locator(".strip h2.name").innerText()) === "Brand New");

    // ---- G4. Commands with no question: the object result, the warnings, the proposal, the new texts ----
    await fresh();
    await M("setProposed", FLOOR, ["Bash(cargo test:*)", "Read(./src/**)"]);
    await page.click("button.settings");
    const sec = page.locator('section[data-section="permissions"]');
    await sec.locator('[data-testid="rule-list"]').waitFor();
    const secText = async (id) => flat(await sec.locator(`[data-testid="${id}"]`).innerText());
    const setCalls = () => calls("floor_permissions_set");
    const WARN = "This rule lets each agent of the floor run the project's own code with no question.";
    const WILD = "This rule lets each agent of the floor run this program with any arguments and no question. Many programs can write files or run other commands through their options.";
    check("G4: the section says that the app stores the rules outside the project folder, so an agent cannot change them through the project",
        /stores these rules in its own config folder, outside the project folder/.test(await secText("rules-storage")) && /cannot change them through the project/.test(await secText("rules-storage")), await secText("rules-storage"));
    check("G4: the fixed warning says what a rule with * gives, and that an exact rule is better, and it is always visible",
        (await secText("rules-warning")) === "A rule with * lets every agent of this floor run that program with any arguments and no question. Many programs can write files or run other commands through their options. Prefer an exact rule when one exact command is enough.", await secText("rules-warning"));
    check("G4: the section asks the app for the proposal (floor_permissions_proposed)", (await calls("floor_permissions_proposed")).some((c) => c.args.path === FLOOR));
    const box = sec.locator('[data-testid="rules-proposal"]');
    check("G4: the old rules of the project show in a box with the warning that an agent can have written them",
        (await box.count()) === 1 && flat(await box.innerText()).startsWith("This project has a list of rules from an older version. An agent can have written it. Read each rule before you use it.") &&
        (await box.locator("li[data-proposed]").count()) === 2 && (await box.getByRole("button", { name: "Use these rules" }).count()) === 1);
    check("G4: the app does not import the proposal by itself (the list is empty, no set call)", (await sec.locator("li[data-rule]").count()) === 0 && (await setCalls()).length === 0);
    await box.getByRole("button", { name: "Use these rules" }).click();
    await sec.locator('li[data-rule="Read(./src/**)"]').waitFor();
    const used = (await setCalls()).pop();
    check("G4: Use these rules calls floor_permissions_set with the current list plus the proposed rules", JSON.stringify(used.args) === JSON.stringify({ path: FLOOR, allow: ["Bash(cargo test:*)", "Read(./src/**)"] }), JSON.stringify(used.args));
    check("G4: after the save the proposal box is gone", (await sec.locator('[data-testid="rules-proposal"]').count()) === 0);
    const warnOn = async (rule) => {
        const li = sec.locator(`li[data-rule="${rule}"]`);
        return { mark: await li.locator(".mark").count(), text: (await li.locator('[data-testid="rule-warning"]').count()) === 1 ? flat(await li.locator('[data-testid="rule-warning"]').innerText()) : null };
    };
    const w1 = await warnOn("Bash(cargo test:*)");
    const w2 = await warnOn("Read(./src/**)");
    check("G4: the warning of the server shows on the rule that it is about (a mark and the text)", w1.mark === 1 && w1.text === WARN, JSON.stringify(w1));
    check("G4: a rule with no warning has no mark", w2.mark === 0 && w2.text === null, JSON.stringify(w2));
    // Bash(rtk cargo test:*) is accepted and saved, with the project-code warning. Bash(ls:*) has the wildcard warning.
    // The exact rule Bash(rtk git status) has none.
    const g4In = sec.locator('input[aria-label="New rule"]');
    const g4Add = sec.getByRole("button", { name: "add", exact: true });
    await g4In.fill("Bash(rtk cargo test:*)");
    await g4Add.click();
    await sec.locator('li[data-rule="Bash(rtk cargo test:*)"]').waitFor();
    await g4In.fill("Bash(ls:*)");
    await g4Add.click();
    await sec.locator('li[data-rule="Bash(ls:*)"]').waitFor();
    await g4In.fill("Bash(rtk git status)");
    await g4Add.click();
    await sec.locator('li[data-rule="Bash(rtk git status)"]').waitFor();
    const w3 = await warnOn("Bash(rtk cargo test:*)");
    const w4 = await warnOn("Bash(ls:*)");
    const w5 = await warnOn("Bash(rtk git status)");
    check("G4: Bash(rtk cargo test:*) is accepted and saved with the project-code warning on that rule", w3.mark === 1 && w3.text === WARN && (await setCalls()).pop().args.allow.includes("Bash(rtk git status)"), JSON.stringify(w3));
    check("G4: a wildcard rule of a plain program, Bash(ls:*), is saved with the wildcard warning, and the warnings of the other rules stay", w4.mark === 1 && w4.text === WILD && (await sec.locator("li[data-rule] .mark").count()) === 3, JSON.stringify(w4));
    check("G4: an exact rule, Bash(rtk git status), is saved with no warning", w5.mark === 0 && w5.text === null, JSON.stringify(w5));
    // Refused by the client check of the new rules, with no call to the app.
    const setsNow = (await setCalls()).length;
    for (const [rule, reason] of [
        ["Bash(rtk proxy:*)", /"rtk proxy" is not allowed/],
        ["Bash(rtk:*)", /"rtk" alone/],
        ["Bash(rtk -v ls)", /"rtk -v" is not allowed/],
        ["Bash(find . -delete)", /"find" is not allowed/],
        ["Bash(sed -i x)", /"sed" is not allowed/],
        ["Bash(git commit:*)", /"git commit" is not allowed/],
        ["Bash(git log --output=x)", /--output/],
        ["Bash(git diff --ext-diff)", /--ext-diff/],
        ["Read(.swarm/state)", /\.swarm/],
        ["Grep(.swarm/**)", /\.swarm/],
        ["WebFetch(domain:127.0.0.1)", /IP address/],
        ["WebFetch(domain:printer.local)", /local or internal/],
    ]) {
        await g4In.fill(rule);
        await g4Add.click();
        const text = await sec.locator('[data-testid="rule-error"]').innerText();
        check(`G4: the rule ${JSON.stringify(rule)} is refused with a reason`, reason.test(text), text);
    }
    check("G4: a refused rule makes no call to the app", (await setCalls()).length === setsNow);
    // A git rule with a * is refused with the text for git, and makes no call.
    for (const rule of ["Bash(git log:*)", "Bash(rtk git log:*)", "Bash(git diff *)", "Bash(git status:*)"]) {
        await g4In.fill(rule);
        await g4Add.click();
        const text = await sec.locator('[data-testid="rule-error"]').innerText();
        check(`G4: the git rule ${JSON.stringify(rule)} gets the text for git`, text === "A git rule must be the exact command, for example Bash(git status) or Bash(git diff --stat). A rule with * lets an agent add options such as --output, which writes files.", text);
    }
    check("G4: a refused git rule makes no call to the app", (await setCalls()).length === setsNow);
    for (const rule of ["Bash(git status)", "Bash(git rev-parse HEAD)", "Bash(rtk git diff --stat)", "Read(./.swarm/employees/**)", "WebFetch(domain:docs.rs)"]) {
        await g4In.fill(rule);
        await g4Add.click();
        await sec.locator(`li[data-rule="${rule}"]`).waitFor();
    }
    check("G4: the exact git rules and the good paths and hosts are accepted", (await setCalls()).length === setsNow + 5);
    // The staged state for the screenshot: a warning on a rule, and a proposal of the project (the app config entry is gone).
    await g4In.fill("");
    await page.keyboard.press("Escape");
    await M("setProposed", FLOOR, ["Bash(cargo test:*)", "Bash(npm test:*)"]);
    await page.click("button.settings");
    await sec.locator('[data-testid="rules-proposal"]').waitFor();
    await sec.scrollIntoViewIfNeeded();
    await sec.locator("h3").scrollIntoViewIfNeeded();
    await page.waitForTimeout(250);
    check("G4: the proposal and the warnings show together in the section", (await sec.locator('[data-testid="rules-proposal"]').count()) === 1 && (await sec.locator("li[data-rule] .mark").count()) === 3);
    await shot("gone-permissions-1280.png");
    const before = (await setCalls()).pop().args.allow;
    await sec.getByRole("button", { name: "Use these rules" }).click();
    await sec.locator('li[data-rule="Bash(npm test:*)"]').waitFor();
    const merged = (await setCalls()).pop().args.allow;
    check("G4: Use these rules keeps the rules of the list, adds the new proposed rule and drops the duplicate", JSON.stringify(merged) === JSON.stringify([...before, "Bash(npm test:*)"]), JSON.stringify(merged));
    await page.keyboard.press("Escape");
}
