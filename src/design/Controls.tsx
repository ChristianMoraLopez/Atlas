import * as Switch from "@radix-ui/react-switch";
import type { ReactNode } from "react";
import styles from "./Controls.module.scss";

/** A labelled instrument switch (Radix Switch: keyboard and screen-reader ready). */
export function SourceSwitch({ checked, onChange, label, detail, icon }: { checked: boolean; onChange: (value: boolean) => void; label: string; detail: string; icon: ReactNode }) {
  return <label className={styles.switchRow} data-on={checked || undefined}>
    <span className={styles.switchIcon}>{icon}</span>
    <span className={styles.switchText}><b>{label}</b><span>{detail}</span></span>
    <Switch.Root className={styles.switch} checked={checked} onCheckedChange={onChange}>
      <Switch.Thumb className={styles.thumb} />
    </Switch.Root>
  </label>;
}
