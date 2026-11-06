// Claude — Date 11/06/2026
// What ⌘C last copied, kept for the rest of the run so it can be pasted into
// another diagram. It's the app's own clipboard: it isn't the system's, so
// nothing the user copied elsewhere is touched and nothing here leaks out.

import type { Part } from './model';

let held: readonly Part[] = [];

export const getClip = (): readonly Part[] => held;
export const setClip = (parts: readonly Part[]) => {
  held = parts;
};
