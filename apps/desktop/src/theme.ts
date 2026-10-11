// Theme setting: dark (default), light, or match the system. The choice is a
// per-machine convenience kept in localStorage; storage can be missing or
// blocked, so every access is guarded and the app still renders.
import { useCallback, useEffect, useState } from "react";

export type ThemeChoice = "dark" | "light" | "system";
export type Theme = "dark" | "light";

export const THEME_KEY = "space-optimiser:theme";
const LIGHT_QUERY = "(prefers-color-scheme: light)";

function isChoice(value: unknown): value is ThemeChoice {
  return value === "dark" || value === "light" || value === "system";
}

export function loadTheme(): ThemeChoice {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    if (isChoice(stored)) return stored;
  } catch {
    // Blocked storage: use the default.
  }
  return "dark";
}

export function saveTheme(choice: ThemeChoice): void {
  try {
    localStorage.setItem(THEME_KEY, choice);
  } catch {
    // Blocked storage: the choice lasts until the window closes.
  }
}

function lightQuery(): MediaQueryList | null {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return null;
  return window.matchMedia(LIGHT_QUERY);
}

export function resolveTheme(choice: ThemeChoice): Theme {
  if (choice !== "system") return choice;
  return lightQuery()?.matches ? "light" : "dark";
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

/** The current choice, a setter that saves it, and live system tracking. */
export function useTheme(): [ThemeChoice, (choice: ThemeChoice) => void] {
  const [choice, setChoice] = useState<ThemeChoice>(loadTheme);

  useEffect(() => {
    applyTheme(resolveTheme(choice));
    if (choice !== "system") return;
    const query = lightQuery();
    if (!query) return;
    const follow = (e: { matches: boolean }) => applyTheme(e.matches ? "light" : "dark");
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, [choice]);

  const choose = useCallback((next: ThemeChoice) => {
    saveTheme(next);
    setChoice(next);
  }, []);

  return [choice, choose];
}
