import { describe, expect, it } from "vitest";
import { describeError, isFloorGone, isPathError, noticePlan, programProblem, splitFileError, tidyError } from "./messages";

describe("tidyError", () => {
    it("removes the code prefix and starts with a capital letter", () => {
        expect(tidyError("NO_KEY: no OpenAI key is known")).toBe("No OpenAI key is known.");
        expect(tidyError("NO_FLOOR: /x has no .swarm/ folder")).toBe("/x has no .swarm/ folder.");
        expect(tidyError("cannot start Cy: the CLI is gone")).toContain("Cannot start Cy");
    });

    it("adds a hint for a folder that you cannot write", () => {
        const t = tidyError("I/O error on /p/.swarm: Permission denied (os error 13)");
        expect(t.startsWith("I/O error on /p/.swarm")).toBe(true);
        expect(t).toContain("You cannot write in this folder. Choose a folder that you own.");
    });

    it("adds a hint for a folder that does not exist", () => {
        expect(tidyError("the folder /x does not exist or is not a folder")).toMatch(/^The folder \/x does not exist.*Check the path/);
    });

    it("points to Settings for a program that is missing", () => {
        const t = tidyError("cannot start Cy: cannot start /x/fake2.sh: Unable to spawn /x/fake2.sh (ENOENT)");
        expect(t).toContain("Change the program path in Settings > Programs.");
        // The text of the mock already says where to fix it.
        expect(tidyError('cannot start sam: the program "codex" was not found. Set its path in Programs.')).not.toContain("Settings > Programs");
    });

    it("does not end a sentence twice, and handles an empty text", () => {
        expect(tidyError("It failed.")).toBe("It failed.");
        expect(tidyError("")).toBe("Something went wrong.");
        expect(tidyError("BUSY:")).toBe("Something went wrong.");
    });
});

describe("describeError", () => {
    it("offers the Settings action only for a program problem", () => {
        expect(describeError('cannot start sam: the program "codex" was not found.').action).toBe("settings-programs");
        expect(describeError("Could not resume pm: forced test failure").action).toBeNull();
        expect(programProblem("the task T-1 is not found")).toBe(false);
    });
});

describe("splitFileError", () => {
    it("puts the file name first and the folder apart", () => {
        const e = splitFileError("/work/floor/.swarm/employees/broken.md: line 1, column 15: a quote is missing");
        expect(e.file).toBe("broken.md");
        expect(e.folder).toBe("/work/floor/.swarm/employees");
        expect(e.message).toBe("line 1, column 15: a quote is missing");
    });

    it("keeps a text that has no file name", () => {
        expect(splitFileError("something odd")).toEqual({ file: "", folder: "", message: "something odd" });
    });
});

describe("the path errors of floor_open and floor_create", () => {
    it("removes the code prefix of each of them, and of FLOOR_GONE", () => {
        expect(tidyError("NOT_ABSOLUTE: Type the full path of the folder, for example /home/you/project.")).toBe(
            "Type the full path of the folder, for example /home/you/project.",
        );
        expect(tidyError("NOT_FOUND: The folder /x/y does not exist.")).toBe("The folder /x/y does not exist.");
        expect(tidyError("NOT_A_FOLDER: /x/a.txt is a file, not a folder.")).toBe("/x/a.txt is a file, not a folder.");
        expect(tidyError("NO_WRITE: You cannot write in the folder /ro.")).toBe("You cannot write in the folder /ro.");
        expect(tidyError("FLOOR_GONE: The folder of this floor is gone: /x.")).toBe("The folder of this floor is gone: /x.");
    });

    it("knows the four path errors, and not NO_FLOOR", () => {
        for (const code of ["NOT_ABSOLUTE", "NOT_FOUND", "NOT_A_FOLDER", "NO_WRITE"]) expect(isPathError(`${code}: text`)).toBe(true);
        expect(isPathError("NO_FLOOR: /x has no .swarm/ folder")).toBe(false);
        expect(isPathError("FLOOR_GONE: The folder of this floor is gone: /x.")).toBe(false);
        expect(isPathError("the task T-1 is not found")).toBe(false);
        expect(isFloorGone("FLOOR_GONE: The folder of this floor is gone: /x.")).toBe(true);
        expect(isFloorGone("NOT_FOUND: x")).toBe(false);
    });
});

describe("noticePlan", () => {
    it("shows the error kinds as an error toast", () => {
        for (const kind of ["task_blocked", "resume_failed", "floor_gone"]) {
            expect(noticePlan(kind).tone).toBe("error");
        }
    });

    it("shows floor_back as an information toast that closes by itself", () => {
        expect(noticePlan("floor_back")).toEqual({ tone: "info", stay: false });
    });

    it("shows task_done_unmerged as an information toast that stays", () => {
        expect(noticePlan("task_done_unmerged")).toEqual({ tone: "info", stay: true });
    });

    it("shows a kind that it does not know as an information toast", () => {
        expect(noticePlan("something_new")).toEqual({ tone: "info", stay: false });
        // The app removed this kind (docs/ipc-m3.md): it is not an error any more.
        expect(noticePlan("permissions_restored")).toEqual({ tone: "info", stay: false });
    });
});
