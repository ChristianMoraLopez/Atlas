import * as Popover from "@radix-ui/react-popover";
import * as Tooltip from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import { PersonalSeal } from "./Personal";
import { ACCENTS, useAccent, type Accent } from "./identity";
import styles from "./Masthead.module.scss";

export type Tool = { label: string; icon: ReactNode; onClick: () => void; danger?: boolean };

const ACCENT_NAMES: Record<Accent, string> = { pine: "Pine", terracotta: "Terracotta", indigo: "Indigo", graphite: "Graphite" };

/** Icon button with a real tooltip instead of a native title. */
export function ToolButton({ label, icon, onClick, danger, children }: Partial<Tool> & { label: string; children?: ReactNode }) {
  return <Tooltip.Root>
    <Tooltip.Trigger asChild>
      <button type="button" aria-label={label} className={styles.tool} data-danger={danger || undefined} onClick={onClick}>{icon}{children}</button>
    </Tooltip.Trigger>
    <Tooltip.Portal>
      <Tooltip.Content side="bottom" sideOffset={8} className={styles.tooltip}>{label}<Tooltip.Arrow className={styles.tooltipArrow} /></Tooltip.Content>
    </Tooltip.Portal>
  </Tooltip.Root>;
}

/** The accent lives behind one swatch: a popover with the four map inks. */
function AccentMenu() {
  const { t } = useI18n();
  const [accent, setAccent] = useAccent();
  return <Popover.Root>
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <Popover.Trigger asChild>
          <button type="button" className={styles.tool} aria-label={t("Your colour")}><span className={styles.swatch} data-option={accent} /></button>
        </Popover.Trigger>
      </Tooltip.Trigger>
      <Tooltip.Portal><Tooltip.Content side="bottom" sideOffset={8} className={styles.tooltip}>{t("Your colour")}<Tooltip.Arrow className={styles.tooltipArrow} /></Tooltip.Content></Tooltip.Portal>
    </Tooltip.Root>
    <Popover.Portal>
      <Popover.Content align="end" sideOffset={10} className={styles.inks}>
        <p className={styles.inksTitle}>{t("Map ink")}</p>
        <div role="radiogroup" aria-label={t("Your colour")} className={styles.inksGrid}>
          {ACCENTS.map(option => <button key={option} type="button" role="radio" aria-checked={accent === option} className={styles.ink} onClick={() => setAccent(option)}>
            <span className={styles.swatch} data-option={option} />{t(ACCENT_NAMES[option])}
          </button>)}
        </div>
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}

/** The map's title block: who the sheet belongs to, its source, and the instruments. */
export function Masthead({ name, caption, badge, sourceTitle, sourceDetail, groups, onLanguage }: {
  onLanguage: () => void;
  name: string;
  caption: string;
  badge?: ReactNode;
  sourceTitle?: string;
  sourceDetail?: string;
  groups: Tool[][];
}) {
  const { lang } = useI18n();
  return <Tooltip.Provider delayDuration={250}>
    <header className={styles.masthead}>
      <div className={styles.inner}>
        <div className={styles.identity}><PersonalSeal name={name} caption={caption} />{badge}</div>
        {sourceTitle && <div className={styles.source}>
          <span className={styles.sourceTitle}>{sourceTitle}</span>
          <span className={styles.sourceDetail} title={sourceDetail}>{sourceDetail}</span>
        </div>}
        <nav className={styles.instruments} aria-label="Atlas">
          <AccentMenu />
          <ToolButton label={lang === "es" ? "ES · Switch to English" : "EN · Cambiar a español"} onClick={onLanguage}>
            <span className={styles.lang}>{lang === "es" ? "ES" : "EN"}</span>
          </ToolButton>
          {groups.map((group, index) => <span key={index} className={styles.group}>
            {group.map(tool => <ToolButton key={tool.label} {...tool} />)}
          </span>)}
        </nav>
      </div>
    </header>
  </Tooltip.Provider>;
}
