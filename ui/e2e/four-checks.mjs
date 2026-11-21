// Checks of the four items of 2026-11-21 on the mock page: the "Browse..." button of the two
// "open folder" fields, the switch of the desktop notification in Settings, and the buttons "test" and
// "reset" of the program paths. `mock-page.mjs` calls `fourChecks` after `lastChecks`.
// The real dialog and the real notification need Tauri: the real app is checked on Xvfb (see the report).
import { join } from "node:path";

const FLOOR = "/home/demo/tower";
const BROWSE_TITLE = "The folder dialog needs the desktop app. A browser cannot open it. Type the path.";

export async function fourChecks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const calls = (command) => page.evaluate((c) => window.__towerMockCalls.filter((x) => x.command === c), command);
    const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
    const cssColor = (value) =>
        page.evaluate((v) => {
            const t = document.createElement("span");
            t.style.color = v;
            document.body.appendChild(t);
            const c = getComputedStyle(t).color;
            t.remove();
            return c;
        }, value);
    const openSettings = async () => {
        await page.click("button.settings");
        await dialog.waitFor();
    };
    const closeSettings = async () => {
        await page.keyboard.press("Escape");
        await dialog.waitFor({ state: "detached" });
    };
    const openFloorsList = async () => {
        if ((await page.locator("details.floors[open]").count()) === 0) await page.click("details.floors > summary");
    };
    async function fresh(hook = false) {
        await page.evaluate(() => localStorage.clear());
        await page.setViewportSize({ width: 1280, height: 800 });
        if (hook) await page.addInitScript(() => (window.__towerMockBrowse = () => "/home/demo/second"));
        await page.goto(url);
        await page.waitForSelector(".storey.on");
    }

    // ================= Item 3. The Browse button =================
    await fresh();
    const colBrowse = page.locator('form.open [data-testid="browse"]');
    check("browse: the left column has a Browse... button next to the path field and the open button",
        (await colBrowse.count()) === 1 && (await colBrowse.innerText()).trim() === "Browse..." &&
            (await page.locator("form.open .line > *").evaluateAll((els) => els.map((e) => e.tagName + (e.type ? ":" + e.type : "")).join(" "))) === "INPUT:text BUTTON:button BUTTON:submit");
    check("browse: in the mock the button is disabled, and its title says why",
        (await colBrowse.isDisabled()) && (await colBrowse.getAttribute("title")) === BROWSE_TITLE, await colBrowse.getAttribute("title"));
    const fits = await page.evaluate(() => {
        const form = document.querySelector("form.open");
        const input = form.querySelector("input");
        return { overflow: form.scrollWidth > form.clientWidth + 1, input: Math.round(input.getBoundingClientRect().width) };
    });
    check("browse: the three controls fit in the column and the path field stays usable", !fits.overflow && fits.input >= 70, JSON.stringify(fits));
    const before = (await calls("floor_open")).length;
    await colBrowse.click({ force: true }).catch(() => {});
    check("browse: a click on the disabled button calls nothing", (await calls("floor_open")).length === before);
    await shot("four-browse-column.png");

    // The first-start card.
    await openFloorsList();
    for (const path of ["/home/demo/second", "/home/demo/big", "/home/demo/gone", FLOOR]) {
        const forget = page.locator(`.floors li[data-path="${path}"] button:not(.floor-text)`);
        if ((await forget.count()) === 0) continue;
        await forget.click();
        await forget.click();
    }
    await page.locator('[data-testid="empty-office"]').waitFor();
    const cardBrowse = page.locator('[data-testid="empty-office"] [data-testid="browse"]');
    check("browse: the first-start card has the button too, disabled in the mock with the same title",
        (await cardBrowse.count()) === 1 && (await cardBrowse.isDisabled()) && (await cardBrowse.getAttribute("title")) === BROWSE_TITLE);
    check("browse: the card keeps its Open button, off while the path is empty",
        await page.locator('[data-testid="empty-office"] button.primary').isDisabled());
    await shot("four-browse-card.png");

    // With a dialog (the hook of the e2e check): the path goes in the field, and the floor does not open.
    await fresh(true);
    const hooked = page.locator('form.open [data-testid="browse"]');
    check("browse: with a dialog the button is on", await hooked.isEnabled());
    const opens = (await calls("floor_open")).length;
    await hooked.click();
    await page.waitForFunction(() => document.querySelector("#floor-path")?.value === "/home/demo/second");
    check("browse: the chosen path goes in the field of the left column", true);
    await page.waitForTimeout(200);
    check("browse: the floor is not opened by the button", (await calls("floor_open")).length === opens &&
        (await page.locator('.storey.on[data-path="/home/demo/tower"]').count()) === 1);
    await page.click('form.open button[type="submit"]');
    await page.waitForSelector('.storey.on[data-path="/home/demo/second"]');
    check("browse: the operator presses open and the floor opens", (await calls("floor_open")).length > opens);
    // The same in the card.
    await openFloorsList();
    for (const path of ["/home/demo/second", "/home/demo/big", "/home/demo/gone", FLOOR]) {
        const forget = page.locator(`.floors li[data-path="${path}"] button:not(.floor-text)`);
        if ((await forget.count()) === 0) continue;
        await forget.click();
        await forget.click();
    }
    await page.locator('[data-testid="empty-office"]').waitFor();
    await page.locator('[data-testid="empty-office"] [data-testid="browse"]').click();
    await page.waitForFunction(() => document.querySelector("#start-path")?.value === "/home/demo/second");
    check("browse: the chosen path goes in the field of the card, and the Open button turns on",
        await page.locator('[data-testid="empty-office"] button.primary').isEnabled());

    // ================= Item 2. The switch of the desktop notification =================
    await fresh();
    await openSettings();
    const iface = dialog.locator('section[data-section="interface"]');
    const sw = iface.locator('input[name="notifyOnQuestion"]');
    check("notify: the Interface section has the switch with its text, on by default",
        (await iface.innerText()).includes("Desktop notification when an agent asks a question") && (await sw.isChecked()));
    const setCalls = (await calls("settings_set")).length;
    await sw.uncheck();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "settings_set").length > n, setCalls);
    const last = (await calls("settings_set")).pop();
    check("notify: a click calls settings_set with notifyOnQuestion false and no other change",
        JSON.stringify(last.args) === JSON.stringify({ openaiKey: null, imageModel: null, notifyOnQuestion: false }), JSON.stringify(last.args));
    await closeSettings();
    await openSettings();
    check("notify: the switch is still off after the panel is closed and opened again",
        !(await dialog.locator('input[name="notifyOnQuestion"]').isChecked()));
    await shot("four-notify-off.png");
    await dialog.locator('input[name="notifyOnQuestion"]').check();
    await page.waitForFunction(() => window.__towerMockCalls.filter((c) => c.command === "settings_set").pop()?.args.notifyOnQuestion === true);
    check("notify: the switch goes on again", true);
    // The debug bar switch and the other settings stay as they were.
    check("notify: the other switch of the section is not changed",
        (await iface.locator('input[name="debugBar"]').count()) === 1);
    await closeSettings();

    // ================= Item 4. test and reset for the program paths =================
    await fresh();
    await openSettings();
    const programs = dialog.locator("section[data-section=programs]");
    const row = (name) => programs.locator(`[data-program="${name}"]`);
    check("programs: each of claude, codex and node has a test button",
        (await programs.locator("button.test").count()) === 3);
    check("programs: with no override set there is no reset button", (await programs.locator("button.reset").count()) === 0);

    await page.evaluate(() => (window.__towerMockConfig = { programTestMs: 500 }));
    await row("claude").locator("button.test").click();
    check("programs: while the test runs the button says so and is off",
        (await row("claude").locator("button.test").innerText()).trim() === "testing..." && (await row("claude").locator("button.test").isDisabled()));
    await programs.locator('[data-testid="test-claude"]').waitFor();
    await page.evaluate(() => delete window.__towerMockConfig);
    const good = programs.locator('[data-testid="test-claude"]');
    const goodColor = await good.evaluate((e) => getComputedStyle(e).color);
    check("programs: a good test shows a green mark and the version",
        (await good.getAttribute("data-ok")) === "true" && (await good.innerText()).includes("2.1.330 (Claude Code)") && (await good.innerText()).includes("✓"),
        await good.innerText());
    check("programs: the good result is green (the colour --ok)", goodColor === (await cssColor("var(--ok)")), goodColor);
    check("programs: the test calls program_test with the name only", JSON.stringify((await calls("program_test")).pop().args) === JSON.stringify({ name: "claude" }));

    await row("codex").locator("button.test").click();
    const bad = programs.locator('[data-testid="test-codex"]');
    await bad.waitFor();
    check("programs: a program that is not found shows a red mark and the error text",
        (await bad.getAttribute("data-ok")) === "false" && (await bad.innerText()).includes("not found") && (await bad.innerText()).includes("✗"), await bad.innerText());
    check("programs: the error result is red (the colour --danger)", (await bad.evaluate((e) => getComputedStyle(e).color)) === (await cssColor("var(--danger)")));

    // An override that fails: set, test, reset.
    await row("claude").locator("input").fill("/opt/tools/broken-claude");
    await row("claude").locator('button[type="submit"]').click();
    await row("claude").locator("button.reset").waitFor();
    check("programs: a set path shows the reset button and the path of the operator",
        (await row("claude").locator("button.reset").count()) === 1 && (await row("claude").locator('[data-testid="override-claude"]').innerText()).includes("/opt/tools/broken-claude") &&
            (await programs.locator("button.reset").count()) === 1);
    check("programs: a change of the path clears the old test result", (await programs.locator('[data-testid="test-claude"]').count()) === 0);
    await row("claude").locator("button.test").click();
    const broken = programs.locator('[data-testid="test-claude"]');
    await broken.waitFor();
    check("programs: the test of the broken program shows the error with a red mark",
        (await broken.getAttribute("data-ok")) === "false" && (await broken.innerText()).includes("exit code 2"), await broken.innerText());
    await shot("four-programs-bad.png");
    const resetFrom = (await calls("programs_set")).length;
    await row("claude").locator("button.reset").click();
    await page.waitForFunction(() => !document.querySelector('[data-program="claude"] button.reset'));
    const resetCall = (await calls("programs_set")).slice(resetFrom).pop();
    check("programs: reset calls programs_set with an empty path", JSON.stringify(resetCall.args) === JSON.stringify({ name: "claude", path: "" }), JSON.stringify(resetCall.args));
    check("programs: after the reset the row shows the path that the app finds, and no override note",
        (await row("claude").locator(".value").innerText()).trim() === "/usr/local/bin/claude" && (await programs.locator('[data-testid="override-claude"]').count()) === 0);
    check("programs: the reset clears the old result", (await programs.locator('[data-testid="test-claude"]').count()) === 0);
    await row("claude").locator("button.test").click();
    await programs.locator('[data-testid="test-claude"][data-ok="true"]').waitFor();
    check("programs: the test after the reset is good again", true);
    await shot("four-programs-good.png");
    await closeSettings();
}
