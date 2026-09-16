import { useEffect, useMemo, useRef, useState } from "react";
import ConnectorInstaller from "./ConnectorInstaller";
import AtlasIntro from "./AtlasIntro";
import AtlasLoader from "./AtlasLoader";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import {
  AlertCircle, ArrowRight, Bot, CalendarDays, Check, ChevronDown, Cloud,
  ExternalLink, FilePlus2, FileSpreadsheet, FileText, FolderOpen, HardDrive,
  Inbox, Loader2, LogOut, Mail, PenLine, Plus, RefreshCw, Save, Settings,
  ShieldCheck, Sparkles, Trash2, UserRound, X
} from "lucide-react";
import { api, fromLocalInput, localDate, toLocalInput } from "./lib";
import type { AppStatus, AutomationMode, AutomationRequest, ExportResult, ExtractionResult, Interaction, TrackerDestination, TrackerDestinationKind, UserProfile } from "./types";

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

function LoadingScreen() {
  return <main className="min-h-screen" aria-busy="true">
    <AtlasLoader show message="Preparing your workspace" detail="Starting Atlas Local AI and loading your tracker settings." mode="screen" delay={0} />
  </main>;
}

function Brand() {
  return <div className="flex items-center gap-3">
    <div className="grid h-10 w-10 place-items-center rounded-xl bg-pine text-white"><span className="font-display text-xl italic">A</span></div>
    <div><p className="font-display text-lg leading-none">Atlas</p><p className="mt-1 text-[10px] font-bold uppercase tracking-[.18em] text-ink/45">Interaction tracker</p></div>
  </div>;
}

function LoginScreen({ onSignIn, onEditMicrosoft, busy, error }: { onSignIn: () => void; onEditMicrosoft: () => void; busy: boolean; error: string }) {
  return <main className="mx-auto flex min-h-screen max-w-6xl items-center px-10 py-12" aria-busy={busy}>
    <section className="grid w-full grid-cols-[1.05fr_.95fr] overflow-hidden rounded-[2rem] bg-[#081c1a] shadow-panel">
      <div className="relative overflow-hidden p-14 text-white">
        <div className="absolute -right-32 -top-32 h-96 w-96 rounded-full border border-white/10" />
        <div className="absolute -right-16 -top-16 h-64 w-64 rounded-full border border-white/10" />
        <Brand />
        <div className="relative mt-24">
          <p className="mb-4 text-xs font-bold uppercase tracking-[.2em] text-[#8dd7c4]">Your work, accounted for</p>
          <h1 className="font-display text-6xl leading-[1.02]">Real interactions.<br /><span className="italic text-[#8dd7c4]">Nothing invented.</span></h1>
          <p className="mt-7 max-w-lg text-base leading-7 text-white/60">Build the Circana tracker from meetings and mail already in your Microsoft 365 account, then review every row before it reaches Excel.</p>
        </div>
        <div className="mt-16 flex items-center gap-3 text-xs font-semibold text-white/55"><ShieldCheck className="h-5 w-5 text-[#8dd7c4]" /> Delegated access · Secure token storage · Local AI only</div>
      </div>
      <div className="m-4 flex flex-col justify-center rounded-[1.4rem] bg-white p-12 text-ink">
        <div className="grid h-12 w-12 place-items-center rounded-xl bg-[#eef6ff] text-[#2563a8]"><UserRound className="h-6 w-6" /></div>
        <h2 className="mt-7 font-display text-3xl">Welcome to Atlas</h2>
        <p className="mt-3 text-sm leading-6 text-ink/55">Sign in with your company Microsoft account. Atlas can only read your delegated calendar and mail data.</p>
        {error && <div className="mt-6"><ErrorBanner message={error} /></div>}
        <button className="btn-primary mt-8 h-12" onClick={onSignIn} disabled={busy}>
          {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <svg viewBox="0 0 24 24" className="h-4 w-4" aria-hidden><path fill="#f35325" d="M1 1h10v10H1z"/><path fill="#81bc06" d="M13 1h10v10H13z"/><path fill="#05a6f0" d="M1 13h10v10H1z"/><path fill="#ffba08" d="M13 13h10v10H13z"/></svg>}
          Sign in with Microsoft <ArrowRight className="h-4 w-4" />
        </button>
        <button className="mt-4 text-xs font-bold text-pine hover:underline" onClick={onEditMicrosoft} disabled={busy}>Usar el conector de Power Automate</button>
        <p className="mt-5 text-center text-[11px] leading-5 text-ink/40">Your refresh token is stored by Windows Credential Manager and is never written to a project file.</p>
      </div>
    </section>
    <AtlasLoader show={busy} message="Connecting your Microsoft account" detail="Complete any sign-in or company verification shown by Microsoft." />
  </main>;
}

function ProfileForm({ initial, onSave, title = "Set up your profile", cancel }: { initial?: UserProfile; onSave: (p: UserProfile) => Promise<void>; title?: string; cancel?: () => void }) {
  const [profile, setProfile] = useState(initial ?? emptyProfile);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const update = (key: keyof UserProfile, value: string) => setProfile(p => ({ ...p, [key]: value }));
  const submit = async (e: React.FormEvent) => {
    e.preventDefault(); setError("");
    if (!profile.loginId.trim() || !profile.fullName.trim()) return setError("Login ID and full name are required.");
    setBusy(true); try { await onSave(profile); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <form onSubmit={submit} className="card w-full max-w-2xl p-8" aria-busy={busy}>
    <div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.18em] text-pine">Personal defaults</p><h2 className="mt-2 font-display text-3xl">{title}</h2><p className="mt-2 text-sm text-ink/50">Used only in fields Atlas is allowed to write.</p></div>{cancel && <button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={cancel}><X /></button>}</div>
    <div className="mt-8 grid grid-cols-2 gap-5">
      <label><span className="label">Corp ID / Login ID *</span><input className="field" value={profile.loginId} onChange={e => update("loginId", e.target.value)} /></label>
      <label><span className="label">Full name *</span><input className="field" value={profile.fullName} onChange={e => update("fullName", e.target.value)} /></label>
      <label><span className="label">Area</span><input className="field" value={profile.area} onChange={e => update("area", e.target.value)} /></label>
      <label><span className="label">Team lead</span><input className="field" value={profile.teamLead} onChange={e => update("teamLead", e.target.value)} /></label>
      <label className="col-span-2"><span className="label">Circana manager</span><input className="field" value={profile.circanaManager} onChange={e => update("circanaManager", e.target.value)} /></label>
    </div>
    {error && <div className="mt-5"><ErrorBanner message={error} /></div>}
    <div className="mt-7 flex justify-end gap-3">{cancel && <button type="button" className="btn-secondary" onClick={cancel}>Cancel</button>}<button className="btn-primary" disabled={busy}>{busy && <Loader2 className="h-4 w-4 animate-spin" />}Save profile</button></div>
    <AtlasLoader show={busy} message="Saving your Atlas profile" detail="Applying the defaults used in your tracker rows." />
  </form>;
}

function SetupScreen({ status, onSaved }: { status: AppStatus; onSaved: (p: UserProfile) => Promise<void> }) {
  const sourceLabel = status.sourceMode === "power_automate_folder" ? "Power Automate inbox" : status.account?.email;
  return <main className="min-h-screen px-10 py-8"><header className="mx-auto flex max-w-6xl items-center justify-between"><Brand /><span className="chip bg-mint text-pine"><Check className="h-3 w-3" /> {sourceLabel}</span></header><div className="mx-auto grid min-h-[calc(100vh-6rem)] max-w-6xl place-items-center"><ProfileForm onSave={onSaved} /></div></main>;
}

function DestinationSetupScreen({ initial, initialAutoSync, initialAutoSyncTime, initialAutomationMode, bridgeMode, onSave, onCancel }: { initial?: TrackerDestination; initialAutoSync: boolean; initialAutoSyncTime: string; initialAutomationMode: AutomationMode; bridgeMode: boolean; onSave: (destination: TrackerDestination, autoSync: boolean, autoSyncTime: string, automationMode: AutomationMode) => Promise<void>; onCancel?: () => void }) {
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
    if (!value.trim()) return setError(kind === "share_point" || kind === "share_point_flow" ? "Paste the SharePoint workbook link." : "Choose an Excel workbook.");
    setBusy(true);
    try { await onSave({ kind, value: value.trim(), localPath: kind === "share_point" && localPath ? localPath : undefined }, autoSync, autoSyncTime, automationMode); }
    catch (failure) { setError(String(failure)); }
    finally { setBusy(false); }
  };
  const options: { kind: TrackerDestinationKind; title: string; detail: string; icon: React.ReactNode }[] = [
    { kind: "local_existing", title: "Existing Excel tracker", detail: "Keep macros, tables and formulas", icon: <HardDrive className="h-5 w-5" /> },
    { kind: "local_new", title: "Create a new tracker", detail: "Create one compatible .xlsx file", icon: <FilePlus2 className="h-5 w-5" /> },
    { kind: "share_point", title: "Synced SharePoint", detail: bridgeMode ? "Use the OneDrive-synced copy" : "Update the online .xlsx or .xlsm directly", icon: <Cloud className="h-5 w-5" /> },
    ...(bridgeMode ? [{ kind: "share_point_flow" as TrackerDestinationKind, title: "SharePoint cloud flow", detail: "Create the second import ZIP", icon: <RefreshCw className="h-5 w-5" /> }] : []),
  ];
  return <main className="mx-auto min-h-screen max-w-6xl px-10 py-10" aria-busy={busy}>
    <header className="flex items-center justify-between"><Brand />{onCancel && <button className="btn-secondary" onClick={onCancel}>Cancel</button>}</header>
    <form onSubmit={submit} className="card mx-auto mt-10 max-w-4xl p-9">
      <p className="text-xs font-bold uppercase tracking-[.18em] text-pine">One-time tracker setup</p>
      <h1 className="mt-2 font-display text-4xl">Where should Atlas write?</h1>
      <p className="mt-3 max-w-2xl text-sm leading-6 text-ink/50">Choose a local workbook, a synchronized SharePoint copy, or the optional second cloud flow. Atlas keeps this destination for manual and automatic runs.</p>
      <div className={`mt-8 grid gap-3 ${options.length === 4 ? "grid-cols-4" : "grid-cols-3"}`}>{options.map(option => <button type="button" key={option.kind} onClick={() => changeKind(option.kind)} className={`rounded-2xl border p-4 text-left ${kind === option.kind ? "border-pine/35 bg-mint/55" : "border-ink/10 bg-white"}`}><span className={`grid h-10 w-10 place-items-center rounded-xl ${kind === option.kind ? "bg-pine text-white" : "bg-cream text-ink/45"}`}>{option.icon}</span><b className="mt-4 block text-sm">{option.title}</b><span className="mt-1 block text-[11px] leading-5 text-ink/45">{option.detail}</span></button>)}</div>
      <div className="mt-7">{kind === "share_point" || kind === "share_point_flow" ? <div><label><span className="label">SharePoint or OneDrive workbook link</span><textarea className="field min-h-24 resize-none font-mono text-xs" value={value} onChange={event => setValue(event.target.value)} placeholder="https://tenant-my.sharepoint.com/:x:/r/.../Tracker.xlsm?web=1" /></label>{kind === "share_point_flow" ? <div className="mt-4 rounded-2xl border border-pine/15 bg-mint/25 p-5"><p className="text-sm font-bold">Second Power Automate solution</p><p className="mt-2 text-xs leading-5 text-ink/55">Saving creates a personalized <code>AtlasTrackerWriter</code> ZIP and places the required Office Script in your synchronized OneDrive. Import that ZIP once and map OneDrive, SharePoint, and Excel with your Circana account.</p><p className="mt-2 text-[11px] leading-5 text-ink/45">The tenant must allow Office Scripts. Atlas queues one JSON package at the chosen daily time; the flow retries safely using each row’s hidden source ID.</p></div> : bridgeMode ? <div className="mt-4 rounded-2xl border border-pine/15 bg-mint/25 p-5"><div className="flex flex-wrap gap-3"><button type="button" className="btn-secondary" disabled={!value.trim()} onClick={() => void openUrl(value.trim())}><ExternalLink className="h-4 w-4" />Abrir enlace</button><button type="button" className="btn-primary" onClick={chooseSyncedCopy}><FolderOpen className="h-4 w-4" />{localPath ? "Cambiar copia sincronizada" : "Seleccionar copia sincronizada"}</button></div>{localPath && <p className="mt-3 break-all rounded-xl bg-white/70 p-3 text-xs text-ink/55">{localPath}</p>}<ol className="mt-4 list-decimal space-y-2 pl-5 text-xs leading-5 text-ink/60"><li>El propietario comparte la carpeta del tracker contigo y permite editar.</li><li>En OneDrive, selecciona la carpeta y pulsa <b>Agregar acceso directo a Mis archivos</b>.</li><li>Espera a que aparezca bajo <b>OneDrive – Circana</b> y elige el archivo exacto aquí.</li></ol></div> : <span className="mt-2 block text-[11px] leading-5 text-ink/45">Saving opens Microsoft once for delegated file consent.</span>}</div> : <div><button type="button" className="btn-secondary" onClick={chooseLocal}><FolderOpen className="h-4 w-4" />{value ? "Change workbook" : "Choose workbook"}</button>{value && <p className="mt-3 break-all rounded-xl bg-cream p-3 text-xs text-ink/55">{value}</p>}</div>}</div>
      <div className="mt-7 rounded-2xl border border-pine/15 bg-mint/35 p-4"><label className="flex items-start gap-4"><input type="checkbox" checked={autoSync} onChange={event => setAutoSync(event.target.checked)} className="mt-1 h-4 w-4 accent-pine" /><span><b className="block text-sm">Automatic tracker</b><span className="mt-1 block text-xs leading-5 text-ink/50">Runs under your Windows account without opening the main window. Atlas appears only when the run fails or fewer than three activities need attention.</span></span></label>{autoSync && <div className="mt-4 grid grid-cols-2 gap-3"><button type="button" onClick={() => setAutomationMode("startup_previous_workday")} className={`rounded-xl border p-4 text-left ${automationMode === "startup_previous_workday" ? "border-pine/35 bg-white" : "border-ink/10 bg-white/45"}`}><b className="block text-sm">When Windows starts</b><span className="mt-1 block text-[11px] leading-5 text-ink/50">Recommended. Process the previous business day after sign-in.</span></button><button type="button" onClick={() => setAutomationMode("daily_time")} className={`rounded-xl border p-4 text-left ${automationMode === "daily_time" ? "border-pine/35 bg-white" : "border-ink/10 bg-white/45"}`}><b className="block text-sm">At a chosen time</b><span className="mt-1 block text-[11px] leading-5 text-ink/50">Keep Atlas hidden and process the current day at the time below.</span></button></div>}{autoSync && automationMode === "daily_time" && <label className="mt-4 block w-36"><span className="label">Run at</span><input type="time" className="field" value={autoSyncTime} onChange={event => setAutoSyncTime(event.target.value)} /></label>}</div>
      {error && <div className="mt-5"><ErrorBanner message={error} /></div>}
      <div className="mt-7 flex justify-end"><button className="btn-primary" disabled={busy}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}Save tracker setup</button></div>
    </form>
    <AtlasLoader
      show={busy}
      message={kind === "share_point_flow" ? "Preparing your SharePoint flow" : "Saving your tracker destination"}
      detail={kind === "share_point_flow" ? "Personalizing the writer solution and its Office Script." : "Checking the workbook and scheduling the daily run."}
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
  const styles = item.sourceKind === "calendar" ? "bg-[#e7f0ff] text-[#315f9e]" : item.sourceKind === "email" ? "bg-[#f1eaff] text-[#71509d]" : item.sourceKind === "teams_chat" ? "bg-[#fff0df] text-[#9a5a1e]" : "bg-mint text-pine";
  const Icon = item.sourceKind === "calendar" ? CalendarDays : item.sourceKind === "email" ? Mail : item.sourceKind === "teams_chat" ? Sparkles : PenLine;
  return <span className={`chip ${styles}`}><Icon className="h-3 w-3" />{item.sourceKind === "teams_chat" ? (item.aiSuggested ? "AI summary" : "Teams evidence") : item.sourceKind.replace("_", " ")}</span>;
}

function SelectCell({ value, options, onChange }: { value: string; options: string[]; onChange: (v: string) => void }) {
  return <div className="relative"><select className="w-full appearance-none bg-transparent py-1 pr-5 text-xs outline-none" value={value} onChange={e => onChange(e.target.value)}>{options.map(x => <option key={x}>{x}</option>)}</select><ChevronDown className="pointer-events-none absolute right-0 top-1.5 h-3 w-3 text-ink/35" /></div>;
}

function InteractionTable({ items, setItems }: { items: Interaction[]; setItems: React.Dispatch<React.SetStateAction<Interaction[]>> }) {
  const set = (index: number, patch: Partial<Interaction>) => setItems(old => old.map((v, i) => i === index ? { ...v, ...patch } : v));
  const removeManual = (index: number) => setItems(old => old.filter((_, i) => i !== index));
  return <div className="overflow-auto rounded-xl border border-ink/10 bg-white">
    <table className="w-full min-w-[1420px] border-collapse text-left">
      <thead className="sticky top-0 z-10 bg-[#f2f4ef] text-[10px] font-extrabold uppercase tracking-[.13em] text-ink/45"><tr>
        <th className="w-12 px-3 py-3">Use</th><th className="px-3 py-3">Source</th><th className="px-3 py-3">Type</th><th className="px-3 py-3">Start</th><th className="px-3 py-3">End</th><th className="px-3 py-3">Status</th><th className="px-3 py-3">Client type</th><th className="px-3 py-3">End client</th><th className="px-3 py-3">Category</th><th className="px-3 py-3">Subcategory</th><th className="px-3 py-3">Priority</th><th className="px-3 py-3">Incident</th><th className="w-[300px] px-3 py-3">Comments</th><th className="w-10" />
      </tr></thead>
      <tbody className="divide-y divide-ink/5">
      {items.map((item, i) => {
        const fullyEditable = item.sourceKind === "manual" || item.aiSuggested;
        return <tr key={item.sourceId} className={`${item.selected ? "" : "opacity-50"} align-top hover:bg-cream/35`}>
          <td className="px-3 py-3"><input type="checkbox" className="h-4 w-4 accent-pine" checked={item.selected} onChange={e => set(i, { selected: e.target.checked, reviewed: e.target.checked ? true : item.reviewed })} /></td>
          <td className="whitespace-nowrap px-3 py-3"><SourceBadge item={item} />{item.aiSuggested ? <input className="mt-1 block w-40 bg-transparent text-[10px] text-ink/45 outline-none" value={item.evidenceLabel} onChange={e => set(i, { evidenceLabel: e.target.value })} /> : <p className="mt-1 max-w-40 truncate text-[10px] text-ink/35" title={item.evidenceLabel}>{item.evidenceLabel}</p>}</td>
          <td className="px-3 py-3">{fullyEditable ? <SelectCell value={item.interactionType} options={["Meeting", "Task"]} onChange={v => set(i, { interactionType: v as Interaction["interactionType"] })} /> : <span className="text-xs">{item.interactionType}</span>}</td>
          <td className="px-3 py-3">{fullyEditable ? <input type="datetime-local" className="w-[155px] bg-transparent text-xs outline-none" value={toLocalInput(item.interactionDateTime)} onChange={e => set(i, { interactionDateTime: fromLocalInput(e.target.value), receptionDateTime: fromLocalInput(e.target.value) })} /> : <span className="whitespace-nowrap text-xs">{new Date(item.interactionDateTime).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>}</td>
          <td className="px-3 py-3">{fullyEditable ? <input type="datetime-local" className="w-[155px] bg-transparent text-xs outline-none" value={toLocalInput(item.resolutionDateTime)} onChange={e => set(i, { resolutionDateTime: fromLocalInput(e.target.value) })} /> : <span className="whitespace-nowrap text-xs">{item.resolutionDateTime ? new Date(item.resolutionDateTime).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "—"}</span>}</td>
          <td className="whitespace-nowrap px-3 py-3"><span className={`chip ${item.status === "Resolved" ? "bg-mint text-pine" : "bg-[#fff0df] text-[#9a5a1e]"}`}>{item.status}</span></td>
          <td className="px-3 py-3">{fullyEditable ? <SelectCell value={item.clientType} options={["", "Circana", "End_Client", "Capgemini"]} onChange={v => set(i, { clientType: v as Interaction["clientType"] })} /> : <span className="text-xs">{item.clientType || "—"}</span>}</td>
          <td className="px-3 py-3">{fullyEditable ? <input className="w-28 bg-transparent text-xs outline-none" placeholder="Blank" value={item.endClient} onChange={e => set(i, { endClient: e.target.value })} /> : <span className="text-xs">{item.endClient || "—"}</span>}</td>
          <td className="px-3 py-3"><input className="w-28 bg-transparent text-xs outline-none" placeholder="Fill in" value={item.category} onChange={e => set(i, { category: e.target.value })} /></td>
          <td className="px-3 py-3"><input className="w-28 bg-transparent text-xs outline-none" placeholder="Fill in" value={item.subcategory} onChange={e => set(i, { subcategory: e.target.value })} /></td>
          <td className="px-3 py-3"><SelectCell value={item.priority} options={["Low", "Intermediate", "High"]} onChange={v => set(i, { priority: v as Interaction["priority"] })} /></td>
          <td className="px-3 py-3"><input className="w-24 bg-transparent text-xs outline-none" placeholder="—" value={item.incidentNumber} onChange={e => set(i, { incidentNumber: e.target.value })} /></td>
          <td className="px-3 py-3"><textarea rows={2} className="w-full resize-none bg-transparent text-xs leading-5 outline-none" placeholder="Add a factual note" value={item.comments} onChange={e => set(i, { comments: e.target.value })} /></td>
          <td className="px-2 py-3">{item.sourceKind === "manual" && <button title="Remove manual entry" onClick={() => removeManual(i)} className="rounded-md p-1 text-ink/35 hover:bg-red-50 hover:text-red-600"><Trash2 className="h-4 w-4" /></button>}</td>
        </tr>;
      })}
      </tbody>
    </table>
  </div>;
}

function ManualModal({ date, onClose, onAdd }: { date: string; onClose: () => void; onAdd: (i: Interaction) => void }) {
  const [type, setType] = useState<Interaction["interactionType"]>("Task");
  const [start, setStart] = useState(""); const [end, setEnd] = useState(""); const [clientType, setClientType] = useState<Interaction["clientType"]>(""); const [endClient, setEndClient] = useState(""); const [comments, setComments] = useState(""); const [error, setError] = useState("");
  const add = (e: React.FormEvent) => { e.preventDefault(); if (!start || !comments.trim()) return setError("Start time and a factual description are required."); const id = crypto.randomUUID(); const startIso = fromLocalInput(`${date}T${start}`); const endIso = end ? fromLocalInput(`${date}T${end}`) : ""; onAdd({ sourceKind: "manual", sourceId: `manual:${id}`, interactionType: type, receptionDateTime: startIso, interactionDateTime: startIso, resolutionDateTime: endIso, clientType, endClient, status: endIso && new Date(endIso) <= new Date() ? "Resolved" : "In Progress", resolutionType: endIso && new Date(endIso) <= new Date() ? "Processed & Resolved" : "", category: "", subcategory: "", priority: type === "Task" ? "High" : "Intermediate", incidentNumber: "", comments: comments.trim(), selected: true, reviewed: true, manualAuthored: true, aiSuggested: false, evidenceLabel: "Typed manually by you" }); };
  return <div className="fixed inset-0 z-50 grid place-items-center bg-[#081c1a]/45 p-8 backdrop-blur-sm"><form onSubmit={add} className="card w-full max-w-xl p-7">
    <div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.16em] text-pine">Manual provenance</p><h2 className="mt-2 font-display text-3xl">Add a real interaction</h2><p className="mt-2 text-sm text-ink/50">Only enter work that actually happened. Atlas does not fill these fields for you.</p></div><button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={onClose}><X /></button></div>
    <div className="mt-7 grid grid-cols-2 gap-4">
      <label><span className="label">Interaction type</span><select className="field" value={type} onChange={e => setType(e.target.value as Interaction["interactionType"])}><option>Task</option><option>Meeting</option></select></label>
      <label><span className="label">Date</span><input className="field bg-cream/70" value={date} disabled /></label>
      <label><span className="label">Start time *</span><input type="time" className="field" value={start} onChange={e => setStart(e.target.value)} /></label>
      <label><span className="label">End time</span><input type="time" className="field" value={end} onChange={e => setEnd(e.target.value)} /></label>
      <label><span className="label">Client type</span><select className="field" value={clientType} onChange={e => setClientType(e.target.value as Interaction["clientType"])}><option value="">Undetermined</option><option>Circana</option><option>End_Client</option><option>Capgemini</option></select></label>
      <label><span className="label">Real client name</span><input className="field" value={endClient} onChange={e => setEndClient(e.target.value)} /></label>
      <label className="col-span-2"><span className="label">What happened? *</span><textarea className="field min-h-24 resize-none" value={comments} onChange={e => setComments(e.target.value)} placeholder="Describe the real work task in English" /></label>
    </div>
    {error && <div className="mt-4"><ErrorBanner message={error} /></div>}
    <div className="mt-6 flex justify-end gap-3"><button type="button" className="btn-secondary" onClick={onClose}>Cancel</button><button className="btn-primary"><Plus className="h-4 w-4" />Add interaction</button></div>
  </form></div>;
}

function ExportPanel({ destination, autoSync, autoSyncTime, automationMode, bridgeMode, items, busy, onSave, onSync, onEdit }: { destination: TrackerDestination; autoSync: boolean; autoSyncTime: string; automationMode: AutomationMode; bridgeMode: boolean; items: Interaction[]; busy: boolean; onSave: () => void; onSync: () => void; onEdit: () => void }) {
  const isRemote = destination.kind === "share_point" || destination.kind === "share_point_flow";
  const writerFlow = destination.kind === "share_point_flow";
  return <aside className="card h-fit p-5"><div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.16em] text-pine">Tracker</p><h3 className="mt-2 font-display text-2xl">Configured once</h3></div><span className={`grid h-9 w-9 place-items-center rounded-xl ${isRemote ? "bg-[#e7f0ff] text-[#315f9e]" : "bg-mint text-pine"}`}>{isRemote ? <Cloud className="h-4 w-4" /> : <FileSpreadsheet className="h-4 w-4" />}</span></div>
    <p className="mt-3 text-xs leading-5 text-ink/45">Only selected, verified rows are written. Existing macros, formulas, and other worksheets are preserved.</p>
    <p className="mt-4 line-clamp-4 break-all rounded-xl bg-cream p-3 text-[10px] leading-4 text-ink/55">{destination.value}</p>
    {writerFlow && <div className="mt-3 rounded-xl border border-pine/15 bg-mint/35 p-3"><b className="text-xs">Import connector 2 once</b><p className="mt-1 text-[10px] leading-4 text-ink/50">Map OneDrive, SharePoint and Excel connections, then leave the flow active.</p><div className="mt-3 flex gap-2"><button className="btn-secondary flex-1 px-3 py-2 text-[10px]" onClick={() => void api.showWriterPackage().catch(failure => window.alert(String(failure)))}><FolderOpen className="h-3 w-3" />Show ZIP</button><button className="btn-secondary flex-1 px-3 py-2 text-[10px]" onClick={() => void api.openPowerAutomate().catch(failure => window.alert(String(failure)))}><ExternalLink className="h-3 w-3" />Import</button></div></div>}
    <div className="mt-3 flex items-center justify-between"><span className={`chip ${autoSync ? "bg-mint text-pine" : "bg-cream text-ink/50"}`}>{autoSync ? (automationMode === "startup_previous_workday" ? "Windows start · previous day" : `Daily · ${autoSyncTime}`) : "Automatic run off"}</span><button className="text-[11px] font-bold text-pine hover:underline" onClick={onEdit}>Change setup</button></div>
    <button className="btn-primary mt-5 w-full" onClick={onSync} disabled={busy}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}{bridgeMode ? "Import today’s inbox" : "Sync today’s calendar"}</button>
    <button className="btn-secondary mt-2 w-full" onClick={onSave} disabled={busy || !items.some(item => item.selected)}><Save className="h-4 w-4" />Save selected preview</button>
  </aside>;
}

function SuccessModal({ result, onClose }: { result: ExportResult; onClose: () => void }) {
  const remote = result.path.startsWith("https://");
  return <div className="fixed inset-0 z-50 grid place-items-center bg-[#081c1a]/45 p-8 backdrop-blur-sm"><div className="card w-full max-w-lg p-8 text-center"><div className="mx-auto grid h-14 w-14 place-items-center rounded-full bg-mint text-pine"><Check className="h-7 w-7" /></div><h2 className="mt-5 font-display text-3xl">{result.queued ? "Sent to SharePoint queue" : "Tracker saved"}</h2><p className="mt-2 text-sm text-ink/50">{result.queued ? `${result.inserted} rows are ready for the cloud writer` : `${result.inserted} added · ${result.updated} refreshed · ${result.skipped} skipped`}</p><p className="mt-5 break-all rounded-xl bg-cream p-3 text-xs text-ink/55">{result.path}</p>{remote ? <button className="btn-primary mt-6 w-full" onClick={() => void api.openTracker().catch(failure => window.alert(String(failure)))}><ExternalLink className="h-4 w-4" />Open in SharePoint</button> : <div className="mt-6 grid grid-cols-2 gap-3"><button className="btn-secondary" onClick={() => revealItemInDir(result.path)}><FolderOpen className="h-4 w-4" />Open folder</button><button className="btn-primary" onClick={() => void api.openTracker().catch(failure => window.alert(String(failure)))}><ExternalLink className="h-4 w-4" />Open file</button></div>}<button className="mt-5 text-xs font-bold text-ink/45 hover:text-ink" onClick={onClose}>Back to tracker</button></div></div>;
}

function Workspace({ status, refreshStatus, signOut, editMicrosoft, editDestination }: { status: AppStatus; refreshStatus: () => Promise<void>; signOut: () => Promise<void>; editMicrosoft: () => void; editDestination: () => void }) {
  const profile = status.profile!; const destination = status.destination!; const bridgeMode = status.sourceMode === "power_automate_folder"; const [date, setDate] = useState(localDate()); const [includeEmail, setIncludeEmail] = useState(true); const [includeTeams, setIncludeTeams] = useState(true); const [items, setItems] = useState<Interaction[]>([]); const [warnings, setWarnings] = useState<string[]>([]); const [busy, setBusy] = useState(false); const busyRef = useRef(false); const [error, setError] = useState(""); const [syncNotice, setSyncNotice] = useState(""); const [manual, setManual] = useState(false); const [editingProfile, setEditingProfile] = useState(false); const [success, setSuccess] = useState<ExportResult>();
  const [workStatus, setWorkStatus] = useState<{ message: string; detail: string }>();
  const counts = useMemo(() => ({ all: items.length, selected: items.filter(i => i.selected).length, review: items.filter(i => !i.reviewed).length }), [items]);
  const startWork = (message: string, detail: string) => { if (busyRef.current) return false; busyRef.current = true; setWorkStatus({ message, detail }); setBusy(true); setError(""); setSyncNotice(""); return true; };
  const endWork = () => { busyRef.current = false; setBusy(false); setWorkStatus(undefined); };
  const completionWarning = (values: Interaction[]) => {
    const count = values.filter(item => item.selected).length;
    return count < 3 ? `Atlas encontró y guardará ${count} ${count === 1 ? "actividad" : "actividades"}. Añade ${3 - count} ${3 - count === 1 ? "tarea real" : "tareas reales"} para llegar al mínimo diario de 3.` : "";
  };
  useEffect(() => {
    let disposed = false;
    void api.loadCachedDay(date).then(result => {
      if (disposed || !result || busyRef.current) return;
      setItems(result.interactions);
      const warning = completionWarning(result.interactions);
      setWarnings(warning ? [...result.warnings, warning] : result.warnings);
      setSyncNotice(`Restored the saved Atlas preview for ${date}.`);
    }).catch(failure => void api.logError("load-cached-day", String(failure)));
    return () => { disposed = true; };
  }, [date]);
  useEffect(() => {
    if (items.length === 0 || busyRef.current) return;
    const timer = window.setTimeout(() => {
      const sourceWarnings = warnings.filter(value => !value.startsWith("Atlas encontró y guardará"));
      void api.saveDayPreview(date, items, sourceWarnings).catch(failure => void api.logError("save-day-preview", String(failure)));
    }, 500);
    return () => window.clearTimeout(timer);
  }, [date, items, warnings]);
  const loadEvidence = async (targetDate: string, email: boolean, teams: boolean): Promise<ExtractionResult> => {
    const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    if (!bridgeMode) return api.extract(targetDate, email, teams, timezone);
    await api.requestBridgeDate(targetDate);
    let lastError = "";
    for (let attempt = 0; attempt <= 26; attempt += 1) {
      try { return await api.extract(targetDate, email, teams, timezone); }
      catch (failure) {
        lastError = String(failure);
        if (!lastError.includes("No se encontró un paquete de evidencia")) throw failure;
        if (attempt === 0) setSyncNotice(`Solicitud enviada para ${targetDate}. Power Automate la procesa en su próximo ciclo y Atlas comprobará la carpeta automáticamente.`);
        if (attempt === 26) break;
        await new Promise(resolve => window.setTimeout(resolve, 15_000));
      }
    }
    throw new Error(`Power Automate no entregó ${targetDate} después de 6 minutos. Confirma que el flujo Atlas actualizado esté activo. Último resultado: ${lastError}`);
  };
  const extract = async () => { if (!startWork(bridgeMode ? `Importing evidence for ${date}` : `Interpreting your workday for ${date}`, bridgeMode ? "Waiting for Power Automate and OneDrive, then separating meetings and work tasks." : "Atlas Local AI is reviewing meetings, mail, and Teams evidence on this computer.")) return; setWarnings([]); try { const result = await loadEvidence(date, includeEmail, includeTeams); setItems(result.interactions); const warning = completionWarning(result.interactions); setWarnings(warning ? [...result.warnings, warning] : result.warnings); setSyncNotice(bridgeMode ? `Evidencia de ${date} importada y organizada por fuente.` : ""); } catch (e) { setError(String(e)); } finally { endWork(); } };
  const changeTeams = (enabled: boolean) => setIncludeTeams(enabled);
  const saveSelected = async () => {
    const selectedItems = items.filter(i => i.selected);
    if (selectedItems.length === 0) return setError("Select at least one real interaction to save.");
    if (!startWork("Writing selected activities", "Updating the tracker without overwriting unrelated rows or existing work.")) return;
    try { setSuccess(await api.export(date, profile, items)); const warning = completionWarning(items); if (warning) setWarnings(old => [...old.filter(value => !value.startsWith("Atlas encontró y guardará")), warning]); await refreshStatus(); } catch (e) { setError(String(e)); } finally { endWork(); }
  };
  const syncCalendar = async (automatic = false, request?: AutomationRequest) => {
    if (!startWork(automatic ? "Running your daily Atlas update" : "Importing today’s work", "Collecting today’s evidence, interpreting work tasks, and updating the tracker.")) return;
    const targetDate = request?.date ?? localDate(); setDate(targetDate); setWarnings([]);
    try {
      const result = await loadEvidence(targetDate, true, true);
      setItems(result.interactions);
      const warning = completionWarning(result.interactions);
      setWarnings(warning ? [...result.warnings, warning] : result.warnings);
      const selectedCount = result.interactions.filter(item => item.selected).length;
      if (selectedCount > 0) {
        const saved = await api.export(targetDate, profile, result.interactions);
        if (automatic) setSyncNotice(saved.queued ? `Automatic daily run queued ${saved.inserted} rows for SharePoint.${warning ? ` ${warning}` : ""}` : `Automatic daily run complete: ${saved.inserted} added, ${saved.updated} refreshed.${warning ? ` ${warning}` : ""}`);
        else setSuccess(saved);
        await refreshStatus();
      } else {
        setSyncNotice("Atlas no encontró reuniones ni tareas de trabajo para guardar. Añádelas manualmente si realizaste trabajo que no aparece en Microsoft 365.");
      }
      if (request) await api.completeScheduled(request.runKey, true, selectedCount < 3);
    } catch (e) {
      setError(`${automatic ? "Automatic daily run failed: " : ""}${String(e)}`);
      if (request) await api.completeScheduled(request.runKey, false, true).catch(() => undefined);
    } finally { endWork(); }
  };
  const recheckLocalAi = async () => {
    if (!startWork("Checking Atlas Local AI", "Confirming that the bundled service and interpretation model are ready.")) return;
    try { await refreshStatus(); }
    catch (failure) { setError(String(failure)); }
    finally { endWork(); }
  };
  const saveProfile = async (p: UserProfile) => { await api.saveProfile(p); setEditingProfile(false); await refreshStatus(); };
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
    <header className="sticky top-0 z-30 border-b border-white/70 bg-cream/80 px-8 py-4 backdrop-blur-xl"><div className="mx-auto flex max-w-[1540px] items-center justify-between"><Brand /><div className="flex items-center gap-2"><div className="mr-3 max-w-xs text-right"><p className="text-xs font-bold">{bridgeMode ? "Power Automate Inbox" : status.account?.displayName}</p><p className="truncate text-[10px] text-ink/40">{bridgeMode ? status.bridgeFolder : status.account?.email}</p></div><button title="Tracker destination" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={editDestination}><FileSpreadsheet className="h-4 w-4" /></button><button title="Open failure log" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => openPath(status.logPath)}><FileText className="h-4 w-4" /></button><button title="Data source" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={editMicrosoft}>{bridgeMode ? <Inbox className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}</button><button title="Profile settings" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => setEditingProfile(true)}><Settings className="h-4 w-4" /></button>{!bridgeMode && <button title="Sign out" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-red-50 hover:text-red-600" onClick={signOut}><LogOut className="h-4 w-4" /></button>}</div></div></header>
    <main className="mx-auto max-w-[1540px] px-8 py-8">
      <div className="grid grid-cols-[minmax(0,1fr)_290px] gap-6">
        <section className="min-w-0">
          <div className="mb-7 flex items-end justify-between"><div><p className="text-xs font-bold uppercase tracking-[.18em] text-pine">Daily workspace</p><h1 className="mt-2 font-display text-4xl">Record your daily work.</h1><p className="mt-2 text-sm text-ink/50">Meetings and concrete work tasks become separate tracker rows.</p></div><div className="flex items-center gap-2"><span className="chip bg-white text-ink/55"><Inbox className="h-3 w-3" />{counts.all} found</span><span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{counts.selected} selected</span>{counts.review > 0 && <span className="chip bg-[#fff0df] text-[#9a5a1e]">{counts.review} to review</span>}</div></div>
          <div className="card p-5"><div className="grid grid-cols-[220px_1fr_1fr_auto] items-end gap-4"><label><span className="label">Workday</span><input type="date" max={localDate()} className="field" value={date} disabled={busy} onChange={e => { setDate(e.target.value); setItems([]); setSyncNotice(""); }} /></label><Toggle checked={includeEmail} onChange={setIncludeEmail} label="Infer from mail" detail="Work tasks, without inbox noise" icon={<Mail className="h-4 w-4" />} /><Toggle checked={includeTeams} onChange={changeTeams} label="Infer from Teams" detail="Separate task for each activity" icon={<Bot className="h-4 w-4" />} /><button className="btn-primary h-[46px] px-5" disabled={busy || ((includeEmail || includeTeams) && (!status.ollamaRunning || !status.ollamaModelAvailable))} onClick={extract}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}{bridgeMode ? "Import selected day" : "Extract selected day"}</button></div>
            {status.ollamaRunning && status.ollamaModelAvailable && <div className="mt-4 flex items-center gap-3 rounded-xl border border-pine/15 bg-mint/55 px-4 py-3 text-xs text-pine"><Bot className="h-5 w-5" /><span><b>Atlas Local AI is ready.</b> Mail and Teams evidence stay on this computer and are interpreted by <code>{status.ollamaModel}</code>.</span></div>}
            {(!status.ollamaRunning || !status.ollamaModelAvailable) && <div className="mt-4 flex items-center gap-3 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-xs text-red-800"><Bot className="h-5 w-5" /><span className="flex-1"><b>Bundled Local AI could not start.</b> {status.localAiError ?? "Extract the complete Atlas portable ZIP again."}</span><button className="font-bold underline" onClick={() => void recheckLocalAi()}>Recheck</button></div>}
          </div>
          {error && <div className="mt-5"><ErrorBanner message={error} onClose={() => setError("")} /></div>}{syncNotice && <div className="mt-5 flex items-center gap-3 rounded-xl border border-pine/15 bg-mint/55 px-4 py-3 text-sm text-pine"><Check className="h-4 w-4" />{syncNotice}</div>}{warnings.map((w, i) => <div className="mt-3" key={i}><ErrorBanner message={w} /></div>)}
          <div className="mt-6 flex items-center justify-between"><div><h2 className="font-display text-2xl">Preview</h2><p className="mt-1 text-xs text-ink/45">Atlas selects finished meetings and concrete work tasks, including tasks still in progress. Reminders and automatic notices stay out.</p></div><button className="btn-secondary" onClick={() => setManual(true)}><Plus className="h-4 w-4" />Añadir tarea manual</button></div>
          <div className="mt-4">{items.length ? <InteractionTable items={items} setItems={setItems} /> : <div className="card grid min-h-64 place-items-center p-10 text-center"><div><div className="mx-auto grid h-12 w-12 place-items-center rounded-2xl bg-mint text-pine"><CalendarDays /></div><h3 className="mt-4 font-display text-xl">Choose a day and {bridgeMode ? "import" : "extract"}</h3><p className="mt-2 max-w-sm text-xs leading-5 text-ink/45">Atlas shows only interactions backed by your calendar, selected mail, chat evidence, or details you type manually.</p></div></div>}</div>
        </section>
        <div className="space-y-5"><ExportPanel destination={destination} autoSync={status.autoSync} autoSyncTime={status.autoSyncTime} automationMode={status.automationMode} bridgeMode={bridgeMode} items={items} busy={busy} onSave={saveSelected} onSync={() => void syncCalendar(false)} onEdit={editDestination} /><aside className="rounded-2xl bg-[#081c1a] p-5 text-white"><ShieldCheck className="h-5 w-5 text-[#8dd7c4]" /><h3 className="mt-4 font-display text-xl">Mínimo diario: 3</h3><p className="mt-2 text-xs leading-5 text-white/55">Atlas siempre guarda las actividades reales encontradas. Si hay menos de tres, abre la aplicación para que puedas añadir las que faltan.</p></aside></div>
      </div>
    </main>
    {manual && <ManualModal date={date} onClose={() => setManual(false)} onAdd={item => { setItems(old => [...old, item]); setManual(false); }} />}
    {editingProfile && <div className="fixed inset-0 z-50 grid place-items-center bg-[#081c1a]/45 p-8 backdrop-blur-sm"><ProfileForm initial={profile} title="Profile settings" onSave={saveProfile} cancel={() => setEditingProfile(false)} /></div>}
    {success && <SuccessModal result={success} onClose={() => setSuccess(undefined)} />}
    <AtlasLoader show={busy} message={workStatus?.message ?? "Atlas is working"} detail={workStatus?.detail} />
  </div>;
}

function AtlasApp() {
  const [status, setStatus] = useState<AppStatus>(); const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const [installing, setInstalling] = useState(false); const [editingDestination, setEditingDestination] = useState(false);
  const refresh = async () => { setStatus(await api.status()); };
  useEffect(() => { refresh().catch(e => setError(String(e))); }, []);
  useEffect(() => { if (status?.scheduledLaunch && (!status.configured || !status.signedIn || !status.profile || !status.destination || !status.autoSync)) void api.completeScheduled("", false, true); }, [status]);
  useEffect(() => { const onError = (event: ErrorEvent) => { void api.logError("window", event.message); }; const onRejection = (event: PromiseRejectionEvent) => { void api.logError("promise", String(event.reason)); }; window.addEventListener("error", onError); window.addEventListener("unhandledrejection", onRejection); return () => { window.removeEventListener("error", onError); window.removeEventListener("unhandledrejection", onRejection); }; }, []);
  const signIn = async () => { setBusy(true); setError(""); try { setStatus(await api.signIn()); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  const signOut = async () => { await api.signOut(); await refresh(); };
  const saveProfile = async (p: UserProfile) => { await api.saveProfile(p); await refresh(); };
  const usePowerAutomate = async (folder: string) => { setStatus(await api.savePowerAutomateFolder(folder)); setInstalling(false); };
  const saveDestination = async (destination: TrackerDestination, autoSync: boolean, autoSyncTime: string, automationMode: AutomationMode) => { setStatus(await api.saveDestination(destination, autoSync, autoSyncTime, automationMode)); setEditingDestination(false); };
  if (!status) return error ? <main className="grid min-h-screen place-items-center p-10"><ErrorBanner message={error} /></main> : <LoadingScreen />;
  if (!status.configured || installing) return <ConnectorInstaller onClose={status.configured ? () => setInstalling(false) : undefined} onUseFolder={usePowerAutomate} />;
  if (!status.signedIn) return <LoginScreen onSignIn={signIn} onEditMicrosoft={() => setInstalling(true)} busy={busy} error={error} />;
  if (!status.profile) return <SetupScreen status={status} onSaved={saveProfile} />;
  if (!status.destination || editingDestination) return <DestinationSetupScreen initial={status.destination} initialAutoSync={status.autoSync} initialAutoSyncTime={status.autoSyncTime} initialAutomationMode={status.automationMode} bridgeMode={status.sourceMode === "power_automate_folder"} onSave={saveDestination} onCancel={status.destination ? () => setEditingDestination(false) : undefined} />;
  return <Workspace status={status} refreshStatus={refresh} signOut={signOut} editMicrosoft={() => setInstalling(true)} editDestination={() => setEditingDestination(true)} />;
}

export default function App() {
  return <AtlasIntro><AtlasApp /></AtlasIntro>;
}
