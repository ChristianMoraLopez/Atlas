import { animate, useInView } from "motion/react";
import gsap from "gsap";
import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useI18n } from "../i18n";
import type { Interaction, SourceKind } from "../types";
import TopoMap from "./TopoMap";
import { firstName, greetingKey, initials, seedFrom } from "./identity";
import { SOURCE_ORDER, SourceMark, useRouteFocus, waypoints } from "./route";
import styles from "./Personal.module.scss";

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

const SOURCE_LABELS: Record<SourceKind, string> = { calendar: "Meetings", email: "Mail", teams_chat: "Teams", manual: "Manual" };

/** Counts up to its value when it scrolls into view (adapted from React Bits' CountUp). */
function CountUp({ value }: { value: number }) {
  const element = useRef<HTMLSpanElement>(null);
  const seen = useInView(element, { once: true });
  const shown = useRef(0);
  useEffect(() => {
    if (!seen || !element.current) return;
    const node = element.current;
    const controls = animate(shown.current, value, {
      duration: 0.9,
      ease: [0.22, 0.8, 0.3, 1],
      onUpdate: latest => { shown.current = latest; node.textContent = String(Math.round(latest)); },
    });
    return () => controls.stop();
  }, [seen, value]);
  return <span ref={element}>0</span>;
}

/** Splits text into letters that rise into place (adapted from React Bits' SplitText). */
function Letters({ text }: { text: string }) {
  return <>{Array.from(text).map((char, index) => <span key={index} data-letter aria-hidden="true">{char === " " ? " " : char}</span>)}</>;
}

const LANES: Record<SourceKind, number> = { calendar: 18, email: 40, teams_chat: 62, manual: 84 };

/** The day as a survey route across four lanes, one per source, in time order. */
function DayRoute({ items }: { items: Interaction[] }) {
  const { t } = useI18n();
  const { focused, setFocused } = useRouteFocus();
  const scope = useRef<HTMLDivElement>(null);
  const points = useMemo(() => waypoints(items), [items]);
  const hours = points.map(point => point.at.getHours() + point.at.getMinutes() / 60);
  const start = Math.min(8, ...hours.map(Math.floor));
  const end = Math.max(18, ...hours.map(Math.ceil));
  const ticks = Array.from({ length: end - start + 1 }, (_, index) => start + index);
  const x = (hour: number) => ((hour - start) / (end - start)) * 100;
  const trail = points.map((point, index) => `${index ? "L" : "M"}${(x(hours[index]) * 10).toFixed(1)} ${LANES[point.item.sourceKind]}`).join(" ");
  const routeKey = points.map(point => point.item.sourceId).join("|");

  useLayoutEffect(() => {
    if (!scope.current) return;
    const media = gsap.matchMedia();
    const context = gsap.context(() => {
      media.add("(prefers-reduced-motion: no-preference)", () => {
        gsap.timeline({ defaults: { ease: "power3.out" } })
          .from("[data-trail]", { strokeDashoffset: 1, duration: 1.4, ease: "power2.inOut" }, 0.25);
      });
    }, scope);
    return () => { context.revert(); media.revert(); };
  }, [routeKey]);

  return <div ref={scope} className={styles.route} aria-label={t("Your day as a route")}>
    <div className={styles.lanes}>
      {SOURCE_ORDER.map(kind => <span key={kind} className={styles.laneLabel} data-source={kind} style={{ top: `${LANES[kind]}%` }}><SourceMark kind={kind} size={9} /></span>)}
      <div className={styles.field}>
        {SOURCE_ORDER.map(kind => <span key={kind} className={styles.lane} style={{ top: `${LANES[kind]}%` }} />)}
        {ticks.map(tick => <span key={tick} className={styles.tick} style={{ left: `${x(tick)}%` }}><span>{String(tick).padStart(2, "0")}</span></span>)}
        {points.length > 1 && <svg className={styles.trail} viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true">
          <path d={trail} pathLength={1} data-trail />
        </svg>}
        {points.map(({ item, at, number }, index) => <button
          type="button"
          key={item.sourceId}
          data-waypoint
          data-source={item.sourceKind}
          data-off={!item.selected || undefined}
          data-focus={focused === item.sourceId || undefined}
          className={styles.waypoint}
          style={{ left: `${x(hours[index])}%`, top: `${LANES[item.sourceKind]}%`, "--i": index } as React.CSSProperties}
          title={`${String(number).padStart(2, "0")} · ${at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} · ${item.evidenceLabel || item.comments}`}
          onMouseEnter={() => setFocused(item.sourceId)}
          onMouseLeave={() => setFocused(undefined)}
          onFocus={() => setFocused(item.sourceId)}
          onBlur={() => setFocused(undefined)}
          onClick={() => document.getElementById(`row-${item.sourceId}`)?.scrollIntoView({ behavior: "smooth", block: "center" })}
        >
          <SourceMark kind={item.sourceKind} size={13} hollow={!item.selected} />
          <span className={styles.waypointNumber}>{String(number).padStart(2, "0")}</span>
        </button>)}
      </div>
    </div>
  </div>;
}

function Compass() {
  return <svg className={styles.compass} viewBox="0 0 64 64" aria-hidden="true">
    <circle cx="32" cy="32" r="27" className={styles.compassRing} />
    <circle cx="32" cy="32" r="22" className={styles.compassRingFine} />
    <path d="M32 6 37 32 32 58 27 32Z" className={styles.compassNeedleBack} />
    <path d="M32 6 37 32H27Z" className={styles.compassNeedle} />
    <path d="M6 32 32 28 58 32 32 36Z" className={styles.compassCross} />
    <text x="32" y="4.2" textAnchor="middle" className={styles.compassNorth}>N</text>
  </svg>;
}

/** Day of the year, used as the map sheet number. */
function sheetNumber(date: Date) {
  const start = new Date(date.getFullYear(), 0, 0);
  return Math.floor((date.getTime() - start.getTime()) / 86_400_000);
}

export function DayHero({ name, date, items, children }: { name: string; date: string; items: Interaction[]; children?: React.ReactNode }) {
  const { t, lang } = useI18n();
  const scope = useRef<HTMLElement>(null);
  const day = new Date(`${date}T12:00:00`);
  const today = date === new Date().toLocaleDateString("en-CA");
  const dateLabel = day.toLocaleDateString(lang === "es" ? "es-CO" : "en-US", { weekday: "long", day: "numeric", month: "long" });
  const counts = SOURCE_ORDER.map(kind => ({ kind, count: items.filter(item => item.sourceKind === kind).length }));
  const greeting = `${t(greetingKey(new Date().getHours()))}, `;

  useLayoutEffect(() => {
    if (!scope.current) return;
    const media = gsap.matchMedia();
    const context = gsap.context(() => {
      media.add("(prefers-reduced-motion: no-preference)", () => {
        gsap.timeline({ defaults: { ease: "power3.out" } })
          .from("[data-letter]", { yPercent: 70, opacity: 0, rotate: 4, duration: 0.6, stagger: 0.018 }, 0)
          .from("[data-compass]", { rotate: -120, opacity: 0, duration: 1.2, ease: "elastic.out(1, 0.6)" }, 0.1);
      });
    }, scope);
    return () => { context.revert(); media.revert(); };
  }, [greeting]);

  return <section ref={scope} className={styles.sheet}>
    <TopoMap seed={seedFrom(name)} step={10} summits className={styles.sheetMap} />
    <span className={styles.neatline} aria-hidden="true" />
    <span className={`${styles.gridRefs} ${styles.gridRefsTop}`} aria-hidden="true">{["A", "B", "C", "D", "E", "F"].map(letter => <span key={letter}>{letter}</span>)}</span>
    <span className={`${styles.gridRefs} ${styles.gridRefsSide}`} aria-hidden="true">{["1", "2", "3", "4"].map(digit => <span key={digit}>{digit}</span>)}</span>

    <div className={styles.sheetHead}>
      <div className={styles.titleBlock}>
        <p className={styles.eyebrow}>
          <span className={styles.sheetCode}>{t("Sheet")} {String(sheetNumber(day)).padStart(3, "0")}</span>
          <span>{today ? t("Your map for today") : t("Your map for {date}", { date: dateLabel })}</span>
          {today && <span className={styles.date}>{dateLabel}</span>}
        </p>
        <h1 className={styles.greeting} aria-label={`${greeting}${firstName(name)}.`}>
          <Letters text={greeting} /><em><Letters text={firstName(name)} /></em><Letters text="." />
        </h1>
      </div>
      <span data-compass className={styles.compassWrap}><Compass /></span>
    </div>

    <div className={styles.legendRow}>
      <dl className={styles.legend} aria-label={t("Legend")}>
        <dt className={styles.legendTitle}>{t("Legend")}</dt>
        {counts.map(({ kind, count }, index) => <dd key={kind} style={{ "--i": index } as React.CSSProperties} data-legend-row data-source={kind} data-empty={count === 0 || undefined} className={styles.legendItem}>
          <SourceMark kind={kind} size={11} />
          <span className={styles.legendLabel}>{t(SOURCE_LABELS[kind])}</span>
          <b className={styles.legendCount}><CountUp value={count} /></b>
        </dd>)}
      </dl>
      <div className={styles.tally}>{children}</div>
    </div>

    <DayRoute items={items} />

    <p className={styles.scale}>{t("Each waypoint is a real interaction, numbered in time order.")}</p>
  </section>;
}

/** Three summits for the daily minimum: one fills for every selected activity. */
export function Summits({ selected, goal = 3 }: { selected: number; goal?: number }) {
  const { t } = useI18n();
  const reached = Math.min(selected, goal);
  return <aside className={styles.summits}>
    <TopoMap seed={seedFrom("atlas-summits")} variant="night" step={9} className={styles.summitsMap} />
    <p className={styles.summitsEyebrow}>{t("Daily minimum")}</p>
    <svg className={styles.summitsChart} viewBox="0 0 180 64" role="img" aria-label={t("{count} of {goal} activities", { count: reached, goal })}>
      <path d="M0 62H180" className={styles.summitsGround} />
      {Array.from({ length: goal }, (_, index) => {
        const left = 8 + index * 58;
        const height = 34 + index * 10;
        return <g key={index} data-reached={index < reached || undefined} className={styles.summit}>
          <path d={`M${left} 62 ${left + 26} ${62 - height} ${left + 52} 62Z`} />
          <path d={`M${left + 26} ${62 - height} l-3 -6 h6Z`} className={styles.summitFlag} />
        </g>;
      })}
    </svg>
    <p className={styles.summitsCount}><b>{reached}</b> / {goal}</p>
    <p className={styles.summitsCopy}>{t("Atlas always saves the real activities found. If there are fewer than three, it opens the app so you can add the missing ones.")}</p>
  </aside>;
}

/** Empty preview: an unsurveyed sheet waiting for its first route. */
export function BlankSheet({ name, title, detail }: { name: string; title: string; detail: string }) {
  return <div className={styles.blank}>
    <TopoMap seed={seedFrom(name) ^ 0x5bd1e995} step={12} className={styles.blankMap} />
    <svg className={styles.blankRoute} viewBox="0 0 220 70" aria-hidden="true">
      <path d="M8 52C40 52 46 18 82 22s44 34 76 26 38-30 54-34" pathLength={1} />
      <circle cx="8" cy="52" r="4.5" />
      <g transform="translate(212 14)"><circle r="8" /><text y="3.5" textAnchor="middle">?</text></g>
    </svg>
    <h3 className={styles.blankTitle}>{title}</h3>
    <p className={styles.blankDetail}>{detail}</p>
  </div>;
}
