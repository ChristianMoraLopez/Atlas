import { useEffect, useState } from "react";
import { Bot, Check, ChevronDown, Loader2, Save, X } from "lucide-react";
import { api } from "./lib";
import { useT } from "./i18n";
import type { AiPromptInfo } from "./types";

const MAX_CUSTOM = 500;

// Labels for the backend preset ids; the exact English rule sent to the model
// is shown under each label so nothing about the prompt stays hidden.
const PRESET_LABELS: Record<string, string> = {
  spanish_summaries: "Write summaries in Spanish",
  ignore_newsletters: "Ignore newsletters and automated notifications",
  qa_requests_are_tasks: "QA requests always count as tasks",
  ignore_personal_threads: "Ignore personal or social threads",
};

const CUSTOM_SUGGESTIONS = [
  "Only report work for the Manufacturing team",
  "Ignore threads where I am only in CC",
  "Treat requests from my manager as high priority",
];

export default function AiTransparencyModal({ onClose, onSaved }: { onClose: () => void; onSaved?: () => Promise<void> }) {
  const t = useT();
  const [info, setInfo] = useState<AiPromptInfo>();
  const [presets, setPresets] = useState<string[]>([]);
  const [custom, setCustom] = useState("");
  const [showEvidence, setShowEvidence] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    void api.getAiPromptInfo().then(result => {
      setInfo(result);
      setPresets(result.instructions.presets);
      setCustom(result.instructions.custom);
    }).catch(failure => setError(String(failure)));
  }, []);

  const togglePreset = (id: string) => {
    setSaved(false);
    setPresets(old => old.includes(id) ? old.filter(value => value !== id) : [...old, id]);
  };

  const save = async () => {
    setBusy(true); setError(""); setSaved(false);
    try {
      await api.saveAiInstructions(presets, custom);
      setSaved(true);
      if (onSaved) await onSaved();
    } catch (failure) { setError(String(failure)); }
    finally { setBusy(false); }
  };

  return <div className="fixed inset-0 z-50 grid place-items-center bg-[#081c1a]/45 p-8 backdrop-blur-sm">
    <div className="card flex max-h-[88vh] w-full max-w-3xl flex-col p-7">
      <div className="flex items-start justify-between">
        <div>
          <p className="flex items-center gap-2 text-xs font-bold uppercase tracking-[.16em] text-pine"><Bot className="h-4 w-4" />{t("AI transparency")}</p>
          <h2 className="mt-2 font-display text-3xl">{t("View AI prompt")}</h2>
          <p className="mt-2 text-sm text-ink/50">{t("Exactly what Atlas sends to the local model, and the extra rules you control.")}</p>
        </div>
        <button type="button" className="rounded-lg p-2 hover:bg-cream" onClick={onClose}><X /></button>
      </div>
      {error && <p role="alert" className="mt-4 rounded-xl bg-red-50 p-3 text-sm text-red-800">{error}</p>}
      {!info ? <div className="grid flex-1 place-items-center py-16"><Loader2 className="h-6 w-6 animate-spin text-pine" /></div> : <div className="mt-6 min-h-0 flex-1 space-y-6 overflow-y-auto pr-2">
        <section>
          <h3 className="text-sm font-extrabold">1 · {t("Core prompt (read-only)")}</h3>
          <p className="mt-1 text-xs leading-5 text-ink/45">{t("This prompt is fixed and never changes. Your rules are appended after it and can never override it.")}</p>
          <pre className="mt-3 max-h-56 overflow-auto whitespace-pre-wrap rounded-xl bg-[#081c1a] p-4 font-mono text-[11px] leading-5 text-[#c9e8dd]">{info.coreTemplate}</pre>
        </section>
        <section>
          <h3 className="text-sm font-extrabold">2 · {t("Additional instructions")}</h3>
          <p className="mt-1 text-xs leading-5 text-ink/45">{t("Preset rules that narrow what the AI reports.")}</p>
          <div className="mt-3 grid grid-cols-2 gap-2">
            {info.presets.map(preset => <button type="button" key={preset.id} onClick={() => togglePreset(preset.id)} className={`rounded-xl border p-3 text-left ${presets.includes(preset.id) ? "border-pine/35 bg-mint/55" : "border-ink/10 bg-white"}`}>
              <span className="flex items-center gap-2 text-xs font-bold"><span className={`grid h-4 w-4 place-items-center rounded border ${presets.includes(preset.id) ? "border-pine bg-pine text-white" : "border-ink/25"}`}>{presets.includes(preset.id) && <Check className="h-3 w-3" />}</span>{t(PRESET_LABELS[preset.id] ?? preset.id)}</span>
              <span className="mt-1.5 block font-mono text-[10px] leading-4 text-ink/40">{preset.rule}</span>
            </button>)}
          </div>
        </section>
        <section>
          <h3 className="text-sm font-extrabold">3 · {t("Custom rule")}</h3>
          <p className="mt-1 text-xs leading-5 text-ink/45">{t("One short rule in your own words (max 500 characters). It is appended as a single USER RULES line.")}</p>
          <textarea className="field mt-3 min-h-20 resize-none" maxLength={MAX_CUSTOM} value={custom} placeholder={t("Write a custom rule or pick a suggestion")} onChange={event => { setCustom(event.target.value); setSaved(false); }} />
          <div className="mt-1 flex items-center justify-between text-[10px] text-ink/40"><span className="font-bold uppercase tracking-wider">{t("Suggestions")}</span><span>{custom.length}/{MAX_CUSTOM}</span></div>
          <div className="mt-2 flex flex-wrap gap-2">
            {CUSTOM_SUGGESTIONS.map(suggestion => <button type="button" key={suggestion} className="chip bg-cream text-ink/55 hover:bg-mint hover:text-pine" onClick={() => { setCustom(t(suggestion)); setSaved(false); }}>{t(suggestion)}</button>)}
          </div>
        </section>
        <section className="rounded-xl border border-ink/10 bg-cream/60 p-4">
          <h3 className="text-xs font-extrabold uppercase tracking-wider text-ink/55">{t("Active rules sent with the next run")}</h3>
          {info.activeRules.length ? <ul className="mt-2 list-disc space-y-1 pl-5 font-mono text-[11px] leading-5 text-ink/60">{info.activeRules.map(rule => <li key={rule}>{rule}</li>)}</ul> : <p className="mt-2 text-[11px] text-ink/45">{t("No extra rules active. The core prompt decides alone.")}</p>}
        </section>
        <section>
          <button type="button" className="flex items-center gap-2 text-xs font-extrabold uppercase tracking-wider text-ink/55" onClick={() => setShowEvidence(!showEvidence)}><ChevronDown className={`h-3.5 w-3.5 transition-transform ${showEvidence ? "" : "-rotate-90"}`} />{t("Last submission to the model")}</button>
          {showEvidence && (info.lastSubmission ? <div className="mt-2">
            <p className="text-[11px] text-ink/45">{t("{count} evidence lines · source: {source}", { count: info.lastSubmission.evidenceLines.length, source: info.lastSubmission.sourceLabel })} · {new Date(info.lastSubmission.createdAt).toLocaleString()}</p>
            <pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap rounded-xl bg-cream p-3 font-mono text-[10px] leading-4 text-ink/55">{info.lastSubmission.evidenceLines.join("\n")}</pre>
          </div> : <p className="mt-2 text-[11px] text-ink/45">{t("No interpretation has run yet in this session.")}</p>)}
        </section>
      </div>}
      <div className="mt-6 flex items-center justify-end gap-3 border-t border-ink/10 pt-5">
        {saved && <span className="mr-auto flex items-center gap-2 text-xs font-bold text-pine"><Check className="h-4 w-4" />{t("Your rules take effect on the next interpretation.")}</span>}
        <button type="button" className="btn-secondary" onClick={onClose}>{t("Cancel")}</button>
        <button type="button" className="btn-primary" disabled={busy || !info} onClick={() => void save()}>{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}{t("Save instructions")}</button>
      </div>
    </div>
  </div>;
}
