import { useCallback, useEffect, useState } from "react";

// Personal touches: every person gets a stable map drawn from their name and
// can pick the accent the whole interface uses. Nothing here leaves the PC.

export type Accent = "pine" | "terracotta" | "indigo" | "graphite";
export const ACCENTS: Accent[] = ["pine", "terracotta", "indigo", "graphite"];
const ACCENT_KEY = "atlas.accent";

/** FNV-1a: a stable 32-bit seed for a person's map. */
export function seedFrom(text: string): number {
  let hash = 0x811c9dc5;
  for (const char of text.trim().toLowerCase()) {
    hash ^= char.codePointAt(0)!;
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** mulberry32: small deterministic PRNG so the same seed always draws the same map. */
export function random(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = Math.imul(state ^ (state >>> 15), 1 | state);
    value = (value + Math.imul(value ^ (value >>> 7), 61 | value)) ^ value;
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return "A";
  return (parts[0][0] + (parts.length > 1 ? parts[parts.length - 1][0] : "")).toUpperCase();
}

export function firstName(name: string): string {
  return name.trim().split(/\s+/)[0] ?? "";
}

export function greetingKey(hour: number): string {
  if (hour < 12) return "Good morning";
  if (hour < 19) return "Good afternoon";
  return "Good evening";
}

function readAccent(): Accent {
  try {
    const stored = window.localStorage.getItem(ACCENT_KEY);
    return ACCENTS.includes(stored as Accent) ? (stored as Accent) : "pine";
  } catch {
    return "pine";
  }
}

function applyAccent(accent: Accent) {
  if (accent === "pine") delete document.documentElement.dataset.accent;
  else document.documentElement.dataset.accent = accent;
}

// Applied before the first render so there is no flash of the default accent.
applyAccent(readAccent());

const listeners = new Set<(accent: Accent) => void>();

export function useAccent(): [Accent, (accent: Accent) => void] {
  const [accent, setAccentState] = useState<Accent>(readAccent);
  useEffect(() => {
    listeners.add(setAccentState);
    return () => { listeners.delete(setAccentState); };
  }, []);
  const setAccent = useCallback((next: Accent) => {
    applyAccent(next);
    try { window.localStorage.setItem(ACCENT_KEY, next); } catch { /* preference stays for this session */ }
    listeners.forEach(listener => listener(next));
  }, []);
  return [accent, setAccent];
}

/** Reads a token channel ("r g b") from the live stylesheet. */
export function tokenColor(name: string, alpha = 1): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(`--atlas-${name}`).trim() || "15 79 69";
  return `rgb(${value} / ${alpha})`;
}
