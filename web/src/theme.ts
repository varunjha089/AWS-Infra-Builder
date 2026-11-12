// Day / night theme: `data-theme` on <html> switches the CSS variables; remembered in this window's storage.
import { useSyncExternalStore } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type Theme = "day" | "night";

function stored(): Theme {
  try { return localStorage.getItem("aib-theme") === "night" ? "night" : "day"; } catch { return "day"; }
}

let current: Theme = stored();
const subs = new Set<() => void>();

function apply() {
  document.documentElement.dataset.theme = current;
  getCurrentWindow().setTheme(current === "night" ? "dark" : "light").catch(() => {});
}
apply();

export function setTheme(t: Theme) {
  current = t;
  try { localStorage.setItem("aib-theme", t); } catch { /* not remembered */ }
  apply();
  subs.forEach((f) => f());
}
export const toggleTheme = () => setTheme(current === "night" ? "day" : "night");

/** the current theme; components using it redraw when it changes */
export function useTheme(): Theme {
  return useSyncExternalStore((f) => { subs.add(f); return () => subs.delete(f); }, () => current);
}

/** a kind's colour as neon: full saturation and a light tone, so it glows on a dark panel */
export function neon(hex: string): string {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) return hex;
  const [r, g, b] = [1, 2, 3].map((i) => parseInt(m[i], 16) / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0;
  if (max !== min) {
    const d = max - min;
    h = max === r ? ((g - b) / d + (g < b ? 6 : 0)) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h /= 6;
  }
  const grey = max - min < 0.08;
  const sat = grey ? 0.25 : 1, light = grey ? 0.75 : 0.62;
  const q = light < 0.5 ? light * (1 + sat) : light + sat - light * sat, p = 2 * light - q;
  const f = (t: number) => { t = (t + 1) % 1; return t < 1 / 6 ? p + (q - p) * 6 * t : t < 1 / 2 ? q : t < 2 / 3 ? p + (q - p) * (2 / 3 - t) * 6 : p; };
  const to = (v: number) => Math.round(v * 255).toString(16).padStart(2, "0");
  return `#${to(f(h + 1 / 3))}${to(f(h))}${to(f(h - 1 / 3))}`;
}
