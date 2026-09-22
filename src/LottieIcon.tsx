import { Lottie } from "lottie-react";
import robotHello from "./assets/lottie/robot-hello.json";
import trophy from "./assets/lottie/trophy.json";
import auditDoc from "./assets/lottie/audit-doc.json";
import mailHello from "./assets/lottie/mail-hello.json";
import mailDelivery from "./assets/lottie/mail-delivery.json";
import analytics from "./assets/lottie/analytics.json";
import clipboard from "./assets/lottie/clipboard.json";

// Bundled Lottie animations (LottieFiles, Lottie Simple License) so they work
// fully offline inside the desktop app.
const animations = {
  robotHello,
  trophy,
  auditDoc,
  mailHello,
  mailDelivery,
  analytics,
  clipboard,
} as const;

export type LottieName = keyof typeof animations;

export default function LottieIcon({
  name,
  className,
  loop = true,
}: {
  name: LottieName;
  className?: string;
  loop?: boolean;
}) {
  return (
    <Lottie
      src={animations[name]}
      loop={loop}
      autoplay
      className={className}
      aria-hidden="true"
    />
  );
}
