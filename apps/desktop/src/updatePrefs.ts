// "Check for updates when Space Optimiser starts": off by default, so the
// app contacts nothing unless asked (phase 15). A per-machine convenience in
// localStorage, guarded like the theme: storage can be missing or blocked.
export const AUTO_CHECK_KEY = "space-optimiser:check-updates-at-start";

export function loadAutoCheck(): boolean {
  try {
    return localStorage.getItem(AUTO_CHECK_KEY) === "on";
  } catch {
    return false;
  }
}

export function saveAutoCheck(on: boolean): void {
  try {
    if (on) localStorage.setItem(AUTO_CHECK_KEY, "on");
    else localStorage.removeItem(AUTO_CHECK_KEY);
  } catch {
    // Blocked storage: the choice lasts until the window closes.
  }
}
