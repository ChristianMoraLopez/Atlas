import { useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  AlertCircle, ArrowRight, Bot, CalendarDays, Check, ChevronDown, ExternalLink,
  FilePlus2, FolderOpen, Inbox, Loader2, LogOut, Mail, PenLine, Plus, RefreshCw,
  Save, Settings, ShieldCheck, Sparkles, Trash2, UserRound, X
} from "lucide-react";
import { api, fromLocalInput, localDate, toLocalInput } from "./lib";
import type { AppStatus, ExportResult, Interaction, UserProfile } from "./types";

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
  return <main className="grid min-h-screen place-items-center"><div className="text-center">
    <div className="mx-auto mb-5 grid h-14 w-14 place-items-center rounded-2xl bg-pine text-white shadow-panel"><Loader2 className="h-6 w-6 animate-spin" /></div>
    <p className="text-sm font-bold text-ink/60">Preparing your tracker…</p>
  </div></main>;
}

function Brand() {
  return <div className="flex items-center gap-3">
    <div className="grid h-10 w-10 place-items-center rounded-xl bg-pine text-white"><span className="font-display text-xl italic">A</span></div>
    <div><p className="font-display text-lg leading-none">Atlas</p><p className="mt-1 text-[10px] font-bold uppercase tracking-[.18em] text-ink/45">Interaction tracker</p></div>
  </div>;
}

function ConfigurationScreen() {
  return <main className="mx-auto flex min-h-screen max-w-6xl items-center px-10 py-12">
    <section className="grid w-full grid-cols-[1.1fr_.9fr] overflow-hidden rounded-[2rem] bg-[#081c1a] text-white shadow-panel">
      <div className="p-14">
        <div className="mb-16"><Brand /></div>
        <p className="mb-4 text-xs font-bold uppercase tracking-[.2em] text-[#8dd7c4]">One-time build setup</p>
        <h1 className="max-w-xl font-display text-5xl leading-[1.05]">Connect Atlas to your Microsoft 365 tenant.</h1>
        <p className="mt-6 max-w-xl text-base leading-7 text-white/65">This build is missing its public Azure application identifiers. Add them as GitHub repository variables, then publish a new version tag.</p>
      </div>
      <div className="m-4 rounded-[1.4rem] bg-white p-10 text-ink">
        <h2 className="font-display text-2xl">Required variables</h2>
        <div className="mt-7 space-y-4">
          {[["CIRCANA_AZURE_CLIENT_ID", "Application (client) ID"], ["CIRCANA_AZURE_TENANT_ID", "Directory (tenant) ID"]].map(([key, desc]) =>
            <div key={key} className="rounded-xl border border-ink/10 bg-cream/60 p-4"><code className="text-sm font-bold text-pine">{key}</code><p className="mt-1 text-xs text-ink/55">{desc}</p></div>)}
        </div>
        <a className="btn-primary mt-8" href="https://entra.microsoft.com" target="_blank" rel="noreferrer">Open Microsoft Entra <ExternalLink className="h-4 w-4" /></a>
      </div>
    </section>
  </main>;
}

function LoginScreen({ onSignIn, busy, error }: { onSignIn: () => void; busy: boolean; error: string }) {
  return <main className="mx-auto flex min-h-screen max-w-6xl items-center px-10 py-12">
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
        <p className="mt-5 text-center text-[11px] leading-5 text-ink/40">Your refresh token is stored by Windows Credential Manager and is never written to a project file.</p>
      </div>
    </section>
  </main>;
}

function ProfileForm({ initial, onSave, title = "Set up your profile", cancel, model, onModelChange }: { initial?: UserProfile; onSave: (p: UserProfile) => Promise<void>; title?: string; cancel?: () => void; model?: string; onModelChange?: (value: string) => void }) {
  const [profile, setProfile] = useState(initial ?? emptyProfile);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const update = (key: keyof UserProfile, value: string) => setProfile(p => ({ ...p, [key]: value }));
  const submit = async (e: React.FormEvent) => {
    e.preventDefault(); setError("");
    if (!profile.loginId.trim() || !profile.fullName.trim()) return setError("Login ID and full name are required.");
    setBusy(true); try { await onSave(profile); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <form onSubmit={submit} className="card w-full max-w-2xl p-8">
    <div className="flex items-start justify-between"><div><p className="text-xs font-bold uppercase tracking-[.18em] text-pine">Personal defaults</p><h2 className="mt-2 font-display text-3xl">{title}</h2><p className="mt-2 text-sm text-ink/50">Used only in fields Atlas is allowed to write.</p></div>{cancel && <button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={cancel}><X /></button>}</div>
    <div className="mt-8 grid grid-cols-2 gap-5">
      <label><span className="label">Corp ID / Login ID *</span><input className="field" value={profile.loginId} onChange={e => update("loginId", e.target.value)} /></label>
      <label><span className="label">Full name *</span><input className="field" value={profile.fullName} onChange={e => update("fullName", e.target.value)} /></label>
      <label><span className="label">Area</span><input className="field" value={profile.area} onChange={e => update("area", e.target.value)} /></label>
      <label><span className="label">Team lead</span><input className="field" value={profile.teamLead} onChange={e => update("teamLead", e.target.value)} /></label>
      <label className="col-span-2"><span className="label">Circana manager</span><input className="field" value={profile.circanaManager} onChange={e => update("circanaManager", e.target.value)} /></label>
      {model !== undefined && onModelChange && <label className="col-span-2"><span className="label">Local Ollama model</span><input className="field" value={model} onChange={e => onModelChange(e.target.value)} placeholder="qwen2.5:3b" /><span className="mt-1.5 block text-[11px] text-ink/40">This model is contacted only at 127.0.0.1:11434.</span></label>}
    </div>
    {error && <div className="mt-5"><ErrorBanner message={error} /></div>}
    <div className="mt-7 flex justify-end gap-3">{cancel && <button type="button" className="btn-secondary" onClick={cancel}>Cancel</button>}<button className="btn-primary" disabled={busy}>{busy && <Loader2 className="h-4 w-4 animate-spin" />}Save profile</button></div>
  </form>;
}

function SetupScreen({ status, onSaved }: { status: AppStatus; onSaved: (p: UserProfile) => Promise<void> }) {
  return <main className="min-h-screen px-10 py-8"><header className="mx-auto flex max-w-6xl items-center justify-between"><Brand /><span className="chip bg-mint text-pine"><Check className="h-3 w-3" /> {status.account?.email}</span></header><div className="mx-auto grid min-h-[calc(100vh-6rem)] max-w-6xl place-items-center"><ProfileForm onSave={onSaved} /></div></main>;
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
  return <span className={`chip ${styles}`}><Icon className="h-3 w-3" />{item.sourceKind === "teams_chat" ? "AI · unverified" : item.sourceKind.replace("_", " ")}</span>;
}

function SelectCell({ value, options, onChange }: { value: string; options: string[]; onChange: (v: string) => void }) {
  return <div className="relative"><select className="w-full appearance-none bg-transparent py-1 pr-5 text-xs outline-none" value={value} onChange={e => onChange(e.target.value)}>{options.map(x => <option key={x}>{x}</option>)}</select><ChevronDown className="pointer-events-none absolute right-0 top-1.5 h-3 w-3 text-ink/35" /></div>;
}

function InteractionTable({ items, setItems, onLogAiManually }: { items: Interaction[]; setItems: React.Dispatch<React.SetStateAction<Interaction[]>>; onLogAiManually: () => void }) {
  const set = (index: number, patch: Partial<Interaction>) => setItems(old => old.map((v, i) => i === index ? { ...v, ...patch } : v));
  const removeManual = (index: number) => setItems(old => old.filter((_, i) => i !== index));
  return <div className="overflow-auto rounded-xl border border-ink/10 bg-white">
    <table className="w-full min-w-[1420px] border-collapse text-left">
      <thead className="sticky top-0 z-10 bg-[#f2f4ef] text-[10px] font-extrabold uppercase tracking-[.13em] text-ink/45"><tr>
        <th className="w-12 px-3 py-3">Use</th><th className="px-3 py-3">Source</th><th className="px-3 py-3">Type</th><th className="px-3 py-3">Start</th><th className="px-3 py-3">End</th><th className="px-3 py-3">Client type</th><th className="px-3 py-3">End client</th><th className="px-3 py-3">Category</th><th className="px-3 py-3">Subcategory</th><th className="px-3 py-3">Priority</th><th className="px-3 py-3">Incident</th><th className="w-[300px] px-3 py-3">Comments</th><th className="w-10" />
      </tr></thead>
      <tbody className="divide-y divide-ink/5">
      {items.map((item, i) => {
        const fullyEditable = item.sourceKind === "manual" || item.sourceKind === "teams_chat";
        return <tr key={item.sourceId} className={`${item.selected ? "" : "opacity-50"} align-top hover:bg-cream/35`}>
          <td className="px-3 py-3">{item.sourceKind === "teams_chat" ? <button onClick={onLogAiManually} title="Open a blank manual form; no AI content is copied" className="rounded-lg border border-amber-200 bg-amber-50 px-2 py-1 text-[10px] font-bold text-amber-800">Log manually</button> : <input type="checkbox" className="h-4 w-4 accent-pine" checked={item.selected} onChange={e => set(i, { selected: e.target.checked, reviewed: e.target.checked ? true : item.reviewed })} />}</td>
          <td className="whitespace-nowrap px-3 py-3"><SourceBadge item={item} />{item.sourceKind === "teams_chat" ? <input className="mt-1 block w-40 bg-transparent text-[10px] text-ink/45 outline-none" value={item.evidenceLabel} onChange={e => set(i, { evidenceLabel: e.target.value })} /> : <p className="mt-1 max-w-40 truncate text-[10px] text-ink/35" title={item.evidenceLabel}>{item.evidenceLabel}</p>}</td>
          <td className="px-3 py-3">{fullyEditable ? <SelectCell value={item.interactionType} options={["Meeting", "E-Mail", "Task"]} onChange={v => set(i, { interactionType: v as Interaction["interactionType"] })} /> : <span className="text-xs">{item.interactionType}</span>}</td>
          <td className="px-3 py-3">{fullyEditable ? <input type="datetime-local" className="w-[155px] bg-transparent text-xs outline-none" value={toLocalInput(item.interactionDateTime)} onChange={e => set(i, { interactionDateTime: fromLocalInput(e.target.value), receptionDateTime: fromLocalInput(e.target.value) })} /> : <span className="whitespace-nowrap text-xs">{new Date(item.interactionDateTime).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>}</td>
          <td className="px-3 py-3">{fullyEditable ? <input type="datetime-local" className="w-[155px] bg-transparent text-xs outline-none" value={toLocalInput(item.resolutionDateTime)} onChange={e => set(i, { resolutionDateTime: fromLocalInput(e.target.value) })} /> : <span className="whitespace-nowrap text-xs">{item.resolutionDateTime ? new Date(item.resolutionDateTime).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "—"}</span>}</td>
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
      <label><span className="label">Interaction type</span><select className="field" value={type} onChange={e => setType(e.target.value as Interaction["interactionType"])}><option>Task</option><option>Meeting</option><option>E-Mail</option></select></label>
      <label><span className="label">Date</span><input className="field bg-cream/70" value={date} disabled /></label>
      <label><span className="label">Start time *</span><input type="time" className="field" value={start} onChange={e => setStart(e.target.value)} /></label>
      <label><span className="label">End time</span><input type="time" className="field" value={end} onChange={e => setEnd(e.target.value)} /></label>
      <label><span className="label">Client type</span><select className="field" value={clientType} onChange={e => setClientType(e.target.value as Interaction["clientType"])}><option value="">Undetermined</option><option>Circana</option><option>End_Client</option><option>Capgemini</option></select></label>
      <label><span className="label">Real client name</span><input className="field" value={endClient} onChange={e => setEndClient(e.target.value)} /></label>
      <label className="col-span-2"><span className="label">What happened? *</span><textarea className="field min-h-24 resize-none" value={comments} onChange={e => setComments(e.target.value)} placeholder="Type a factual description of the work you completed" /></label>
    </div>
    {error && <div className="mt-4"><ErrorBanner message={error} /></div>}
    <div className="mt-6 flex justify-end gap-3"><button type="button" className="btn-secondary" onClick={onClose}>Cancel</button><button className="btn-primary"><Plus className="h-4 w-4" />Add interaction</button></div>
  </form></div>;
}

function ExportPanel({ date, profile, items, onDone }: { date: string; profile: UserProfile; items: Interaction[]; onDone: (r: ExportResult) => void }) {
  const [mode, setMode] = useState<"new" | "existing">("new"); const [path, setPath] = useState(""); const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  useEffect(() => setPath(""), [mode]);
  const choose = async () => {
    if (mode === "existing") { const chosen = await open({ multiple: false, filters: [{ name: "Excel workbook", extensions: ["xlsx", "xlsm"] }] }); if (chosen) setPath(chosen as string); }
    else { const chosen = await save({ defaultPath: `Tracker_${profile.loginId}_${date}.xlsx`, filters: [{ name: "Excel workbook", extensions: ["xlsx"] }] }); if (chosen) setPath(chosen); }
  };
  const submit = async () => { setError(""); if (!path) return setError("Choose an output file first."); if (!items.some(x => x.selected)) return setError("Select at least one real interaction to export."); setBusy(true); try { onDone(await api.export(path, mode === "existing", date, profile, items)); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  return <aside className="card h-fit p-5"><p className="text-xs font-bold uppercase tracking-[.16em] text-pine">Export</p><h3 className="mt-2 font-display text-2xl">Ready for Excel</h3><p className="mt-2 text-xs leading-5 text-ink/45">Only selected and verified rows are written. Formula-driven orange columns remain untouched.</p>
    <div className="mt-5 space-y-2">{(["new", "existing"] as const).map(value => <button key={value} onClick={() => setMode(value)} className={`flex w-full items-center gap-3 rounded-xl border p-3 text-left ${mode === value ? "border-pine/30 bg-mint/50" : "border-ink/10 bg-white"}`}><span className={`grid h-8 w-8 place-items-center rounded-lg ${mode === value ? "bg-pine text-white" : "bg-cream text-ink/45"}`}>{value === "new" ? <FilePlus2 className="h-4 w-4" /> : <FolderOpen className="h-4 w-4" />}</span><span><b className="block text-xs">{value === "new" ? "Create new tracker file" : "Open existing Excel"}</b><span className="text-[10px] text-ink/40">{value === "new" ? "Start a clean .xlsx" : "Upsert your tab only"}</span></span></button>)}</div>
    <button className="btn-secondary mt-4 w-full" onClick={choose}><FolderOpen className="h-4 w-4" />{path ? "Change file" : "Choose file"}</button>{path && <p className="mt-2 break-all rounded-lg bg-cream/70 p-2 text-[10px] text-ink/50">{path}</p>}
    {error && <div className="mt-4"><ErrorBanner message={error} /></div>}
    <button className="btn-primary mt-4 w-full" onClick={submit} disabled={busy}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}Save tracker</button>
  </aside>;
}

function SuccessModal({ result, onClose }: { result: ExportResult; onClose: () => void }) {
  return <div className="fixed inset-0 z-50 grid place-items-center bg-[#081c1a]/45 p-8 backdrop-blur-sm"><div className="card w-full max-w-lg p-8 text-center"><div className="mx-auto grid h-14 w-14 place-items-center rounded-full bg-mint text-pine"><Check className="h-7 w-7" /></div><h2 className="mt-5 font-display text-3xl">Tracker saved</h2><p className="mt-2 text-sm text-ink/50">{result.inserted} added · {result.updated} refreshed · {result.skipped} skipped</p><p className="mt-5 break-all rounded-xl bg-cream p-3 text-xs text-ink/55">{result.path}</p><div className="mt-6 grid grid-cols-2 gap-3"><button className="btn-secondary" onClick={() => revealItemInDir(result.path)}><FolderOpen className="h-4 w-4" />Open folder</button><button className="btn-primary" onClick={() => openPath(result.path)}><ExternalLink className="h-4 w-4" />Open file</button></div><button className="mt-5 text-xs font-bold text-ink/45 hover:text-ink" onClick={onClose}>Back to tracker</button></div></div>;
}

function Workspace({ status, refreshStatus, signOut }: { status: AppStatus; refreshStatus: () => Promise<void>; signOut: () => Promise<void> }) {
  const profile = status.profile!; const [date, setDate] = useState(localDate()); const [includeEmail, setIncludeEmail] = useState(false); const [includeTeams, setIncludeTeams] = useState(false); const [items, setItems] = useState<Interaction[]>([]); const [warnings, setWarnings] = useState<string[]>([]); const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const [manual, setManual] = useState(false); const [editingProfile, setEditingProfile] = useState(false); const [settingsModel, setSettingsModel] = useState(status.ollamaModel); const [success, setSuccess] = useState<ExportResult>();
  const counts = useMemo(() => ({ all: items.length, selected: items.filter(i => i.selected).length, review: items.filter(i => !i.reviewed).length }), [items]);
  const extract = async () => { setBusy(true); setError(""); setWarnings([]); try { const result = await api.extract(date, includeEmail, includeTeams, Intl.DateTimeFormat().resolvedOptions().timeZone); setItems(result.interactions); setWarnings(result.warnings); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  const saveProfile = async (p: UserProfile) => { await api.saveProfile(p); await api.setOllamaModel(settingsModel); setEditingProfile(false); await refreshStatus(); };
  return <div className="min-h-screen">
    <header className="sticky top-0 z-30 border-b border-white/70 bg-cream/80 px-8 py-4 backdrop-blur-xl"><div className="mx-auto flex max-w-[1540px] items-center justify-between"><Brand /><div className="flex items-center gap-2"><div className="mr-3 text-right"><p className="text-xs font-bold">{status.account?.displayName}</p><p className="text-[10px] text-ink/40">{status.account?.email}</p></div><button title="Profile settings" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-mint" onClick={() => setEditingProfile(true)}><Settings className="h-4 w-4" /></button><button title="Sign out" className="rounded-xl border border-ink/10 bg-white p-2.5 hover:bg-red-50 hover:text-red-600" onClick={signOut}><LogOut className="h-4 w-4" /></button></div></div></header>
    <main className="mx-auto max-w-[1540px] px-8 py-8">
      <div className="grid grid-cols-[minmax(0,1fr)_290px] gap-6">
        <section className="min-w-0">
          <div className="mb-7 flex items-end justify-between"><div><p className="text-xs font-bold uppercase tracking-[.18em] text-pine">Daily workspace</p><h1 className="mt-2 font-display text-4xl">Build your interaction record.</h1><p className="mt-2 text-sm text-ink/50">Evidence first. Review it once. Export without duplicates.</p></div><div className="flex items-center gap-2"><span className="chip bg-white text-ink/55"><Inbox className="h-3 w-3" />{counts.all} found</span><span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{counts.selected} selected</span>{counts.review > 0 && <span className="chip bg-[#fff0df] text-[#9a5a1e]">{counts.review} to review</span>}</div></div>
          <div className="card p-5"><div className="grid grid-cols-[220px_1fr_1fr_auto] items-end gap-4"><label><span className="label">Workday</span><input type="date" max={localDate()} className="field" value={date} onChange={e => { setDate(e.target.value); setItems([]); }} /></label><Toggle checked={includeEmail} onChange={setIncludeEmail} label="Include mail" detail="You choose each message" icon={<Mail className="h-4 w-4" />} /><Toggle checked={includeTeams} onChange={setIncludeTeams} label="Teams chat suggestions" detail="Local Ollama · review required" icon={<Bot className="h-4 w-4" />} /><button className="btn-primary h-[46px] px-5" disabled={busy || (includeTeams && !status.ollamaRunning)} onClick={extract}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}Extract</button></div>
            {includeTeams && !status.ollamaRunning && <div className="mt-4 flex items-center gap-3 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-xs text-amber-900"><Bot className="h-5 w-5" /><span className="flex-1"><b>Ollama is not running.</b> Install it and start <code>{status.ollamaModel}</code>. Chat text is only sent to <code>127.0.0.1</code>.</span><a className="font-bold underline" href="https://ollama.com/download/windows" target="_blank" rel="noreferrer">Install Ollama</a></div>}
            {includeTeams && status.ollamaRunning && !status.ollamaModelAvailable && <div className="mt-4 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-xs text-amber-900">Ollama is running, but model <code>{status.ollamaModel}</code> is unavailable. Run <code>ollama pull {status.ollamaModel}</code>.</div>}
          </div>
          {error && <div className="mt-5"><ErrorBanner message={error} onClose={() => setError("")} /></div>}{warnings.map((w, i) => <div className="mt-3" key={i}><ErrorBanner message={w} /></div>)}
          <div className="mt-6 flex items-center justify-between"><div><h2 className="font-display text-2xl">Preview</h2><p className="mt-1 text-xs text-ink/45">Mail remains unchecked until confirmed. AI chat suggestions are reference-only and must be logged through a blank manual form.</p></div><button className="btn-secondary" onClick={() => setManual(true)}><Plus className="h-4 w-4" />Add interaction manually</button></div>
          <div className="mt-4">{items.length ? <InteractionTable items={items} setItems={setItems} onLogAiManually={() => setManual(true)} /> : <div className="card grid min-h-64 place-items-center p-10 text-center"><div><div className="mx-auto grid h-12 w-12 place-items-center rounded-2xl bg-mint text-pine"><CalendarDays /></div><h3 className="mt-4 font-display text-xl">Choose a day and extract</h3><p className="mt-2 max-w-sm text-xs leading-5 text-ink/45">Atlas shows only interactions backed by your calendar, selected mail, chat evidence, or details you type manually.</p></div></div>}</div>
        </section>
        <div className="space-y-5"><ExportPanel date={date} profile={profile} items={items} onDone={setSuccess} /><aside className="rounded-2xl bg-[#081c1a] p-5 text-white"><ShieldCheck className="h-5 w-5 text-[#8dd7c4]" /><h3 className="mt-4 font-display text-xl">Provenance protected</h3><p className="mt-2 text-xs leading-5 text-white/55">The exporter independently validates every source. There is no filler generator and no minimum row target.</p></aside></div>
      </div>
    </main>
    {manual && <ManualModal date={date} onClose={() => setManual(false)} onAdd={item => { setItems(old => [...old, item]); setManual(false); }} />}
    {editingProfile && <div className="fixed inset-0 z-50 grid place-items-center bg-[#081c1a]/45 p-8 backdrop-blur-sm"><ProfileForm initial={profile} title="Profile settings" onSave={saveProfile} cancel={() => setEditingProfile(false)} model={settingsModel} onModelChange={setSettingsModel} /></div>}
    {success && <SuccessModal result={success} onClose={() => setSuccess(undefined)} />}
  </div>;
}

export default function App() {
  const [status, setStatus] = useState<AppStatus>(); const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const refresh = async () => { setStatus(await api.status()); };
  useEffect(() => { refresh().catch(e => setError(String(e))); }, []);
  const signIn = async () => { setBusy(true); setError(""); try { setStatus(await api.signIn()); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  const signOut = async () => { await api.signOut(); await refresh(); };
  const saveProfile = async (p: UserProfile) => { await api.saveProfile(p); await refresh(); };
  if (!status) return error ? <main className="grid min-h-screen place-items-center p-10"><ErrorBanner message={error} /></main> : <LoadingScreen />;
  if (!status.configured) return <ConfigurationScreen />;
  if (!status.signedIn) return <LoginScreen onSignIn={signIn} busy={busy} error={error} />;
  if (!status.profile) return <SetupScreen status={status} onSaved={saveProfile} />;
  return <Workspace status={status} refreshStatus={refresh} signOut={signOut} />;
}
