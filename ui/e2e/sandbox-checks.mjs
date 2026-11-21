// Checks of the section "Codex sandbox" in Settings, on the mock page (decision O7, option A):
// the probe result in each state, the button "Test again", the switch (off by default, disabled
// with its reason when the probe does not say "works", on and off), and the refusal of the app.
// `mock-page.mjs` calls `sandboxChecks` after `toastChecks`. The mock takes the probe result from
// `window.__towerMockConfig.sandboxProbe`. The real probe needs `bwrap` and the desktop app.
import { join } from "node:path";

const HELP = "On Ubuntu 24.04 and later, AppArmor blocks bwrap by default. Add an AppArmor profile that lets /usr/bin/bwrap use user namespaces, then press Test again.";
const NETWORK = "With the sandbox on, the commands of a Codex employee have no network access (for example a package install fails).";
const DOWN = "The setting is on, but the sandbox does not work now. Codex employees cannot start until you repair the sandbox or turn the setting off.";
const UID_ERROR = "bwrap: setting up uid map: Permission denied";

export async function sandboxChecks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const calls = (command) => page.evaluate((c) => window.__towerMockCalls.filter((x) => x.command === c), command);
    const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
    const section = dialog.locator('section[data-section="codex-sandbox"]');
    const state = section.locator('[data-testid="sandbox-state"]');
    const sw = section.locator('input[name="codexSandbox"]');
    const again = section.locator('button[name="sandboxTest"]');
    const config = (value) => page.evaluate((v) => (window.__towerMockConfig = v), value);
    const openSettings = async () => {
        await page.click("button.settings");
        await dialog.waitFor();
    };
    const closeSettings = async () => {
        await page.keyboard.press("Escape");
        await dialog.waitFor({ state: "detached" });
    };
    // A new page: the mock starts again (the switch is off). `probe` is the config of the mock.
    async function fresh(probe) {
        await page.evaluate(() => localStorage.clear());
        await page.setViewportSize({ width: 1280, height: 800 });
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        await config({ sandboxProbe: probe });
    }
    const settled = () => page.waitForFunction(() => ["works", "fails", "missing"].includes(document.querySelector('[data-testid="sandbox-state"]')?.dataset.state));
    const text = async () => (await section.innerText()).replace(/\s+/g, " ");

    // ================= The probe says works =================
    await fresh({ state: "works" });
    await openSettings();
    await settled();
    check("sandbox: the section is in the panel before Interface, with the title Codex sandbox",
        JSON.stringify(await page.$$eval('[role="dialog"] section', (s) => s.map((x) => x.dataset.section).slice(-2))) ===
            JSON.stringify(["codex-sandbox", "interface"]) && (await section.locator("h3").textContent()) === "Codex sandbox");
    check("sandbox: the panel ran the probe when it opened", (await calls("codex_sandbox_probe")).length === 1);
    check("sandbox: works shows 'The Codex sandbox works on this machine.' and no help line",
        (await state.innerText()).includes("The Codex sandbox works on this machine.") &&
            (await state.getAttribute("data-state")) === "works" &&
            (await section.locator('[data-testid="sandbox-help"]').count()) === 0);
    check("sandbox: the switch has its text, it is off by default and it is enabled",
        (await text()).includes("Use the Codex sandbox for Codex employees") && !(await sw.isChecked()) && (await sw.isEnabled()));
    check("sandbox: no reason line shows while the switch is enabled", (await section.locator('[data-testid="sandbox-reason"]').count()) === 0);
    check("sandbox: state (a) off: the section is in the mode off, with no red line, and the network line shows",
        (await section.getAttribute("data-mode")) === "off" && (await section.locator('[data-testid="sandbox-down"]').count()) === 0 &&
            (await section.locator('[data-testid="sandbox-network"]').innerText()) === NETWORK);
    check("sandbox: the old help line no longer says the commands have no network (the new line says it)", !(await text()).includes("The commands have no network access."));
    const note = await text();
    check("sandbox: a line says what the switch changes, and that it applies at the next start of a session",
        note.includes("the Codex CLI enforces the policy of each Codex employee") &&
            note.includes("reports folder") &&
            note.includes("only the role text limits a Codex employee") &&
            note.includes("The change applies at the next start of a session."), note);
    await shot("sandbox-works-off.png");

    const before = (await calls("settings_set")).length;
    await sw.check();
    await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "settings_set").length > n, before);
    const on = (await calls("settings_set")).pop();
    check("sandbox: a click calls settings_set with codexSandbox true and no other change",
        JSON.stringify(on.args) === JSON.stringify({ openaiKey: null, imageModel: null, notifyOnQuestion: null, codexSandbox: true }), JSON.stringify(on.args));
    check("sandbox: the switch is on", await sw.isChecked());
    await closeSettings();
    await openSettings();
    await settled();
    check("sandbox: the switch is still on after the panel is closed and opened again", await sw.isChecked());
    check("sandbox: state (b) on and the probe works: the mode is on-works, with no red line",
        (await section.getAttribute("data-mode")) === "on-works" && (await section.locator('[data-testid="sandbox-down"]').count()) === 0);
    await shot("sandbox-works-on.png");
    await sw.uncheck();
    await page.waitForFunction(() => window.__towerMockCalls.filter((c) => c.command === "settings_set").pop()?.args.codexSandbox === false);
    check("sandbox: a click on the switch turns it off (codexSandbox false)", !(await sw.isChecked()));
    // The notification switch has its old call shape: no `codexSandbox` key.
    await closeSettings();

    // ================= The probe fails =================
    await fresh({ state: "fails", detail: UID_ERROR });
    await openSettings();
    await settled();
    check("sandbox: fails shows the first line of the error", (await state.innerText()).includes(UID_ERROR) && (await state.getAttribute("data-state")) === "fails");
    check("sandbox: fails shows the line about the AppArmor profile",
        (await section.locator('[data-testid="sandbox-help"]').innerText()) === HELP);
    check("sandbox: the network line shows also when the probe fails, and there is no red line while the setting is off",
        (await section.locator('[data-testid="sandbox-network"]').innerText()) === NETWORK && (await section.locator('[data-testid="sandbox-down"]').count()) === 0 &&
            (await section.getAttribute("data-mode")) === "off");
    check("sandbox: the help line has no file reference", !(await section.locator('[data-testid="sandbox-help"]').innerText()).includes("docs/spec.md"));
    check("sandbox: the switch is disabled and off, with its reason",
        !(await sw.isEnabled()) && !(await sw.isChecked()) &&
            (await section.locator('[data-testid="sandbox-reason"]').innerText()) === "The switch stays off because the test did not pass.");
    const calls0 = (await calls("settings_set")).length;
    await sw.click({ force: true }).catch(() => {});
    await page.waitForTimeout(150);
    check("sandbox: a click on the disabled switch calls nothing", (await calls("settings_set")).length === calls0 && !(await sw.isChecked()));
    await shot("sandbox-fails.png");

    // "Test again": the state changes when the machine changes.
    await config({ sandboxProbe: { state: "works" }, sandboxProbeMs: 500 });
    const probes = (await calls("codex_sandbox_probe")).length;
    await again.click();
    check("sandbox: Test again shows 'Testing the sandbox...' and disables the button and the switch",
        (await state.innerText()).includes("Testing the sandbox...") && !(await again.isEnabled()) && !(await sw.isEnabled()));
    await settled();
    check("sandbox: Test again called the probe once more, and the state is works",
        (await calls("codex_sandbox_probe")).length === probes + 1 && (await state.innerText()).includes("works on this machine"));
    check("sandbox: the help line and the reason are gone, and the switch is enabled",
        (await section.locator('[data-testid="sandbox-help"]').count()) === 0 &&
            (await section.locator('[data-testid="sandbox-reason"]').count()) === 0 && (await sw.isEnabled()));
    await closeSettings();

    // ================= The probe says missing =================
    await fresh({ state: "missing" });
    await openSettings();
    await settled();
    check("sandbox: missing shows the text of the probe and the line about bubblewrap",
        (await state.innerText()).includes("bwrap was not found in the PATH of a session") &&
            (await section.locator('[data-testid="sandbox-help"]').innerText()).startsWith("Install bubblewrap") && !(await sw.isEnabled()));
    await closeSettings();

    // ================= The app refuses the switch =================
    // The panel saw `works`. Then the machine changed. The switch is enabled, and the app refuses it.
    await fresh({ state: "works" });
    await openSettings();
    await settled();
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR } });
    await sw.check();
    const refusal = section.locator('[data-testid="action-error-codex-sandbox"]');
    await refusal.waitFor();
    check("sandbox: a refusal of the app shows as a line in the section, with the text of the problem",
        (await refusal.innerText()).includes("does not work on this machine") && (await refusal.innerText()).includes(UID_ERROR), await refusal.innerText());
    await settled();
    check("sandbox: after a refusal the switch is off, and the state shows the new result",
        !(await sw.isChecked()) && (await state.getAttribute("data-state")) === "fails" && !(await sw.isEnabled()));
    check("sandbox: the refusal text stays after the new test", (await refusal.count()) === 1);
    await shot("sandbox-refused.png");
    await closeSettings();

    // ================= The switch is on, and the machine changes (state c) =================
    await fresh({ state: "works" });
    await openSettings();
    await settled();
    await sw.check();
    await page.waitForFunction(() => window.__towerMockCalls.filter((c) => c.command === "settings_set").pop()?.args.codexSandbox === true);
    await closeSettings();
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR } });
    const setCalls = (await calls("settings_set")).length;
    await openSettings();
    await settled();
    const down = section.locator('[data-testid="sandbox-down"]');
    check("sandbox: state (c) on and the probe fails: a red line says the sandbox does not work and what to do",
        (await down.count()) === 1 && (await down.innerText()).replace(/\s+/g, " ") === DOWN && (await section.getAttribute("data-mode")) === "on-down", await section.innerText());
    check("sandbox: the red line is red (the danger colour) and the stored value did not change by itself",
        (await down.evaluate((el) => getComputedStyle(el).color)) !== (await section.locator('[data-testid="sandbox-help"]').evaluate((el) => getComputedStyle(el).color)) &&
            (await calls("settings_set")).length === setCalls);
    check("sandbox: a switch that is on can be turned off while the test fails",
        (await sw.isChecked()) && (await sw.isEnabled()) && (await state.getAttribute("data-state")) === "fails");
    check("sandbox: state (c) also shows the help line for the probe and the network line",
        (await section.locator('[data-testid="sandbox-help"]').innerText()) === HELP && (await section.locator('[data-testid="sandbox-network"]').count()) === 1);
    await shot("sandbox-on-down.png");
    // The switch stays enabled while a new test runs, so the operator can turn it off at any time.
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR }, sandboxProbeMs: 400 });
    await again.click();
    check("sandbox: the switch that is on stays enabled during a test, and the red line is not shown while the test runs",
        (await sw.isEnabled()) && (await down.count()) === 0 && (await section.getAttribute("data-mode")) === "on-testing");
    await settled();
    check("sandbox: the red line is back after the test", (await down.count()) === 1);
    await sw.uncheck();
    await page.waitForFunction(() => window.__towerMockCalls.filter((c) => c.command === "settings_set").pop()?.args.codexSandbox === false);
    check("sandbox: it is off after that, the red line is gone, and the switch is disabled again",
        !(await sw.isChecked()) && !(await sw.isEnabled()) && (await down.count()) === 0 && (await section.getAttribute("data-mode")) === "off");
    await closeSettings();

    // ================= The refused start of a Codex employee: the toast and the button =================
    await fresh({ state: "works" });
    await page.evaluate(() => window.__towerMock.patchFloor("/home/demo/tower", { employee: { slug: "eng-lin", patch: { cli: "codex" } } }));
    await openSettings();
    await settled();
    await sw.check();
    await page.waitForFunction(() => window.__towerMockCalls.filter((c) => c.command === "settings_set").pop()?.args.codexSandbox === true);
    await closeSettings();
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR } });
    await page.locator('.emp[data-slug="eng-lin"] .hit').click();
    await page.locator(".bar button.start").click();
    const toast = page.locator(".toast").filter({ hasText: "does not work on this machine now" });
    await toast.waitFor();
    const toastText = await toast.innerText();
    check("sandbox: a refused start shows an error toast with the text, with no code prefix",
        toastText.includes("The Codex sandbox does not work on this machine now (" + UID_ERROR + ")") &&
            toastText.includes('The start of Lin is refused, because the setting "Use the Codex sandbox" is on.') && !toastText.includes("SANDBOX_DOWN"), toastText);
    check("sandbox: the toast has the button Open Settings", (await toast.getByRole("button", { name: "Open Settings" }).count()) === 1);
    await page.waitForTimeout(1500);
    check("sandbox: the error toast stays", (await toast.count()) === 1);
    check("sandbox: the refusal did not change the stored value (no settings_set after the switch on)",
        (await calls("settings_set")).pop().args.codexSandbox === true);
    await toast.getByRole("button", { name: "Open Settings" }).click();
    await dialog.waitFor();
    await settled();
    await page.waitForTimeout(200);
    const inView = await page.evaluate(() => {
        const d = document.querySelector('[role="dialog"]').getBoundingClientRect();
        const t = document.querySelector('[data-section="codex-sandbox"]').getBoundingClientRect();
        return t.top >= d.top - 1 && t.top < d.bottom;
    });
    check("sandbox: the button opens Settings at the section Codex sandbox, where the red line shows",
        inView && (await dialog.getAttribute("data-wanted")) === "codex-sandbox" && (await down.count()) === 1 && (await sw.isChecked()) && (await sw.isEnabled()));
    await shot("sandbox-toast-open.png");
    await closeSettings();

    // The notice of a start by an employee (kind sandbox_down) and the notice of the team resume are error toasts that stay too.
    await page.evaluate(() => window.__towerMock.notice("/home/demo/tower", 'SANDBOX_DOWN: The Codex sandbox does not work on this machine now (x). The start of Sam is refused, because the setting "Use the Codex sandbox" is on. Repair the sandbox, or turn the setting off in Settings > Codex sandbox to start Codex with no sandbox.', "sandbox_down"));
    const noticeToast = page.locator(".toast").filter({ hasText: "The start of Sam is refused" });
    await noticeToast.waitFor();
    check("sandbox: a sandbox_down notice is an error toast with the button Open Settings",
        (await noticeToast.getByRole("button", { name: "Open Settings" }).count()) === 1 && !(await noticeToast.innerText()).includes("SANDBOX_DOWN") &&
            (await noticeToast.getAttribute("data-kind")) !== "info", await noticeToast.innerText());
    await page.waitForTimeout(1500);
    check("sandbox: the notice toast stays", (await noticeToast.count()) === 1);
    await noticeToast.getByRole("button", { name: "Open Settings" }).click();
    await dialog.waitFor();
    check("sandbox: the button of the notice opens Settings at the section Codex sandbox",
        (await dialog.getAttribute("data-wanted")) === "codex-sandbox" && await page.evaluate(() => {
            const d = document.querySelector('[role="dialog"]').getBoundingClientRect();
            const t = document.querySelector('[data-section="codex-sandbox"]').getBoundingClientRect();
            return t.top >= d.top - 1 && t.top < d.bottom;
        }));
    await closeSettings();
}
