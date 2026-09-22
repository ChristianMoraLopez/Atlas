import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle, Bot, Check, ChevronDown, Clock, FileSpreadsheet, FolderOpen,
  History, Loader2, Mail, Plus, Save, Search, Trash2, UserRound, X
} from "lucide-react";
import { api, formatDateTime } from "./lib";
import { useT } from "./i18n";
import LottieIcon from "./LottieIcon";
import type { QaAuditee, QaCase, QaConfig, QaScheduleMode } from "./types";

const YNA = ["Y", "N", "N/A"];
const SENTIMENTS = ["Positive", "Neutral", "Negative"];
const AUTO_FAILS: [string, string][] = [
  ["none", "No Autofail"],
  ["no_initial_response", "No initial response"],
  ["missed_priority", "Missed/Incorrect Priority Handling"],
  ["no_status_updates", "Failure to Provide Status Updates"],
  ["non_adherence_sops", "Non-Adherence to SOPs"],
  ["no_cases", "No Cases Available for audit"],
];

const emptyAuditee: QaAuditee = { name: "", email: "", customRules: "", watched: true, historicalDone: false };

function Banner({ tone, message, onClose }: { tone: "error" | "info" | "success"; message: string; onClose?: () => void }) {
  const styles = tone === "error"
    ? "border-red-200 bg-red-50 text-red-800"
    : tone === "success"
      ? "border-pine/20 bg-mint/70 text-pine"
      : "border-pine/15 bg-mint/55 text-pine";
  return <div className={`pop flex items-start gap-3 rounded-xl border px-4 py-3 text-sm ${styles}`}>
    {tone === "error"
      ? <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
      : tone === "success"
        ? <LottieIcon name="trophy" loop={false} className="h-10 w-10 shrink-0" />
        : <Check className="mt-0.5 h-4 w-4 shrink-0" />}
    <span className="flex-1 whitespace-pre-line">{message}</span>
    {onClose && <button onClick={onClose}><X className="h-4 w-4" /></button>}
  </div>;
}

function CriterionSelect({ value, options, onChange, disabled }: {
  value: string; options: string[]; onChange: (v: string) => void; disabled?: boolean;
}) {
  return <div className="relative">
    <select
      className="field appearance-none pr-8"
      value={value}
      disabled={disabled}
      onChange={e => onChange(e.target.value)}
    >
      {options.map(o => <option key={o} value={o}>{o}</option>)}
    </select>
    <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/35" />
  </div>;
}

function CaseCard({ item, onChange }: { item: QaCase; onChange: (next: QaCase) => void }) {
  const t = useT();
  const [showEvidence, setShowEvidence] = useState(false);
  const set = (patch: Partial<QaCase>) => onChange({ ...item, ...patch, reviewed: true });
  const criterion = (
    label: string,
    valueKey: keyof QaCase,
    notesKey: keyof QaCase,
    options: string[] = YNA,
  ) => (
    <div className="rounded-xl border border-ink/8 bg-white p-3">
      <div className="grid grid-cols-[150px_1fr] items-start gap-3">
        <div>
          <span className="label">{t(label)}</span>
          <CriterionSelect value={String(item[valueKey])} options={options} onChange={v => set({ [valueKey]: v } as Partial<QaCase>)} />
        </div>
        <label>
          <span className="label">{t("Notes/Examples")}</span>
          <textarea
            className="field min-h-[38px] resize-y text-xs"
            value={String(item[notesKey])}
            onChange={e => set({ [notesKey]: e.target.value } as Partial<QaCase>)}
          />
        </label>
      </div>
    </div>
  );
  return <div className={`card p-5 ${item.reviewed ? "ring-1 ring-pine/30" : ""}`}>
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="grid h-8 w-8 place-items-center rounded-xl bg-mint text-pine"><UserRound className="h-4 w-4" /></span>
          <div>
            <p className="text-sm font-bold">{item.analystName}</p>
            <p className="text-[11px] text-ink/45">{item.analystEmail}</p>
          </div>
        </div>
        <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-3">
          <label><span className="label">{t("Request ID")}</span>
            <input className="field" value={item.requestId} onChange={e => set({ requestId: e.target.value })} /></label>
          <label><span className="label">{t("Request Date")}</span>
            <input type="date" className="field" value={item.requestDate} onChange={e => set({ requestDate: e.target.value })} /></label>
          <label><span className="label">{t("Request Source")}</span>
            <CriterionSelect value={item.requestSource} options={["Email", "IRIS"]} onChange={v => set({ requestSource: v })} /></label>
        </div>
      </div>
      <div className="flex shrink-0 flex-col items-end gap-2">
        <label className="flex cursor-pointer items-center gap-2 text-xs font-bold text-ink/60">
          <input type="checkbox" className="h-4 w-4 accent-[#24776a]" checked={item.selected}
            onChange={e => onChange({ ...item, selected: e.target.checked })} />
          {t("Include in export")}
        </label>
        <span className={`chip ${item.reviewed ? "bg-mint text-pine" : "bg-[#fff0df] text-[#9a5a1e]"}`}>
          {item.reviewed ? t("Reviewed") : t("Pending review")}
        </span>
      </div>
    </div>
    <div className="mt-4 space-y-3">
      {criterion("Initial Response", "initialResponse", "initialResponseNotes")}
      {criterion("Customer Sentiment", "customerSentiment", "customerSentimentNotes", SENTIMENTS)}
      {criterion("Adherence", "adherence", "adherenceNotes")}
      {criterion("Status", "status", "statusNotes")}
      {criterion("Update/Follow Up", "updateFollowUp", "updateFollowUpNotes")}
      <div className="rounded-xl border border-ink/8 bg-white p-3">
        <span className="label">{t("QA Auto fail?")}</span>
        <div className="relative">
          <select className="field appearance-none pr-8" value={item.autoFail} onChange={e => set({ autoFail: e.target.value })}>
            {AUTO_FAILS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
          <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/35" />
        </div>
      </div>
    </div>
    {item.evidence.length > 0 && <div className="mt-4">
      <button type="button" onClick={() => setShowEvidence(v => !v)}
        className="flex items-center gap-2 text-xs font-bold text-pine underline">
        <Mail className="h-3.5 w-3.5" />
        {showEvidence ? t("Hide evidence") : t("Show evidence ({count})", { count: item.evidence.length })}
      </button>
      {showEvidence && <div className="mt-2 space-y-2">
        {item.evidence.map((ev, i) => <div key={i} className="rounded-xl border border-ink/8 bg-cream/60 p-3">
          <p className="text-[11px] font-bold text-ink/60">{ev.label}</p>
          <p className="mt-1 text-xs leading-5 text-ink/55">{ev.excerpt}{ev.excerpt.length >= 220 ? "…" : ""}</p>
        </div>)}
      </div>}
    </div>}
  </div>;
}

const firedKey = (slot: string) => `qa-fired-${new Date().toISOString().slice(0, 10)}-${slot}`;
const wasFired = (slot: string) => localStorage.getItem(firedKey(slot)) === "1";
const markFired = (slot: string) => localStorage.setItem(firedKey(slot), "1");

function timeReached(hhmm: string): boolean {
  const [h, m] = hhmm.split(":").map(Number);
  if (Number.isNaN(h) || Number.isNaN(m)) return false;
  const now = new Date();
  return now.getHours() * 60 + now.getMinutes() >= h * 60 + m;
}

/**
 * QA scheduler, mounted once in the workspace so triggers fire regardless of
 * the active tab: twice-daily mailbox checks (morning/afternoon), plus the
 * weekly analysis at PC startup or at a chosen time. Default mode is manual
 * (the manager runs the analysis by hand).
 */
export function useQaScheduler() {
  useEffect(() => {
    const tick = async () => {
      let cfg: QaConfig;
      try { cfg = await api.qaGetConfig(); } catch { return; }
      if (cfg.watchEnabled && cfg.auditees.some(a => a.watched)) {
        if (timeReached(cfg.checkMorning) && !wasFired("check-morning")) {
          markFired("check-morning");
          try { await api.qaCheckNewMail(); } catch (e) { void api.logError("qa-watch", String(e)); }
        }
        if (timeReached(cfg.checkAfternoon) && !wasFired("check-afternoon")) {
          markFired("check-afternoon");
          try { await api.qaCheckNewMail(); } catch (e) { void api.logError("qa-watch", String(e)); }
        }
      }
      if (cfg.scheduleMode === "daily_time" && cfg.auditees.length > 0
        && timeReached(cfg.scheduleTime) && !wasFired("analysis")) {
        markFired("analysis");
        try { await api.qaExtract(false, "scheduled"); } catch (e) { void api.logError("qa-scheduled", String(e)); }
      }
    };
    const startup = async () => {
      try {
        const cfg = await api.qaGetConfig();
        if (cfg.scheduleMode === "startup" && cfg.auditees.length > 0 && !wasFired("startup")) {
          markFired("startup");
          await api.qaExtract(false, "scheduled");
        }
      } catch (e) { void api.logError("qa-startup", String(e)); }
    };
    void startup();
    void tick();
    const timer = window.setInterval(() => void tick(), 60_000);
    return () => window.clearInterval(timer);
  }, []);
}

export default function QaPanel() {
  const t = useT();
  useQaScheduler();
  const [config, setConfig] = useState<QaConfig>();
  const [cases, setCases] = useState<QaCase[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [noticeTone, setNoticeTone] = useState<"info" | "success">("info");
  const [busy, setBusy] = useState<"" | "save" | "extract" | "historical" | "export" | "watch">("");
  const [configSaved, setConfigSaved] = useState(true);
  const [keywordsText, setKeywordsText] = useState("");

  useEffect(() => {
    api.qaGetConfig()
      .then(cfg => {
        setConfig(cfg);
        setKeywordsText(cfg.subjectKeywords.join(", "));
      })
      .catch(e => setError(String(e)));
    api.qaLoadLastRun()
      .then(last => {
        if (last && last.result.cases.length > 0) {
          setCases(last.result.cases);
          setWarnings(last.result.warnings);
          if (last.source === "scheduled") {
            setNotice(t("Showing the scheduled run from {date}. Review it and export when ready.", { date: formatDateTime(last.ranAt) }));
          }
        }
      })
      .catch(() => undefined);
  }, []);

  const refreshConfig = useCallback(async () => {
    try {
      const cfg = await api.qaGetConfig();
      setConfig(cfg);
      setKeywordsText(cfg.subjectKeywords.join(", "));
    } catch { /* keep current state */ }
  }, []);

  const updateConfig = (patch: Partial<QaConfig>) => {
    setConfigSaved(false);
    setConfig(old => old ? { ...old, ...patch } : old);
  };
  const updateAuditee = (index: number, patch: Partial<QaAuditee>) => {
    setConfigSaved(false);
    setConfig(old => old ? {
      ...old,
      auditees: old.auditees.map((a, i) => i === index ? { ...a, ...patch } : a),
    } : old);
  };

  const pickFolder = async () => {
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder === "string") updateConfig({ outputFolder: folder });
  };

  const saveConfig = async () => {
    if (!config) return;
    setBusy("save"); setError(""); setNotice("");
    try {
      await api.qaSaveConfig({
        ...config,
        subjectKeywords: keywordsText.split(",").map(k => k.trim()).filter(Boolean),
      });
      setConfigSaved(true);
      setNoticeTone("info");
      setNotice(t("QA configuration saved."));
      await refreshConfig();
    } catch (e) { setError(String(e)); }
    finally { setBusy(""); }
  };

  const extract = async (historical: boolean) => {
    setBusy(historical ? "historical" : "extract"); setError(""); setNotice(""); setWarnings([]);
    try {
      if (!configSaved) await saveConfig();
      const result = await api.qaExtract(historical, "manual");
      setCases(result.cases);
      setWarnings(result.warnings);
      if (historical) await refreshConfig();
      // A manual run acknowledges the pending watch notifications.
      if (config && config.pendingWatch.length > 0) {
        await api.qaSaveConfig({ ...config, pendingWatch: [] }).catch(() => undefined);
        await refreshConfig();
      }
      if (!result.cases.length) {
        setNoticeTone("info");
        setNotice(t("No QA cases were found in the manager mailbox for the configured period."));
      }
    } catch (e) { setError(String(e)); }
    finally { setBusy(""); }
  };

  const dismissWatch = async () => {
    if (!config) return;
    try {
      await api.qaSaveConfig({ ...config, pendingWatch: [] });
      await refreshConfig();
    } catch (e) { setError(String(e)); }
  };

  const exportCases = async () => {
    setBusy("export"); setError(""); setNotice("");
    try {
      const result = await api.qaExport(cases);
      setNoticeTone("success");
      setNotice(t("QA export complete: {count} row(s) written into {files} workbook(s).", { count: result.written, files: result.files.length }));
    } catch (e) { setError(String(e)); }
    finally { setBusy(""); }
  };

  if (!config) return <div className="card grid min-h-64 place-items-center p-10"><Loader2 className="h-6 w-6 animate-spin text-pine" /></div>;

  const selectedCount = cases.filter(c => c.selected).length;
  const pendingHistorical = config.auditees.filter(a => !a.historicalDone);
  const scheduleModes: [QaScheduleMode, string][] = [
    ["manual", t("Manual (I run it myself)")],
    ["startup", t("When the PC starts")],
    ["daily_time", t("At a chosen time")],
  ];

  return <div className="space-y-6">
    <div className="rise mb-1 flex items-end justify-between">
      <div>
        <p className="text-xs font-bold uppercase tracking-[.18em] text-pine">{t("QA Audit")}</p>
        <h1 className="mt-2 font-display text-4xl">{t("Audit your team's cases.")}</h1>
        <p className="mt-2 text-sm text-ink/50">{t("Atlas finds the team's QA conversations in your mailbox, the local AI evaluates the rubric, and you review before exporting.")}</p>
      </div>
      <div className="flex items-center gap-2">
        <span className="chip bg-white text-ink/55"><Search className="h-3 w-3" />{t("{count} cases", { count: cases.length })}</span>
        <span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{t("{count} selected", { count: selectedCount })}</span>
      </div>
    </div>

    {error && <Banner tone="error" message={error} onClose={() => setError("")} />}
    {notice && <Banner tone={noticeTone} message={notice} onClose={() => setNotice("")} />}
    {warnings.map((w, i) => <Banner key={i} tone="error" message={w} />)}
    {config.pendingWatch.length > 0 && <div className="watch-ring pop flex items-center gap-3 rounded-xl border border-[#e89969]/40 bg-[#fff0df] px-4 py-3 text-sm text-[#9a5a1e]">
      <LottieIcon name="mailDelivery" className="h-12 w-12 shrink-0" />
      <span className="flex-1">{t("New mail arrived from: {names}. Run the QA analysis to audit it.", { names: config.pendingWatch.join(", ") })}</span>
      <button className="font-bold underline" disabled={busy !== ""} onClick={() => void extract(false)}>{t("Run now")}</button>
      <button onClick={() => void dismissWatch()}><X className="h-4 w-4" /></button>
    </div>}

    <div className="card card-lift rise rise-1 p-5">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-xl">{t("People to audit")}</h2>
        <button className="btn-secondary" onClick={() => updateConfig({ auditees: [...config.auditees, { ...emptyAuditee }] })}>
          <Plus className="h-4 w-4" />{t("Add person")}
        </button>
      </div>
      <p className="mt-1 text-xs text-ink/45">{t("Atlas watches your mailbox for mail from these people. You can adjust the evaluation rules per person (for example, when response-time ranges do not apply to their role).")}</p>
      <div className="mt-4 space-y-3">
        {config.auditees.length === 0 && <p className="rounded-xl border border-dashed border-ink/15 p-4 text-center text-xs text-ink/40">{t("No one on the list yet. Add the first person to audit.")}</p>}
        {config.auditees.map((a, i) => <div key={i} className="pop rounded-xl border border-ink/8 bg-white p-4">
          <div className="grid grid-cols-1 gap-3 md:grid-cols-[1fr_1fr_auto]">
            <label><span className="label">{t("Name")}</span>
              <input className="field" value={a.name} placeholder="Christian Rey Mora Lopez"
                onChange={e => updateAuditee(i, { name: e.target.value })} /></label>
            <label><span className="label">{t("Email")}</span>
              <input className="field" value={a.email} placeholder="nombre@circana.com"
                onChange={e => updateAuditee(i, { email: e.target.value })} /></label>
            <div className="flex items-end gap-2">
              <label className="flex cursor-pointer items-center gap-2 pb-3 text-xs font-bold text-ink/60" title={t("Notify when new mail arrives from this person")}>
                <input type="checkbox" className="h-4 w-4 accent-[#24776a]" checked={a.watched}
                  onChange={e => updateAuditee(i, { watched: e.target.checked })} />
                {t("Watch")}
              </label>
              <button className="rounded-xl border border-ink/10 bg-white p-2.5 text-ink/40 hover:bg-red-50 hover:text-red-600"
                title={t("Remove")} onClick={() => updateConfig({ auditees: config.auditees.filter((_, j) => j !== i) })}>
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          </div>
          <div className="mt-3 flex items-center gap-2">
            {a.historicalDone
              ? <span className="chip bg-mint text-pine"><Check className="h-3 w-3" />{t("Historical audit done")}</span>
              : <span className="chip bg-[#fff0df] text-[#9a5a1e]"><History className="h-3 w-3" />{t("Historical audit pending")}</span>}
            {a.historicalDone && <button className="text-[11px] font-bold text-pine underline"
              onClick={() => updateAuditee(i, { historicalDone: false })}>{t("Mark historical as pending")}</button>}
          </div>
          <label className="mt-3 block"><span className="label">{t("Manager rules for this person (optional)")}</span>
            <textarea className="field min-h-[44px] resize-y text-xs" value={a.customRules}
              placeholder={t("Example: response-time ranges do not apply; this CSA works ticket-based via IRIS.")}
              onChange={e => updateAuditee(i, { customRules: e.target.value })} /></label>
        </div>)}
      </div>
      <div className="mt-4 grid grid-cols-1 items-end gap-4 md:grid-cols-[140px_1fr_1fr_auto]">
        <label><span className="label">{t("Days to look back")}</span>
          <input type="number" min={1} max={31} className="field" value={config.lookbackDays}
            onChange={e => updateConfig({ lookbackDays: Math.max(1, Math.min(31, Number(e.target.value) || 7)) })} /></label>
        <label><span className="label">{t("Vertical/Team")}</span>
          <input className="field" value={config.vertical}
            onChange={e => updateConfig({ vertical: e.target.value })} /></label>
        <label><span className="label">{t("Output folder")}</span>
          <button className="field flex items-center gap-2 text-left" onClick={pickFolder}>
            <FolderOpen className="h-4 w-4 shrink-0 text-ink/40" />
            <span className="truncate">{config.outputFolder || t("Choose a folder…")}</span>
          </button></label>
        <label><span className="label">{t("QA subject keywords")}</span>
          <input className="field" value={keywordsText} placeholder="QA, audit"
            onChange={e => { setConfigSaved(false); setKeywordsText(e.target.value); }} /></label>
      </div>
      <div className="mt-4 grid grid-cols-1 items-end gap-4 md:grid-cols-[1fr_140px_140px_140px]">
        <label><span className="label">{t("Run the weekly analysis")}</span>
          <div className="relative">
            <select className="field appearance-none pr-8" value={config.scheduleMode}
              onChange={e => updateConfig({ scheduleMode: e.target.value as QaScheduleMode })}>
              {scheduleModes.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
            <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/35" />
          </div></label>
        {config.scheduleMode === "daily_time" && <label><span className="label">{t("Analysis time")}</span>
          <input type="time" className="field" value={config.scheduleTime}
            onChange={e => updateConfig({ scheduleTime: e.target.value })} /></label>}
        <label className="flex cursor-pointer items-center gap-2 pb-3 text-xs font-bold text-ink/60" title={t("Check the mailbox twice a day and notify when an audited person writes")}>
          <input type="checkbox" className="h-4 w-4 accent-[#24776a]" checked={config.watchEnabled}
            onChange={e => updateConfig({ watchEnabled: e.target.checked })} />
          {t("Watch mailbox")}
        </label>
        {config.watchEnabled && <label><span className="label">{t("Morning check")}</span>
          <input type="time" className="field" value={config.checkMorning}
            onChange={e => updateConfig({ checkMorning: e.target.value })} /></label>}
        {config.watchEnabled && <label><span className="label">{t("Afternoon check")}</span>
          <input type="time" className="field" value={config.checkAfternoon}
            onChange={e => updateConfig({ checkAfternoon: e.target.value })} /></label>}
      </div>
      <div className="mt-4 flex items-center gap-3">
        <button className="btn-secondary" disabled={busy !== ""} onClick={saveConfig}>
          {busy === "save" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}{t("Save configuration")}
        </button>
        {!configSaved && <span className="text-xs text-[#9a5a1e]">{t("You have unsaved changes.")}</span>}
      </div>
    </div>

    <div className="card card-lift rise rise-2 p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-4">
          <div className="lottie-frame h-20 w-20 shrink-0">
            <LottieIcon name="robotHello" className="h-16 w-16" />
          </div>
          <div>
            <h2 className="font-display text-xl">{t("Find and evaluate cases")}</h2>
            <p className="mt-1 max-w-xl text-xs text-ink/45">{t("Searches your mailbox for QA conversations with the people on the list and evaluates them with the bundled local AI. Nothing leaves this computer.")}</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {pendingHistorical.length > 0 && <button className="btn-secondary h-[46px]" disabled={busy !== ""} onClick={() => void extract(true)}
            title={t("Pulls every QA conversation these people have ever sent. It can take a while.")}>
            {busy === "historical" ? <Loader2 className="h-4 w-4 animate-spin" /> : <History className="h-4 w-4" />}
            {busy === "historical" ? t("Auditing history…") : t("Historical audit ({count})", { count: pendingHistorical.length })}
          </button>}
          <button className="btn-primary h-[46px] px-5" disabled={busy !== "" || config.auditees.length === 0} onClick={() => void extract(false)}>
            {busy === "extract" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Bot className="h-4 w-4" />}
            {busy === "extract" ? t("Evaluating…") : t("Find QA cases (this week)")}
          </button>
        </div>
      </div>
      <p className="mt-3 flex items-center gap-2 text-[11px] text-ink/40"><Clock className="h-3 w-3" />{t("Weekly audits cover the configured lookback days and are grouped by ISO week in Excel.")}</p>
    </div>

    {cases.length > 0 && <div className="space-y-4">
      <div className="rise flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="lottie-frame h-12 w-12">
            <LottieIcon name="clipboard" className="h-10 w-10" />
          </div>
          <h2 className="font-display text-2xl">{t("Review before exporting")}</h2>
        </div>
        <div className="flex items-center gap-2">
          <button className="btn-secondary" disabled={busy !== ""} onClick={() => void api.qaOpenFolder().catch(e => setError(String(e)))}>
            <FolderOpen className="h-4 w-4" />{t("Open folder")}
          </button>
          <button className="btn-primary" disabled={busy !== "" || selectedCount === 0 || !config.outputFolder} onClick={exportCases}>
            {busy === "export" ? <Loader2 className="h-4 w-4 animate-spin" /> : <FileSpreadsheet className="h-4 w-4" />}
            {t("Export {count} case(s)", { count: selectedCount })}
          </button>
        </div>
      </div>
      {!config.outputFolder && <Banner tone="error" message={t("Choose an output folder above before exporting.")} />}
      {cases.map((item, i) => <div key={item.caseId} className="rise" style={{ animationDelay: `${Math.min(i, 6) * 70}ms` }}>
        <CaseCard item={item}
          onChange={next => setCases(old => old.map(c => c.caseId === next.caseId ? next : c))} />
      </div>)}
    </div>}

    {cases.length === 0 && busy === "" && <div className="card rise rise-3 grid min-h-48 place-items-center p-10 text-center">
      <div>
        <div className="lottie-frame mx-auto h-36 w-36">
          <LottieIcon name="mailHello" className="h-32 w-32" />
        </div>
        <h3 className="mt-4 font-display text-xl">{t("Set up the list and run the analysis")}</h3>
        <p className="mt-2 max-w-md text-xs leading-5 text-ink/45">{t("Each conversation becomes a draft QA audit with the form fields ready to review. The manager always has the last word before anything is exported.")}</p>
      </div>
    </div>}
  </div>;
}
