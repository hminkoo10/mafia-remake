import type { PlayerDto } from "../types";
import { TEAM_COLORS } from "../types";

interface Props {
  players: PlayerDto[];
  selectedId?: string;
  onSelect?: (id: string) => void;
  highlightVotes?: Record<string, number>;
}

export function PlayerList({ players, selectedId, onSelect, highlightVotes }: Props) {
  const alive = players.filter((p) => p.alive);
  const dead = players.filter((p) => !p.alive);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={{ fontSize: 12, color: "#888", marginBottom: 4 }}>
        생존 {alive.length}명 / 사망 {dead.length}명
      </div>
      {alive.map((p) => (
        <PlayerCard
          key={p.id}
          player={p}
          selected={selectedId === p.id}
          votes={highlightVotes?.[p.id]}
          onClick={() => onSelect?.(p.id)}
          clickable={!!onSelect && !p.is_you}
        />
      ))}
      {dead.length > 0 && (
        <>
          <div style={{ fontSize: 11, color: "#555", margin: "6px 0 2px" }}>─ 사망</div>
          {dead.map((p) => (
            <PlayerCard key={p.id} player={p} clickable={false} />
          ))}
        </>
      )}
    </div>
  );
}

interface CardProps {
  player: PlayerDto;
  selected?: boolean;
  votes?: number;
  onClick?: () => void;
  clickable: boolean;
}

function PlayerCard({ player, selected, votes, onClick, clickable }: CardProps) {
  const teamColor = player.role_team ? TEAM_COLORS[player.role_team] : "#666";

  return (
    <div
      onClick={clickable ? onClick : undefined}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "8px 10px",
        borderRadius: 8,
        background: selected ? "rgba(88,101,242,0.3)" : "rgba(255,255,255,0.05)",
        border: selected ? "1px solid #5865f2" : "1px solid rgba(255,255,255,0.08)",
        cursor: clickable ? "pointer" : "default",
        opacity: player.alive ? 1 : 0.45,
        transition: "background 0.15s",
      }}
    >
      {/* 팀 컬러 도트 */}
      <div style={{
        width: 8, height: 8, borderRadius: "50%",
        background: player.alive ? teamColor : "#444",
        flexShrink: 0,
      }} />

      {/* 이름 */}
      <span style={{
        flex: 1,
        fontSize: 14,
        fontWeight: player.is_you ? 600 : 400,
        color: player.alive ? "#e0e0e0" : "#666",
      }}>
        {player.name}
        {player.is_you && <span style={{ color: "#5865f2", fontSize: 11, marginLeft: 4 }}>(나)</span>}
      </span>

      {/* 역할 배지 */}
      {player.role && (
        <span style={{
          fontSize: 11,
          padding: "2px 6px",
          borderRadius: 4,
          background: `${teamColor}22`,
          color: teamColor,
          border: `1px solid ${teamColor}44`,
        }}>
          {player.role}
        </span>
      )}

      {/* 득표수 */}
      {votes !== undefined && votes > 0 && (
        <span style={{
          fontSize: 13,
          fontWeight: 700,
          color: "#ff6b6b",
          minWidth: 20,
          textAlign: "right",
        }}>
          {votes}표
        </span>
      )}
    </div>
  );
}
