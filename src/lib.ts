import { invoke } from "@tauri-apps/api/core";
import type { AiPromptInfo, AppStatus, AutomationMode, ExportResult, ExtractionResult, Interaction, QaConfig, QaCase, QaExportResult, QaExtractionResult, QaLastRun, QaWatchStatus, TrackerDestination, UserProfile } from "./types";

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    void invoke("log_frontend_error", { context: command, message: String(error) }).catch(() => undefined);
    throw error;
  }
}

export const api = {
  status: () => call<AppStatus>("get_app_status"),
  savePowerAutomateFolder: (folder: string) => call<AppStatus>("save_power_automate_folder", { folder }),
  signIn: () => call<AppStatus>("sign_in"),
  signOut: () => call<void>("sign_out"),
  saveProfile: (profile: UserProfile) => call<void>("save_profile", { profile }),
  saveAiInstructions: (presets: string[], custom: string) => call<AppStatus>("save_ai_instructions", { presets, custom }),
  saveLanguage: (language: string) => call<AppStatus>("save_language", { language }),
  getAiPromptInfo: () => call<AiPromptInfo>("get_ai_prompt_info"),
  saveDestination: (destination: TrackerDestination, autoSync: boolean, autoSyncTime: string, automationMode: AutomationMode) =>
    call<AppStatus>("save_tracker_destination", { destination, autoSync, autoSyncTime, automationMode }),
  extract: (date: string, includeEmail: boolean, includeTeams: boolean, timezone: string) =>
    call<ExtractionResult>("extract_interactions", { date, includeEmail, includeTeams, timezone }),
  loadCachedDay: (date: string) => call<ExtractionResult | null>("load_cached_day", { date }),
  saveDayPreview: (date: string, interactions: Interaction[], warnings: string[]) =>
    call<void>("save_day_preview", { date, interactions, warnings }),
  requestBridgeDate: (date: string) => call<void>("request_bridge_date", { date }),
  export: (date: string, profile: UserProfile, interactions: Interaction[]) =>
    call<ExportResult>("export_configured_tracker", { date, profile, interactions }),
  openTracker: () => call<void>("open_tracker_destination"),
  showWriterPackage: () => call<void>("show_tracker_writer_package"),
  openPowerAutomate: () => call<void>("open_power_automate_portal"),
  completeScheduled: (runKey: string, successful: boolean, needsAttention: boolean) =>
    call<void>("complete_scheduled_launch", { runKey, successful, needsAttention }),
  backgroundReady: () => call<void>("background_frontend_ready"),
  logError: (context: string, message: string) => invoke<void>("log_frontend_error", { context, message }),
  qaGetConfig: () => call<QaConfig>("qa_get_config"),
  qaSaveConfig: (config: QaConfig) => call<void>("qa_save_config", { config }),
  qaExtract: (historical: boolean, source: string) => call<QaExtractionResult>("qa_extract_cases", { historical, source }),
  qaCheckNewMail: () => call<QaWatchStatus>("qa_check_new_mail"),
  qaExport: (cases: QaCase[]) => call<QaExportResult>("qa_export_cases", { cases }),
  qaOpenFolder: () => call<void>("qa_open_output_folder"),
  qaLoadLastRun: () => call<QaLastRun | null>("qa_load_last_run")
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
