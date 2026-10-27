import { motion } from "motion/react";
import { CalendarDays, Mail, MessagesSquare, PenLine } from "lucide-react";
import { useMemo } from "react";
import { useI18n } from "../i18n";
import type { Interaction, SourceKind } from "../types";
import TopoMap from "./TopoMap";
import { ACCENTS, firstName, greetingKey, initials, seedFrom, useAccent, type Accent } from "./identity";
import styles from "./Personal.module.css";

/** The person's seal: their own contour map with their initials, used as the app mark. */
export function PersonalSeal({ name, caption }: { name: string; caption: string }) {
  return <div className={styles.sealRow}>
    <span className={styles.seal} title={name}>
      <TopoMap seed={seedFrom(name)} step={4} className={styles.sealMap} />
      <span className={styles.sealInitials}>{initials(name)}</span>
    </span>
    <span>
      <span className={styles.sealWord}>Atlas</span>
      <span className={styles.sealCaption}>{caption}</span>
    </span>
  </div>;
}

const ACCENT_NAMES: Record<Accent, string> = { pine: "Pine", terracotta: "Terracotta", indigo: "Indigo", graphite: "Graphite" };

export function AccentPicker() {
  const { t } = useI18n();
  const [accent, setAccent] = useAccent();
  return <div className={styles.accents} role="radiogroup" aria-label={t("Your colour")}>
    {ACCENTS.map(option => <button
      key={option}
      type="button"
      role="radio"
      aria-checked={accent === option}
      title={t(ACCENT_NAMES[option])}
      className={styles.accent}
      data-option={option}
      onClick={() => setAccent(option)}
    />)}
  </div>;
}

const SOURCES: { kind: SourceKind; label: string; Icon: typeof Mail }[] = [
  { kind: "calendar", label: "Meetings", Icon: CalendarDays },
  { kind: "email", label: "Mail", Icon: Mail },
  { kind: "teams_chat", label: "Teams", Icon: MessagesSquare },
  { kind: "manual", label: "Manual", Icon: PenLine },
];

/** The day drawn as a route: one waypoint per interaction, placed at its time. */
function DayRoute({ items }: { items: Interaction[] }) {
  const { t } = useI18n();
  const points = useMemo(() => items
    .map(item => ({ item, at: new Date(item.interactionDateTime) }))
    .filter(point => !Number.isNaN(point.at.getTime()))
    .sort((a, b) => a.at.getTime() - b.at.getTime()), [items]);
  const hours = points.map(point => point.at.getHours() + point.at.getMinutes() / 60);
  const start = Math.min(8, ...hours.map(Math.floor));
  const end = Math.max(18, ...hours.map(Math.ceil));
  const ticks = Array.from({ length: end - start + 1 }, (_, index) => start + index);
  const position = (hour: number) => `${((hour - start) / (end - start)) * 100}%`;
  return <div className={styles.route} aria-label={t("Your day as a route")}>
    <div className={styles.routeTrack}>
      <motion.span className={styles.routeLine} initial={{ scaleX: 0 }} animate={{ scaleX: 1 }} transition={{ duration: 0.9, ease: [0.22, 0.8, 0.3, 1] }} />
      {ticks.map(tick => <span key={tick} className={styles.routeTick} style={{ left: position(tick) }}><span>{String(tick).padStart(2, "0")}</span></span>)}
      {points.map(({ item, at }, index) => <motion.span
        key={item.sourceId}
        className={styles.waypoint}
        data-source={item.sourceKind}
        data-off={!item.selected || undefined}
        style={{ left: position(hours[index]) }}
        title={`${at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} · ${item.comments || item.evidenceLabel}`}
        initial={{ opacity: 0, y: -8, scale: 0.4 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ delay: 0.35 + index * 0.035, type: "spring", stiffness: 420, damping: 22 }}
      />)}
    </div>
  </div>;
}

export function DayHero({ name, date, items, children }: { name: string; date: string; items: Interaction[]; children?: React.ReactNode }) {
  const { t, lang } = useI18n();
  const day = new Date(`${date}T12:00:00`);
  const today = date === new Date().toLocaleDateString("en-CA");
  const dateLabel = day.toLocaleDateString(lang === "es" ? "es-CO" : "en-US", { weekday: "long", day: "numeric", month: "long" });
  const counts = SOURCES.map(source => ({ ...source, count: items.filter(item => item.sourceKind === source.kind).length }));
  return <section className={styles.hero}>
    <TopoMap seed={seedFrom(name)} step={10} summits className={styles.heroMap} />
    <div className={styles.heroCopy}>
      <p className={styles.eyebrow}>{today ? t("Your map for today") : t("Your map for {date}", { date: dateLabel })}{today && <> · <span className={styles.date}>{dateLabel}</span></>}</p>
      <motion.h1 className={styles.greeting} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, ease: [0.22, 0.8, 0.3, 1] }}>
        {t(greetingKey(new Date().getHours()))}, <em>{firstName(name)}</em>.
      </motion.h1>
      <div className={styles.legend}>
        {counts.map(({ kind, label, Icon, count }) => <span key={kind} className={styles.legendItem} data-source={kind} data-empty={count === 0 || undefined}><Icon aria-hidden /> <b>{count}</b> {t(label)}</span>)}
        {children}
      </div>
    </div>
    <DayRoute items={items} />
  </section>;
}
