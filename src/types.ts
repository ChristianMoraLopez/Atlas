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

export interface AppStatus {
  configured: boolean;
  signedIn: boolean;
  account?: AccountInfo;
  profile?: UserProfile;
  ollamaRunning: boolean;
  ollamaModelAvailable: boolean;
  ollamaModel: string;
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
}
