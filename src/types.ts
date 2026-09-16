export type SourceKind = "calendar" | "email" | "teams_chat" | "manual";

export interface UserProfile {
  loginId: string;
  fullName: string;
  area: string;
  teamLead: string;
  circanaManager: string;
}

export interface AccountInfo {
  displayName: string;
  email: string;
}

export type SourceMode = "microsoft_graph" | "power_automate_folder";

export type TrackerDestinationKind = "local_existing" | "local_new" | "share_point" | "share_point_flow";
export type AutomationMode = "startup_previous_workday" | "daily_time";

export interface TrackerDestination {
  kind: TrackerDestinationKind;
  value: string;
  localPath?: string;
  writerPackagePath?: string;
}

export interface AppStatus {
  configured: boolean;
  signedIn: boolean;
  sourceMode: SourceMode;
  bridgeFolder?: string;
  account?: AccountInfo;
  profile?: UserProfile;
  ollamaRunning: boolean;
  ollamaModelAvailable: boolean;
  ollamaModel: string;
  localAiError?: string;
  destination?: TrackerDestination;
  autoSync: boolean;
  autoSyncTime: string;
  automationMode: AutomationMode;
  scheduledLaunch: boolean;
  logPath: string;
}

export interface Interaction {
  sourceKind: SourceKind;
  sourceId: string;
  interactionType: "Meeting" | "E-Mail" | "Task";
  receptionDateTime: string;
  interactionDateTime: string;
  resolutionDateTime?: string;
  clientType: "Circana" | "End_Client" | "Capgemini" | "";
  endClient: string;
  status: "Resolved" | "In Progress";
  resolutionType: "Processed & Resolved" | "";
  category: string;
  subcategory: string;
  priority: "Low" | "Intermediate" | "High";
  incidentNumber: string;
  comments: string;
  selected: boolean;
  reviewed: boolean;
  manualAuthored: boolean;
  aiSuggested: boolean;
  evidenceLabel: string;
}

export interface ExtractionResult {
  interactions: Interaction[];
  warnings: string[];
}

export interface ExportResult {
  path: string;
  inserted: number;
  updated: number;
  skipped: number;
  queued: boolean;
}

export interface AutomationRequest {
  date: string;
  runKey: string;
  reason: string;
}
