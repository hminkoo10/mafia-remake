import { useState } from "react";
import type { GameState, PlayerDto } from "../types";
import { sendAction } from "../api";

interface Props {
  state: GameState;
  onActionSent: () => void;
}

export function ActionPanel({ state, onActionSent }: Props) {
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const me = state.players.find((p) => p.is_you);

  if (!me?.alive || !state.can_act) return null;
  if (!["Night", "Day"].includes(state.phase)) return null;

  const canActTonight = state.phase === "Night";
  const alivePlayers = state.players.filter((p) => p.alive && !p.is_you);

  // 밤 행동 중 이미 지목한 대상
  const alreadyTargeted = state.my_night_target
    ? state.players.find((p) => p.id === state.my_night_target)
    : null;

  async function submitNightAction(targetId: string | null) {
    setPending(true);
    setMsg(null);
    const res = await sendAction({
      action: "night_action",
      target_id: targetId ?? undefined,
    });
    setMsg(res.ok ? "✅ 제출 완료" : res.message ?? "오류 발생");
    if (res.ok) {
      setSelectedTarget(null);
      onActionSent();
    }
    setPending(false);
  }

  const actionLabel = nightActionLabel(me.role);

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      gap: 10,
      padding: "12px 14px",
      borderRadius: 10,
      background: "rgba(92,107,192,0.12)",
      border: "1px solid rgba(92,107,192,0.3)",
    }}>
      <div style={{ fontSize: 13, fontWeight: 600, color: "#9fa8da" }}>
        {canActTonight ? `🌙 ${actionLabel}` : "☀️ 낮 행동"}
      </div>

      {alreadyTargeted && (
        <div style={{ fontSize: 12, color: "#81c784" }}>
          지목 완료: <strong>{alreadyTargeted.name}</strong>
        </div>
      )}

      {/* 대상 선택 목록 */}
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {alivePlayers.map((p) => (
          <TargetRow
            key={p.id}
            player={p}
            selected={selectedTarget === p.id}
            onClick={() => setSelectedTarget(p.id === selectedTarget ? null : p.id)}
            disabled={pending}
          />
        ))}
      </div>

      {/* 제출 버튼들 */}
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <button
          disabled={pending || !selectedTarget}
          onClick={() => submitNightAction(selectedTarget)}
          style={btnStyle("#5c6bc0", pending || !selectedTarget)}
        >
          ✔ 대상 지목
        </button>
        <button
          disabled={pending}
          onClick={() => submitNightAction(null)}
          style={btnStyle("#607d8b", pending)}
        >
          ✖ 스킵
        </button>
      </div>

      {msg && (
        <div style={{
          fontSize: 12,
          color: msg.startsWith("✅") ? "#81c784" : "#ff6b6b",
        }}>
          {msg}
        </div>
      )}
    </div>
  );
}

function TargetRow({
  player, selected, onClick, disabled,
}: {
  player: PlayerDto;
  selected: boolean;
  onClick: () => void;
  disabled: boolean;
}) {
  return (
    <div
      onClick={disabled ? undefined : onClick}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "7px 10px",
        borderRadius: 7,
        background: selected ? "rgba(92,107,192,0.3)" : "rgba(255,255,255,0.04)",
        border: selected ? "1px solid #5c6bc0" : "1px solid rgba(255,255,255,0.08)",
        cursor: disabled ? "default" : "pointer",
        transition: "background 0.15s",
      }}
    >
      <div style={{
        width: 7, height: 7, borderRadius: "50%",
        background: selected ? "#9fa8da" : "#444",
        transition: "background 0.15s",
      }} />
      <span style={{ fontSize: 14 }}>{player.name}</span>
      {player.role && (
        <span style={{ marginLeft: "auto", fontSize: 11, color: "#666" }}>{player.role}</span>
      )}
    </div>
  );
}

function btnStyle(bg: string, disabled: boolean): React.CSSProperties {
  return {
    flex: 1,
    padding: "9px 0",
    borderRadius: 7,
    border: "none",
    background: disabled ? "#2a2a3a" : bg,
    color: disabled ? "#555" : "#fff",
    fontSize: 13,
    fontWeight: 600,
    cursor: disabled ? "default" : "pointer",
    transition: "background 0.2s",
  };
}

function nightActionLabel(role: string | null): string {
  const map: Record<string, string> = {
    마피아: "마피아 공격 대상",
    의사: "치료 대상 선택",
    경찰: "조사 대상 선택",
    요원: "조사 대상 선택",
    자경단: "처단 대상 선택",
    탐정: "조사 대상 선택",
    기자: "취재 대상 선택",
    대부: "공격 지시 대상",
    건달: "위협 대상 선택",
    교주: "포섭 대상 선택",
    광신도: "포섭 대상 선택",
    마담: "유혹 대상 선택",
    마녀: "저주 대상 선택",
    스파이: "감시 대상 선택",
    간호사: "처방 대상 선택",
    영매: "교신 대상 선택",
    성직자: "정화 대상 선택",
    테러리스트: "폭탄 대상 선택",
    과학자: "소생 대상 선택",
  };
  return role ? (map[role] ?? "행동 대상 선택") : "행동 대상 선택";
}
