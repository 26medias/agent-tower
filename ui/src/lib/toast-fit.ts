// How many toasts fit whole in the stack when they are folded. The stack has a height limit (it stops above the last
// 8 terminal rows). A toast that the limit cuts would force the operator to scroll for it, so the stack shows only the
// toasts that fit, and the line "N older messages wait" counts the others.

export interface FitInput {
    /** The height that the stack may use, in px (its `max-height`). */
    available: number;
    /** The folded heights of the newest toasts, the newest first, in px. A height that is not known yet is `Infinity`. */
    heights: number[];
    /** The gap between two items of the stack, in px. */
    gap: number;
    /** The height of the line "N older messages wait", in px. It shows when some toasts are not shown. */
    moreHeight: number;
    /** The number of toasts that the store holds. */
    total: number;
    /** The most toasts that show at one time. */
    max: number;
}

/**
 * The number of toasts to show, from 1 to `max`. It is the greatest number whose toasts, the gaps and the line for the
 * older ones (when some toasts stay out) all fit in `available`. One toast always shows, even when it does not fit:
 * the stack then scrolls inside. With no toast the number is 0.
 */
export function toastsThatFit({ available, heights, gap, moreHeight, total, max }: FitInput): number {
    const most = Math.min(max, total, heights.length);
    if (most <= 0) return 0;
    let best = 1;
    let used = 0;
    for (let count = 1; count <= most; count++) {
        used += heights[count - 1] + (count > 1 ? gap : 0);
        const line = total > count ? gap + moreHeight : 0;
        if (used + line <= available) best = count;
    }
    return best;
}
