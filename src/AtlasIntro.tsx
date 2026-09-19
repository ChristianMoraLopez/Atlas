import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useT } from "./i18n";

export default function AtlasIntro({ children }: { children: ReactNode }) {
  const t = useT();
  const [leaving, setLeaving] = useState(false);
  const [finished, setFinished] = useState(false);
  const close = useCallback(() => {
    setLeaving(true);
    window.setTimeout(() => setFinished(true), 420);
  }, []);

  useEffect(() => {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const timer = window.setTimeout(close, reduced ? 450 : 2600);
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("keydown", escape);
    return () => { window.clearTimeout(timer); window.removeEventListener("keydown", escape); };
  }, [close]);

  return <>
    <div className={finished ? "" : "pointer-events-none select-none"} aria-hidden={!finished}>{children}</div>
    {!finished && <section className={`atlas-intro ${leaving ? "atlas-intro--leaving" : ""}`} aria-label={t("Atlas is starting")}>
      <div className="atlas-intro__glow atlas-intro__glow--one" />
      <div className="atlas-intro__glow atlas-intro__glow--two" />
      <div className="atlas-intro__grid" />
      <div className="atlas-intro__content">
        <svg className="atlas-intro__mark" viewBox="0 0 180 180" role="img" aria-label="Atlas">
          <circle className="atlas-intro__orbit" cx="90" cy="90" r="72" />
          <path className="atlas-intro__trace" d="M38 137 86 36c2-5 8-5 10 0l47 101M57 112h67" />
          <path className="atlas-intro__ledger" d="M64 132h51M69 145h41" />
          <circle className="atlas-intro__node atlas-intro__node--one" cx="38" cy="137" r="5" />
          <circle className="atlas-intro__node atlas-intro__node--two" cx="90" cy="28" r="5" />
          <circle className="atlas-intro__node atlas-intro__node--three" cx="143" cy="137" r="5" />
        </svg>
        <div className="atlas-intro__word" aria-label="Atlas">
          {[..."ATLAS"].map((letter, index) => <span key={letter + index} style={{ "--atlas-letter": index } as React.CSSProperties}>{letter}</span>)}
        </div>
        <p className="atlas-intro__tagline">MEET · DO · RECORD</p>
      </div>
      <button className="atlas-intro__skip" onClick={close}>{t("Skip")} <span>Esc</span></button>
    </section>}
  </>;
}
