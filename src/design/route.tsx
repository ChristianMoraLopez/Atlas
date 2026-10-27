import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import type { Interaction, SourceKind } from "../types";

// The day is drawn as a survey route: every interaction is a numbered
// waypoint, in time order. The hero map and the logbook table share the
// numbering and the waypoint in focus, so hovering one lights up the other.

export type Waypoint = { item: Interaction; number: number; at: Date };

export function waypoints(items: Interaction[]): Waypoint[] {
  return items
    .map(item => ({ item, at: new Date(item.interactionDateTime) }))
    .filter(point => !Number.isNaN(point.at.getTime()))
    .sort((a, b) => a.at.getTime() - b.at.getTime())
    .map((point, index) => ({ ...point, number: index + 1 }));
}

export function useWaypointNumbers(items: Interaction[]) {
  return useMemo(() => new Map(waypoints(items).map(point => [point.item.sourceId, point.number])), [items]);
}

type Focus = { focused?: string; setFocused: (sourceId?: string) => void };
const RouteFocus = createContext<Focus>({ setFocused: () => undefined });

export function RouteFocusProvider({ children }: { children: ReactNode }) {
  const [focused, setFocused] = useState<string>();
  const value = useMemo(() => ({ focused, setFocused }), [focused]);
  return <RouteFocus.Provider value={value}>{children}</RouteFocus.Provider>;
}

export const useRouteFocus = () => useContext(RouteFocus);

/** Map symbols: each source has its own shape, so colour is never the only cue. */
export function SourceMark({ kind, size = 12, hollow = false, className }: { kind: SourceKind; size?: number; hollow?: boolean; className?: string }) {
  const paint = hollow ? { fill: "none", stroke: "currentColor", strokeWidth: 1.6 } : { fill: "currentColor" };
  return <svg viewBox="0 0 12 12" width={size} height={size} className={className} aria-hidden="true">
    {kind === "calendar" && <circle cx="6" cy="6" r={hollow ? 4.6 : 5} {...paint} />}
    {kind === "email" && <rect x="2.2" y="2.2" width="7.6" height="7.6" transform="rotate(45 6 6)" {...paint} />}
    {kind === "teams_chat" && <path d="M6 1.2 11 10.4H1z" {...paint} strokeLinejoin="round" />}
    {kind === "manual" && <path d="M4.4 1h3.2v3.4H11v3.2H7.6V11H4.4V7.6H1V4.4h3.4z" {...paint} />}
  </svg>;
}

export const SOURCE_ORDER: SourceKind[] = ["calendar", "email", "teams_chat", "manual"];
