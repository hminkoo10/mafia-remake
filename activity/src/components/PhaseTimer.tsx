import { useEffect, useState } from "react";
import type { Phase } from "../types";
import { PHASE_LABELS } from "../types";

interface Props {
  phase: Phase;
  dayNumber: number;
  phaseEndsAt: number | null; // unix ms
}

export function PhaseTimer({ phase, dayNumber, phaseEndsAt }: Props) {
  const [remaining, setRemaining] = useState<number | null>(null);

  useEffect(() => {
    if (!phaseEndsAt) {
      setRemaining(null);
      return;
    }
    const update = () => {
      const diff = Math.max(0, Math.ceil((phaseEndsAt - Date.now()) / 1000));
      setRemaining(diff);
    };
    update();
    const id = setInterval(update, 500);
    return () => clearInterval(id);
  }, [phaseEndsAt]);

  const phaseColor: Record<Phase, string> = {
    Night: "#5c6bc0",
    Day: "#fdd835",
    Vote: "#ef5350",
    FinalDefense: "#ff7043",
    ConfirmVote: "#ab47bc",
    Ended: "#78909c",
  };

  const color = phaseColor[phase];

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 14px",
      borderRadius: 10,
      background: `${color}18`,
      border: `1px solid ${color}44`,
    }}>
      <div>
        <div style={{ fontSize: 18, fontWeight: 700, color }}>
          {PHASE_LABELS[phase]}
        </div>
        <div style={{ fontSize: 12, color: "#888", marginTop: 2 }}>
          {dayNumber}일차
        </div>
      </div>

      {remaining !== null && (
        <div style={{
          fontSize: 28,
          fontWeight: 700,
          color: remaining <= 10 ? "#ff6b6b" : color,
          fontVariantNumeric: "tabular-nums",
          transition: "color 0.3s",
        }}>
          {String(Math.floor(remaining / 60)).padStart(2, "0")}:
          {String(remaining % 60).padStart(2, "0")}
        </div>
      )}
    </div>
  );
}
