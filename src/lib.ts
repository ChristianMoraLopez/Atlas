import { invoke } from "@tauri-apps/api/core";
import type { AppStatus, ExportResult, ExtractionResult, Interaction, UserProfile } from "./types";

export const api = {
  status: () => invoke<AppStatus>("get_app_status"),
  signIn: () => invoke<AppStatus>("sign_in"),
  signOut: () => invoke<void>("sign_out"),
  saveProfile: (profile: UserProfile) => invoke<void>("save_profile", { profile }),
  setOllamaModel: (model: string) => invoke<void>("set_ollama_model", { model }),
  extract: (date: string, includeEmail: boolean, includeTeams: boolean, timezone: string) =>
    invoke<ExtractionResult>("extract_interactions", { date, includeEmail, includeTeams, timezone }),
  export: (path: string, existing: boolean, date: string, profile: UserProfile, interactions: Interaction[]) =>
    invoke<ExportResult>("export_tracker", { path, existing, date, profile, interactions })
};

export function localDate(): string {
  const now = new Date();
  return new Date(now.getTime() - now.getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
}

export function formatDateTime(value?: string): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

export function toLocalInput(value?: string): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value.slice(0, 16);
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

export function fromLocalInput(value: string): string {
  return value ? new Date(value).toISOString() : "";
}
