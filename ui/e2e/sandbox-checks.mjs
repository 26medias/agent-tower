// Checks of the section "Codex sandbox" in Settings on the mock page (decision O7, option A, and the decision of
// 2026-11-21: the default is "Automatic"). The control has three choices: Automatic, Always, Never. A line says what
// is in effect now. The checks cover the probe in each state, the button "Test again", the three choices, the refusal of
// the app, the toast of a refused start ("Always") and the information toast of "Automatic" with a sandbox that does
// not work. `mock-page.mjs` calls `sandboxChecks` after `toastChecks`. The mock takes the probe result from
// `window.__towerMockConfig.sandboxProbe`. The real probe needs `bwrap` and the desktop app.
import { join } from "node:path";

const HELP = "On Ubuntu 24.04 and later, AppArmor blocks bwrap by default. Add an AppArmor profile that lets /usr/bin/bwrap use user namespaces, then press Test again.";
const NETWORK = "With the sandbox on, the commands of a Codex employee have no network access (for example a package install fails).";
const DOWN = 'The setting is "Always", but the sandbox does not work now. Codex employees cannot start until you repair the sandbox or choose "Automatic" or "Never".';
const UID_ERROR = "bwrap: setting up uid map: Permission denied";
const AUTO = "Automatic (recommended): use the sandbox when it works on this machine";
const ALWAYS = "Always: refuse to start a Codex employee when the sandbox does not work";
const NEVER = "Never: Codex employees have full access";
const ON_NOW = "In effect now: the sandbox is on.";
const OFF_NEVER = "In effect now: the sandbox is off, because you chose Never.";
const OFF_DOWN = `In effect now: the sandbox is off, because it does not work on this machine (${UID_ERROR}).`;
const REFUSED = `In effect now: Codex employees cannot start, because the sandbox does not work on this machine (${UID_ERROR}).`;
const AUTO_NOTICE = "The Codex sandbox does not work on this machine, so Codex employees run with full access. See Settings > Codex sandbox.";
const REFUSAL_START =
    `Lin did not start: the Codex sandbox does not work on this machine now (${UID_ERROR}). The setting "Codex sandbox" is "Always". Repair the sandbox, or choose "Automatic" or "Never" in Settings > Codex sandbox.`;

export async function sandboxChecks({ page, check, url, shots }) {
    const shot = (name) => page.screenshot({ path: join(shots, name) });
    const calls = (command) => page.evaluate((c) => window.__towerMockCalls.filter((x) => x.command === c), command);
    const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
    const section = dialog.locator('section[data-section="codex-sandbox"]');
    const state = section.locator('[data-testid="sandbox-state"]');
    const radio = (value) => section.locator(`input[name="codexSandboxMode"][data-choice="${value}"]`);
    const effective = section.locator('[data-testid="sandbox-effective"]');
    const again = section.locator('button[name="sandboxTest"]');
    const down = section.locator('[data-testid="sandbox-down"]');
    const reason = section.locator('[data-testid="sandbox-reason"]');
    const help = section.locator('[data-testid="sandbox-help"]');
    const config = (value) => page.evaluate((v) => (window.__towerMockConfig = v), value);
    const openSettings = async () => {
        await page.click("button.settings");
        await dialog.waitFor();
    };
    const closeSettings = async () => {
        await page.keyboard.press("Escape");
        await dialog.waitFor({ state: "detached" });
    };
    // A new page: the mock starts again (the mode is Automatic). `probe` is the config of the mock.
    async function fresh(probe) {
        await page.evaluate(() => localStorage.clear());
        await page.setViewportSize({ width: 1280, height: 800 });
        await page.goto(url);
        await page.waitForSelector(".storey.on");
        await config({ sandboxProbe: probe });
    }
    // The test ended, and the line "In effect now" shows its state (on, off or refused).
    const settled = (expected) =>
        page.waitForFunction(
            (want) =>
                ["works", "fails", "missing"].includes(document.querySelector('[data-testid="sandbox-state"]')?.dataset.state) &&
                (want === undefined || document.querySelector('[data-testid="sandbox-effective"]')?.dataset.effective === want),
            expected,
        );
    const text = async () => (await section.innerText()).replace(/\s+/g, " ");
    const lastSet = async () => (await calls("settings_set")).pop();
    const choose = async (value, expectedCalls) => {
        await radio(value).check();
        await page.waitForFunction((n) => window.__towerMockCalls.filter((c) => c.command === "settings_set").length > n, expectedCalls);
    };
    const toastOf = (needle) => page.locator(".toast").filter({ hasText: needle });
    const inView = () =>
        page.evaluate(() => {
            const d = document.querySelector('[role="dialog"]').getBoundingClientRect();
            const t = document.querySelector('[data-section="codex-sandbox"]').getBoundingClientRect();
            return t.top >= d.top - 1 && t.top < d.bottom;
        });
    const makeCodex = (slug) =>
        page.evaluate((s) => window.__towerMock.patchFloor("/home/demo/tower", { employee: { slug: s, patch: { cli: "codex" } } }), slug);

    // ================= The probe says works (the default is Automatic) =================
    await fresh({ state: "works" });
    await openSettings();
    await settled("on");
    check("sandbox: the section is in the panel before Interface, with the title Codex sandbox",
        JSON.stringify(await page.$$eval('[role="dialog"] section', (s) => s.map((x) => x.dataset.section).slice(-2))) ===
            JSON.stringify(["codex-sandbox", "interface"]) && (await section.locator("h3").textContent()) === "Codex sandbox");
    check("sandbox: the panel ran the probe when it opened", (await calls("codex_sandbox_probe")).length === 1);
    check("sandbox: works shows 'The Codex sandbox works on this machine.' and no help line",
        (await state.innerText()).includes("The Codex sandbox works on this machine.") &&
            (await state.getAttribute("data-state")) === "works" && (await help.count()) === 0);
    const labels = await section.locator('label.check span').allInnerTexts();
    check("sandbox: one control with three choices, in plain words, in this order",
        JSON.stringify(labels) === JSON.stringify([AUTO, ALWAYS, NEVER]), JSON.stringify(labels));
    check("sandbox: the three choices are radio buttons of one group, with the values auto, on and off",
        (await section.locator('input[type="radio"][name="codexSandboxMode"]').count()) === 3 &&
            (await section.locator('input[type="checkbox"][name="codexSandbox"]').count()) === 0);
    check("sandbox: Automatic is chosen by default and the three choices are enabled",
        (await radio("auto").isChecked()) && !(await radio("on").isChecked()) && !(await radio("off").isChecked()) &&
            (await radio("auto").isEnabled()) && (await radio("on").isEnabled()) && (await radio("off").isEnabled()));
    check("sandbox: a line under the control says 'In effect now: the sandbox is on.'",
        (await effective.innerText()) === ON_NOW && (await effective.getAttribute("data-effective")) === "on", await effective.innerText());
    check("sandbox: no reason line shows while Always is enabled", (await reason.count()) === 0);
    check("sandbox: Automatic with a working sandbox: the section has the mode auto, no red line, and the network line shows",
        (await section.getAttribute("data-mode")) === "auto" && (await section.getAttribute("data-effective")) === "on" && (await down.count()) === 0 &&
            (await section.locator('[data-testid="sandbox-network"]').innerText()) === NETWORK);
    check("sandbox: the old help line no longer says the commands have no network (the new line says it)", !(await text()).includes("The commands have no network access."));
    const note = await text();
    check("sandbox: a line says what the sandbox changes, and that it applies at the next start of a session",
        note.includes("the Codex CLI enforces the policy of each Codex employee") &&
            note.includes("reports folder") &&
            note.includes("only the role text limits a Codex employee") &&
            note.includes("The change applies at the next start of a session."), note);
    check("sandbox: the panel read settings_get again after the probe (the line comes from the app)", (await calls("settings_get")).length >= 2);
    await shot("sandbox-auto-works.png");

    let before = (await calls("settings_set")).length;
    await choose("on", before);
    const on = await lastSet();
    check("sandbox: a click on Always calls settings_set with codexSandboxMode on and no other change",
        JSON.stringify(on.args) === JSON.stringify({ openaiKey: null, imageModel: null, notifyOnQuestion: null, codexSandboxMode: "on" }), JSON.stringify(on.args));
    check("sandbox: Always is chosen, and the line says that the sandbox is on", (await radio("on").isChecked()) && (await effective.innerText()) === ON_NOW);
    await closeSettings();
    await openSettings();
    await settled("on");
    check("sandbox: Always is still chosen after the panel is closed and opened again", await radio("on").isChecked());
    check("sandbox: Always with a working sandbox: the mode is on, with no red line",
        (await section.getAttribute("data-mode")) === "on" && (await down.count()) === 0);
    await shot("sandbox-always-works.png");

    before = (await calls("settings_set")).length;
    await choose("off", before);
    check("sandbox: a click on Never calls settings_set with codexSandboxMode off",
        (await lastSet()).args.codexSandboxMode === "off" && (await radio("off").isChecked()));
    check("sandbox: Never: the line says that the sandbox is off because of the choice, also when the probe works",
        (await effective.innerText()) === OFF_NEVER && (await effective.getAttribute("data-effective")) === "off" && (await state.getAttribute("data-state")) === "works",
        await effective.innerText());
    await shot("sandbox-never.png");
    before = (await calls("settings_set")).length;
    await choose("auto", before);
    check("sandbox: a click on Automatic calls settings_set with codexSandboxMode auto, and the line says on",
        (await lastSet()).args.codexSandboxMode === "auto" && (await radio("auto").isChecked()) && (await effective.innerText()) === ON_NOW);
    // The old call shape stays for the other switches: no codexSandbox and no codexSandboxMode key.
    check("sandbox: the old boolean codexSandbox is not sent by the panel", (await calls("settings_set")).every((c) => !("codexSandbox" in c.args)));
    await closeSettings();

    // ================= The probe fails (the default is Automatic) =================
    await fresh({ state: "fails", detail: UID_ERROR });
    await openSettings();
    await settled("off");
    check("sandbox: fails shows the first line of the error", (await state.innerText()).includes(UID_ERROR) && (await state.getAttribute("data-state")) === "fails");
    check("sandbox: fails shows the line about the AppArmor profile", (await help.innerText()) === HELP);
    check("sandbox: Automatic with a failing sandbox: the line says that it is off because it does not work, with the probe error",
        (await effective.innerText()) === OFF_DOWN && (await effective.getAttribute("data-effective")) === "off", await effective.innerText());
    check("sandbox: Automatic with a failing sandbox shows no red line, and the network line shows",
        (await down.count()) === 0 && (await section.getAttribute("data-effective")) === "off" && (await section.locator('[data-testid="sandbox-network"]').innerText()) === NETWORK);
    check("sandbox: the help line has no file reference", !(await help.innerText()).includes("docs/spec.md"));
    check("sandbox: Always is disabled with its reason, and Automatic (chosen) and Never stay enabled",
        !(await radio("on").isEnabled()) && (await radio("auto").isChecked()) && (await radio("auto").isEnabled()) && (await radio("off").isEnabled()) &&
            (await reason.innerText()) === '"Always" is not possible because the test did not pass.', await reason.innerText().catch(() => "no reason"));
    const calls0 = (await calls("settings_set")).length;
    await radio("on").click({ force: true }).catch(() => {});
    await page.waitForTimeout(150);
    check("sandbox: a click on the disabled Always calls nothing", (await calls("settings_set")).length === calls0 && !(await radio("on").isChecked()));
    await shot("sandbox-auto-fails.png");

    // "Test again": the state changes when the machine changes.
    await config({ sandboxProbe: { state: "works" }, sandboxProbeMs: 500 });
    const probes = (await calls("codex_sandbox_probe")).length;
    await again.click();
    check("sandbox: Test again shows 'Testing the sandbox...' and 'In effect now: testing', and disables the button and Always",
        (await state.innerText()).includes("Testing the sandbox...") && (await effective.innerText()) === "In effect now: testing the sandbox..." &&
            !(await again.isEnabled()) && !(await radio("on").isEnabled()));
    await settled("on");
    check("sandbox: Test again called the probe once more, the state is works, and the line says on",
        (await calls("codex_sandbox_probe")).length === probes + 1 && (await state.innerText()).includes("works on this machine") && (await effective.innerText()) === ON_NOW);
    check("sandbox: the help line and the reason are gone, and Always is enabled",
        (await help.count()) === 0 && (await reason.count()) === 0 && (await radio("on").isEnabled()));
    await closeSettings();

    // ================= The probe says missing =================
    await fresh({ state: "missing" });
    await openSettings();
    await settled("off");
    check("sandbox: missing shows the text of the probe and the line about bubblewrap",
        (await state.innerText()).includes("bwrap was not found in the PATH of a session") &&
            (await help.innerText()).startsWith("Install bubblewrap") && !(await radio("on").isEnabled()));
    check("sandbox: missing in Automatic: the line says the sandbox is off because it does not work",
        (await effective.innerText()).startsWith("In effect now: the sandbox is off, because it does not work on this machine (bwrap was not found"), await effective.innerText());
    await closeSettings();

    // ================= The app refuses Always =================
    // The panel saw `works`. Then the machine changed. Always is enabled, and the app refuses it.
    await fresh({ state: "works" });
    await openSettings();
    await settled("on");
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR } });
    await radio("on").check();
    const refusal = section.locator('[data-testid="action-error-codex-sandbox"]');
    await refusal.waitFor();
    check("sandbox: a refusal of the app shows as a line in the section, with the text of the problem",
        (await refusal.innerText()).includes("does not work on this machine") && (await refusal.innerText()).includes(UID_ERROR), await refusal.innerText());
    await settled("off");
    check("sandbox: after a refusal Automatic is still chosen, the state shows the new result, and Always is disabled",
        (await radio("auto").isChecked()) && !(await radio("on").isChecked()) && (await state.getAttribute("data-state")) === "fails" && !(await radio("on").isEnabled()));
    check("sandbox: the refusal text stays after the new test", (await refusal.count()) === 1);
    await shot("sandbox-refused.png");
    await closeSettings();

    // ================= Always is chosen, and the machine changes (refused in effect) =================
    await fresh({ state: "works" });
    await openSettings();
    await settled("on");
    await choose("on", (await calls("settings_set")).length);
    await closeSettings();
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR } });
    const setCalls = (await calls("settings_set")).length;
    await openSettings();
    await settled("refused");
    check("sandbox: Always and a failing probe: a red line says the sandbox does not work and what to do",
        (await down.count()) === 1 && (await down.innerText()).replace(/\s+/g, " ") === DOWN && (await section.getAttribute("data-effective")) === "refused", await section.innerText());
    check("sandbox: the line 'In effect now' says that Codex employees cannot start, with the probe error",
        (await effective.innerText()) === REFUSED, await effective.innerText());
    check("sandbox: the red line is red (the danger colour) and the stored value did not change by itself",
        (await down.evaluate((el) => getComputedStyle(el).color)) !== (await help.evaluate((el) => getComputedStyle(el).color)) &&
            (await calls("settings_set")).length === setCalls && (await radio("on").isChecked()));
    check("sandbox: Always can be left while the test fails: the three choices are enabled",
        (await radio("on").isEnabled()) && (await radio("auto").isEnabled()) && (await radio("off").isEnabled()) && (await state.getAttribute("data-state")) === "fails");
    check("sandbox: the refused state also shows the help line for the probe and the network line",
        (await help.innerText()) === HELP && (await section.locator('[data-testid="sandbox-network"]').count()) === 1);
    await shot("sandbox-always-down.png");
    // The choices stay enabled while a new test runs, so the operator can leave "Always" at any time.
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR }, sandboxProbeMs: 400 });
    await again.click();
    check("sandbox: Always stays enabled during a test, and the red line is not shown while the test runs",
        (await radio("on").isEnabled()) && (await radio("auto").isEnabled()) && (await down.count()) === 0 && (await section.getAttribute("data-effective")) === "testing");
    await settled("refused");
    check("sandbox: the red line is back after the test", (await down.count()) === 1);
    await choose("auto", (await calls("settings_set")).length);
    check("sandbox: Automatic after that: the red line is gone, the line says off because it does not work, and Always is disabled",
        (await radio("auto").isChecked()) && (await down.count()) === 0 && (await effective.innerText()) === OFF_DOWN && !(await radio("on").isEnabled()), await effective.innerText());
    await closeSettings();

    // ================= The refused start of a Codex employee ("Always"): the toast and the button =================
    await fresh({ state: "works" });
    await makeCodex("eng-lin");
    await openSettings();
    await settled("on");
    await choose("on", (await calls("settings_set")).length);
    await closeSettings();
    await config({ sandboxProbe: { state: "fails", detail: UID_ERROR } });
    await page.locator('.emp[data-slug="eng-lin"] .hit').click();
    await page.locator(".bar button.start").click();
    const toast = toastOf("did not start");
    await toast.waitFor();
    const toastText = await toast.innerText();
    check("sandbox: a refused start shows an error toast that starts with the fact, with no code prefix",
        toastText.startsWith(`Lin did not start: the Codex sandbox does not work on this machine now (${UID_ERROR}).`) &&
            toastText.includes('The setting "Codex sandbox" is "Always".') && !toastText.includes("SANDBOX_DOWN") && toastText.includes(REFUSAL_START), toastText);
    check("sandbox: the toast has the button Open Settings", (await toast.getByRole("button", { name: "Open Settings" }).count()) === 1);
    await page.waitForTimeout(1500);
    check("sandbox: the error toast stays", (await toast.count()) === 1);
    check("sandbox: the refusal did not change the stored value (the last settings_set is the choice of Always)",
        (await lastSet()).args.codexSandboxMode === "on");
    check("sandbox: the refused start is not a Codex start with full access", (await page.evaluate(() => window.__towerMock.codexStarts.length)) === 0);
    await toast.getByRole("button", { name: "Open Settings" }).click();
    await dialog.waitFor();
    await settled("refused");
    await page.waitForTimeout(200);
    check("sandbox: the button opens Settings at the section Codex sandbox, where the red line shows",
        (await inView()) && (await dialog.getAttribute("data-wanted")) === "codex-sandbox" && (await down.count()) === 1 && (await radio("on").isChecked()) && (await radio("on").isEnabled()));
    await shot("sandbox-toast-open.png");
    await closeSettings();

    // The notice of a start by an employee (kind sandbox_down) is an error toast that stays too.
    await page.evaluate((t) => window.__towerMock.notice("/home/demo/tower", t, "sandbox_down"), `SANDBOX_DOWN: ${REFUSAL_START.replace("Lin", "Sam")}`);
    const noticeToast = toastOf("Sam did not start");
    await noticeToast.waitFor();
    check("sandbox: a sandbox_down notice is an error toast with the button Open Settings",
        (await noticeToast.getByRole("button", { name: "Open Settings" }).count()) === 1 && !(await noticeToast.innerText()).includes("SANDBOX_DOWN") &&
            (await noticeToast.getAttribute("data-kind")) !== "info", await noticeToast.innerText());
    await page.waitForTimeout(1500);
    check("sandbox: the notice toast stays", (await noticeToast.count()) === 1);
    await noticeToast.getByRole("button", { name: "Open Settings" }).click();
    await dialog.waitFor();
    check("sandbox: the button of the notice opens Settings at the section Codex sandbox",
        (await dialog.getAttribute("data-wanted")) === "codex-sandbox" && (await inView()));
    await closeSettings();

    // ================= Automatic with a failing sandbox: the Codex start works, and one information toast shows =================
    await fresh({ state: "fails", detail: UID_ERROR });
    await makeCodex("eng-lin");
    await makeCodex("pm");
    await page.locator('.emp[data-slug="eng-lin"] .hit').click();
    await page.locator(".bar button.start").click();
    const autoToast = toastOf("so Codex employees run with full access");
    await autoToast.waitFor();
    check("sandbox: Automatic with a failing sandbox: the Codex start is not refused (no error toast), and the app gives the sandbox flag false",
        (await toastOf("did not start").count()) === 0 && JSON.stringify(await page.evaluate(() => window.__towerMock.codexStarts.map((x) => `${x.slug}:${x.sandbox}`))) === JSON.stringify(["eng-lin:false"]));
    check("sandbox: one information toast says the sandbox does not work and Codex runs with full access, with the button Open Settings",
        (await autoToast.innerText()).includes(AUTO_NOTICE) && (await autoToast.getAttribute("data-kind")) === "info" &&
            (await autoToast.getByRole("button", { name: "Open Settings" }).count()) === 1, await autoToast.innerText());
    await page.waitForTimeout(1500);
    check("sandbox: that information toast stays until the operator closes it", (await autoToast.count()) === 1);
    await shot("sandbox-auto-toast.png");
    // A second Codex start in the same run: the mock sends no second notice (mock.test.ts counts the events), and the
    // one toast is still there.
    await page.locator('.emp[data-slug="pm"] .hit').click();
    await page.locator(".bar button.start").click();
    await page.waitForFunction(() => window.__towerMock.codexStarts.length === 2);
    await page.waitForTimeout(300);
    check("sandbox: a second Codex start in the same run leaves one information toast", (await toastOf("so Codex employees run with full access").count()) === 1);
    const stored = await page.evaluate(() => window.__towerMock.codexStarts.map((x) => `${x.slug}:${x.sandbox}`));
    check("sandbox: both Codex starts had full access", JSON.stringify(stored) === JSON.stringify(["eng-lin:false", "pm:false"]), JSON.stringify(stored));
    await autoToast.getByRole("button", { name: "Open Settings" }).click();
    await dialog.waitFor();
    await settled("off");
    check("sandbox: the button of the information toast opens Settings at the section, where the line says off because it does not work",
        (await dialog.getAttribute("data-wanted")) === "codex-sandbox" && (await inView()) && (await effective.innerText()) === OFF_DOWN, await effective.innerText());
    await closeSettings();

    // ================= Automatic with a working sandbox: the Codex start gets the sandbox, and no toast shows =================
    await fresh({ state: "works" });
    await makeCodex("eng-lin");
    await page.locator('.emp[data-slug="eng-lin"] .hit').click();
    await page.locator(".bar button.start").click();
    await page.waitForFunction(() => window.__towerMock.codexStarts.length === 1);
    await page.waitForTimeout(300);
    check("sandbox: Automatic with a working sandbox: the Codex start gets the sandbox and no toast shows",
        (await page.evaluate(() => window.__towerMock.codexStarts[0].sandbox)) === true && (await toastOf("run with full access").count()) === 0 && (await page.locator(".toast").count()) === 0);

    // ================= Never: a Codex start has full access, with no probe and no toast =================
    await fresh({ state: "works" });
    await makeCodex("eng-lin");
    await openSettings();
    await settled("on");
    await choose("off", (await calls("settings_set")).length);
    await closeSettings();
    const probesBefore = (await calls("codex_sandbox_probe")).length;
    await page.locator('.emp[data-slug="eng-lin"] .hit').click();
    await page.locator(".bar button.start").click();
    await page.waitForFunction(() => window.__towerMock.codexStarts.length === 1);
    check("sandbox: Never: the Codex start has full access, the panel ran no probe, and no toast shows",
        (await page.evaluate(() => window.__towerMock.codexStarts[0].sandbox)) === false &&
            (await calls("codex_sandbox_probe")).length === probesBefore && (await page.locator(".toast").count()) === 0);
}
