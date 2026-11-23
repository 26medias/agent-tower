// README screenshots of the mock page (T-14): docs/images/{office,org-chart,task-board,terminals}.png
// and the frames of docs/images/demo.gif.
// Usage: cd ui && pnpm exec vite dev --port 15733 --strictPort   (other shell), then:
//        node prototype/readme-shots.mjs <outDir> [office|org-chart|task-board|terminals|demo ...]
// Env: URL (default http://127.0.0.1:15733/), CHROME (default: the newest Chromium of the Playwright cache).
// The terminal text is staged demo text (readme-demo-text.js). A route patches the served mock.ts
// in this browser only; no file in ui/src changes.
import { existsSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
const { chromium } = createRequire(new URL("../ui/package.json", import.meta.url))("playwright-core");
const PAGE_URL = process.env.URL ?? "http://127.0.0.1:15733/";
const OUT = process.argv[2];
const only = process.argv.slice(3);
function findChrome() {
    const cache = join(homedir(), ".cache", "ms-playwright");
    const dirs = readdirSync(cache).filter((d) => /^chromium-\d+$/.test(d)).sort().reverse();
    for (const d of dirs) {
        const p = join(cache, d, "chrome-linux64", "chrome");
        if (existsSync(p)) return p;
    }
    return undefined;
}
const exe = process.env.CHROME ?? findChrome();
let W = 1440, H = 900;
const b = await chromium.launch({ executablePath: exe, headless: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function page(layout, video = null) {
    const ctx = await b.newContext({ viewport: { width: W, height: H }, deviceScaleFactor: 1, ...(video ? { recordVideo: { dir: video, size: { width: W, height: H } } } : {}) });
    await ctx.addInitScript((l) => {
        localStorage.setItem("agent-tower:layout", JSON.stringify(l));
    }, { col: null, office: null, info: null, board: null, boardCollapsed: null, ...layout });
    // Staged demo text in the terminals (T-14): patch the served mock module in this browser only.
    await ctx.addInitScript({ path: import.meta.dirname + "/readme-demo-text.js" });
    await ctx.route("**/src/lib/mock.ts**", async (route) => {
        const res = await route.fetch();
        let body = await res.text();
        const before = body;
        body = body.replace("function banner(s, emp) {", "function banner(s, emp) { if (globalThis.__demoText) { text(s, globalThis.__demoText(emp)); return; }");
        body = body.replace('const showPrompt = (s) => text(s, "\\x1B[32m$\\x1B[0m ");', 'const showPrompt = (s) => { if (!globalThis.__demoText) text(s, "\\x1B[32m$\\x1B[0m "); };');
        body = body.replace('enc.encode("\\r\\n\\x1B[2mchannel online', 'enc.encode(globalThis.__demoText ? "" : "\\r\\n\\x1B[2mchannel online');
        const n = [body.includes("globalThis.__demoText(emp)"), body.includes("if (!globalThis.__demoText) text"), body.includes('globalThis.__demoText ? "" :')];
        if (n.includes(false)) throw new Error("mock patch failed: " + n);
        await route.fulfill({ response: res, body });
    });
    const p = await ctx.newPage();
    p.on("pageerror", (e) => console.log("ERR", String(e)));
    await p.goto(PAGE_URL);
    // The "mock" badge exists only without Tauri. The real app does not show it.
    await p.addStyleTag({ content: "span.mock{display:none!important}" });
    await p.waitForSelector("button.row");
    await sleep(800);
    // Forget the demo floor whose folder is missing, as an operator does (no file is deleted).
    await p.click("text=FLOORS");
    await sleep(200);
    const gone = p.locator("li[data-path='/home/demo/gone'] button[title^='Forget']");
    if (await gone.count()) {
        await gone.click();
        await sleep(150);
        await gone.click(); // the second click confirms ("sure?")
        await sleep(400);
    }
    await p.click("text=FLOORS");
    await sleep(200);
    return p;
}

async function openFloor(p, path) {
    await p.fill("input[placeholder='/path/to/project']", path);
    await p.click("button.primary:has-text('open')");
    await sleep(1200);
}

async function start(p, names) {
    for (const n of names) {
        await p.click(`button.row:has-text('${n}')`);
        await sleep(250);
        const s = p.locator(".info button:has-text('start'), button:text-is('start')").first();
        if (await s.count()) { await s.click(); await sleep(900); }
    }
}

const STATUS = { todo: "To do", in_progress: "In progress", blocked: "Blocked", review: "Review", done: "Done" };
const BIG_TASKS = [
    ["Set up the nightly build", "Build the app each night and keep the logs.", "Mo", "done"],
    ["Add a dark mode to the settings page", "Follow the colours of the design file.", "Kai", "in_progress"],
    ["Draw the icons for the new toolbar", "Twelve icons, 24 x 24 px, pixel style.", "Noor", "in_progress"],
    ["Update the user guide for version 2", "Add the new pages and the screenshots.", "Bartholomew", "in_progress"],
    ["Fix the crash when the file list is empty", "Steps are in the support ticket.", "Leo", "review"],
    ["Write the tests for the export feature", "CSV and JSON export, with large files.", "Rhea", "review"],
    ["Choose the font for the landing page", "Wait for the brand colours.", "Wolfeschlegel", "blocked"],
    ["Plan the release of version 2", "List the steps and the dates.", "Pia", "todo"],
];
async function makeTasks(p, list) {
    for (const [title, desc, who, status] of list) {
        await p.click("button.new");
        await p.fill("input[aria-label='Title of the new task']", title);
        await p.fill("textarea[aria-label='Description of the new task']", desc);
        await p.click("button:has-text('Make the task')");
        await sleep(300);
        if (!(await p.locator("[aria-label='Task detail']").count())) {
            await p.click(`button.card:has-text(${JSON.stringify(title)})`);
            await sleep(300);
        }
        await p.selectOption("select#assign-to", { label: who });
        await p.click("button:text-is('Assign')");
        await sleep(300);
        if (status !== "todo") {
            await p.selectOption("select#task-status", { label: STATUS[status] });
            await p.click("button:has-text('Set the status')");
            await sleep(300);
        }
        await p.click("button[aria-label='Back to the list of tasks']");
        await sleep(300);
    }
}

const shots = {
    async terminals() {
        const p = await page({ boardCollapsed: false, office: 300, col: 260, info: 560 });
        await openFloor(p, "/home/demo/big");
        await makeTasks(p, BIG_TASKS);
        await p.click("button.collapse");
        await start(p, ["Eve", "Pia", "Noor", "Leo", "Rhea", "Kai"]);
        await sleep(2500);
        await p.locator(".tab", { hasText: "Kai" }).locator("button.main").click();
        await sleep(800);
        return p;
    },
    async office() {
        const p = await page({ boardCollapsed: false, board: 300, col: 270, office: 600 });
        await openFloor(p, "/home/demo/big");
        await makeTasks(p, BIG_TASKS);
        await start(p, ["Eve", "Pia", "Kai", "Leo", "Noor", "Rhea", "Bartholomew"]);
        await sleep(2500);
        await p.click(`button.row:has-text('Pia')`);
        await sleep(600);
        return p;
    },
    async "org-chart"() {
        W = 1600;
        const p = await page({ boardCollapsed: false, office: 720, col: 220, info: 220 });
        await openFloor(p, "/home/demo/big");
        await makeTasks(p, BIG_TASKS);
        await p.click("button.collapse");
        await start(p, ["Eve", "Noor", "Rhea", "Kai", "Pia"]);
        await sleep(2500);
        await p.click("button:has-text('Hierarchy')");
        await sleep(1200);
        return p;
    },
    async "task-board"() {
        const p = await page({ boardCollapsed: false, board: 520, col: 260 });
        await openFloor(p, "/home/demo/big");
        await makeTasks(p, BIG_TASKS);
        await start(p, ["Eve", "Pia", "Noor", "Leo", "Rhea", "Kai"]);
        await sleep(2500);
        return p;
    },
};

// The demo GIF: frames of the page during a short scene, with their real times.
async function demo() {
    const { mkdirSync, writeFileSync } = await import("node:fs");
    const dir = `${OUT}/frames`;
    mkdirSync(dir, { recursive: true });
    const p = await page({ boardCollapsed: false, board: 300, col: 270, office: 600 });
    await openFloor(p, "/home/demo/big");
    await makeTasks(p, BIG_TASKS);
    await start(p, ["Eve", "Pia", "Bartholomew", "Leo", "Noor", "Rhea", "Kai"]);
    await sleep(2500);
    await p.click(`button.row:has-text('Pia')`);
    await sleep(600);
    const frames = [];
    let run = true;
    const loop = (async () => {
        while (run) {
            const at = Date.now();
            const file = `${dir}/f${String(frames.length).padStart(4, "0")}.png`;
            await p.screenshot({ path: file });
            frames.push({ file, at });
        }
    })();
    await sleep(1500);
    await p.locator(".tab", { hasText: "Kai" }).locator("button.main").click();
    await sleep(1800);
    await p.click("button:has-text('Hierarchy')");
    await sleep(2200);
    await p.click("button.ghost:text-is('Floor')");
    await sleep(1200);
    await p.click("button.card:has-text('Fix the crash when the file list is empty')");
    await sleep(2200);
    await p.click("button[aria-label='Back to the list of tasks']");
    await p.locator(".tab", { hasText: "Pia" }).locator("button.main").click();
    await sleep(1800);
    run = false;
    await loop;
    const end = Date.now();
    // ffmpeg concat list: each frame stays until the next one.
    let list = "";
    frames.forEach((f, i) => {
        const next = i + 1 < frames.length ? frames[i + 1].at : end;
        list += `file '${resolve(f.file)}'\nduration ${((next - f.at) / 1000).toFixed(3)}\n`;
    });
    list += `file '${resolve(frames[frames.length - 1].file)}'\n`;
    writeFileSync(`${dir}/list.txt`, list);
    console.log("frames", frames.length, "seconds", ((end - frames[0].at) / 1000).toFixed(1));
    await p.context().close();
}
if (only.includes("demo")) { await demo(); await b.close(); process.exit(0); }

for (const [name, fn] of Object.entries(shots)) {
    if (only.length && !only.includes(name)) continue;
    const p = await fn();
    await p.screenshot({ path: `${OUT}/${name}.png` });
    console.log("shot", name);
    await p.context().close();
    W = 1440;
}
await b.close();
