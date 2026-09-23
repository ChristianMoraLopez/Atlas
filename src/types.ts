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
export type AppRole = "cs" | "manager";

export type TrackerDestinationKind = "local_existing" | "local_new" | "share_point" | "share_point_flow";
export type AutomationMode = "startup_previous_workday" | "daily_time";

export interface TrackerDestination {
  kind: TrackerDestinationKind;
  value: string;
  localPath?: string;
  writerPackagePath?: string;
}

export interface AiInstructions {
  presets: string[];
  custom: string;
}

export interface AiPresetInfo {
  id: string;
  rule: string;
}

export interface AiSubmission {
  sourceLabel: string;
  createdAt: string;
  evidenceLines: string[];
  userRules: string[];
}

export interface AiPromptInfo {
  coreTemplate: string;
  presets: AiPresetInfo[];
  instructions: AiInstructions;
  activeRules: string[];
  lastSubmission?: AiSubmission;
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
  language: string;
  aiInstructions: AiInstructions;
  scheduledLaunch: boolean;
  logPath: string;
  appRole?: AppRole;
  qaBridgeFolder?: string;
  solutionOwner?: string;
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

// ---------------------------------------------------------------------------
// QA Audit module types
// ---------------------------------------------------------------------------

export interface QaAuditee {
  name: string;
  email: string;
  customRules: string;
  watched: boolean;
  historicalDone: boolean;
}

export interface QaConfig {
  auditees: QaAuditee[];
  outputFolder?: string;
  lookbackDays: number;
  vertical: string;
  watchEnabled: boolean;
  checkMorning: string;
  checkAfternoon: string;
  subjectKeywords: string[];
  historyMonths: number;
  autoExport: boolean;
}

export type QaHistoricalStatus = "idle" | "running" | "done";

export interface QaEngineStatus {
  source: "power_automate" | "microsoft_graph" | "not_configured";
  connected: boolean;
  historicalStatus: QaHistoricalStatus;
  historicalMonthsDone: number;
  historicalMonthsPlanned: number;
  historicalHorizonMonths: number;
  historicalOldestMonth?: string;
  historicalStartedAt?: string;
  historicalCompletedAt?: string;
  pendingRequestAt?: string;
  queuedWindows: number;
  lastIncrementalAt?: string;
  nextCheck?: string;
  messages: number;
  conversations: number;
  pendingEvaluation: number;
  cases: number;
  needsReview: number;
  reviewed: number;
  newEvidence: number;
  unexported: number;
  activity?: string;
  lastError?: string;
  warnings: string[];
  lastExportAt?: string;
  lastExportError?: string;
  lastLiveSignalAt?: string;
  consecutiveTimeouts: number;
}

export interface QaEvidenceRef {
  messageId: string;
  label: string;
  excerpt: string;
}

export interface QaCase {
  caseId: string;
  analystName: string;
  analystEmail: string;
  auditDate: string;
  requestId: string;
  requestDate: string;
  requestSource: string;
  initialResponse: string;
  initialResponseNotes: string;
  customerSentiment: string;
  customerSentimentNotes: string;
  adherence: string;
  adherenceNotes: string;
  status: string;
  statusNotes: string;
  updateFollowUp: string;
  updateFollowUpNotes: string;
  autoFail: string;
  evidence: QaEvidenceRef[];
  selected: boolean;
  reviewed: boolean;
  newEvidence: boolean;
}

export interface QaCaseEntry extends QaCase {
  exported: boolean;
  lastMessageAt: string;
}

export interface QaExportResult {
  files: string[];
  written: number;
}

export type InstallerPhase = "checking_requirements" | "waiting_sign_in" | "finding_environment" | "finding_connections" | "importing_solution" | "activating_flow" | "verifying_file" | "completed" | "blocked_by_policy";

export interface InstallerSession {
  phase: InstallerPhase;
  installationId: string;
  folder?: string;
  calendarName: string;
  environmentId?: string;
  diagnostic: string;
  lastCheckedAt?: string;
  lastCheckedFile?: string;
  ownerName?: string;
  solutionDisplayName?: string;
}

export interface InstallerSnapshot {
  session: InstallerSession;
  kind: "tracker" | "qa";
  packagePath: string;
  oneDriveRoot?: string;
  pacDetected: boolean;
}
