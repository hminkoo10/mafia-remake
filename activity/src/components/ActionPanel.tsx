import { useState } from "react";
import type { GameState, PlayerDto, ContractorTarget } from "../types";
import { sendAction } from "../api";

interface Props {
  state: GameState;
  onActionSent: () => void;
}

export function ActionPanel({ state, onActionSent }: Props) {
  const me = state.players.find((p) => p.is_you);

  // 밤 행동 결과 배너 (낮/투표/최후변론/찬반 단계에 표시)
  const resultBanner = state.my_action_result ? (
    <div style={{
      padding: "10px 14px", borderRadius: 10,
      background: "rgba(129,199,132,0.12)",
      border: "1px solid rgba(129,199,132,0.35)",
      fontSize: 13, color: "#a5d6a7",
      whiteSpace: "pre-line", lineHeight: 1.6,
    }}>
      🔍 {state.my_action_result}
    </div>
  ) : null;

  const showSkip = state.phase === "Day" && me?.alive;

  // 청부업자 전용 UI
  if (state.contractor_can_act) {
    return (
      <>
        {resultBanner}
        <ContractorPanel state={state} onActionSent={onActionSent} />
        {showSkip && <SkipPanel state={state} onActionSent={onActionSent} />}
      </>
    );
  }

  const showNightAction = state.phase === "Night" && me?.alive && state.can_act;

  return (
    <>
      {resultBanner}
      {showNightAction && <NightActionPanel state={state} onActionSent={onActionSent} />}
      {showSkip && <SkipPanel state={state} onActionSent={onActionSent} />}
    </>
  );
}

// ─── 일반 밤 행동 패널 ───────────────────────────────────────────

function NightActionPanel({ state, onActionSent }: Props) {
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const me = state.players.find((p) => p.is_you);
  const alivePlayers = state.players.filter((p) => p.alive && !p.is_you);
  const alreadyTargeted = state.my_night_target
    ? state.players.find((p) => p.id === state.my_night_target)
    : null;

  async function submit(targetId: string | null) {
    setPending(true);
    setMsg(null);
    const res = await sendAction({ action: "night_action", target_id: targetId ?? undefined });
    setMsg(res.ok ? "✅ 제출 완료" : res.message ?? "오류 발생");
    if (res.ok) { setSelectedTarget(null); onActionSent(); }
    setPending(false);
  }

  const actionLabel = nightActionLabel(me?.role ?? null);

  return (
    <div style={panelStyle("#5c6bc0")}>
      <div style={{ fontSize: 13, fontWeight: 600, color: "#9fa8da" }}>🌙 {actionLabel}</div>
      {alreadyTargeted && (
        <div style={{ fontSize: 12, color: "#81c784" }}>
          지목 완료: <strong>{alreadyTargeted.name}</strong>
        </div>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {alivePlayers.map((p) => (
          <TargetRow
            key={p.id} player={p}
            selected={selectedTarget === p.id}
            onClick={() => setSelectedTarget(p.id === selectedTarget ? null : p.id)}
            disabled={pending}
          />
        ))}
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <button disabled={pending || !selectedTarget} onClick={() => submit(selectedTarget)} style={btnStyle("#5c6bc0", pending || !selectedTarget)}>
          ✔ 대상 지목
        </button>
        <button disabled={pending} onClick={() => submit(null)} style={btnStyle("#607d8b", pending)}>
          ✖ 스킵
        </button>
      </div>
      {msg && <StatusMsg text={msg} />}
    </div>
  );
}

// ─── 청부업자 패널 ───────────────────────────────────────────────

function ContractorPanel({ state, onActionSent }: Props) {
  const [targets, setTargets] = useState<[string, string]>(["", ""]);
  const [roles, setRoles] = useState<[string, string]>(["", ""]);
  const [pending, setPending] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const available = state.contractor_targets;
  const guessRoles = state.contractor_guess_roles;

  function setTarget(slot: 0 | 1, id: string) {
    setTargets((prev) => slot === 0 ? [id, prev[1]] : [prev[0], id]);
  }
  function setRole(slot: 0 | 1, r: string) {
    setRoles((prev) => slot === 0 ? [r, prev[1]] : [prev[0], r]);
  }

  async function submit() {
    if (!targets[0] || !targets[1] || !roles[0] || !roles[1]) {
      setMsg("❌ 대상 2명과 직업 2개를 모두 선택하세요.");
      return;
    }
    if (targets[0] === targets[1]) {
      setMsg("❌ 대상 2명은 서로 달라야 합니다.");
      return;
    }
    setPending(true);
    setMsg(null);
    const res = await sendAction({
      action: "contractor_action",
      contract_target_ids: targets,
      contract_roles: roles,
    });
    setMsg(res.ok ? "✅ 청부 완료" : res.message ?? "오류 발생");
    if (res.ok) onActionSent();
    setPending(false);
  }

  return (
    <div style={panelStyle("#7b1fa2")}>
      <div style={{ fontSize: 13, fontWeight: 600, color: "#ce93d8" }}>🗡️ 청부 대상 선택</div>
      <div style={{ fontSize: 11, color: "#888" }}>
        대상 2명의 직업을 정확히 맞추면 암살합니다.
      </div>

      {([0, 1] as const).map((slot) => (
        <div key={slot} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ fontSize: 12, color: "#ce93d8", fontWeight: 600 }}>대상 {slot + 1}</div>
          <select
            value={targets[slot]}
            onChange={(e) => setTarget(slot, e.target.value)}
            disabled={pending}
            style={selectStyle}
          >
            <option value="">— 대상 선택 —</option>
            {available
              .filter((t: ContractorTarget) => t.id !== targets[slot === 0 ? 1 : 0])
              .map((t: ContractorTarget) => (
                <option key={t.id} value={t.id}>{t.name}</option>
              ))}
          </select>
          <select
            value={roles[slot]}
            onChange={(e) => setRole(slot, e.target.value)}
            disabled={pending}
            style={selectStyle}
          >
            <option value="">— 직업 추측 —</option>
            {guessRoles.map((r: string) => (
              <option key={r} value={r}>{r}</option>
            ))}
          </select>
        </div>
      ))}

      <button disabled={pending} onClick={submit} style={btnStyle("#7b1fa2", pending)}>
        🗡️ 청부 실행
      </button>
      {msg && <StatusMsg text={msg} />}
    </div>
  );
}

// ─── 낮 스킵 투표 패널 ──────────────────────────────────────────

function SkipPanel({ state, onActionSent }: Props) {
  const [pending, setPending] = useState(false);
  const [done, setDone] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  async function skip() {
    setPending(true);
    const res = await sendAction({ action: "skip_vote" });
    if (res.ok) { setDone(true); onActionSent(); }
    else setMsg(res.message ?? "오류 발생");
    setPending(false);
  }

  return (
    <div style={{ ...panelStyle("#607d8b"), flexDirection: "row", alignItems: "center", gap: 12 }}>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 12, color: "#b0bec5", fontWeight: 600 }}>⏩ 낮 스킵 투표</div>
        <div style={{ fontSize: 11, color: "#78909c", marginTop: 2 }}>
          {state.day_skip_count} / {state.day_skip_threshold}명 (과반수)
        </div>
      </div>
      <button
        disabled={pending || done}
        onClick={skip}
        style={{ ...btnStyle("#546e7a", pending || done), flex: "none", padding: "8px 16px" }}
      >
        {done ? "✅ 투표함" : "스킵"}
      </button>
      {msg && <StatusMsg text={msg} />}
    </div>
  );
}

// ─── 공통 컴포넌트 ───────────────────────────────────────────────

function TargetRow({ player, selected, onClick, disabled }: {
  player: PlayerDto; selected: boolean; onClick: () => void; disabled: boolean;
}) {
  return (
    <div
      onClick={disabled ? undefined : onClick}
      style={{
        display: "flex", alignItems: "center", gap: 8,
        padding: "7px 10px", borderRadius: 7,
        background: selected ? "rgba(92,107,192,0.3)" : "rgba(255,255,255,0.04)",
        border: selected ? "1px solid #5c6bc0" : "1px solid rgba(255,255,255,0.08)",
        cursor: disabled ? "default" : "pointer",
        transition: "background 0.15s",
      }}
    >
      <div style={{ width: 7, height: 7, borderRadius: "50%", background: selected ? "#9fa8da" : "#444" }} />
      <span style={{ fontSize: 14 }}>{player.name}</span>
      {player.role && <span style={{ marginLeft: "auto", fontSize: 11, color: "#666" }}>{player.role}</span>}
    </div>
  );
}

function StatusMsg({ text }: { text: string }) {
  return (
    <div style={{ fontSize: 12, color: text.startsWith("✅") ? "#81c784" : "#ff6b6b" }}>
      {text}
    </div>
  );
}

function panelStyle(accent: string): React.CSSProperties {
  return {
    display: "flex", flexDirection: "column", gap: 10,
    padding: "12px 14px", borderRadius: 10,
    background: `color-mix(in srgb, ${accent} 12%, transparent)`,
    border: `1px solid color-mix(in srgb, ${accent} 35%, transparent)`,
  };
}

function btnStyle(bg: string, disabled: boolean): React.CSSProperties {
  return {
    flex: 1, padding: "9px 0", borderRadius: 7, border: "none",
    background: disabled ? "#2a2a3a" : bg,
    color: disabled ? "#555" : "#fff",
    fontSize: 13, fontWeight: 600,
    cursor: disabled ? "default" : "pointer",
    transition: "background 0.2s",
  };
}

const selectStyle: React.CSSProperties = {
  padding: "7px 10px", borderRadius: 7,
  background: "#1e1e2e", border: "1px solid rgba(255,255,255,0.15)",
  color: "#ddd", fontSize: 13, width: "100%",
};

function nightActionLabel(role: string | null): string {
  const map: Record<string, string> = {
    마피아: "마피아 공격 대상", 의사: "치료 대상 선택", 경찰: "조사 대상 선택",
    요원: "조사 대상 선택", 자경단원: "처단 대상 선택", 탐정: "추적 대상 선택",
    사립탐정: "추적 대상 선택", 기자: "취재 대상 선택", 대부: "공격 지시 대상",
    건달: "위협 대상 선택", 교주: "포섭 대상 선택", 광신도: "포섭 대상 선택",
    마담: "유혹 대상 선택", 마녀: "저주 대상 선택", 스파이: "감시 대상 선택",
    간호사: "처방 대상 선택", 영매: "교신 대상 선택", 성직자: "정화 대상 선택",
    테러리스트: "폭탄 대상 선택", 과학자: "소생 대상 선택",
  };
  return role ? (map[role] ?? "행동 대상 선택") : "행동 대상 선택";
}
