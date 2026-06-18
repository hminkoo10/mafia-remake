export type Phase =
  | "Night"
  | "Day"
  | "Vote"
  | "FinalDefense"
  | "ConfirmVote"
  | "Ended";

export type RoleTeam = "Citizen" | "Mafia" | "Cult" | "Neutral";

export interface PlayerDto {
  id: string;
  name: string;
  alive: boolean;
  is_you: boolean;
  role: string | null;
  role_team: RoleTeam | null;
}

export interface GameState {
  in_game: boolean;
  phase: Phase;
  day_number: number;
  phase_ends_at: number | null;
  players: PlayerDto[];
  my_role: string | null;
  my_team: RoleTeam | null;
  can_act: boolean;
  my_night_target: string | null;
  vote_targets: Record<string, number>;
  nominee: string | null;
  confirm_yes: number;
  confirm_no: number;
  winner: string | null;
  public_status: string;
}

export type ActionType =
  | "night_action"
  | "day_vote"
  | "confirm_vote"
  | "skip_vote";

export interface ActionRequest {
  guild_id: string;
  action: ActionType;
  target_id?: string;
  confirm?: boolean;
}

export const PHASE_LABELS: Record<Phase, string> = {
  Night: "🌙 밤",
  Day: "☀️ 낮",
  Vote: "🗳️ 투표",
  FinalDefense: "🗣️ 최후변론",
  ConfirmVote: "⚖️ 처형 확인",
  Ended: "🏁 게임 종료",
};

export const TEAM_COLORS: Record<RoleTeam, string> = {
  Citizen: "#4caf50",
  Mafia: "#f44336",
  Cult: "#9c27b0",
  Neutral: "#ff9800",
};
