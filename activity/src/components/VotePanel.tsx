import { useState } from "react";
import type { GameState, PlayerDto } from "../types";
import { sendAction } from "../api";

interface Props {
  state: GameState;
  onActionSent: () => void;
}

export function VotePanel({ state, onActionSent }: Props) {
  const [pending, setPending] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const me = state.players.find((p) => p.is_you);
  const alivePlayers = state.players.filter((p) => p.alive);

  async function vote(targetId: string | null) {
    setPending(true);
    setMsg(null);
    const res = await sendAction({ action: "day_vote", target_id: targetId ?? undefined });
    setMsg(res.ok ? null : res.message ?? "오류 발생");
    if (res.ok) onActionSent();
    setPending(false);
  }

  async function confirmVote(agree: boolean) {
    setPending(true);
    setMsg(null);
    const res = await sendAction({ action: "confirm_vote", confirm: agree });
    setMsg(res.ok ? null : res.message ?? "오류 발생");
    if (res.ok) onActionSent();
    setPending(false);
  }

  if (!me?.alive) {
    return <div style={{ color: "#555", fontSize: 13, textAlign: "center" }}>사망자는 투표할 수 없습니다.</div>;
  }

  // ConfirmVote 단계
  if (state.phase === "ConfirmVote") {
    const nominee = state.players.find((p) => p.id === state.nominee);
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ fontSize: 14, color: "#e0e0e0", textAlign: "center" }}>
          <span style={{ color: "#ff6b6b", fontWeight: 700 }}>
            {nominee?.name ?? "?"}
          </span>
          님을 처형할까요?
        </div>
        <div style={{ fontSize: 13, color: "#aaa", textAlign: "center" }}>
          {state.show_confirm_counts
            ? `찬성 ${state.confirm_yes} / 반대 ${state.confirm_no}`
            : "찬반 집계 비공개"}
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <Button
            label="✅ 찬성"
            color="#4caf50"
            disabled={pending}
            onClick={() => confirmVote(true)}
          />
          <Button
            label="❌ 반대"
            color="#f44336"
            disabled={pending}
            onClick={() => confirmVote(false)}
          />
        </div>
        {msg && <div style={{ color: "#ff6b6b", fontSize: 12 }}>{msg}</div>}
      </div>
    );
  }

  // Vote / FinalDefense 단계
  if (state.phase === "Vote") {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ fontSize: 12, color: "#888" }}>
          처형할 플레이어를 선택하세요
        </div>
        {alivePlayers.map((p) => {
          const voteCount = state.vote_targets[p.id] ?? 0;
          return (
            <VoteRow
              key={p.id}
              player={p}
              votes={voteCount}
              disabled={pending}
              onClick={() => vote(p.id)}
            />
          );
        })}
        <Button
          label="🤫 스킵"
          color="#607d8b"
          disabled={pending}
          onClick={() => vote(null)}
        />
        {msg && <div style={{ color: "#ff6b6b", fontSize: 12 }}>{msg}</div>}
      </div>
    );
  }

  return null;
}

function VoteRow({
  player, votes, disabled, onClick,
}: {
  player: PlayerDto;
  votes: number;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "8px 10px",
        borderRadius: 8,
        background: "rgba(255,255,255,0.04)",
        border: "1px solid rgba(255,255,255,0.1)",
        cursor: disabled ? "default" : "pointer",
      }}
      onClick={disabled ? undefined : onClick}
    >
      <span style={{ flex: 1, fontSize: 14 }}>{player.name}</span>
      {votes > 0 && (
        <span style={{
          background: "#ff6b6b22",
          color: "#ff6b6b",
          borderRadius: 4,
          padding: "1px 6px",
          fontSize: 12,
          fontWeight: 700,
        }}>
          {votes}표
        </span>
      )}
    </div>
  );
}

function Button({
  label, color, disabled, onClick,
}: {
  label: string;
  color: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      disabled={disabled}
      onClick={onClick}
      style={{
        flex: 1,
        padding: "10px 0",
        borderRadius: 8,
        border: "none",
        background: disabled ? "#333" : color,
        color: "#fff",
        fontSize: 14,
        fontWeight: 600,
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
        transition: "opacity 0.2s",
      }}
    >
      {label}
    </button>
  );
}
