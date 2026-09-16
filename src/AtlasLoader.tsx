import { useEffect, useState } from "react";

type AtlasLoaderProps = {
  show: boolean;
  message: string;
  detail?: string;
  mode?: "overlay" | "screen";
  delay?: number;
};

export default function AtlasLoader({
  show,
  message,
  detail,
  mode = "overlay",
  delay = 180,
}: AtlasLoaderProps) {
  const [visible, setVisible] = useState(show && delay === 0);

  useEffect(() => {
    if (!show) {
      setVisible(false);
      return;
    }

    const timer = window.setTimeout(() => setVisible(true), delay);
    return () => window.clearTimeout(timer);
  }, [delay, show]);

  if (!visible) return null;

  return <div
    className={`atlas-loader atlas-loader--${mode}`}
    role="status"
    aria-live="polite"
    aria-busy="true"
    aria-label={message}
  >
    <div className="atlas-loader__glow atlas-loader__glow--mint" aria-hidden="true" />
    <div className="atlas-loader__glow atlas-loader__glow--coral" aria-hidden="true" />
    <div className="atlas-loader__card">
      <svg className="atlas-loader__mark" viewBox="0 0 120 120" aria-hidden="true">
        <circle className="atlas-loader__orbit-base" cx="60" cy="60" r="48" />
        <circle className="atlas-loader__orbit-runner" cx="60" cy="60" r="48" />
        <path className="atlas-loader__trace" d="M27 91 56 29c2-4 6-4 8 0l29 62M38 76h44" />
        <path className="atlas-loader__ledger" d="M42 88h37M46 98h29" />
        <circle className="atlas-loader__node atlas-loader__node--one" cx="27" cy="91" r="3.5" />
        <circle className="atlas-loader__node atlas-loader__node--two" cx="60" cy="23" r="3.5" />
        <circle className="atlas-loader__node atlas-loader__node--three" cx="93" cy="91" r="3.5" />
      </svg>
      <div className="atlas-loader__copy">
        <p className="atlas-loader__eyebrow">ATLAS · WORK IN MOTION</p>
        <p className="atlas-loader__message">{message}</p>
        {detail && <p className="atlas-loader__detail">{detail}</p>}
        <div className="atlas-loader__progress" aria-hidden="true"><span /><span /><span /></div>
      </div>
    </div>
  </div>;
}
