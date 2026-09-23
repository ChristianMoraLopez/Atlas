import { useCallback, useEffect, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import {
  AlertCircle, Bot, Check, ChevronDown, Clock, FileSpreadsheet, FolderOpen, History, Loader2,
  Mail, Plus, RefreshCw, RotateCcw, Save, Search, Settings2, Sparkles, Trash2, UserRound, X, Zap
} from "lucide-react";
import { api, formatDateTime } from "./lib";
import { useT } from "./i18n";
import LottieIcon from "./LottieIcon";
import type { QaAuditee, QaCase, QaCaseEntry, QaConfig, QaEngineStatus } from "./types";

const YNA = ["Y", "N", "N/A"];
const SENTIMENTS = ["Positive", "Neutral", "Negative"];
const AUTO_FAILS: [string, string][] = [
  ["none", "No Autofail"],
  ["no_initial_response", "No initial response"],
  ["missed_priority", "Missed/Incorrect Priority Handling"],
  ["no_status_updates", "Failure to Provide Status Updates"],
  ["non_adherence_sops", "Non-Adherence to SOPs"],
];
const PAGE = 25;

type Filter = "review" | "new" | "reviewed" | "all";

const emptyAuditee: QaAuditee = { name: "", email: "", customRules: "", watched: true, historicalDone: false };

function Banner({ tone, message, onClose }: { tone: "error" | "info" | "success"; message: string; onClose?: () => void }) {
  const styles = tone === "error" ? "border-red-200 bg-red-50 text-red-800" : "border-pine/20 bg-mint/60 text-pine";
  return <div className={`pop flex items-start gap-3 rounded-xl border px-4 py-3 text-sm ${styles}`}>
    {tone === "error" ? <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" /> : <Check className="mt-0.5 h-4 w-4 shrink-0" />}
    <span className="flex-1 whitespace-pre-line">{message}</span>
    {onClose && <button onClick={onClose}><X className="h-4 w-4" /></button>}
  </div>;
}

function Select({ value, options, onChange }: { value: string; options: [string, string][]; onChange: (v: string) => void }) {
  return <div className="relative">
    <select className="field appearance-none pr-8" value={value} onChange={e => onChange(e.target.value)}>
      {options.map(([v, label]) => <option key={v} value={v}>{label}</option>)}
    </select>
    <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/35" />
  </div>;
}

function Stat({ label, value, tone }: { label: string; value: number | string; tone?: "warn" }) {
  return <div className="rounded-xl bg-white/[.07] px-4 py-3">
    <p className={`font-display text-2xl ${tone === "warn" ? "text-[#f3b58c]" : "text-white"}`}>{value}</p>
    <p className="mt-0.5 text-[10px] font-bold uppercase tracking-[.14em] text-white/45">{label}</p>
  </div>;
}

function activityLabel(activity: string | undefined, t: ReturnType<typeof useT>): string {
  if (!activity) return t("Waiting for the next step");
  if (activity === "syncing") return t("Syncing the mailbox");
  if (activity === "exporting") return t("Writing the Excel workbooks");
  const match = /^evaluating:(\d+):(\d+)$/.exec(activity);
  if (match) return t("Evaluating conversation {current} of {total}", { current: match[1], total: match[2] });
  return activity;
}

function StatusHero({ status, busy, onSync, onExport, onOpenFolder, onRestart }: {
  status: QaEngineStatus; busy: boolean; onSync: () => void; onExport: () => void; onOpenFolder: () => void; onRestart: () => void;
}) {
  const t = useT();
  const [showWarnings, setShowWarnings] = useState(false);
  const planned = Math.max(status.historicalMonthsPlanned, 1);
  const progress = status.historicalStatus === "done" ? 100 : Math.round((status.historicalMonthsDone / Math.max(planned, Math.min(status.historicalHorizonMonths, planned + 1))) * 100);
  const nextCheck = status.nextCheck?.startsWith("+1 ") ? t("tomorrow {time}", { time: status.nextCheck.slice(3) }) : status.nextCheck;
  const historyText = status.historicalStatus === "running"
    ? t("Importing history: {done} of {planned} months read (up to {horizon})", { done: status.historicalMonthsDone, planned: status.historicalMonthsPlanned, horizon: status.historicalHorizonMonths })
    : status.historicalStatus === "done"
      ? t("History imported back to {month} · finished {date}", { month: status.historicalOldestMonth ?? "—", date: formatDateTime(status.historicalCompletedAt) })
      : t("History starts as soon as you add people to audit");
  return <section className="rise overflow-hidden rounded-[1.6rem] bg-[#081c1a] p-6 text-white shadow-panel">
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div className="flex items-center gap-4">
        <div className="relative grid h-12 w-12 place-items-center rounded-2xl bg-white/10">
          {status.activity ? <Loader2 className="h-5 w-5 animate-spin text-[#8dd7c4]" /> : <Bot className="h-5 w-5 text-[#8dd7c4]" />}
          <span className={`absolute -right-0.5 -top-0.5 h-3 w-3 rounded-full border-2 border-[#081c1a] ${status.connected ? "bg-[#5fd3a4]" : "bg-[#e9704f]"}`} />
        </div>
        <div>
          <p className="text-[10px] font-bold uppercase tracking-[.2em] text-[#8dd7c4]">{status.source === "power_automate" ? t("Power Automate · your AtlasQA flows") : status.source === "microsoft_graph" ? "Microsoft Graph" : t("Connector not ready")}</p>
          <p className="mt-1 font-display text-xl">{activityLabel(status.activity, t)}</p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <button className="inline-flex items-center gap-2 rounded-xl bg-white/10 px-4 py-2.5 text-sm font-bold hover:bg-white/20" disabled={busy || !status.connected} onClick={onSync}><RefreshCw className="h-4 w-4" />{t("Sync now")}</button>
        <button className="inline-flex items-center gap-2 rounded-xl bg-[#8dd7c4] px-4 py-2.5 text-sm font-bold text-[#081c1a] hover:bg-[#a9e3d4]" disabled={busy} onClick={onExport}><FileSpreadsheet className="h-4 w-4" />{t("Write to Excel")}</button>
        <button className="rounded-xl bg-white/10 p-2.5 hover:bg-white/20" title={t("Open folder")} onClick={onOpenFolder}><FolderOpen className="h-4 w-4" /></button>
      </div>
    </div>
    <div className="mt-6">
      <div className="flex items-center justify-between text-xs text-white/60"><span className="flex items-center gap-2"><History className="h-3.5 w-3.5" />{historyText}</span>{status.historicalStatus === "done" && <button className="font-bold text-[#8dd7c4] underline" onClick={onRestart}>{t("Import again")}</button>}</div>
      <div className="mt-2 h-2 overflow-hidden rounded-full bg-white/10"><div className={`h-full rounded-full transition-all duration-700 ${status.historicalStatus === "running" ? "bg-[#8dd7c4]" : "bg-[#5fd3a4]"}`} style={{ width: `${status.historicalStatus === "idle" ? 0 : Math.min(100, progress)}%` }} /></div>
    </div>
    <div className="mt-5 grid grid-cols-5 gap-3">
      <Stat label={t("QA messages")} value={status.messages} />
      <Stat label={t("Conversations")} value={status.conversations} />
      <Stat label={t("To evaluate")} value={status.pendingEvaluation} />
      <Stat label={t("Need review")} value={status.needsReview} tone={status.needsReview ? "warn" : undefined} />
      <Stat label={t("Pending Excel")} value={status.unexported} />
    </div>
    <div className="mt-5 flex flex-wrap items-center gap-x-5 gap-y-2 text-[11px] text-white/55">
      <span className="flex items-center gap-1.5"><Clock className="h-3.5 w-3.5" />{t("Next mailbox check: {time}", { time: nextCheck ?? "—" })}</span>
      <span className="flex items-center gap-1.5"><Zap className="h-3.5 w-3.5" />{status.lastLiveSignalAt ? t("Last analyst mail: {date}", { date: formatDateTime(status.lastLiveSignalAt) }) : t("Live alerts active")}</span>
      <span className="flex items-center gap-1.5"><RefreshCw className="h-3.5 w-3.5" />{t("Last sync: {date}", { date: formatDateTime(status.lastIncrementalAt) })}</span>
      <span className="flex items-center gap-1.5"><FileSpreadsheet className="h-3.5 w-3.5" />{t("Last Excel update: {date}", { date: formatDateTime(status.lastExportAt) })}</span>
    </div>
    {(status.lastError || status.lastExportError || status.warnings.length > 0) && <div className="mt-4 rounded-xl bg-[#e9704f]/15 p-3 text-xs text-[#ffd9c7]">
      {status.lastError && <p className="flex gap-2"><AlertCircle className="h-4 w-4 shrink-0" />{status.lastError}</p>}
      {status.lastExportError && <p className="mt-1 flex gap-2"><AlertCircle className="h-4 w-4 shrink-0" />{t("Excel: {error}", { error: status.lastExportError })}</p>}
      {status.warnings.length > 0 && <button className="mt-1 font-bold underline" onClick={() => setShowWarnings(v => !v)}>{showWarnings ? t("Hide notices") : t("Show {count} notice(s)", { count: status.warnings.length })}</button>}
      {showWarnings && <ul className="mt-2 list-disc space-y-1 pl-5">{status.warnings.slice().reverse().map((w, i) => <li key={i}>{w}</li>)}</ul>}
    </div>}
  </section>;
}

function CaseCard({ item, onSave, onReevaluate }: { item: QaCaseEntry; onSave: (next: QaCase) => Promise<void>; onReevaluate: (id: string) => void }) {
  const t = useT();
  const [draft, setDraft] = useState<QaCase>(item);
  const [dirty, setDirty] = useState(false);
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [showEvidence, setShowEvidence] = useState(false);
  useEffect(() => { if (!dirty) setDraft(item); }, [item, dirty]);
  const set = (patch: Partial<QaCase>) => { setDraft(old => ({ ...old, ...patch })); setDirty(true); };
  const save = async (patch: Partial<QaCase>) => {
    setSaving(true);
    try { await onSave({ ...draft, ...patch }); setDirty(false); } finally { setSaving(false); }
  };
  const flags = [draft.initialResponse, draft.adherence, draft.status, draft.updateFollowUp];
  const fails = flags.filter(v => v === "N").length;
  const criterion = (label: string, valueKey: keyof QaCase, notesKey: keyof QaCase, options: string[] = YNA) => (
    <div className="grid grid-cols-[150px_1fr] items-start gap-3 rounded-xl border border-ink/8 bg-white p-3">
      <div><span className="label">{t(label)}</span><Select value={String(draft[valueKey])} options={options.map(o => [o, o])} onChange={v => set({ [valueKey]: v } as Partial<QaCase>)} /></div>
      <label><span className="label">{t("Notes/Examples")}</span><textarea className="field min-h-[38px] resize-y text-xs" value={String(draft[notesKey])} onChange={e => set({ [notesKey]: e.target.value } as Partial<QaCase>)} /></label>
    </div>
  );
  return <div className={`card overflow-hidden ${item.newEvidence ? "ring-2 ring-[#e89969]/60" : ""}`}>
    <button className="flex w-full items-center gap-4 px-5 py-4 text-left hover:bg-cream/40" onClick={() => setOpen(v => !v)}>
      <span className={`grid h-9 w-9 shrink-0 place-items-center rounded-xl ${item.reviewed ? "bg-mint text-pine" : "bg-[#fff0df] text-[#9a5a1e]"}`}>{item.reviewed ? <Check className="h-4 w-4" /> : <UserRound className="h-4 w-4" />}</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-bold">{item.requestId || t("Untitled request")}</span>
        <span className="mt-0.5 block truncate text-[11px] text-ink/45">{item.analystName} · {item.requestDate || "—"} · {item.requestSource}</span>
      </span>
      {item.newEvidence && <span className="chip bg-[#fff0df] text-[#9a5a1e]"><Sparkles className="h-3 w-3" />{t("New mail")}</span>}
      {item.autoFail !== "none" && <span className="chip bg-red-50 text-red-700">{t("Auto fail")}</span>}
      <span className={`chip ${fails ? "bg-[#fff0df] text-[#9a5a1e]" : "bg-mint text-pine"}`}>{fails ? t("{count} N", { count: fails }) : t("All criteria met")}</span>
      <span className={`chip ${item.exported ? "bg-cream text-ink/50" : "bg-[#e7f0ff] text-[#315f9e]"}`}>{item.exported ? t("In Excel") : t("Pending Excel")}</span>
      <ChevronDown className={`h-4 w-4 shrink-0 text-ink/35 transition ${open ? "rotate-180" : ""}`} />
    </button>
    {open && <div className="border-t border-ink/8 bg-cream/30 p-5">
      <div className="grid grid-cols-3 gap-3">
        <label><span className="label">{t("Request ID")}</span><input className="field" value={draft.requestId} onChange={e => set({ requestId: e.target.value })} /></label>
        <label><span className="label">{t("Request Date")}</span><input type="date" className="field" value={draft.requestDate} onChange={e => set({ requestDate: e.target.value })} /></label>
        <label><span className="label">{t("Request Source")}</span><Select value={draft.requestSource} options={[["Email", "Email"], ["IRIS", "IRIS"]]} onChange={v => set({ requestSource: v })} /></label>
      </div>
      <div className="mt-3 space-y-3">
        {criterion("Initial Response", "initialResponse", "initialResponseNotes")}
        {criterion("Customer Sentiment", "customerSentiment", "customerSentimentNotes", SENTIMENTS)}
        {criterion("Adherence", "adherence", "adherenceNotes")}
        {criterion("Status", "status", "statusNotes")}
        {criterion("Update/Follow Up", "updateFollowUp", "updateFollowUpNotes")}
        <div className="rounded-xl border border-ink/8 bg-white p-3"><span className="label">{t("QA Auto fail?")}</span><Select value={draft.autoFail} options={AUTO_FAILS} onChange={v => set({ autoFail: v })} /></div>
      </div>
      {item.evidence.length > 0 && <div className="mt-4">
        <button type="button" onClick={() => setShowEvidence(v => !v)} className="flex items-center gap-2 text-xs font-bold text-pine underline"><Mail className="h-3.5 w-3.5" />{showEvidence ? t("Hide evidence") : t("Show evidence ({count})", { count: item.evidence.length })}</button>
        {showEvidence && <div className="mt-2 space-y-2">{item.evidence.map((ev, i) => <div key={i} className="rounded-xl border border-ink/8 bg-white p-3"><p className="text-[11px] font-bold text-ink/60">{ev.label}</p><p className="mt-1 text-xs leading-5 text-ink/55">{ev.excerpt}{ev.excerpt.length >= 220 ? "…" : ""}</p></div>)}</div>}
      </div>}
      <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
        <label className="flex cursor-pointer items-center gap-2 text-xs font-bold text-ink/60"><input type="checkbox" className="h-4 w-4 accent-[#0f4f45]" checked={draft.selected} onChange={e => void save({ selected: e.target.checked })} />{t("Include in Excel")}</label>
        <div className="flex gap-2">
          <button className="btn-secondary" disabled={saving} onClick={() => onReevaluate(item.caseId)} title={t("Discard this evaluation and let the local AI evaluate the whole conversation again")}><RotateCcw className="h-4 w-4" />{t("Evaluate again")}</button>
          <button className="btn-primary" disabled={saving} onClick={() => void save({ reviewed: true })}>{saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}{dirty || !item.reviewed ? t("Save review") : t("Reviewed")}</button>
        </div>
      </div>
    </div>}
  </div>;
}

function SetupCard({ config, onSaved, startOpen }: { config: QaConfig; onSaved: () => Promise<void>; startOpen: boolean }) {
  const t = useT();
  const [draft, setDraft] = useState(config);
  const [keywords, setKeywords] = useState(config.subjectKeywords.join(", "));
  const [open, setOpen] = useState(startOpen);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => { if (!dirty) { setDraft(config); setKeywords(config.subjectKeywords.join(", ")); } }, [config, dirty]);
  const update = (patch: Partial<QaConfig>) => { setDraft(old => ({ ...old, ...patch })); setDirty(true); };
  const updateAuditee = (index: number, patch: Partial<QaAuditee>) => update({ auditees: draft.auditees.map((a, i) => i === index ? { ...a, ...patch } : a) });
  const pickFolder = async () => { const folder = await openDialog({ directory: true, multiple: false }); if (typeof folder === "string") update({ outputFolder: folder }); };
  const save = async () => {
    setSaving(true); setError("");
    try {
      await api.qaSaveConfig({ ...draft, subjectKeywords: keywords.split(",").map(k => k.trim()).filter(Boolean) });
      setDirty(false);
      await onSaved();
    } catch (e) { setError(String(e)); } finally { setSaving(false); }
  };
  return <section className="card rise rise-1">
    <button className="flex w-full items-center gap-3 px-5 py-4 text-left" onClick={() => setOpen(v => !v)}>
      <span className="grid h-9 w-9 place-items-center rounded-xl bg-mint text-pine"><Settings2 className="h-4 w-4" /></span>
      <span className="flex-1"><span className="block font-display text-xl">{t("People and rules")}</span><span className="block text-[11px] text-ink/45">{t("{count} people audited · checks at {morning} and {afternoon}", { count: config.auditees.length, morning: config.checkMorning, afternoon: config.checkAfternoon })}</span></span>
      {dirty && <span className="chip bg-[#fff0df] text-[#9a5a1e]">{t("Unsaved changes")}</span>}
      <ChevronDown className={`h-4 w-4 text-ink/35 transition ${open ? "rotate-180" : ""}`} />
    </button>
    {open && <div className="border-t border-ink/8 p-5">
      <div className="flex items-center justify-between"><p className="text-xs text-ink/50">{t("Atlas imports the full QA history of each new person, then keeps it current twice a day and whenever they write to you.")}</p><button className="btn-secondary" onClick={() => update({ auditees: [...draft.auditees, { ...emptyAuditee }] })}><Plus className="h-4 w-4" />{t("Add person")}</button></div>
      <div className="mt-4 space-y-3">
        {draft.auditees.length === 0 && <p className="rounded-xl border border-dashed border-ink/15 p-4 text-center text-xs text-ink/40">{t("No one on the list yet. Add the first person to audit.")}</p>}
        {draft.auditees.map((a, i) => <div key={i} className="rounded-xl border border-ink/8 bg-white p-4">
          <div className="grid grid-cols-[1fr_1fr_auto] gap-3">
            <label><span className="label">{t("Name")}</span><input className="field" value={a.name} placeholder="Christian Mora" onChange={e => updateAuditee(i, { name: e.target.value })} /></label>
            <label><span className="label">{t("Email")}</span><input className="field" value={a.email} placeholder="nombre@circana.com" onChange={e => updateAuditee(i, { email: e.target.value })} /></label>
            <div className="flex items-end gap-2 pb-0.5">
              <label className="flex cursor-pointer items-center gap-2 pb-2.5 text-xs font-bold text-ink/60" title={t("Sync as soon as this person writes to you")}><input type="checkbox" className="h-4 w-4 accent-[#0f4f45]" checked={a.watched} onChange={e => updateAuditee(i, { watched: e.target.checked })} />{t("Live alerts")}</label>
              <button className="rounded-xl border border-ink/10 bg-white p-2.5 text-ink/40 hover:bg-red-50 hover:text-red-600" title={t("Remove")} onClick={() => update({ auditees: draft.auditees.filter((_, j) => j !== i) })}><Trash2 className="h-4 w-4" /></button>
            </div>
          </div>
          <div className="mt-3 flex items-center gap-2">{a.historicalDone ? <span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{t("History imported")}</span> : <span className="chip bg-[#fff0df] text-[#9a5a1e]"><History className="h-3 w-3" />{t("History pending")}</span>}</div>
          <label className="mt-3 block"><span className="label">{t("Manager rules for this person (optional)")}</span><textarea className="field min-h-[44px] resize-y text-xs" value={a.customRules} placeholder={t("Example: response-time ranges do not apply; this CSA works ticket-based via IRIS.")} onChange={e => updateAuditee(i, { customRules: e.target.value })} /></label>
        </div>)}
      </div>
      <div className="mt-5 grid grid-cols-4 gap-4">
        <label className="col-span-2"><span className="label">{t("Vertical/Team")}</span><input className="field" value={draft.vertical} onChange={e => update({ vertical: e.target.value })} /></label>
        <label className="col-span-2"><span className="label">{t("QA subject keywords")}</span><input className="field" value={keywords} placeholder="QA, audit" onChange={e => { setKeywords(e.target.value); setDirty(true); }} /></label>
        <label className="col-span-2"><span className="label">{t("Excel output folder")}</span><button className="field flex items-center gap-2 text-left" onClick={pickFolder}><FolderOpen className="h-4 w-4 shrink-0 text-ink/40" /><span className="truncate">{draft.outputFolder || t("Choose a folder…")}</span></button></label>
        <label><span className="label">{t("Morning check")}</span><input type="time" className="field" value={draft.checkMorning} onChange={e => update({ checkMorning: e.target.value })} /></label>
        <label><span className="label">{t("Afternoon check")}</span><input type="time" className="field" value={draft.checkAfternoon} onChange={e => update({ checkAfternoon: e.target.value })} /></label>
        <label><span className="label">{t("History limit (months)")}</span><input type="number" min={1} max={120} className="field" value={draft.historyMonths} onChange={e => update({ historyMonths: Math.max(1, Math.min(120, Number(e.target.value) || 36)) })} /></label>
        <label className="col-span-3 flex items-end gap-6 pb-3">
          <span className="flex cursor-pointer items-center gap-2 text-xs font-bold text-ink/60"><input type="checkbox" className="h-4 w-4 accent-[#0f4f45]" checked={draft.watchEnabled} onChange={e => update({ watchEnabled: e.target.checked })} />{t("Sync when an audited analyst writes to me")}</span>
          <span className="flex cursor-pointer items-center gap-2 text-xs font-bold text-ink/60"><input type="checkbox" className="h-4 w-4 accent-[#0f4f45]" checked={draft.autoExport} onChange={e => update({ autoExport: e.target.checked })} />{t("Write evaluated cases to Excel automatically")}</span>
        </label>
      </div>
      {error && <div className="mt-4"><Banner tone="error" message={error} onClose={() => setError("")} /></div>}
      <div className="mt-5 flex justify-end"><button className="btn-primary" disabled={saving || !dirty} onClick={() => void save()}>{saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}{t("Save configuration")}</button></div>
    </div>}
  </section>;
}

export default function QaPanel() {
  const t = useT();
  const [config, setConfig] = useState<QaConfig>();
  const [status, setStatus] = useState<QaEngineStatus>();
  const [cases, setCases] = useState<QaCaseEntry[]>([]);
  const [filter, setFilter] = useState<Filter>("review");
  const [analyst, setAnalyst] = useState("");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(PAGE);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);

  const refreshStatus = useCallback(async () => { try { setStatus(await api.qaStatus()); } catch (e) { setError(String(e)); } }, []);
  const refreshCases = useCallback(async () => { try { setCases(await api.qaCases()); } catch (e) { setError(String(e)); } }, []);
  const refreshConfig = useCallback(async () => { try { setConfig(await api.qaGetConfig()); } catch (e) { setError(String(e)); } }, []);

  useEffect(() => {
    void refreshConfig(); void refreshStatus(); void refreshCases();
    const timer = window.setInterval(() => void refreshStatus(), 5_000);
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen("atlas-qa-updated", () => { void refreshStatus(); void refreshCases(); void refreshConfig(); })
      .then(unlisten => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; window.clearInterval(timer); stop?.(); };
  }, [refreshCases, refreshConfig, refreshStatus]);

  const analysts = useMemo(() => Array.from(new Set(cases.map(c => c.analystName))).sort(), [cases]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return cases.filter(c =>
      (filter === "all" || (filter === "review" && !c.reviewed) || (filter === "reviewed" && c.reviewed) || (filter === "new" && c.newEvidence))
      && (!analyst || c.analystName === analyst)
      && (!needle || `${c.requestId} ${c.analystName} ${c.analystEmail}`.toLowerCase().includes(needle)));
  }, [cases, filter, analyst, query]);
  useEffect(() => setLimit(PAGE), [filter, analyst, query]);

  const withBusy = async (operation: () => Promise<void>) => {
    setBusy(true); setError(""); setNotice("");
    try { await operation(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const syncNow = () => withBusy(async () => { await api.qaSyncNow(); setNotice(t("Sync requested. Your flow answers within about five minutes; Atlas keeps working in the background.")); await refreshStatus(); });
  const exportNow = () => withBusy(async () => {
    if (!config?.outputFolder) throw new Error(t("Choose an Excel output folder in People and rules first."));
    const result = await api.qaExportNow(false);
    setNotice(t("Excel updated: {count} row(s) in {files} workbook(s).", { count: result.written, files: result.files.length }));
    await refreshCases(); await refreshStatus();
  });
  const restart = () => withBusy(async () => { await api.qaRestartHistory(); setNotice(t("The full history will be imported again.")); await refreshConfig(); await refreshStatus(); });
  const saveCase = async (next: QaCase) => { await api.qaUpdateCase(next); await refreshCases(); await refreshStatus(); };
  const reevaluate = (caseId: string) => void withBusy(async () => { await api.qaReevaluate(caseId); setNotice(t("The conversation will be evaluated again in the background.")); await refreshCases(); });

  if (!config || !status) return <div className="card grid min-h-64 place-items-center p-10"><Loader2 className="h-6 w-6 animate-spin text-pine" /></div>;

  const counts: Record<Filter, number> = {
    review: cases.filter(c => !c.reviewed).length,
    new: cases.filter(c => c.newEvidence).length,
    reviewed: cases.filter(c => c.reviewed).length,
    all: cases.length,
  };
  const tabs: [Filter, string][] = [["review", t("Needs review")], ["new", t("New mail")], ["reviewed", t("Reviewed")], ["all", t("All")]];

  return <div className="space-y-6">
    <div className="rise flex items-end justify-between">
      <div>
        <p className="text-xs font-bold uppercase tracking-[.18em] text-pine">{t("QA Audit")}</p>
        <h1 className="mt-2 font-display text-4xl">{t("Your team's QA, always up to date.")}</h1>
        <p className="mt-2 max-w-3xl text-sm text-ink/50">{t("Atlas imports the QA history of every analyst, checks your mailbox twice a day and whenever an analyst writes to you, evaluates each conversation with local AI and fills the Excel workbooks. You review and have the last word.")}</p>
      </div>
    </div>
    {error && <Banner tone="error" message={error} onClose={() => setError("")} />}
    {notice && <Banner tone="info" message={notice} onClose={() => setNotice("")} />}
    <StatusHero status={status} busy={busy} onSync={() => void syncNow()} onExport={() => void exportNow()} onOpenFolder={() => void api.qaOpenFolder().catch(e => setError(String(e)))} onRestart={() => void restart()} />
    <SetupCard config={config} startOpen={config.auditees.length === 0 || !config.outputFolder} onSaved={async () => { await refreshConfig(); await refreshStatus(); setNotice(t("QA configuration saved.")); }} />

    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex rounded-xl border border-ink/10 bg-white p-1">
          {tabs.map(([key, label]) => <button key={key} onClick={() => setFilter(key)} className={`rounded-lg px-3 py-1.5 text-xs font-bold ${filter === key ? "bg-pine text-white" : "text-ink/55 hover:bg-cream"}`}>{label} <span className="opacity-60">{counts[key]}</span></button>)}
        </div>
        <div className="flex items-center gap-2">
          <div className="w-56"><Select value={analyst} options={[["", t("All analysts")], ...analysts.map(a => [a, a] as [string, string])]} onChange={setAnalyst} /></div>
          <div className="relative w-64"><Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/35" /><input className="field pl-9" placeholder={t("Search request or analyst")} value={query} onChange={e => setQuery(e.target.value)} /></div>
        </div>
      </div>
      {filtered.slice(0, limit).map(item => <CaseCard key={item.caseId} item={item} onSave={saveCase} onReevaluate={reevaluate} />)}
      {filtered.length > limit && <button className="btn-secondary w-full" onClick={() => setLimit(l => l + PAGE)}>{t("Show {count} more", { count: Math.min(PAGE, filtered.length - limit) })}</button>}
      {filtered.length === 0 && <div className="card grid min-h-48 place-items-center p-10 text-center">
        <div>
          <div className="lottie-frame mx-auto h-32 w-32"><LottieIcon name={cases.length ? "clipboard" : "mailHello"} className="h-28 w-28" /></div>
          <h3 className="mt-4 font-display text-xl">{cases.length ? t("Nothing in this view") : t("Your cases will appear here")}</h3>
          <p className="mt-2 max-w-md text-xs leading-5 text-ink/45">{cases.length ? t("Try another filter.") : t("Add the people you audit. Atlas asks your QA flow for their history in monthly batches every few minutes and evaluates each conversation as it arrives.")}</p>
        </div>
      </div>}
    </section>
  </div>;
}
