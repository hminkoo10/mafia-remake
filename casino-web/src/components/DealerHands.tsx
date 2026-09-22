import { useEffect, useMemo, useState, type CSSProperties } from "react";
import "./dealer-hands.css";

export type DealerMood = "idle" | "deal" | "flip";
export type DealerHandsProps = {
  mood: DealerMood;
  targets: Array<[number, number]>;
  center?: [number, number];
  reducedMotion?: boolean;
};

const SHOE_POSITION: [number, number] = [84, 62];

function clampPercent(value: number) {
  return Math.max(0, Math.min(100, value));
}

function Hand({ side = "right", className = "" }: { side?: "left" | "right"; className?: string }) {
  return (
    <svg
      aria-hidden="true"
      className={`dealer-hands__hand dealer-hands__hand--${side} ${className}`}
      viewBox="0 0 180 150"
      role="presentation"
    >
      <path className="dealer-hands__sleeve" d="M0 0h68l28 70-36 80H0z" />
      <path className="dealer-hands__sleeve-cuff" d="M54 52l42 18-21 46-42-18z" />
      <path
        className="dealer-hands__skin"
        d="M65 72c8-13 15-22 28-25l35-10c6-2 10 0 11 4s-1 8-7 10l-22 8 35-7c6-1 10 2 10 6s-3 7-8 8l-35 9 32-2c6 0 10 3 10 7s-4 7-9 7l-37 4 23 4c6 1 9 5 8 9s-5 6-10 5l-34-4c-10-1-18-3-24-10L53 94z"
      />
      <path className="dealer-hands__knuckle" d="M98 63l17-5M100 77l18-4M100 91l17-1" />
      <path className="dealer-hands__cuff-trim" d="M55 53l41 17" />
    </svg>
  );
}

function CardBack({ className = "" }: { className?: string }) {
  return <span aria-hidden="true" className={`dealer-hands__card-back ${className}`} />;
}

function Position(point: [number, number]) {
  return { left: `${clampPercent(point[0])}%`, top: `${clampPercent(point[1])}%` };
}

export function DealerHands({ mood, targets, center = [50, 48], reducedMotion = false }: DealerHandsProps) {
  const [systemReducedMotion, setSystemReducedMotion] = useState(false);
  const [targetIndex, setTargetIndex] = useState(0);

  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setSystemReducedMotion(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    setTargetIndex(0);
    if (mood !== "deal" || reducedMotion || systemReducedMotion) return;
    const timer = window.setInterval(() => setTargetIndex((index) => index + 1), 900);
    return () => window.clearInterval(timer);
  }, [mood, reducedMotion, systemReducedMotion, targets.length]);

  const motionDisabled = reducedMotion || systemReducedMotion;
  const safeTarget = targets.length ? targets[targetIndex % targets.length] : center;
  const targetStyle = useMemo(() => Position(safeTarget), [safeTarget]);
  const centerStyle = useMemo(() => Position(center), [center]);
  const shoeStyle = useMemo(() => Position(SHOE_POSITION), []);

  return (
    <div
      className={`dealer-hands dealer-hands--${mood}${motionDisabled ? " dealer-hands--reduced-motion" : ""}`}
      aria-hidden="true"
    >
      <div className="dealer-hands__shoe" style={shoeStyle}>
        <span className="dealer-hands__shoe-lip" />
        <span className="dealer-hands__shoe-label">SHOE</span>
      </div>

      <div className="dealer-hands__scene" style={centerStyle}>
        <div className="dealer-hands__idle-pair">
          <Hand side="left" />
          <Hand side="right" />
        </div>

        <div className="dealer-hands__flip-pair">
          <Hand side="left" />
          <Hand side="right" />
          <CardBack className="dealer-hands__flip-card" />
        </div>
      </div>

      <div
        key={`deal-${targetIndex}`}
        className="dealer-hands__deal-path"
        style={{ "--deal-x": targetStyle.left, "--deal-y": targetStyle.top } as CSSProperties}
      >
        <Hand side="right" className="dealer-hands__dealing-hand" />
        <CardBack className="dealer-hands__dealing-card" />
      </div>
    </div>
  );
}

export default DealerHands;
