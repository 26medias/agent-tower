// Checks of the notice `exchange_stopped` on the mock page (docs/ipc-final.md, spec 4.5, 2026-11-21): the app refused a
// message because two employees sent each other 8 messages in 5 minutes. The toast is an error toast that stays, with the
// button "Open" for the tab of the employee that was refused. The list "Team activity" gets one row for it.
// `mock-page.mjs` calls `exchangeChecks` after `sandboxChecks`, with its own browser page and the mock backend. The mock
// has no tools for employees, so the checks send the event through `window.__towerMock.notice`, as the real app does.
import { join } from "node:path";

const FLOOR = "/home/demo/tower";
const TEXT = "The app stopped an exchange between Maya and Ada: 8 messages in 5 minutes.";

export async function exchangeChecks({ page, check, url, shots }) {
    const M = (name, ...args) => page.evaluate(([n, a]) => window.__towerMock[n](...a), [name, args]);
    const toasts = page.locator(".toast").filter({ hasText: "The app stopped an exchange" });
    const activity = page.locator('[data-testid="team-activity"]');

    await page.evaluate(() => localStorage.clear());
    await page.setViewportSize({ width: 2560, height: 1400 });
    await page.goto(url);
    await page.waitForSelector(".storey.on");
    await M("freeze", true);
    const ceo = await M("startSession", FLOOR, "ceo");
    const pm = await M("startSession", FLOOR, "pm");
    await page.waitForSelector(`.tab[data-session="${pm}"]`);
    for (const id of [ceo, pm]) await M("setStatus", id, "idle");
    // The tab of the CEO is the active tab. The notice is about Maya (the pm), who was refused.
    await page.locator(`.tab[data-session="${ceo}"] button.main`).click();
    check("exchange: the tab of the ceo is active before the notice", (await page.locator(`.tab.active[data-session="${ceo}"]`).count()) === 1);

    await M("notice", FLOOR, TEXT, "exchange_stopped", "pm");
    await toasts.first().waitFor();
    check("exchange: the notice is one toast with the text of the notice", (await toasts.count()) === 1 && (await toasts.first().locator(".text").innerText()) === TEXT);
    check("exchange: the toast is an error toast (red, role alert)", (await toasts.first().getAttribute("data-kind")) === "error" && (await toasts.first().getAttribute("role")) === "alert");
    check("exchange: the toast has the button Open", (await toasts.first().locator("button.act").innerText()) === "Open");
    // An information toast closes after 6 seconds. This one stays.
    await page.waitForTimeout(7000);
    check("exchange: the toast is still there after 7 seconds (it stays until it is closed)", (await toasts.count()) === 1);

    // The row in Team activity.
    const rows = activity.locator('li button[data-kind="exchange"]');
    check("exchange: Team activity has one row of the kind exchange, with the text of the notice", (await rows.count()) === 1 && (await rows.first().locator(".text").innerText()) === TEXT);

    // "Open" goes to the tab of the employee that was refused, and the toast closes.
    await toasts.first().locator("button.act").click();
    await page.waitForSelector(`.tab.active[data-session="${pm}"]`);
    check("exchange: a click on Open goes to the tab of the refused employee (Maya)", (await page.locator(".bar").getAttribute("data-slug")) === "pm" && (await page.locator(`.tab.active[data-session="${pm}"]`).count()) === 1);
    check("exchange: the toast closed after the click on Open", (await toasts.count()) === 0);

    // The row goes to the same tab.
    await page.locator(`.tab[data-session="${ceo}"] button.main`).click();
    await rows.first().click();
    check("exchange: a click on the row goes to the same tab", (await page.locator(`.tab.active[data-session="${pm}"]`).count()) === 1);

    // A second notice (another window) is a second toast and a second row. The same text twice shows one toast.
    await M("notice", FLOOR, TEXT, "exchange_stopped", "pm");
    await M("notice", FLOOR, TEXT, "exchange_stopped", "pm");
    await toasts.first().waitFor();
    check("exchange: the same text twice shows one toast (as for any toast)", (await toasts.count()) === 1);
    await page.screenshot({ path: join(shots, "exchange-toast.png") });
    await page.locator('.toast button[aria-label="Dismiss"]').first().click();
    check("exchange: the operator can close the toast with the x", (await toasts.count()) === 0);
}
