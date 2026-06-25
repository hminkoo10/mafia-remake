import type { RoleTeam } from "../types";
import { TEAM_COLORS } from "../types";

interface Props {
  role: string | null;
  team: RoleTeam | null;
}

const ROLE_DESCRIPTIONS: Record<string, string> = {
  시민: "특별한 능력이 없습니다. 낮에 마피아를 찾아 처형하세요.",
  마피아: "매 밤 시민을 한 명 공격합니다.",
  경찰: "매 밤 한 명을 조사해 마피아팀 여부를 확인합니다.",
  의사: "매 밤 한 명을 치료해 마피아 공격을 막습니다.",
  요원: "매 밤 한 명을 조사합니다.",
  자경단: "마피아를 직접 처단할 수 있습니다.",
  탐정: "낮에 특수 조사를 수행합니다.",
  기자: "조사 결과를 공개할 수 있습니다.",
  해커: "낮에 상대방의 행동을 해킹합니다.",
  테러리스트: "폭탄으로 자신과 대상을 제거합니다.",
  정치인: "투표권이 2표입니다.",
  판사: "찬반 투표 결과를 결정할 수 있습니다.",
  심리학자: "낮에 한 명을 관찰합니다.",
  도둑: "다른 플레이어의 역할을 훔칩니다.",
  군인: "한 번 마피아 공격을 막을 수 있습니다.",
  간호사: "의사를 돕습니다.",
  예언자: "특수 승리 조건이 있습니다.",
  최면술사: "밤에 최면을 누적하고 낮에 해제해 비시민 직업을 확인합니다.",
  영매: "죽은 플레이어와 소통합니다.",
  연인: "짝꿍이 죽으면 함께 사망합니다.",
  대부: "경찰 조사에 시민으로 위장됩니다.",
  건달: "마피아팀이지만 처음엔 혼자입니다.",
  스파이: "마피아 정보를 수집합니다.",
  교주: "신도를 포섭해 교주팀을 만듭니다.",
  광신도: "교주의 명령을 따릅니다.",
  마담: "낮에 한 명을 유혹해 행동을 막습니다.",
  마녀: "저주를 걸어 플레이어를 교주팀으로 만듭니다.",
  과학자: "죽은 플레이어를 소생시킵니다.",
  청부업자: "특수 계약으로 승리를 노립니다.",
  조커: "처형당하면 승리합니다.",
  성직자: "저주를 정화합니다.",
  개구리: "마피아팀에서 벗어날 수 있습니다.",
  고양이: "특수 능력을 가집니다.",
};

export function RoleCard({ role, team }: Props) {
  if (!role || !team) {
    return (
      <div style={{
        padding: "12px 14px",
        borderRadius: 10,
        background: "rgba(255,255,255,0.04)",
        border: "1px solid rgba(255,255,255,0.08)",
        textAlign: "center",
        color: "#555",
        fontSize: 13,
      }}>
        역할 없음 (관전자)
      </div>
    );
  }

  const color = TEAM_COLORS[team];
  const desc = ROLE_DESCRIPTIONS[role] ?? "능력 설명이 없습니다.";

  return (
    <div style={{
      padding: "12px 14px",
      borderRadius: 10,
      background: `${color}12`,
      border: `1px solid ${color}40`,
    }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
        <div style={{
          width: 10, height: 10, borderRadius: "50%",
          background: color, flexShrink: 0,
        }} />
        <span style={{ fontSize: 18, fontWeight: 700, color }}>
          {role}
        </span>
        <span style={{
          marginLeft: "auto",
          fontSize: 11,
          padding: "2px 7px",
          borderRadius: 4,
          background: `${color}22`,
          color: color,
        }}>
          {team === "Citizen" ? "시민팀" :
           team === "Mafia" ? "마피아팀" :
           team === "Cult" ? "교주팀" : "중립"}
        </span>
      </div>
      <p style={{ fontSize: 12, color: "#aaa", lineHeight: 1.5 }}>{desc}</p>
    </div>
  );
}
