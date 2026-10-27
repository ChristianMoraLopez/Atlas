// Injected by scripts/shots.cjs before the app loads: a fake Tauri bridge with
// realistic data, so screens can be rendered in a plain browser.
(() => {
  const q = new URLSearchParams(location.search);
  const screen = q.get("screen") || "workspace";
  const day = "2026-10-22";
  const at = (h, m) => new Date(`${day}T${String(h).padStart(2,"0")}:${String(m).padStart(2,"0")}:00-05:00`).toISOString();
  const row = (kind, id, type, h, m, dur, comments, label, extra = {}) => ({
    sourceKind: kind, sourceId: `${kind}:${id}`, interactionType: type,
    receptionDateTime: at(h, m), interactionDateTime: at(h, m), resolutionDateTime: at(h + Math.floor((m + dur) / 60), (m + dur) % 60),
    clientType: "Circana", endClient: "Circana", status: "Resolved", resolutionType: "Processed & Resolved",
    category: type === "Meeting" ? "Meeting" : "UDB", subcategory: type === "Meeting" ? "T4 Meetings" : "Rules Validation",
    priority: "Intermediate", incidentNumber: "N/A", comments, selected: true, reviewed: true,
    manualAuthored: kind === "manual", aiSuggested: kind === "teams_chat" || kind === "email", evidenceLabel: label, ...extra });
  const items = [
    row("calendar", 1, "Meeting", 9, 45, 15, "", "T4_Daily_BHW_Opella/Gojo/Honest/Maesa/TetraPack_ManService&AnomalyDetection"),
    row("calendar", 2, "Meeting", 10, 0, 30, "", "Recurring Meetings - CI/CS Tetra Pak Team", { subcategory: "Weekly Team Meeting" }),
    row("email", 3, "Task", 11, 12, 30, "Reviewed the UF / NUF brand list Arthur flagged for Tetra Pak.", "AI review · Arthur K · FSN Ultra Filtered", { category: "UDB", subcategory: "NIR / New Products" }),
    row("teams_chat", 4, "Task", 12, 28, 30, "Christian Mora found UPCs for Private label items.", "AI review · Joan Garzon"),
    row("calendar", 5, "Meeting", 14, 45, 15, "", "TetraPak Touchbase - Query", { subcategory: "Circana Client Touchbase" }),
    row("teams_chat", 6, "Task", 15, 1, 30, "Jenn Schaefer queries the Emiri Query Welcome Page update.", "AI review · Jenn Schaefer", { category: "Unify_Request", subcategory: "Update Landing Page", selected: false, reviewed: false }),
    row("email", 7, "Task", 16, 17, 30, "Christian Mora finished reviewing items and will share the Oatly 32oz logic.", "AI review · Navneet Parmar", { status: "In Progress", resolutionDateTime: undefined }),
  ];
  const status = {
    configured: true, signedIn: true, sourceMode: "power_automate_folder",
    bridgeFolder: "C:\\Users\\CNmora01\\OneDrive - Circana\\AtlasBridge\\inbox",
    profile: { loginId: "CMORALOP", fullName: "Christian Rey Mora López", area: "Manufacturing", teamLead: "Bahar Hajian", circanaManager: "Navneet Parmar" },
    ollamaRunning: true, ollamaModelAvailable: true, ollamaModel: "qwen2.5:1.5b-instruct-q4_K_M",
    destination: { kind: "local_existing", value: "C:\\Users\\CNmora01\\OneDrive - Circana\\Tracker\\Interactions 2026.xlsm" },
    autoSync: true, autoSyncTime: "09:30", automationMode: "startup_previous_workday", language: q.get("lang") || "es",
    aiInstructions: { presets: [], custom: "" }, scheduledLaunch: false, logPath: "", appRole: screen === "role" ? undefined : "cs",
    connectorUpdateAvailable: false };
  if (screen === "profile") delete status.profile;
  if (screen === "destination") delete status.destination;
  const catalog = { interactions: ["Meeting", "Task"], clientTypes: ["Circana", "End_Client", "Capgemini", "Others"], clients: { Circana: ["Circana"], End_Client: ["Tetra Pak", "Honest", "Gojo"] },
    categories: [{ name: "Meeting", subcategories: ["T4 Meetings", "Weekly Team Meeting", "Circana Client Touchbase"] }, { name: "UDB", subcategories: ["Rules Validation", "NIR / New Products"] }, { name: "Unify_Request", subcategories: ["Update Landing Page"] }],
    resolutionTypes: [], statuses: ["Resolved", "In Progress"], priorities: ["Low", "Intermediate", "High"], source: "workbook" };
  let callbacks = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback: () => ++callbacks,
    unregisterCallback: () => {},
    invoke: async (cmd, args) => {
      switch (cmd) {
        case "get_app_status": return status;
        case "get_tracker_catalog": return catalog;
        case "load_cached_day": return q.get("empty") ? null : { interactions: items, warnings: [] };
        case "plugin:event|listen": return ++callbacks;
        default: return null;
      }
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
