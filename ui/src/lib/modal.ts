// One helper for every modal and overlay panel (the employee form, the settings panel, the
// report and diff reader). Use it as an action on the root element of the modal: `use:modal={{ onclose }}`.
//
// While the modal is open:
//   - the first field gets the focus (or the element with `data-autofocus`);
//   - Tab and Shift+Tab stay inside the modal;
//   - everything behind the modal is `inert` and `aria-hidden`. A browser with no `inert` gets a
//     fallback: the focusable elements get `tabindex="-1"`, and `[data-inert-fallback]` in app.css
//     blocks the pointer. An element with `data-keep-live` (the toasts) stays usable;
//   - Escape closes the modal. The key never reaches the terminal behind it. Key, input and paste
//     events that come from xterm are stopped at the capture phase of the document, and the
//     terminal is blurred when the modal opens;
//   - when the modal closes, the focus goes back to the element that had it before.
// Several modals can be open at once. Only the last one handles the keys.

const FOCUSABLE = 'a[href], button, input, select, textarea, summary, iframe, [tabindex], [contenteditable="true"]';
const FIELD = "input:not([type=hidden]), select, textarea";
const BLOCKED_EVENTS = ["keydown", "keypress", "keyup", "input", "beforeinput", "paste", "compositionstart", "compositionupdate", "compositionend"];

export interface ModalOptions {
    /** Called when the operator presses Escape. The modal must close itself. */
    onclose: () => void;
}

interface Entry {
    node: HTMLElement;
    onclose: () => void;
}

const stack: Entry[] = [];

/** True while at least one modal is open. Code that moves the focus (the terminal) checks it. */
export function modalOpen(): boolean {
    return stack.length > 0;
}

/** True when this browser has the `inert` attribute (WebKitGTK 2.38 and later has it). */
export function supportsInert(): boolean {
    return typeof HTMLElement !== "undefined" && "inert" in HTMLElement.prototype;
}

function usable(el: HTMLElement): boolean {
    if (el.hasAttribute("disabled") || el.tabIndex < 0 || el.closest("[inert]")) return false;
    return el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";
}

/** The elements that Tab can reach inside `root`, in DOM order. */
export function focusables(root: HTMLElement): HTMLElement[] {
    return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(usable);
}

// ---- everything behind the modal ----

interface Mark {
    count: number;
    inert: boolean;
    ariaHidden: string | null;
    tabs: [HTMLElement, string | null][];
}

const marks = new WeakMap<Element, Mark>();

function mark(el: HTMLElement): void {
    let m = marks.get(el);
    if (!m) {
        m = { count: 0, inert: el.inert === true, ariaHidden: el.getAttribute("aria-hidden"), tabs: [] };
        marks.set(el, m);
        el.setAttribute("aria-hidden", "true");
        if (supportsInert()) {
            el.inert = true;
        } else {
            el.setAttribute("data-inert-fallback", "");
            for (const f of el.querySelectorAll<HTMLElement>(FOCUSABLE)) {
                m.tabs.push([f, f.getAttribute("tabindex")]);
                f.setAttribute("tabindex", "-1");
            }
        }
    }
    m.count += 1;
}

function unmark(el: HTMLElement): void {
    const m = marks.get(el);
    if (!m) return;
    m.count -= 1;
    if (m.count > 0) return;
    marks.delete(el);
    if (m.ariaHidden === null) el.removeAttribute("aria-hidden");
    else el.setAttribute("aria-hidden", m.ariaHidden);
    if (supportsInert()) {
        el.inert = m.inert;
    } else {
        el.removeAttribute("data-inert-fallback");
        for (const [f, tab] of m.tabs) {
            if (tab === null) f.removeAttribute("tabindex");
            else f.setAttribute("tabindex", tab);
        }
    }
}

/** Mark every sibling of the modal and of its ancestors (up to the body). Returns what it marked. */
function isolate(node: HTMLElement): HTMLElement[] {
    const marked: HTMLElement[] = [];
    let el: HTMLElement | null = node;
    while (el && el !== document.body && el.parentElement) {
        for (const sibling of el.parentElement.children) {
            if (sibling === el || !(sibling instanceof HTMLElement)) continue;
            if (sibling.hasAttribute("data-keep-live")) continue;
            if (["SCRIPT", "STYLE", "LINK", "TEMPLATE"].includes(sibling.tagName)) continue;
            mark(sibling);
            marked.push(sibling);
        }
        el = el.parentElement;
    }
    return marked;
}

function firstFocus(node: HTMLElement): HTMLElement | null {
    const wanted = node.querySelector<HTMLElement>("[data-autofocus]");
    if (wanted && usable(wanted)) return wanted;
    const items = focusables(node);
    return items.find((el) => el.matches(FIELD)) ?? items[0] ?? null;
}

// ---- the action ----

export function modal(node: HTMLElement, options: ModalOptions) {
    let current = options;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // The terminal (or any element) must not keep the focus behind the modal.
    if (opener && opener !== document.body) opener.blur();

    const marked = isolate(node);
    const entry: Entry = { node, onclose: () => current.onclose() };
    stack.push(entry);
    const isTop = () => stack[stack.length - 1] === entry;

    if (!node.hasAttribute("tabindex")) node.setAttribute("tabindex", "-1");
    (firstFocus(node) ?? node).focus({ preventScroll: true });

    const onKey = (e: KeyboardEvent): void => {
        if (!isTop() || e.isComposing) return;
        if (e.key === "Escape") {
            e.preventDefault();
            e.stopImmediatePropagation();
            entry.onclose();
            return;
        }
        if (e.key !== "Tab") return;
        const items = focusables(node);
        if (items.length === 0) {
            e.preventDefault();
            node.focus();
            return;
        }
        const first = items[0];
        const last = items[items.length - 1];
        const active = document.activeElement;
        const inside = active instanceof Node && node.contains(active);
        if (e.shiftKey && (active === first || !inside || active === node)) {
            e.preventDefault();
            last.focus();
        } else if (!e.shiftKey && (active === last || !inside || active === node)) {
            e.preventDefault();
            first.focus();
        }
    };

    // The terminal gets no key, input or paste event while a modal is open.
    const onBlocked = (e: Event): void => {
        if (!isTop()) return;
        const target = e.target;
        if (target instanceof Element && target.closest(".xterm")) {
            e.stopImmediatePropagation();
            e.preventDefault();
        }
    };

    // The focus never leaves the modal, except to a toast (a toast can hold an error to close).
    const onFocusIn = (e: FocusEvent): void => {
        if (!isTop()) return;
        const target = e.target;
        if (!(target instanceof Element) || node.contains(target) || target.closest("[data-keep-live]")) return;
        (firstFocus(node) ?? node).focus({ preventScroll: true });
    };

    document.addEventListener("keydown", onKey, true);
    for (const type of BLOCKED_EVENTS) document.addEventListener(type, onBlocked, true);
    document.addEventListener("focusin", onFocusIn, true);

    return {
        update(next: ModalOptions): void {
            current = next;
        },
        destroy(): void {
            document.removeEventListener("keydown", onKey, true);
            for (const type of BLOCKED_EVENTS) document.removeEventListener(type, onBlocked, true);
            document.removeEventListener("focusin", onFocusIn, true);
            const at = stack.indexOf(entry);
            if (at >= 0) stack.splice(at, 1);
            for (const el of marked) unmark(el);
            // Give the focus back, unless another modal is open or the element is gone.
            if (stack.length === 0 && opener && opener.isConnected && opener !== document.body && !opener.hasAttribute("disabled")) {
                opener.focus({ preventScroll: true });
            }
        },
    };
}
