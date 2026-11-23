import { describe, expect, it } from "vitest";
import { isTextField, mayFocusTerminal, type FocusProbe } from "./focus";

function el(tagName: string, opts: { type?: string; editable?: boolean; inXterm?: boolean } = {}): FocusProbe {
    return {
        tagName,
        isContentEditable: opts.editable,
        getAttribute: (name) => (name === "type" ? (opts.type ?? null) : null),
        closest: (selector) => (selector === ".xterm" && opts.inXterm ? {} : null),
    };
}

describe("mayFocusTerminal", () => {
    it("lets the terminal take the focus from the body, a button, a link and a tab", () => {
        expect(mayFocusTerminal(null, false)).toBe(true);
        expect(mayFocusTerminal(el("BODY"), false)).toBe(true);
        expect(mayFocusTerminal(el("BUTTON"), false)).toBe(true);
        expect(mayFocusTerminal(el("A"), false)).toBe(true);
        expect(mayFocusTerminal(el("g"), false)).toBe(true);
        expect(mayFocusTerminal(el("INPUT", { type: "checkbox" }), false)).toBe(true);
    });

    it("never takes the focus while a modal is open", () => {
        expect(mayFocusTerminal(null, true)).toBe(false);
        expect(mayFocusTerminal(el("BUTTON"), true)).toBe(false);
        expect(mayFocusTerminal(el("TEXTAREA", { inXterm: true }), true)).toBe(false);
    });

    it("does not take the focus from a field that the operator types in", () => {
        expect(mayFocusTerminal(el("INPUT"), false)).toBe(false);
        expect(mayFocusTerminal(el("INPUT", { type: "password" }), false)).toBe(false);
        expect(mayFocusTerminal(el("INPUT", { type: "search" }), false)).toBe(false);
        expect(mayFocusTerminal(el("TEXTAREA"), false)).toBe(false);
        expect(mayFocusTerminal(el("SELECT"), false)).toBe(false);
        expect(mayFocusTerminal(el("DIV", { editable: true }), false)).toBe(false);
    });

    it("takes the focus again when it is in a terminal already (the helper textarea of xterm)", () => {
        expect(mayFocusTerminal(el("TEXTAREA", { inXterm: true }), false)).toBe(true);
    });
});

describe("isTextField", () => {
    it("knows the input types that take no text", () => {
        for (const type of ["button", "checkbox", "radio", "range", "submit"]) expect(isTextField(el("INPUT", { type }))).toBe(false);
        for (const type of ["text", "email", "url", "number", "TEXT"]) expect(isTextField(el("INPUT", { type }))).toBe(true);
    });
});
