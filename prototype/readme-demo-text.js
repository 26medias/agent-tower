// Staged demo text for the mock terminals (T-14). It is injected in the browser only.
// The message texts follow crates/core/src/app/bus.rs (format_message, format_task_message,
// answer_line, answer_order) and crates/core/src/app/tasks.rs (status_body, assign_body).
globalThis.__demoText = (emp) => {
    const B = "\x1b[1m", D = "\x1b[2m", G = "\x1b[32m", C = "\x1b[36m", Y = "\x1b[33m", R = "\x1b[0m";
    const answer = (slug) => `To answer, use the tool send(to="${slug}", ...). Answer only if this message gives you work or asks a question. Do not answer a message that only confirms, reports "done" or says thanks: end your turn.`;
    const orders = (id, from) => [
        "Orders of the app for this task:",
        `1. When you start the work, set the status with the tool task_update(task="${id}", status="in_progress").`,
        `2. When the work is complete, write your report to .swarm/reports/${id}.md. Say what you did, what you verified, what you did not do, and your open questions. Then call task_update(task="${id}", status="review", report=".swarm/reports/${id}.md").`,
        `3. Answer the person who gave you this task with the tool send(to="${from}", ...). Write a short message that names the report file. Do not copy the report into the message.`,
    ];
    const step = (t) => `${G}•${R} ${t}`;
    const out = (t) => `  ${D}${t}${R}`;
    const T = {
        Kai: [
            `${B}[task T-2 from Pia (Project Manager)]${R}`,
            "> Title: Add a dark mode to the settings page",
            "> ",
            "> Follow the colours of the design file.",
            ...orders("T-2", "pm"),
            "",
            step(`task_update(task="T-2", status="in_progress")`),
            out("Task T-2 has the status in_progress."),
            step("Read src/settings/SettingsPage.svelte"),
            step("Read src/theme/colors.css"),
            step(`Edit src/theme/colors.css ${G}+38${R} ${"\x1b[31m"}-2${R}`),
            step(`Edit src/settings/SettingsPage.svelte ${G}+21${R} ${"\x1b[31m"}-4${R}`),
            step("Run npm test"),
            out("64 tests passed"),
            "",
            `${B}[message from Noor (Designer)]${R}`,
            "> The dark background is #101826. Use #5ee0c0 for the accent.",
            answer("des-1"),
            "",
            step(`Edit src/theme/colors.css ${G}+2${R} ${"\x1b[31m"}-2${R}`),
            step("Run npm test"),
            out("64 tests passed"),
            `${C}Working on the toggle in the settings page...${R}`,
        ],
        Pia: [
            `${B}[message from Eve (CEO)]${R}`,
            "> Version 2 ships on Friday. Plan the release and give the open work to the team.",
            answer("ceo"),
            "",
            step(`task_list()`),
            step(`task_assign(task="T-2", to="eng-1")`),
            step(`task_assign(task="T-3", to="des-1")`),
            step(`task_assign(task="T-5", to="eng-2")`),
            step(`task_assign(task="T-6", to="qa-2")`),
            step(`send(to="ceo", text="The plan is in T-8. Kai, Noor, Leo and Rhea have work.")`),
            "",
            `${B}[task T-5 from Leo (Engineer)]${R}`,
            "> Title: Fix the crash when the file list is empty",
            "> Status: review",
            "> Report: .swarm/reports/T-5.md",
            "This is a status message of the app. Use the tool task_get(task=\"T-5\") to see the task. Read the report file, if there is one, before you set the status to done. Do not send a message back for this status message.",
            "",
            step(`task_get(task="T-5")`),
            step("Read .swarm/reports/T-5.md"),
            `${C}Reading the report of T-5...${R}`,
        ],
    };
    const lines = T[emp.name] ?? [
        `${B}[message from Pia (Project Manager)]${R}`,
        "> Please read the board. Your task is in the list.",
        answer("pm"),
        "",
        step("task_list()"),
        `${C}Reading the task board...${R}`,
    ];
    return lines.join("\r\n") + "\r\n";
};
