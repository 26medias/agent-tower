// When may the app move the keyboard focus into a terminal?
// The operator chose a tab (a click on a tab, a card, a roster row, "Open", a toast). The terminal of that tab takes the
// focus, so the operator can type at once. Two cases keep the focus where it is:
//   - a modal dialog is open (the settings panel, the employee form, the reader): the modal owns the focus,
//   - the focus is in a field that takes text (an input, a textarea, a select, an editable element) and that is not
//     part of a terminal: the operator types there for another reason.
// A button, a link or the body does not count. The click that chose the tab put the focus on such an element.
// Pure logic with a small element shape, so a unit test needs no DOM.

/** What the rule reads of an element. A DOM `Element` has it. */
export interface FocusProbe {
    tagName: string;
    isContentEditable?: boolean;
    getAttribute(name: string): string | null;
    closest(selector: string): unknown;
}

/** The types of `<input>` that do not take text: a click on them is not typing. */
const NON_TEXT_INPUTS = new Set(["button", "checkbox", "radio", "range", "submit", "reset", "image", "file", "color"]);

/** True when the element is a field that takes text, or a list that the operator picks from. */
export function isTextField(el: FocusProbe): boolean {
    const tag = el.tagName.toUpperCase();
    if (tag === "TEXTAREA" || tag === "SELECT") return true;
    if (tag === "INPUT") return !NON_TEXT_INPUTS.has((el.getAttribute("type") ?? "text").toLowerCase());
    return el.isContentEditable === true;
}

/**
 * True when the terminal may take the focus now. `active` is `document.activeElement`. The terminal's own helper
 * textarea (inside `.xterm`) is not "another field": the focus is in a terminal already.
 */
export function mayFocusTerminal(active: FocusProbe | null, modalIsOpen: boolean): boolean {
    if (modalIsOpen) return false;
    if (!active) return true;
    if (active.closest(".xterm")) return true;
    return !isTextField(active);
}
