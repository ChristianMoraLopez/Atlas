import type { AppRole } from "../types";
import styles from "./Emblem.module.scss";

/** Engraved emblems for the two roles: a day route and a triangulation survey. */
export function Emblem({ role }: { role: AppRole }) {
  return <span className={styles.emblem} data-role={role} aria-hidden="true">
    <svg viewBox="0 0 120 120">
      <circle cx="60" cy="60" r="54" className={styles.ring} />
      <circle cx="60" cy="60" r="46" className={styles.ringFine} />
      {role === "cs" ? <>
        <path d="M24 82C38 82 40 52 56 54s18 22 32 16 10-30 10-30" className={styles.path} pathLength={1} />
        <circle cx="24" cy="82" r="5" className={styles.calendar} />
        <rect x="52" y="50" width="8" height="8" transform="rotate(45 56 54)" className={styles.mail} />
        <path d="M88 63 93 72H83Z" className={styles.teams} />
        <circle cx="98" cy="40" r="4" className={styles.flag} />
      </> : <>
        <path d="M60 24 96 86H24Z" className={styles.path} pathLength={1} />
        <path d="M60 24V86M60 86 42 55M60 86 78 55" className={styles.hair} />
        <circle cx="60" cy="24" r="5" className={styles.flag} />
        <circle cx="24" cy="86" r="4.5" className={styles.calendar} />
        <circle cx="96" cy="86" r="4.5" className={styles.mail} />
        <circle cx="60" cy="62" r="9" className={styles.lens} />
      </>}
    </svg>
  </span>;
}
