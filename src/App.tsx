import { useEffect, useMemo, useRef, useState } from "react";
import ConnectorInstaller from "./ConnectorInstaller";
import AtlasIntro from "./AtlasIntro";
import AtlasLoader from "./AtlasLoader";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "motion/react";
import TopoMap from "./design/TopoMap";
import { AccentPicker, DayHero, PersonalSeal } from "./design/Personal";
import { seedFrom } from "./design/identity";
import {
  AlertCircle, ArrowRight, Bot, CalendarDays, CalendarRange, Check, ChevronDown, Cloud, Columns3,
  ExternalLink, FilePlus2, FileSpreadsheet, FileText, FolderOpen, HardDrive, Home,
  Inbox, ListFilter, Loader2, LogOut, Mail, PenLine, Plus, RefreshCw, Save, Search, Settings,
  ShieldCheck, Sparkles, Trash2, UserRound, X
} from "lucide-react";
import { api, fromLocalInput, localDate, toLocalInput, workdaysBetween } from "./lib";
import AiTransparencyModal from "./AiTransparencyModal";
import LottieIcon, { type LottieName } from "./LottieIcon";
import QaPanel from "./QaPanel";
import { I18nProvider, useI18n, useT } from "./i18n";
import type { AppRole, AppStatus, AutomationMode, AutomationRequest, ExportResult, ExtractionResult, Interaction, TrackerCatalog, TrackerDestination, TrackerDestinationKind, UserProfile } from "./types";

const emptyProfile: UserProfile = {
  loginId: "", fullName: "", area: "Manufacturing", teamLead: "", circanaManager: ""
};

function ErrorBanner({ message, onClose }: { message: string; onClose?: () => void }) {
  return <div className="flex items-start gap-3 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-800">
    <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
    <span className="flex-1">{message}</span>
    {onClose && <button onClick={onClose}><X className="h-4 w-4" /></button>}
  </div>;
}

function SolutionUpdateBanner({ role, onDone }: { role: AppRole; onDone: () => Promise<void> }) {
  const t = useT();
  const [error, setError] = useState("");
  const run = (operation: () => Promise<unknown>) => { setError(""); void operation().catch(failure => setError(String(failure))); };
  return <div className="rise mb-6 rounded-2xl border border-[#e89969]/40 bg-[#fff0df] px-5 py-4 text-sm text-[#7a4516]">
    <div className="flex flex-wrap items-center gap-4">
      <RefreshCw className="h-5 w-5 shrink-0" />
      <span className="min-w-0 flex-1"><b className="block">{t("Your Power Automate solution has an update")}</b><span className="text-xs">{t("Atlas prepared a new version of your personal solution. Import the ZIP in Power Automate (Solutions → Import solution): it updates the same solution and keeps your connections.")}</span></span>
      <button className="btn-secondary" onClick={() => run(() => api.installer.showPackage(role))}><FolderOpen className="h-4 w-4" />{t("Show ZIP")}</button>
      <button className="btn-secondary" onClick={() => run(() => api.installer.openPortal(role))}><ExternalLink className="h-4 w-4" />{t("Open Power Automate")}</button>
      <button className="btn-primary" onClick={() => run(async () => { await api.installer.action(role, { kind: "acknowledge_update" }); await onDone(); })}><Check className="h-4 w-4" />{t("I imported it")}</button>
    </div>
    {error && <p className="mt-2 text-xs text-red-700">{error}</p>}
  </div>;
}

function LoadingScreen() {
  const t = useT();
  return <main className="min-h-screen" aria-busy="true">
    <AtlasLoader show message={t("Preparing your workspace")} detail={t("Starting Atlas Local AI and loading your tracker settings.")} mode="screen" delay={0} />
  </main>;
}

function Brand() {
  const t = useT();
  return <div className="flex items-center gap-3">
    <svg viewBox="0 0 120 120" className="h-10 w-10" aria-hidden="true"><circle cx="60" cy="60" r="54" fill="rgb(var(--atlas-accent))" /><circle cx="60" cy="60" r="44" fill="none" stroke="rgb(var(--atlas-glow) / .45)" strokeDasharray="2 6" /><path d="M33 88 57 36c1.5-3.4 4.5-3.4 6 0l24 52M42 74h36" fill="none" stroke="#fff" strokeWidth="6" strokeLinecap="round" strokeLinejoin="round" /><circle cx="60" cy="27" r="5" fill="rgb(var(--atlas-signal))" /></svg>
    <div><p className="font-display text-lg leading-none">Atlas</p><p className="mt-1 text-[10px] font-bold uppercase tracking-[.18em] text-ink/45">{t("Interaction tracker")}</p></div>
  </div>;
}

function LoginScreen({ onSignIn, onEditMicrosoft, busy, error }: { onSignIn: () => void; onEditMicrosoft: () => void; busy: boolean; error: string }) {
  const t = useT();
  return <main className="mx-auto flex min-h-screen max-w-6xl items-center px-10 py-12" aria-busy={busy}>
    <section className="grid w-full grid-cols-[1.05fr_.95fr] overflow-hidden rounded-[2rem] bg-night shadow-panel">
      <div className="relative overflow-hidden p-14 text-white">
        <div className="pointer-events-none absolute inset-0 opacity-90 [mask-image:linear-gradient(160deg,transparent_15%,#000_70%)]"><TopoMap seed={seedFrom("Atlas")} variant="night" step={11} summits /></div>
        <div className="relative"><Brand /></div>
        <div className="relative mt-24">
          <p className="mb-4 text-xs font-bold uppercase tracking-[.2em] text-glow">{t("Your work, accounted for")}</p>
          <h1 className="font-display text-6xl leading-[1.02]">{t("Real interactions.")}<br /><span className="italic text-glow">{t("Nothing invented.")}</span></h1>
          <p className="mt-7 max-w-lg text-base leading-7 text-white/60">{t("Build the Circana tracker from meetings and mail already in your Microsoft 365 account, then review every row before it reaches Excel.")}</p>
        </div>
        <div className="relative mt-16 flex items-center gap-3 text-xs font-semibold text-white/55"><ShieldCheck className="h-5 w-5 text-glow" /> {t("Delegated access · Secure token storage · Local AI only")}</div>
      </div>
      <div className="m-4 flex flex-col justify-center rounded-[1.4rem] bg-white p-12 text-ink">
        <div className="grid h-12 w-12 place-items-center rounded-xl bg-[#eef6ff] text-[#2563a8]"><UserRound className="h-6 w-6" /></div>
        <h2 className="mt-7 font-display text-3xl">{t("Welcome to Atlas")}</h2>
        <p className="mt-3 text-sm leading-6 text-ink/55">{t("Sign in with your company Microsoft account. Atlas can only read your delegated calendar and mail data.")}</p>
        {error && <div className="mt-6"><ErrorBanner message={error} /></div>}
        <button className="btn-primary mt-8 h-12" onClick={onSignIn} disabled={busy}>
          {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <svg viewBox="0 0 24 24" className="h-4 w-4" aria-hidden><path fill="#f35325" d="M1 1h10v10H1z"/><path fill="#81bc06" d="M13 1h10v10H13z"/><path fill="#05a6f0" d="M1 13h10v10H1z"/><path fill="#ffba08" d="M13 13h10v10H13z"/></svg>}
          {t("Sign in with Microsoft")} <ArrowRight className="h-4 w-4" />
        </button>
        <button className="mt-4 text-xs font-bold text-pine hover:underline" onClick={onEditMicrosoft} disabled={busy}>{t("Use the Power Automate connector")}</button>
        <p className="mt-5 text-center text-[11px] leading-5 text-ink/40">{t("Your refresh token is stored by Windows Credential Manager and is never written to a project file.")}</p>
      </div>
    </section>
    <AtlasLoader show={busy} message={t("Connecting your Microsoft account")} detail={t("Complete any sign-in or company verification shown by Microsoft.")} />
  </main>;
}

function ProfileForm({ initial, onSave, title, cancel }: { initial?: UserProfile; onSave: (p: UserProfile) => Promise<void>; title?: string; cancel?: () => void }) {
  const t = useT();
  const [profile, setProfile] = useState(initial ?? emptyProfile);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const update = (key: keyof UserProfile, value: string) => setProfile(p => ({ ...p, [key]: value }));
  const submit = async (e: React.FormEvent) => {
    e.preventDefault(); setError("");
    if (!profile.loginId.trim() || !profile.fullName.trim()) return setError(t("Login ID and full name are required."));
    setBusy(true); try { await onSave(profile); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <form onSubmit={submit} className="card w-full max-w-2xl p-8" aria-busy={busy}>
    <div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.18em] text-pine">{t("Personal defaults")}</p><h2 className="mt-2 font-display text-3xl">{title ?? t("Set up your profile")}</h2><p className="mt-2 text-sm text-ink/50">{t("Used only in fields Atlas is allowed to write.")}</p></div>{cancel && <button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={cancel}><X /></button>}</div>
    <div className="mt-8 grid grid-cols-2 gap-5">
      <label><span className="label">{t("Corp ID / Login ID *")}</span><input className="field" value={profile.loginId} onChange={e => update("loginId", e.target.value)} /></label>
      <label><span className="label">{t("Full name *")}</span><input className="field" value={profile.fullName} onChange={e => update("fullName", e.target.value)} /></label>
      <label><span className="label">{t("Area")}</span><input className="field" value={profile.area} onChange={e => update("area", e.target.value)} /></label>
      <label><span className="label">{t("Team lead")}</span><input className="field" value={profile.teamLead} onChange={e => update("teamLead", e.target.value)} /></label>
      <label className="col-span-2"><span className="label">{t("Circana manager")}</span><input className="field" value={profile.circanaManager} onChange={e => update("circanaManager", e.target.value)} /></label>
    </div>
    {error && <div className="mt-5"><ErrorBanner message={error} /></div>}
    <div className="mt-7 flex justify-end gap-3">{cancel && <button type="button" className="btn-secondary" onClick={cancel}>{t("Cancel")}</button>}<button className="btn-primary" disabled={busy}>{busy && <Loader2 className="h-4 w-4 animate-spin" />}{t("Save profile")}</button></div>
    <AtlasLoader show={busy} message={t("Saving your Atlas profile")} detail={t("Applying the defaults used in your tracker rows.")} />
  </form>;
}

function SetupScreen({ status, onSaved }: { status: AppStatus; onSaved: (p: UserProfile) => Promise<void> }) {
  const t = useT();
  const sourceLabel = status.sourceMode === "power_automate_folder" ? "Power Automate inbox" : status.account?.email;
  return <main className="min-h-screen px-10 py-8"><header className="mx-auto flex max-w-6xl items-center justify-between"><Brand /><span className="chip bg-mint text-pine"><Check className="h-3 w-3" /> {sourceLabel}</span></header><div className="mx-auto grid min-h-[calc(100vh-6rem)] max-w-6xl place-items-center"><ProfileForm onSave={onSaved} /></div></main>;
}

function DestinationSetupScreen({ initial, initialAutoSync, initialAutoSyncTime, initialAutomationMode, bridgeMode, onSave, onCancel }: { initial?: TrackerDestination; initialAutoSync: boolean; initialAutoSyncTime: string; initialAutomationMode: AutomationMode; bridgeMode: boolean; onSave: (destination: TrackerDestination, autoSync: boolean, autoSyncTime: string, automationMode: AutomationMode) => Promise<void>; onCancel?: () => void }) {
  const t = useT();
  const [kind, setKind] = useState<TrackerDestinationKind>(initial?.kind ?? "local_existing");
  const [value, setValue] = useState(initial?.value ?? "");
  const [localPath, setLocalPath] = useState(initial?.localPath ?? "");
  const [autoSync, setAutoSync] = useState(initialAutoSync);
  const [autoSyncTime, setAutoSyncTime] = useState(initialAutoSyncTime || "17:30");
  const [automationMode, setAutomationMode] = useState<AutomationMode>(initialAutomationMode);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const changeKind = (next: TrackerDestinationKind) => { setKind(next); setValue(next === initial?.kind ? initial.value : ""); setLocalPath(next === initial?.kind ? initial.localPath ?? "" : ""); setError(""); };
  const chooseLocal = async () => {
    const chosen = kind === "local_existing"
      ? await open({ multiple: false, filters: [{ name: "Excel workbook", extensions: ["xlsx", "xlsm"] }] })
      : await save({ defaultPath: `Tracker_${localDate()}.xlsx`, filters: [{ name: "Excel workbook", extensions: ["xlsx"] }] });
    if (chosen) setValue(chosen as string);
  };
  const chooseSyncedCopy = async () => {
    const chosen = await open({ multiple: false, filters: [{ name: "Synced Excel workbook", extensions: ["xlsx", "xlsm"] }] });
    if (chosen) setLocalPath(chosen as string);
  };
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setError("");
    if (!value.trim()) return setError(kind === "share_point" || kind === "share_point_flow" ? t("Paste the SharePoint workbook link.") : t("Choose an Excel workbook."));
    setBusy(true);
    try { await onSave({ kind, value: value.trim(), localPath: kind === "share_point" && localPath ? localPath : undefined }, autoSync, autoSyncTime, automationMode); }
    catch (failure) { setError(String(failure)); }
    finally { setBusy(false); }
  };
  const options: { kind: TrackerDestinationKind; title: string; detail: string; icon: React.ReactNode }[] = [
    { kind: "local_existing", title: t("Existing Excel tracker"), detail: t("Keep macros, tables and formulas"), icon: <HardDrive className="h-5 w-5" /> },
    { kind: "local_new", title: t("Create a new tracker"), detail: t("Create one compatible .xlsx file"), icon: <FilePlus2 className="h-5 w-5" /> },
    { kind: "share_point", title: t("Synced SharePoint"), detail: bridgeMode ? t("Use the OneDrive-synced copy") : t("Update the online .xlsx or .xlsm directly"), icon: <Cloud className="h-5 w-5" /> },
    ...(bridgeMode ? [{ kind: "share_point_flow" as TrackerDestinationKind, title: t("SharePoint cloud flow"), detail: t("Create the second import ZIP"), icon: <RefreshCw className="h-5 w-5" /> }] : []),
  ];
  return <main className="mx-auto min-h-screen max-w-6xl px-10 py-10" aria-busy={busy}>
    <header className="flex items-center justify-between"><Brand />{onCancel && <button className="btn-secondary" onClick={onCancel}>{t("Cancel")}</button>}</header>
    <form onSubmit={submit} className="card mx-auto mt-10 max-w-4xl p-9">
      <p className="text-xs font-bold uppercase tracking-[.18em] text-pine">{t("One-time tracker setup")}</p>
      <h1 className="mt-2 font-display text-4xl">{t("Where should Atlas write?")}</h1>
      <p className="mt-3 max-w-2xl text-sm leading-6 text-ink/50">{t("Choose a local workbook, a synchronized SharePoint copy, or the optional second cloud flow. Atlas keeps this destination for manual and automatic runs.")}</p>
      <div className={`mt-8 grid gap-3 ${options.length === 4 ? "grid-cols-4" : "grid-cols-3"}`}>{options.map(option => <button type="button" key={option.kind} onClick={() => changeKind(option.kind)} className={`rounded-2xl border p-4 text-left ${kind === option.kind ? "border-pine/35 bg-mint/55" : "border-ink/10 bg-white"}`}><span className={`grid h-10 w-10 place-items-center rounded-xl ${kind === option.kind ? "bg-pine text-white" : "bg-cream text-ink/45"}`}>{option.icon}</span><b className="mt-4 block text-sm">{option.title}</b><span className="mt-1 block text-[11px] leading-5 text-ink/45">{option.detail}</span></button>)}</div>
      <div className="mt-7">{kind === "share_point" || kind === "share_point_flow" ? <div><label><span className="label">{t("SharePoint or OneDrive workbook link")}</span><textarea className="field min-h-24 resize-none font-mono text-xs" value={value} onChange={event => setValue(event.target.value)} placeholder="https://tenant-my.sharepoint.com/:x:/r/.../Tracker.xlsm?web=1" /></label>{kind === "share_point_flow" ? <div className="mt-4 rounded-2xl border border-pine/15 bg-mint/25 p-5"><p className="text-sm font-bold">{t("Second Power Automate solution")}</p><p className="mt-2 text-xs leading-5 text-ink/55">{t("Saving creates a personalized AtlasTrackerWriter ZIP and places the required Office Script in your synchronized OneDrive. Import that ZIP once and map OneDrive, SharePoint, and Excel with your Circana account.")}</p><p className="mt-2 text-[11px] leading-5 text-ink/45">{t("The tenant must allow Office Scripts. Atlas queues one JSON package at the chosen daily time; the flow retries safely using each row’s hidden source ID.")}</p></div> : bridgeMode ? <div className="mt-4 rounded-2xl border border-pine/15 bg-mint/25 p-5"><div className="flex flex-wrap gap-3"><button type="button" className="btn-secondary" disabled={!value.trim()} onClick={() => void api.openExternal(value.trim()).catch(failure => setError(String(failure)))}><ExternalLink className="h-4 w-4" />{t("Open link")}</button><button type="button" className="btn-primary" onClick={chooseSyncedCopy}><FolderOpen className="h-4 w-4" />{localPath ? t("Change synced copy") : t("Select synced copy")}</button></div>{localPath && <p className="mt-3 break-all rounded-xl bg-white/70 p-3 text-xs text-ink/55">{localPath}</p>}<ol className="mt-4 list-decimal space-y-2 pl-5 text-xs leading-5 text-ink/60"><li>{t('El propietario comparte la carpeta del tracker contigo y permite editar.')}</li><li>{t('En OneDrive, selecciona la carpeta y pulsa Agregar acceso directo a Mis archivos.')}</li><li>{t('Espera a que aparezca bajo OneDrive – Circana y elige el archivo exacto aquí.')}</li></ol></div> : <span className="mt-2 block text-[11px] leading-5 text-ink/45">Saving opens Microsoft once for delegated file consent.</span>}</div> : <div><button type="button" className="btn-secondary" onClick={chooseLocal}><FolderOpen className="h-4 w-4" />{value ? "Change workbook" : "Choose workbook"}</button>{value && <p className="mt-3 break-all rounded-xl bg-cream p-3 text-xs text-ink/55">{value}</p>}</div>}</div>
      <div className="mt-7 rounded-2xl border border-pine/15 bg-mint/35 p-4"><label className="flex items-start gap-4"><input type="checkbox" checked={autoSync} onChange={event => setAutoSync(event.target.checked)} className="mt-1 h-4 w-4 accent-pine" /><span><b className="block text-sm">{t("Automatic tracker")}</b><span className="mt-1 block text-xs leading-5 text-ink/50">{t("Runs under your Windows account without opening the main window. Atlas stays in the notification area and appears only when the run fails or fewer than three activities need attention.")}</span></span></label>{autoSync && <div className="mt-4 grid grid-cols-2 gap-3"><button type="button" onClick={() => setAutomationMode("startup_previous_workday")} className={`rounded-xl border p-4 text-left ${automationMode === "startup_previous_workday" ? "border-pine/35 bg-white" : "border-ink/10 bg-white/45"}`}><b className="block text-sm">{t("When Windows starts")}</b><span className="mt-1 block text-[11px] leading-5 text-ink/50">{t("Recommended. Processes the previous business day after sign-in, or at 09:30 if the PC stayed on.")}</span></button><button type="button" onClick={() => setAutomationMode("daily_time")} className={`rounded-xl border p-4 text-left ${automationMode === "daily_time" ? "border-pine/35 bg-white" : "border-ink/10 bg-white/45"}`}><b className="block text-sm">{t("At a chosen time")}</b><span className="mt-1 block text-[11px] leading-5 text-ink/50">{t("Keep Atlas hidden and process the current day at the time below.")}</span></button></div>}{autoSync && automationMode === "daily_time" && <label className="mt-4 block w-36"><span className="label">{t("Run at")}</span><input type="time" className="field" value={autoSyncTime} onChange={event => setAutoSyncTime(event.target.value)} /></label>}</div>
      {error && <div className="mt-5"><ErrorBanner message={error} /></div>}
      <div className="mt-7 flex justify-end"><button className="btn-primary" disabled={busy}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}{t("Save tracker setup")}</button></div>
    </form>
    <AtlasLoader
      show={busy}
      message={kind === "share_point_flow" ? t("Preparing your SharePoint flow") : t("Saving your tracker destination")}
      detail={kind === "share_point_flow" ? t("Personalizing the writer solution and its Office Script.") : t("Checking the workbook and scheduling the daily run.")}
    />
  </main>;
}

function Toggle({ checked, onChange, label, detail, icon }: { checked: boolean; onChange: (v: boolean) => void; label: string; detail: string; icon: React.ReactNode }) {
  return <button type="button" onClick={() => onChange(!checked)} className={`flex w-full items-center gap-3 rounded-xl border p-3 text-left ${checked ? "border-pine/25 bg-mint/55" : "border-ink/10 bg-white"}`}>
    <span className={`grid h-9 w-9 place-items-center rounded-lg ${checked ? "bg-pine text-white" : "bg-cream text-ink/45"}`}>{icon}</span>
    <span className="flex-1"><span className="block text-sm font-bold">{label}</span><span className="block text-[11px] text-ink/45">{detail}</span></span>
    <span className={`relative h-5 w-9 rounded-full ${checked ? "bg-pine" : "bg-ink/15"}`}><span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${checked ? "left-[18px]" : "left-0.5"}`} /></span>
  </button>;
}

function SourceBadge({ item }: { item: Interaction }) {
  const t = useT();
  const styles = item.sourceKind === "calendar" ? "bg-[#e7f0ff] text-[#315f9e]" : item.sourceKind === "email" ? "bg-[#f1eaff] text-[#71509d]" : item.sourceKind === "teams_chat" ? "bg-[#fff0df] text-[#9a5a1e]" : "bg-mint text-pine";
  const Icon = item.sourceKind === "calendar" ? CalendarDays : item.sourceKind === "email" ? Mail : item.sourceKind === "teams_chat" ? Sparkles : PenLine;
  return <span className={`chip ${styles}`}><Icon className="h-3 w-3" />{item.sourceKind === "teams_chat" ? (item.aiSuggested ? t("AI summary") : t("Teams evidence")) : t(item.sourceKind.replace("_", " "))}</span>;
}

function SelectCell({ value, options, onChange }: { value: string; options: string[]; onChange: (v: string) => void }) {
  const choices = options.includes(value) || !value ? options : [value, ...options];
  return <div className="relative"><select className={`w-full appearance-none bg-transparent py-1 pr-5 text-xs outline-none ${value ? "" : "text-red-600"}`} value={value} onChange={e => onChange(e.target.value)}>{choices.map(x => <option key={x} value={x}>{x.trim()}</option>)}</select><ChevronDown className="pointer-events-none absolute right-0 top-1.5 h-3 w-3 text-ink/35" /></div>;
}

type ColumnKey = "source" | "type" | "start" | "end" | "status" | "clientType" | "endClient" | "category" | "subcategory" | "priority" | "incident" | "comments";
type RowFilter = "all" | "selected" | "unselected" | "review";

const COLUMNS: { key: ColumnKey; label: string; className?: string }[] = [
  { key: "source", label: "Source" }, { key: "type", label: "Type" }, { key: "start", label: "Start" }, { key: "end", label: "End" },
  { key: "status", label: "Status" }, { key: "clientType", label: "Client type" }, { key: "endClient", label: "End client" },
  { key: "category", label: "Category" }, { key: "subcategory", label: "Subcategory" }, { key: "priority", label: "Priority" },
  { key: "incident", label: "Incident" }, { key: "comments", label: "Comments", className: "w-[300px]" },
];
const HIDDEN_COLUMNS_KEY = "atlas.preview.hiddenColumns";
const SOURCE_FILTERS: { kind: Interaction["sourceKind"] | "all"; label: string }[] = [
  { kind: "all", label: "All" }, { kind: "calendar", label: "Meetings" }, { kind: "email", label: "Mail" }, { kind: "teams_chat", label: "Teams" }, { kind: "manual", label: "Manual" },
];
const ROW_FILTERS: { value: RowFilter; label: string }[] = [
  { value: "all", label: "Every row" }, { value: "selected", label: "Selected" }, { value: "unselected", label: "Not selected" }, { value: "review", label: "To review" },
];

function readHiddenColumns(): ColumnKey[] {
  try { const stored = JSON.parse(localStorage.getItem(HIDDEN_COLUMNS_KEY) ?? "[]"); return Array.isArray(stored) ? stored : []; } catch { return []; }
}

/** What the row is about, in one glance: the evidence title and the first line of its note. */
function rowDescription(item: Interaction) {
  const title = item.evidenceLabel.trim() || item.comments.trim();
  const note = item.comments.trim() && item.comments.trim() !== title ? item.comments.trim() : "";
  return { title, note };
}

function InteractionTable({ items, setItems, catalog }: { items: Interaction[]; setItems: React.Dispatch<React.SetStateAction<Interaction[]>>; catalog?: TrackerCatalog }) {
  const t = useT();
  const subcategories = (category: string) => catalog?.categories.find(entry => entry.name === category)?.subcategories ?? [];
  const clientsFor = (clientType: string) => catalog?.clients[clientType] ?? [];
  const set = (index: number, patch: Partial<Interaction>) => setItems(old => old.map((v, i) => i === index ? { ...v, ...patch } : v));
  const removeManual = (index: number) => setItems(old => old.filter((_, i) => i !== index));
  // Hidden columns, search and filters only change this preview; the export always writes the full tracker format.
  const [hidden, setHidden] = useState<ColumnKey[]>(readHiddenColumns);
  const [columnsOpen, setColumnsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<Interaction["sourceKind"] | "all">("all");
  const [rowFilter, setRowFilter] = useState<RowFilter>("all");
  const toggleColumn = (key: ColumnKey) => setHidden(old => {
    const next = old.includes(key) ? old.filter(value => value !== key) : [...old, key];
    try { localStorage.setItem(HIDDEN_COLUMNS_KEY, JSON.stringify(next)); } catch { /* preview preference only */ }
    return next;
  });
  const shown = (key: ColumnKey) => !hidden.includes(key);
  const visible = useMemo(() => {
    const words = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    return items.map((item, index) => ({ item, index })).filter(({ item }) => {
      if (source !== "all" && item.sourceKind !== source) return false;
      if (rowFilter === "selected" && !item.selected) return false;
      if (rowFilter === "unselected" && item.selected) return false;
      if (rowFilter === "review" && item.reviewed) return false;
      if (!words.length) return true;
      const text = [item.evidenceLabel, item.comments, item.interactionType, item.clientType, item.endClient, item.category, item.subcategory, item.priority, item.incidentNumber, item.status].join(" ").toLocaleLowerCase();
      return words.every(word => text.includes(word));
    });
  }, [items, query, source, rowFilter]);
  const visibleIds = new Set(visible.map(({ item }) => item.sourceId));
  const allVisibleSelected = visible.length > 0 && visible.every(({ item }) => item.selected);
  const selectVisible = (selected: boolean) => setItems(old => old.map(item => visibleIds.has(item.sourceId) ? { ...item, selected, reviewed: selected ? true : item.reviewed } : item));
  const filtered = visible.length !== items.length;
  const clearFilters = () => { setQuery(""); setSource("all"); setRowFilter("all"); };
  return <div className="rounded-xl border border-ink/10 bg-white">
    <div className="flex flex-wrap items-center gap-2 border-b border-ink/10 p-3">
      <label className="flex min-w-[220px] flex-1 items-center gap-2 rounded-lg border border-ink/10 bg-cream/40 px-3 py-2 focus-within:border-pine/40">
        <Search className="h-4 w-4 text-ink/35" />
        <input className="w-full bg-transparent text-xs outline-none" placeholder={t("Search subject, note, client, category…")} value={query} onChange={e => setQuery(e.target.value)} />
        {query && <button type="button" className="text-ink/35 hover:text-ink" title={t("Clear search")} onClick={() => setQuery("")}><X className="h-3.5 w-3.5" /></button>}
      </label>
      <div className="flex rounded-lg border border-ink/10 p-0.5" role="group" aria-label={t("Source")}>
        {SOURCE_FILTERS.map(option => <button key={option.kind} type="button" onClick={() => setSource(option.kind)} className={`rounded-md px-2.5 py-1.5 text-[11px] font-bold ${source === option.kind ? "bg-pine text-white" : "text-ink/55 hover:bg-cream"}`}>{t(option.label)}</button>)}
      </div>
      <label className="relative flex items-center gap-1 rounded-lg border border-ink/10 px-2.5 py-1.5 text-[11px] font-bold text-ink/60">
        <ListFilter className="h-3.5 w-3.5" />
        <select className="appearance-none bg-transparent pr-4 outline-none" value={rowFilter} onChange={e => setRowFilter(e.target.value as RowFilter)}>{ROW_FILTERS.map(option => <option key={option.value} value={option.value}>{t(option.label)}</option>)}</select>
        <ChevronDown className="pointer-events-none absolute right-2 h-3 w-3 text-ink/35" />
      </label>
      <div className="relative">
        <button type="button" className={`flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-[11px] font-bold ${hidden.length ? "border-pine/30 bg-mint/50 text-pine" : "border-ink/10 text-ink/60 hover:bg-cream"}`} onClick={() => setColumnsOpen(open => !open)} aria-expanded={columnsOpen}>
          <Columns3 className="h-3.5 w-3.5" />{hidden.length ? t("Columns · {count} hidden", { count: hidden.length }) : t("Columns")}
        </button>
        {columnsOpen && <>
          <button type="button" aria-label={t("Close")} className="fixed inset-0 z-20 cursor-default" onClick={() => setColumnsOpen(false)} />
          <div className="absolute right-0 z-30 mt-2 w-56 rounded-xl border border-ink/10 bg-white p-2 shadow-xl">
            <p className="px-2 pb-2 pt-1 text-[10px] leading-4 text-ink/45">{t("Hiding a column only changes this preview. The tracker keeps every column.")}</p>
            {COLUMNS.map(column => <label key={column.key} className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-cream/60">
              <input type="checkbox" className="h-3.5 w-3.5 accent-pine" checked={shown(column.key)} onChange={() => toggleColumn(column.key)} />{t(column.label)}
            </label>)}
            {hidden.length > 0 && <button type="button" className="mt-1 w-full rounded-md px-2 py-1.5 text-left text-[11px] font-bold text-pine hover:bg-mint/40" onClick={() => { setHidden([]); try { localStorage.removeItem(HIDDEN_COLUMNS_KEY); } catch { /* preview preference only */ } }}>{t("Show every column")}</button>}
          </div>
        </>}
      </div>
      {filtered && <span className="flex items-center gap-2 rounded-lg bg-[#fff0df] px-2.5 py-1.5 text-[11px] font-bold text-[#9a5a1e]">{t("Showing {shown} of {total}", { shown: visible.length, total: items.length })}<button type="button" className="underline hover:no-underline" onClick={clearFilters}>{t("Clear filters")}</button></span>}
    </div>
    <div className="overflow-auto">
    <table className="w-full border-collapse text-left" style={{ minWidth: `${560 + COLUMNS.filter(column => shown(column.key)).length * 72}px` }}>
      <thead className="sticky top-0 z-10 bg-[#f2f4ef] text-[10px] font-extrabold uppercase tracking-[.13em] text-ink/45"><tr>
        <th className="w-12 px-3 py-3"><input type="checkbox" className="h-4 w-4 accent-pine" title={t("Select the rows shown")} disabled={!visible.length} checked={allVisibleSelected} onChange={e => selectVisible(e.target.checked)} /></th>
        <th className="min-w-[260px] px-3 py-3">{t("Description")}</th>
        {COLUMNS.filter(column => shown(column.key)).map(column => <th key={column.key} className={`px-3 py-3 ${column.className ?? ""}`}>{t(column.label)}</th>)}
        <th className="w-10" />
      </tr></thead>
      <tbody className="divide-y divide-ink/5">
      {visible.map(({ item, index: i }) => {
        const fullyEditable = item.sourceKind === "manual" || item.aiSuggested;
        const { title, note } = rowDescription(item);
        return <tr key={item.sourceId} className={`${item.selected ? "" : "opacity-50"} align-top hover:bg-cream/35`}>
          <td className="px-3 py-3"><input type="checkbox" className="h-4 w-4 accent-pine" checked={item.selected} onChange={e => set(i, { selected: e.target.checked, reviewed: e.target.checked ? true : item.reviewed })} /></td>
          <td className="max-w-[340px] px-3 py-3">
            {item.aiSuggested ? <input className="block w-full bg-transparent text-xs font-bold outline-none" value={item.evidenceLabel} onChange={e => set(i, { evidenceLabel: e.target.value })} /> : <p className="truncate text-xs font-bold" title={title}>{title || "—"}</p>}
            {note && <p className="mt-0.5 line-clamp-2 text-[11px] leading-4 text-ink/50" title={note}>{note}</p>}
            {!shown("source") && <div className="mt-1"><SourceBadge item={item} /></div>}
          </td>
          {shown("source") && <td className="whitespace-nowrap px-3 py-3"><SourceBadge item={item} /></td>}
          {shown("type") && <td className="px-3 py-3">{fullyEditable ? <SelectCell value={item.interactionType} options={catalog?.interactions ?? ["Meeting", "Task"]} onChange={v => set(i, { interactionType: v })} /> : <span className="text-xs">{item.interactionType}</span>}</td>}
          {shown("start") && <td className="px-3 py-3">{fullyEditable ? <input type="datetime-local" className="w-[155px] bg-transparent text-xs outline-none" value={toLocalInput(item.interactionDateTime)} onChange={e => set(i, { interactionDateTime: fromLocalInput(e.target.value), receptionDateTime: fromLocalInput(e.target.value) })} /> : <span className="whitespace-nowrap text-xs">{new Date(item.interactionDateTime).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>}</td>}
          {shown("end") && <td className="px-3 py-3">{fullyEditable ? <input type="datetime-local" className="w-[155px] bg-transparent text-xs outline-none" value={toLocalInput(item.resolutionDateTime)} onChange={e => set(i, { resolutionDateTime: fromLocalInput(e.target.value) })} /> : <span className="whitespace-nowrap text-xs">{item.resolutionDateTime ? new Date(item.resolutionDateTime).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "—"}</span>}</td>}
          {shown("status") && <td className="whitespace-nowrap px-3 py-3"><span className={`chip ${item.status === "Resolved" ? "bg-mint text-pine" : "bg-[#fff0df] text-[#9a5a1e]"}`}>{item.status}</span></td>}
          {shown("clientType") && <td className="px-3 py-3">{fullyEditable ? <SelectCell value={item.clientType} options={catalog?.clientTypes ?? ["Circana", "End_Client", "Capgemini", "Others"]} onChange={v => set(i, { clientType: v, endClient: clientsFor(v)[0] ?? v })} /> : <span className="text-xs">{item.clientType || "—"}</span>}</td>}
          {shown("endClient") && <td className="px-3 py-3">{fullyEditable && clientsFor(item.clientType).length ? <SelectCell value={item.endClient} options={clientsFor(item.clientType)} onChange={v => set(i, { endClient: v })} /> : <span className="text-xs">{item.endClient || "—"}</span>}</td>}
          {shown("category") && <td className="w-32 px-3 py-3">{catalog ? <SelectCell value={item.category} options={catalog.categories.map(entry => entry.name)} onChange={v => set(i, { category: v, subcategory: subcategories(v)[0] ?? v })} /> : <input className="w-28 bg-transparent text-xs outline-none" value={item.category} onChange={e => set(i, { category: e.target.value })} />}</td>}
          {shown("subcategory") && <td className="w-40 px-3 py-3">{catalog ? <SelectCell value={item.subcategory} options={subcategories(item.category)} onChange={v => set(i, { subcategory: v })} /> : <input className="w-28 bg-transparent text-xs outline-none" value={item.subcategory} onChange={e => set(i, { subcategory: e.target.value })} />}</td>}
          {shown("priority") && <td className="px-3 py-3"><SelectCell value={item.priority} options={catalog?.priorities ?? ["Low", "Intermediate", "High"]} onChange={v => set(i, { priority: v })} /></td>}
          {shown("incident") && <td className="px-3 py-3"><input className="w-24 bg-transparent text-xs outline-none" placeholder="N/A" value={item.incidentNumber} onChange={e => set(i, { incidentNumber: e.target.value || "N/A" })} /></td>}
          {shown("comments") && <td className="px-3 py-3"><textarea rows={2} className="w-full resize-none bg-transparent text-xs leading-5 outline-none" placeholder={t("Add a factual note")} value={item.comments} onChange={e => set(i, { comments: e.target.value })} /></td>}
          <td className="px-2 py-3">{item.sourceKind === "manual" && <button title={t("Remove manual entry")} onClick={() => removeManual(i)} className="rounded-md p-1 text-ink/35 hover:bg-red-50 hover:text-red-600"><Trash2 className="h-4 w-4" /></button>}</td>
        </tr>;
      })}
      {!visible.length && <tr><td colSpan={COLUMNS.length + 3} className="px-3 py-10 text-center text-xs text-ink/45">{t("No rows match the search or filters.")}<div className="mt-3"><button type="button" className="btn-secondary px-4 py-2 text-xs" onClick={clearFilters}><X className="h-3.5 w-3.5" />{t("Show all {count} rows", { count: items.length })}</button></div></td></tr>}
      </tbody>
    </table>
    </div>
  </div>;
}

function ManualModal({ date, onClose, onAdd }: { date: string; onClose: () => void; onAdd: (i: Interaction) => void }) {
  const t = useT();
  const [type, setType] = useState<Interaction["interactionType"]>("Task");
  const [start, setStart] = useState(""); const [end, setEnd] = useState(""); const [clientType, setClientType] = useState<Interaction["clientType"]>(""); const [endClient, setEndClient] = useState(""); const [comments, setComments] = useState(""); const [error, setError] = useState("");
  const add = (e: React.FormEvent) => { e.preventDefault(); if (!start || !comments.trim()) return setError(t("Start time and a factual description are required.")); const id = crypto.randomUUID(); const startIso = fromLocalInput(`${date}T${start}`); const endIso = end ? fromLocalInput(`${date}T${end}`) : ""; onAdd({ sourceKind: "manual", sourceId: `manual:${id}`, interactionType: type, receptionDateTime: startIso, interactionDateTime: startIso, resolutionDateTime: endIso, clientType, endClient, status: endIso && new Date(endIso) <= new Date() ? "Resolved" : "In Progress", resolutionType: endIso && new Date(endIso) <= new Date() ? "Processed & Resolved" : "", category: "", subcategory: "", priority: type === "Task" ? "High" : "Intermediate", incidentNumber: "", comments: comments.trim(), selected: true, reviewed: true, manualAuthored: true, aiSuggested: false, evidenceLabel: t("Typed manually by you") }); };
  return <div className="fixed inset-0 z-50 grid place-items-center bg-night/45 p-8 backdrop-blur-sm"><form onSubmit={add} className="card w-full max-w-xl p-7">
    <div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.16em] text-pine">{t("Manual provenance")}</p><h2 className="mt-2 font-display text-3xl">{t("Add a real interaction")}</h2><p className="mt-2 text-sm text-ink/50">{t("Only enter work that actually happened. Atlas does not fill these fields for you.")}</p></div><button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={onClose}><X /></button></div>
    <div className="mt-7 grid grid-cols-2 gap-4">
      <label><span className="label">{t("Interaction type")}</span><select className="field" value={type} onChange={e => setType(e.target.value as Interaction["interactionType"])}><option>Task</option><option>Meeting</option></select></label>
      <label><span className="label">{t("Date")}</span><input className="field bg-cream/70" value={date} disabled /></label>
      <label><span className="label">{t("Start time *")}</span><input type="time" className="field" value={start} onChange={e => setStart(e.target.value)} /></label>
      <label><span className="label">{t("End time")}</span><input type="time" className="field" value={end} onChange={e => setEnd(e.target.value)} /></label>
      <label><span className="label">{t("Client type")}</span><select className="field" value={clientType} onChange={e => setClientType(e.target.value as Interaction["clientType"])}><option value="">{t("Undetermined")}</option><option>Circana</option><option>End_Client</option><option>Capgemini</option></select></label>
      <label><span className="label">{t("Real client name")}</span><input className="field" value={endClient} onChange={e => setEndClient(e.target.value)} /></label>
      <label className="col-span-2"><span className="label">{t("What happened? *")}</span><textarea className="field min-h-24 resize-none" value={comments} onChange={e => setComments(e.target.value)} placeholder={t("Describe the real work task in English")} /></label>
    </div>
    {error && <div className="mt-4"><ErrorBanner message={error} /></div>}
    <div className="mt-6 flex justify-end gap-3"><button type="button" className="btn-secondary" onClick={onClose}>{t("Cancel")}</button><button className="btn-primary"><Plus className="h-4 w-4" />{t("Add interaction")}</button></div>
  </form></div>;
}

function ExportPanel({ destination, autoSync, autoSyncTime, automationMode, bridgeMode, items, busy, onSave, onSync, onEdit }: { destination: TrackerDestination; autoSync: boolean; autoSyncTime: string; automationMode: AutomationMode; bridgeMode: boolean; items: Interaction[]; busy: boolean; onSave: () => void; onSync: () => void; onEdit: () => void }) {
  const t = useT();
  const isRemote = destination.kind === "share_point" || destination.kind === "share_point_flow";
  const writerFlow = destination.kind === "share_point_flow";
  return <aside className="card h-fit p-5"><div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.16em] text-pine">{t("Tracker")}</p><h3 className="mt-2 font-display text-2xl">{t("Configured once")}</h3></div><span className={`grid h-9 w-9 place-items-center rounded-xl ${isRemote ? "bg-[#e7f0ff] text-[#315f9e]" : "bg-mint text-pine"}`}>{isRemote ? <Cloud className="h-4 w-4" /> : <FileSpreadsheet className="h-4 w-4" />}</span></div>
    <p className="mt-3 text-xs leading-5 text-ink/45">{t("Only selected, verified rows are written. Existing macros, formulas, and other worksheets are preserved.")}</p>
    <p className="mt-4 line-clamp-4 break-all rounded-xl bg-cream p-3 text-[10px] leading-4 text-ink/55">{destination.value}</p>
    {writerFlow && <div className="mt-3 rounded-xl border border-pine/15 bg-mint/35 p-3"><b className="text-xs">{t("Import connector 2 once")}</b><p className="mt-1 text-[10px] leading-4 text-ink/50">{t("Map OneDrive, SharePoint and Excel connections, then leave the flow active.")}</p><div className="mt-3 flex gap-2"><button className="btn-secondary flex-1 px-3 py-2 text-[10px]" onClick={() => void api.showWriterPackage().catch(failure => window.alert(String(failure)))}><FolderOpen className="h-3 w-3" />{t("Show ZIP")}</button><button className="btn-secondary flex-1 px-3 py-2 text-[10px]" onClick={() => void api.openPowerAutomate().catch(failure => window.alert(String(failure)))}><ExternalLink className="h-3 w-3" />{t("Import")}</button></div></div>}
    <div className="mt-3 flex items-center justify-between"><span className={`chip ${autoSync ? "bg-mint text-pine" : "bg-cream text-ink/50"}`}>{autoSync ? (automationMode === "startup_previous_workday" ? t("Previous day · startup or 09:30") : `Daily · ${autoSyncTime}`) : t("Automatic run off")}</span><button className="text-[11px] font-bold text-pine hover:underline" onClick={onEdit}>{t("Change setup")}</button></div>
    <button className="btn-primary mt-5 w-full" onClick={onSync} disabled={busy}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}{bridgeMode ? t("Import today’s inbox") : t("Sync today’s calendar")}</button>
    <button className="btn-secondary mt-2 w-full" onClick={onSave} disabled={busy || !items.some(item => item.selected)}><Save className="h-4 w-4" />{t("Save selected preview")}</button>
  </aside>;
}

function SuccessModal({ result, onClose }: { result: ExportResult; onClose: () => void }) {
  const t = useT();
  const remote = result.path.startsWith("https://");
  return <div className="fixed inset-0 z-50 grid place-items-center bg-night/45 p-8 backdrop-blur-sm"><div className="card pop w-full max-w-lg p-8 text-center"><div className="lottie-frame lottie-frame--round mx-auto h-24 w-24"><LottieIcon name="trophy" loop={false} className="h-20 w-20" /></div><h2 className="mt-5 font-display text-3xl">{result.queued ? t("Sent to SharePoint queue") : t("Tracker saved")}</h2><p className="mt-2 text-sm text-ink/50">{result.queued ? t("{count} rows are ready for the cloud writer", { count: result.inserted }) : t("{added} added · {updated} refreshed · {skipped} skipped", { added: result.inserted, updated: result.updated, skipped: result.skipped })}</p><p className="mt-5 break-all rounded-xl bg-cream p-3 text-xs text-ink/55">{result.path}</p>{remote ? <button className="btn-primary mt-6 w-full" onClick={() => void api.openTracker().catch(failure => window.alert(String(failure)))}><ExternalLink className="h-4 w-4" />{t("Open in SharePoint")}</button> : <div className="mt-6 grid grid-cols-2 gap-3"><button className="btn-secondary" onClick={() => void api.revealTracker().catch(failure => window.alert(String(failure)))}><FolderOpen className="h-4 w-4" />{t("Open folder")}</button><button className="btn-primary" onClick={() => void api.openTracker().catch(failure => window.alert(String(failure)))}><ExternalLink className="h-4 w-4" />{t("Open file")}</button></div>}<button className="mt-5 text-xs font-bold text-ink/45 hover:text-ink" onClick={onClose}>{t("Back to tracker")}</button></div></div>;
}

type RangeDay = { date: string; found: number; saved: number; missing: number; error?: string };

const MAX_RANGE_DAYS = 31;

function RangeReportModal({ days, stopped, onOpenDay, onClose }: { days: RangeDay[]; stopped: boolean; onOpenDay: (date: string) => void; onClose: () => void }) {
  const { t, lang } = useI18n();
  const label = (date: string) => {
    const text = new Date(`${date}T12:00:00`).toLocaleDateString(lang === "es" ? "es-CO" : "en-US", { weekday: "short", day: "numeric", month: "short" });
    return text.charAt(0).toUpperCase() + text.slice(1);
  };
  const saved = days.reduce((total, day) => total + day.saved, 0);
  return <div className="fixed inset-0 z-50 grid place-items-center bg-night/45 p-8 backdrop-blur-sm"><div className="card pop flex max-h-[85vh] w-full max-w-2xl flex-col p-7">
    <div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.16em] text-pine">{t("Several days")}</p><h2 className="mt-2 font-display text-3xl">{stopped ? t("Import stopped") : t("Days imported")}</h2><p className="mt-2 text-sm text-ink/50">{t("{days} workdays processed · {rows} rows saved in the tracker. Open a day to review it or add what is missing.", { days: days.length, rows: saved })}</p></div><button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={onClose}><X /></button></div>
    <ul className="mt-6 divide-y divide-ink/5 overflow-auto rounded-xl border border-ink/10 bg-white">
      {days.map(day => <li key={day.date}><button type="button" onClick={() => onOpenDay(day.date)} className="flex w-full items-center gap-4 px-4 py-3 text-left hover:bg-cream/50">
        <span className="w-28 shrink-0 text-sm font-bold">{label(day.date)}</span>
        <span className="flex-1 truncate text-xs text-ink/55">{day.error ? day.error : t("{found} found · {saved} saved", { found: day.found, saved: day.saved })}</span>
        {day.error ? <span className="chip bg-red-50 text-red-700">{t("Error")}</span> : day.missing > 0 ? <span className="chip bg-[#fff0df] text-[#9a5a1e]">{t("{count} missing", { count: day.missing })}</span> : <span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{t("Complete")}</span>}
        <ArrowRight className="h-4 w-4 shrink-0 text-ink/30" />
      </button></li>)}
    </ul>
    <div className="mt-6 flex justify-end"><button className="btn-primary" onClick={onClose}>{t("Back to tracker")}</button></div>
  </div></div>;
}

function Workspace({ status, refreshStatus, signOut, editMicrosoft, editDestination, exitRole }: { status: AppStatus; refreshStatus: () => Promise<void>; signOut: () => Promise<void>; editMicrosoft: () => void; editDestination: () => void; exitRole: () => Promise<void> }) {
  const { lang, setLang } = useI18n();
  const t = useT();
  const profile = status.profile!; const destination = status.destination!; const bridgeMode = status.sourceMode === "power_automate_folder"; const [date, setDate] = useState(localDate()); const [includeEmail, setIncludeEmail] = useState(true); const [includeTeams, setIncludeTeams] = useState(true); const [items, setItems] = useState<Interaction[]>([]); const [warnings, setWarnings] = useState<string[]>([]); const [busy, setBusy] = useState(false); const busyRef = useRef(false); const [error, setError] = useState(""); const [syncNotice, setSyncNotice] = useState(""); const [manual, setManual] = useState(false); const [editingProfile, setEditingProfile] = useState(false); const [success, setSuccess] = useState<ExportResult>();
  const [aiPanel, setAiPanel] = useState(false);
  // Multi-day import: rangeEnd set means "from date to rangeEnd".
  const [rangeEnd, setRangeEnd] = useState<string>();
  const [rangeReport, setRangeReport] = useState<{ days: RangeDay[]; stopped: boolean }>();
  const [rangeRunning, setRangeRunning] = useState(false);
  const [rangeStopping, setRangeStopping] = useState(false);
  const stopRange = useRef(false);
  const [catalog, setCatalog] = useState<TrackerCatalog>();
  useEffect(() => { void api.trackerCatalog().then(setCatalog).catch(failure => void api.logError("tracker-catalog", String(failure))); }, [destination.value, destination.localPath]);
  const [workStatus, setWorkStatus] = useState<{ message: string; detail: string }>();
  const counts = useMemo(() => ({ all: items.length, selected: items.filter(i => i.selected).length, review: items.filter(i => !i.reviewed).length }), [items]);
  const startWork = (message: string, detail: string) => { if (busyRef.current) return false; busyRef.current = true; setWorkStatus({ message, detail }); setBusy(true); setError(""); setSyncNotice(""); return true; };
  const endWork = () => { busyRef.current = false; setBusy(false); setWorkStatus(undefined); };
  const completionWarning = (values: Interaction[]) => {
    const count = values.filter(item => item.selected).length;
    if (count >= 3) return "";
    const missing = 3 - count;
    return lang === "es"
      ? `Atlas encontró y guardará ${count} ${count === 1 ? "actividad" : "actividades"}. Añade ${missing} ${missing === 1 ? "tarea real" : "tareas reales"} para llegar al mínimo diario de 3.`
      : `Atlas found and will save ${count} ${count === 1 ? "activity" : "activities"}. Add ${missing} more real ${missing === 1 ? "task" : "tasks"} to reach the daily minimum of 3.`;
  };
  // Bumped whenever a day is opened, so choosing the day already shown reloads it too.
  const [dayOpened, setDayOpened] = useState(0);
  const openDay = (day: string) => { setDate(day); setItems([]); setWarnings([]); setSyncNotice(""); setDayOpened(count => count + 1); };
  useEffect(() => {
    let disposed = false;
    void api.loadCachedDay(date).then(result => {
      if (disposed || !result || busyRef.current) return;
      setItems(result.interactions);
      const warning = completionWarning(result.interactions);
      setWarnings(warning ? [...result.warnings, warning] : result.warnings);
      setSyncNotice(t("Restored the saved Atlas preview for {date}.", { date }));
    }).catch(failure => void api.logError("load-cached-day", String(failure)));
    return () => { disposed = true; };
  }, [date, dayOpened]);
  useEffect(() => {
    if (items.length === 0 || busyRef.current) return;
    const timer = window.setTimeout(() => {
      const sourceWarnings = warnings.filter(value => !value.startsWith("Atlas encontró y guardará") && !value.startsWith("Atlas found and will save"));
      void api.saveDayPreview(date, items, sourceWarnings).catch(failure => void api.logError("save-day-preview", String(failure)));
    }, 500);
    return () => window.clearTimeout(timer);
  }, [date, items, warnings]);
  const loadEvidence = async (targetDate: string, email: boolean, teams: boolean, waitForDelivery = true): Promise<ExtractionResult> => {
    const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    if (!bridgeMode) return api.extract(targetDate, email, teams, timezone);
    await api.requestBridgeDate(targetDate);
    // A manual import waits (with a notice) while Power Automate delivers the day.
    // A background run never blocks the window: it checks once and, if the day is
    // not there yet, throws so the caller can hand control back to the scheduler.
    const maxAttempts = waitForDelivery ? 26 : 0;
    let lastError = "";
    for (let attempt = 0; attempt <= maxAttempts; attempt += 1) {
      try { return await api.extract(targetDate, email, teams, timezone); }
      catch (failure) {
        lastError = String(failure);
        if (!lastError.includes("No se encontró un paquete de evidencia")) throw failure;
        if (attempt === maxAttempts) { if (!waitForDelivery) throw failure; break; }
        if (attempt === 0) setSyncNotice(t("Request sent for {date}. Power Automate processes it on its next cycle and Atlas will check the folder automatically.", { date: targetDate }));
        await new Promise(resolve => window.setTimeout(resolve, 15_000));
      }
    }
    throw new Error(t("Power Automate did not deliver {date} after 6 minutes. Confirm the updated Atlas flow is active. Last result: {error}", { date: targetDate, error: lastError }));
  };
  const extract = async () => { if (!startWork(bridgeMode ? t("Importing evidence for {date}", { date }) : t("Interpreting your workday for {date}", { date }), bridgeMode ? t("Waiting for Power Automate and OneDrive, then separating meetings and work tasks.") : t("Atlas Local AI is reviewing meetings, mail, and Teams evidence on this computer."))) return; setWarnings([]); try { const result = await loadEvidence(date, includeEmail, includeTeams); setItems(result.interactions); const warning = completionWarning(result.interactions); setWarnings(warning ? [...result.warnings, warning] : result.warnings); setSyncNotice(bridgeMode ? t("Evidence for {date} imported and organized by source.", { date }) : ""); } catch (e) { setError(String(e)); } finally { endWork(); } };
  const changeTeams = (enabled: boolean) => setIncludeTeams(enabled);
  const rangeDays = rangeEnd ? workdaysBetween(date, rangeEnd) : [];
  // Each day is imported, kept as its saved preview and written to the tracker,
  // exactly like the automatic daily run; the report links back to every day.
  const importRange = async () => {
    if (rangeDays.length === 0) return setError(t("The selected range has no workdays."));
    if (rangeDays.length > MAX_RANGE_DAYS) return setError(t("Choose at most {count} workdays at a time.", { count: MAX_RANGE_DAYS }));
    if (!startWork(t("Importing {count} workdays", { count: rangeDays.length }), "")) return;
    stopRange.current = false; setRangeStopping(false); setRangeRunning(true); setWarnings([]);
    const report: RangeDay[] = [];
    try {
      for (const [index, day] of rangeDays.entries()) {
        if (stopRange.current) break;
        setWorkStatus({ message: t("Importing day {current} of {total}", { current: index + 1, total: rangeDays.length }), detail: t("{date}: collecting evidence, interpreting work tasks and updating the tracker.", { date: day }) });
        try {
          const result = await loadEvidence(day, includeEmail, includeTeams);
          await api.saveDayPreview(day, result.interactions, result.warnings);
          const selected = result.interactions.filter(item => item.selected).length;
          let saved = 0;
          if (selected > 0) { const written = await api.export(day, profile, result.interactions); saved = written.inserted + written.updated; }
          report.push({ date: day, found: result.interactions.length, saved, missing: Math.max(0, 3 - selected) });
        } catch (failure) {
          report.push({ date: day, found: 0, saved: 0, missing: 3, error: String(failure) });
        }
      }
      if (report.some(day => day.saved > 0)) await refreshStatus();
    } finally {
      endWork(); setRangeRunning(false); setRangeStopping(false);
    }
    if (report.length) {
      setRangeReport({ days: report, stopped: report.length < rangeDays.length });
      openDay(report[report.length - 1].date);
    }
  };
  const saveSelected = async () => {
    const selectedItems = items.filter(i => i.selected);
    if (selectedItems.length === 0) return setError(t("Select at least one real interaction to save."));
    if (!startWork(t("Writing selected activities"), t("Updating the tracker without overwriting unrelated rows or existing work."))) return;
    try { setSuccess(await api.export(date, profile, items)); const warning = completionWarning(items); if (warning) setWarnings(old => [...old.filter(value => !value.startsWith("Atlas encontró y guardará") && !value.startsWith("Atlas found and will save")), warning]); await refreshStatus(); } catch (e) { setError(String(e)); } finally { endWork(); }
  };
  const syncCalendar = async (automatic = false, request?: AutomationRequest) => {
    if (!startWork(automatic ? t("Running your daily Atlas update") : t("Importing today’s work"), t("Collecting today’s evidence, interpreting work tasks, and updating the tracker."))) {
      // Busy with the user's own work: release the run so it is retried later.
      if (request) void api.completeScheduled(request.runKey, false, false).catch(() => undefined);
      return;
    }
    const targetDate = request?.date ?? localDate(); setDate(targetDate); setWarnings([]);
    try {
      const result = await loadEvidence(targetDate, true, true, !automatic);
      setItems(result.interactions);
      const warning = completionWarning(result.interactions);
      setWarnings(warning ? [...result.warnings, warning] : result.warnings);
      const selectedCount = result.interactions.filter(item => item.selected).length;
      if (selectedCount > 0) {
        const saved = await api.export(targetDate, profile, result.interactions);
        if (automatic) setSyncNotice((saved.queued ? t("Automatic daily run queued {count} rows for SharePoint.", { count: saved.inserted }) : t("Automatic daily run complete: {added} added, {updated} refreshed.", { added: saved.inserted, updated: saved.updated })) + (warning ? ` ${warning}` : ""));
        else setSuccess(saved);
        await refreshStatus();
      } else {
        setSyncNotice(t("Atlas found no meetings or work tasks to save. Add them manually if you did work that does not appear in Microsoft 365."));
      }
      if (request) await api.completeScheduled(request.runKey, true, selectedCount < 3);
    } catch (e) {
      const message = String(e);
      // A background run whose day has not reached OneDrive yet is deferred, not
      // failed: release it quietly so the scheduler retries later and the window
      // stays usable instead of locking on the loading screen.
      if (automatic && message.includes("No se encontró un paquete de evidencia")) {
        setSyncNotice(t("Requested {date} from Power Automate. Atlas will import it automatically once the flow delivers it; meanwhile you can keep working.", { date: targetDate }));
        if (request) await api.completeScheduled(request.runKey, false, false).catch(() => undefined);
      } else {
        setError(`${automatic ? t("Automatic daily run failed: ") : ""}${message}`);
        if (request) await api.completeScheduled(request.runKey, false, true).catch(() => undefined);
      }
    } finally { endWork(); }
  };
  const switchLanguage = async () => {
    const next = lang === "es" ? "en" : "es";
    setLang(next);
    try { await api.saveLanguage(next); } catch (failure) { void api.logError("save-language", String(failure)); }
  };
  const recheckLocalAi = async () => {
    if (!startWork(t("Checking Atlas Local AI"), t("Confirming that the bundled service and interpretation model are ready."))) return;
    try { await refreshStatus(); }
    catch (failure) { setError(String(failure)); }
    finally { endWork(); }
  };
  const saveProfile = async (p: UserProfile) => { await api.saveProfile(p); setEditingProfile(false); await refreshStatus(); };
  useEffect(() => {
    // What the backend is reading or interpreting right now, so a long import
    // never looks stuck. Only shown while Atlas is busy.
    const progress = listen<{ key: string; values: [string, string][] }>("atlas-progress", event => {
      if (!busyRef.current) return;
      const detail = t(event.payload.key, Object.fromEntries(event.payload.values.map(([name, value]) => [name, t(value)])));
      setWorkStatus(current => current ? { ...current, detail } : current);
    });
    return () => { void progress.then(stop => stop()); };
  }, [t]);
  useEffect(() => {
    // The backend keeps the rows and retries while Excel holds the tracker open.
    const waiting = listen("atlas-export-waiting-for-excel", () => setWorkStatus({ message: t("Close the tracker in Excel"), detail: t("Atlas saves the activities automatically as soon as Excel releases the file.") }));
    return () => { void waiting.then(stop => stop()); };
  }, [t]);
  useEffect(() => {
    let disposed = false;
    let unlistenBackground: (() => void) | undefined;
    void listen<AutomationRequest>("atlas-background-daily-run", event => { void syncCalendar(true, event.payload); })
      .then(stop => {
        if (disposed) stop();
        else {
          unlistenBackground = stop;
          void api.backgroundReady().catch(error => api.logError("background-ready", String(error)));
        }
      })
      .catch(error => api.logError("background-listener", String(error)));
    return () => { disposed = true; unlistenBackground?.(); };
  }, []);
  return <div className="min-h-screen" aria-busy={busy}>
    <header className="sticky top-0 z-30 border-b border-white/70 bg-cream/80 px-8 py-4 backdrop-blur-xl"><div className="mx-auto flex max-w-[1540px] items-center justify-between"><PersonalSeal name={profile.fullName} caption={t("Interaction tracker")} /><div className="flex items-center gap-2"><div className="mr-3 max-w-xs text-right"><p className="text-xs font-bold">{bridgeMode ? t("Power Automate Inbox") : status.account?.displayName}</p><p className="truncate text-[10px] text-ink/40">{bridgeMode ? status.bridgeFolder : status.account?.email}</p></div><AccentPicker /><button title={t("Switch language")} className="rounded-xl border border-ink/10 bg-white px-2.5 py-2.5 text-[11px] font-extrabold tracking-wide hover:bg-mint" onClick={() => void switchLanguage()}>{lang === "es" ? "ES" : "EN"}</button><button title={t("Tracker destination")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={editDestination}><FileSpreadsheet className="h-4 w-4" /></button><button title={t("Open failure log")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => void api.openLog().catch(failure => setError(String(failure)))}><FileText className="h-4 w-4" /></button><button title={t("Data source")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={editMicrosoft}>{bridgeMode ? <Inbox className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}</button><button title={t("Change role")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => void exitRole()}><Home className="h-4 w-4" /></button><button title={t("Profile settings")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => setEditingProfile(true)}><Settings className="h-4 w-4" /></button>{!bridgeMode && <button title={t("Sign out")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-red-50 hover:text-red-600" onClick={signOut}><LogOut className="h-4 w-4" /></button>}</div></div></header>
    <main className="mx-auto max-w-[1540px] px-8 py-8">
      {status.connectorUpdateAvailable && <SolutionUpdateBanner role="cs" onDone={refreshStatus} />}
      <div className="grid grid-cols-[minmax(0,1fr)_290px] gap-6">
        <section className="min-w-0">
          <DayHero name={profile.fullName} date={date} items={items}><span className="mx-1 h-4 w-px bg-ink/15" /><span className="chip bg-white text-ink/55"><Inbox className="h-3 w-3" />{t("{count} found", { count: counts.all })}</span><span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{t("{count} selected", { count: counts.selected })}</span>{counts.review > 0 && <span className="chip bg-[#fff0df] text-[#9a5a1e]">{t("{count} to review", { count: counts.review })}</span>}</DayHero>
          <div className="card rise rise-1 p-5"><div className="grid grid-cols-[220px_1fr_1fr_auto] items-end gap-4 max-[1360px]:grid-cols-[200px_1fr_1fr]"><div><div className="mb-1.5 flex items-center justify-between"><span className="label mb-0">{rangeEnd ? t("First day") : t("Workday")}</span><button type="button" disabled={busy} className="flex items-center gap-1 text-[11px] font-bold text-pine hover:underline" onClick={() => setRangeEnd(rangeEnd ? undefined : date)}><CalendarRange className="h-3 w-3" />{rangeEnd ? t("One day") : t("Several days")}</button></div><input type="date" max={localDate()} className="field" value={date} disabled={busy} onChange={e => { if (e.target.value) openDay(e.target.value); }} />{rangeEnd && <label className="mt-2 block"><span className="label">{t("Last day")}</span><input type="date" min={date} max={localDate()} className="field" value={rangeEnd} disabled={busy} onChange={e => setRangeEnd(e.target.value)} /></label>}</div><Toggle checked={includeEmail} onChange={setIncludeEmail} label={t("Infer from mail")} detail={t("Work tasks, without inbox noise")} icon={<Mail className="h-4 w-4" />} /><Toggle checked={includeTeams} onChange={changeTeams} label={t("Infer from Teams")} detail={t("Separate task for each activity")} icon={<Bot className="h-4 w-4" />} /><button className="btn-primary h-[46px] px-5 max-[1360px]:col-span-3 max-[1360px]:justify-self-end" disabled={busy || ((includeEmail || includeTeams) && (!status.ollamaRunning || !status.ollamaModelAvailable))} onClick={() => void (rangeEnd ? importRange() : extract())}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : rangeEnd ? <CalendarRange className="h-4 w-4" /> : <RefreshCw className="h-4 w-4" />}{rangeEnd ? t("Import {count} workdays", { count: rangeDays.length }) : bridgeMode ? t("Import selected day") : t("Extract selected day")}</button></div>
            {status.ollamaRunning && status.ollamaModelAvailable && <button type="button" onClick={() => setAiPanel(true)} title={t("View AI prompt")} className="mt-4 flex w-full items-center gap-3 rounded-xl border border-pine/15 bg-mint/55 px-4 py-3 text-left text-xs text-pine hover:bg-mint"><Bot className="h-5 w-5" /><span className="flex-1"><b>{t("Atlas Local AI is ready.")}</b> {t("Mail and Teams evidence stay on this computer and are interpreted by")} <code>{status.ollamaModel}</code>.</span><span className="shrink-0 font-bold underline">{t("View prompt")}</span></button>}
            {(!status.ollamaRunning || !status.ollamaModelAvailable) && <div role="button" tabIndex={0} onClick={() => setAiPanel(true)} onKeyDown={e => { if (e.key === "Enter") setAiPanel(true); }} title={t("View AI prompt")} className="mt-4 flex cursor-pointer items-center gap-3 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-xs text-red-800"><Bot className="h-5 w-5" /><span className="flex-1"><b>{t("Bundled Local AI could not start.")}</b> {status.localAiError ?? t("Run the Atlas installer again to repair it.")}</span><button className="font-bold underline" onClick={e => { e.stopPropagation(); void recheckLocalAi(); }}>{t("Recheck")}</button></div>}
          </div>
          {error && <div className="mt-5"><ErrorBanner message={error} onClose={() => setError("")} /></div>}{syncNotice && <div className="mt-5 flex items-center gap-3 rounded-xl border border-pine/15 bg-mint/55 px-4 py-3 text-sm text-pine"><Check className="h-4 w-4" />{syncNotice}</div>}{warnings.map((w, i) => <div className="mt-3" key={i}><ErrorBanner message={w} /></div>)}
          <div className="rise rise-2 mt-6 flex items-center justify-between"><div><h2 className="font-display text-2xl">{t("Preview")}</h2><p className="mt-1 text-xs text-ink/45">{t("Atlas selects finished meetings and concrete work tasks, including tasks still in progress. Reminders and automatic notices stay out.")}</p></div><button className="btn-secondary" onClick={() => setManual(true)}><Plus className="h-4 w-4" />{t("Add manual task")}</button></div>
          <div className="mt-4">{items.length ? <InteractionTable items={items} setItems={setItems} catalog={catalog} /> : <div className="card rise rise-3 grid min-h-64 place-items-center p-10 text-center"><div><div className="lottie-frame mx-auto h-32 w-32"><LottieIcon name="analytics" className="h-28 w-28" /></div><h3 className="mt-4 font-display text-xl">{bridgeMode ? t("Choose a day and import") : t("Choose a day and extract")}</h3><p className="mt-2 max-w-sm text-xs leading-5 text-ink/45">{t("Atlas shows only interactions backed by your calendar, selected mail, chat evidence, or details you type manually.")}</p></div></div>}</div>
        </section>
        <div className="space-y-5"><ExportPanel destination={destination} autoSync={status.autoSync} autoSyncTime={status.autoSyncTime} automationMode={status.automationMode} bridgeMode={bridgeMode} items={items} busy={busy} onSave={saveSelected} onSync={() => void syncCalendar(false)} onEdit={editDestination} /><aside className="rounded-2xl bg-night p-5 text-white"><ShieldCheck className="h-5 w-5 text-glow" /><h3 className="mt-4 font-display text-xl">{t("Daily minimum: 3")}</h3><p className="mt-2 text-xs leading-5 text-white/55">{t("Atlas always saves the real activities found. If there are fewer than three, it opens the app so you can add the missing ones.")}</p></aside></div>
      </div>
    </main>
    {aiPanel && <AiTransparencyModal onClose={() => setAiPanel(false)} onSaved={refreshStatus} />}
    {manual && <ManualModal date={date} onClose={() => setManual(false)} onAdd={item => { setItems(old => [...old, item]); setManual(false); }} />}
    {editingProfile && <div className="fixed inset-0 z-50 grid place-items-center bg-night/45 p-8 backdrop-blur-sm"><ProfileForm initial={profile} title={t("Profile settings")} onSave={saveProfile} cancel={() => setEditingProfile(false)} /></div>}
    {success && <SuccessModal result={success} onClose={() => setSuccess(undefined)} />}
    {rangeReport && <RangeReportModal days={rangeReport.days} stopped={rangeReport.stopped} onClose={() => setRangeReport(undefined)} onOpenDay={day => { setRangeReport(undefined); setRangeEnd(undefined); openDay(day); }} />}
    <AtlasLoader show={busy} message={workStatus?.message ?? t("Atlas is working")} detail={rangeStopping ? t("Stopping after the current day…") : workStatus?.detail} action={rangeRunning && !rangeStopping ? { label: t("Stop after this day"), onClick: () => { stopRange.current = true; setRangeStopping(true); } } : undefined} />
  </div>;
}

function RoleScreen({ onChoose, busy, error }: { onChoose: (role: AppRole) => void; busy: boolean; error: string }) {
  const { lang, setLang } = useI18n();
  const t = useT();
  const switchLanguage = async () => {
    const next = lang === "es" ? "en" : "es";
    setLang(next);
    try { await api.saveLanguage(next); } catch (failure) { void api.logError("save-language", String(failure)); }
  };
  const card = (role: AppRole, lottie: LottieName, eyebrow: string, title: string, detail: string, installs: string[], rhythm: string, delay: string) => (
    <button disabled={busy} onClick={() => onChoose(role)}
      className={`card card-lift group flex flex-col items-start gap-4 p-7 text-left rise ${delay} disabled:opacity-60`}>
      <span className="lottie-frame h-28 w-28 self-center"><LottieIcon name={lottie} className="h-24 w-24" /></span>
      <span className="text-[10px] font-bold uppercase tracking-[.2em] text-pine">{eyebrow}</span>
      <span className="-mt-2 font-display text-2xl">{title}</span>
      <span className="text-sm leading-6 text-ink/55">{detail}</span>
      <span className="w-full rounded-xl bg-cream/80 p-3 text-[11px] leading-5 text-ink/60">
        <b className="block text-ink/75">{t("Installs your own Power Automate solution")}</b>
        {installs.map(item => <span key={item} className="mt-1 flex items-start gap-2"><Check className="mt-0.5 h-3 w-3 shrink-0 text-pine" />{item}</span>)}
      </span>
      <span className="flex items-center gap-2 text-[11px] text-ink/50"><RefreshCw className="h-3 w-3" />{rhythm}</span>
      <span className="mt-1 flex items-center gap-2 text-sm font-bold text-pine">{t("Start")}<ArrowRight className="h-4 w-4 transition group-hover:translate-x-1" /></span>
    </button>
  );
  return <main className="grid min-h-screen place-items-center px-8 py-12" aria-busy={busy}>
    <div className="w-full max-w-4xl">
      <div className="rise mb-10 flex flex-col items-center text-center">
        <div className="flex w-full items-center justify-between"><span className="w-10" /><Brand /><button title={t("Switch language")} className="rounded-xl border border-ink/10 bg-white px-2.5 py-2.5 text-[11px] font-extrabold tracking-wide hover:bg-mint" onClick={() => void switchLanguage()}>{lang === "es" ? "ES" : "EN"}</button></div>
        <h1 className="mt-8 font-display text-4xl">{t("How will you use Atlas?")}</h1>
        <p className="mt-2 max-w-xl text-sm text-ink/50">{t("Your role decides which Power Automate solution Atlas prepares for you. Each one is personalized with your name and installation ID, so it never collides with a colleague's.")}</p>
      </div>
      {error && <div className="mb-5"><ErrorBanner message={error} /></div>}
      <div className="grid gap-5 sm:grid-cols-2">
        {card("cs", "analytics", t("Tracker · CS"), t("Daily interactions tracker"), t("Records your own work from calendar, mail and Teams evidence and writes the Circana tracker."), [t("Export evidence to OneDrive (every 5 minutes)"), t("Capture Teams messages as they arrive"), "Outlook · Teams · OneDrive"], t("Previous workday at Windows start, or at 09:30 if the PC stayed on"), "rise-1")}
        {card("manager", "auditDoc", t("QA · Manager"), t("Team QA audit"), t("Imports the QA history of the analysts you audit, evaluates every conversation with local AI and fills their Excel workbooks."), [t("Export mailbox evidence on request (every 5 minutes)"), t("Watch mail from the analysts you audit"), "Outlook · OneDrive"], t("Full history first, then twice a day and on every analyst email"), "rise-2")}
      </div>
    </div>
  </main>;
}

function ManagerWorkspace({ status, refreshStatus, signOut, exitRole }: { status: AppStatus; refreshStatus: () => Promise<void>; signOut: () => Promise<void>; exitRole: () => Promise<void> }) {
  const { lang, setLang } = useI18n();
  const t = useT();
  const [error, setError] = useState("");
  const bridgeMode = status.sourceMode === "power_automate_folder";
  const switchLanguage = async () => {
    const next = lang === "es" ? "en" : "es";
    setLang(next);
    try { await api.saveLanguage(next); } catch (failure) { void api.logError("save-language", String(failure)); }
  };
  return <div className="min-h-screen">
    <header className="sticky top-0 z-30 border-b border-white/70 bg-cream/80 px-8 py-4 backdrop-blur-xl"><div className="mx-auto flex max-w-[1540px] items-center justify-between"><div className="flex items-center gap-4"><PersonalSeal name={status.solutionOwner ?? status.profile?.fullName ?? "Atlas"} caption={t("Interaction tracker")} /><span className="chip bg-night text-glow">{t("QA · Manager")}</span></div><div className="flex items-center gap-2"><AccentPicker /><div className="mr-3 max-w-xs text-right"><p className="text-xs font-bold">{bridgeMode ? (status.solutionOwner ?? t("Power Automate QA")) : status.account?.displayName}</p><p className="truncate text-[10px] text-ink/40">{bridgeMode ? status.qaBridgeFolder : status.account?.email}</p></div><button title={t("Switch language")} className="rounded-xl border border-ink/10 bg-white px-2.5 py-2.5 text-[11px] font-extrabold tracking-wide hover:bg-mint" onClick={() => void switchLanguage()}>{lang === "es" ? "ES" : "EN"}</button><button title={t("Open failure log")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => void api.openLog().catch(failure => setError(String(failure)))}><FileText className="h-4 w-4" /></button><button title={t("Change role")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => void exitRole()}><Home className="h-4 w-4" /></button>{!bridgeMode && <button title={t("Sign out")} className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-red-50 hover:text-red-600" onClick={signOut}><LogOut className="h-4 w-4" /></button>}</div></div></header>
    <main className="mx-auto max-w-[1540px] px-8 py-8">{status.connectorUpdateAvailable && <SolutionUpdateBanner role="manager" onDone={refreshStatus} />}{error && <div className="mb-5"><ErrorBanner message={error} onClose={() => setError("")} /></div>}<QaPanel /></main>
  </div>;
}

function AtlasApp() {
  const { setLang } = useI18n();
  const [status, setStatus] = useState<AppStatus>(); const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const [installing, setInstalling] = useState(false); const [editingDestination, setEditingDestination] = useState(false);
  const refresh = async () => { setStatus(await api.status()); };
  useEffect(() => { if (status?.language === "es" || status?.language === "en") setLang(status.language as "es" | "en"); }, [status?.language, setLang]);
  useEffect(() => { refresh().catch(e => setError(String(e))); }, []);
  const scheduledChecked = useRef(false);
  useEffect(() => {
    if (!status?.scheduledLaunch || scheduledChecked.current) return;
    scheduledChecked.current = true;
    // A startup launch only needs the window when the chosen role's setup is
    // incomplete; otherwise Atlas keeps working from the notification area.
    const trackerReady = status.appRole === "cs" && status.profile && status.destination && status.autoSync;
    const ready = status.appRole && status.configured && status.signedIn && (status.appRole === "manager" || trackerReady);
    void api.completeScheduled("", Boolean(ready), !ready);
  }, [status]);
  useEffect(() => { const onError = (event: ErrorEvent) => { void api.logError("window", event.message); }; const onRejection = (event: PromiseRejectionEvent) => { void api.logError("promise", String(event.reason)); }; window.addEventListener("error", onError); window.addEventListener("unhandledrejection", onRejection); return () => { window.removeEventListener("error", onError); window.removeEventListener("unhandledrejection", onRejection); }; }, []);
  const signIn = async () => { setBusy(true); setError(""); try { setStatus(await api.signIn()); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  const signOut = async () => { await api.signOut(); await refresh(); };
  const saveProfile = async (p: UserProfile) => { await api.saveProfile(p); await refresh(); };
  const useConnector = async (folder: string) => { setStatus(status?.appRole === "manager" ? await api.saveQaBridgeFolder(folder) : await api.savePowerAutomateFolder(folder)); setInstalling(false); };
  const saveDestination = async (destination: TrackerDestination, autoSync: boolean, autoSyncTime: string, automationMode: AutomationMode) => { setStatus(await api.saveDestination(destination, autoSync, autoSyncTime, automationMode)); setEditingDestination(false); };
  const chooseRole = async (role: AppRole) => { setBusy(true); setError(""); try { setStatus(await api.saveAppRole(role)); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  const exitRole = async () => { setInstalling(false); setStatus(await api.saveAppRole(null)); };
  const screen = ((): [string, JSX.Element] => {
    if (!status) return error ? ["error", <main className="grid min-h-screen place-items-center p-10"><ErrorBanner message={error} /></main>] : ["loading", <LoadingScreen />];
    // 1. The role comes first: it decides which personalized solution is built.
    if (!status.appRole) return ["role", <RoleScreen onChoose={role => void chooseRole(role)} busy={busy} error={error} />];
    // 2. The Power Automate connector of that role.
    if (!status.configured || installing) return ["connector", <ConnectorInstaller key={status.appRole} role={status.appRole} defaultOwner={status.solutionOwner ?? status.profile?.fullName} onClose={status.configured ? () => setInstalling(false) : undefined} onChangeRole={() => void exitRole()} onUseFolder={useConnector} />];
    if (!status.signedIn) return ["login", <LoginScreen onSignIn={signIn} onEditMicrosoft={() => setInstalling(true)} busy={busy} error={error} />];
    if (status.appRole === "manager") return ["manager", <ManagerWorkspace status={status} refreshStatus={refresh} signOut={signOut} exitRole={exitRole} />];
    if (!status.profile) return ["profile", <SetupScreen status={status} onSaved={saveProfile} />];
    if (!status.destination || editingDestination) return ["destination", <DestinationSetupScreen initial={status.destination} initialAutoSync={status.autoSync} initialAutoSyncTime={status.autoSyncTime} initialAutomationMode={status.automationMode} bridgeMode={status.sourceMode === "power_automate_folder"} onSave={saveDestination} onCancel={status.destination ? () => setEditingDestination(false) : undefined} />];
    return ["workspace", <Workspace status={status} refreshStatus={refresh} signOut={signOut} editMicrosoft={() => setInstalling(true)} editDestination={() => setEditingDestination(true)} exitRole={exitRole} />];
  })();
  // Screens cross-fade. Only opacity is animated: a transform would turn this
  // wrapper into the containing block of the fixed modals and loaders inside.
  return <AnimatePresence mode="wait" initial={false}>
    <motion.div key={screen[0]} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.28, ease: "easeOut" }}>{screen[1]}</motion.div>
  </AnimatePresence>;
}

export default function App() {
  return <I18nProvider><AtlasIntro><AtlasApp /></AtlasIntro></I18nProvider>;
}
