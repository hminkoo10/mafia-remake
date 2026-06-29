export type Phase =
  | "Night"
  | "Day"
  | "Vote"
  | "FinalDefense"
  | "ConfirmVote"
  | "Ended";

export type RoleTeam = "Citizen" | "Mafia" | "Cult" | "Neutral";
export type ActivitySpecialAction = "hacker" | "vigilante" | "psychologist" | "hypnotist";

export interface PlayerDto {
  id: string;
  name: string;
  alive: boolean;
  is_you: boolean;
  role: string | null;
  role_team: RoleTeam | null;
}

export interface ContractorTarget {
  id: string;
  name: string;
}

export interface GameState {
  game_key: string;
  in_game: boolean;
  phase: Phase;
  day_number: number;
  phase_ends_at: number | null;
  players: PlayerDto[];
  my_role: string | null;
  my_team: RoleTeam | null;
  can_act: boolean;
  my_night_target: string | null;
  my_action_result: string | null;    // 밤 행동 결과 (낮에 표시)
  night_target_ids: string[];
  night_action_can_skip: boolean;
  special_action: ActivitySpecialAction | null;
  special_action_target_ids: string[];
  vote_targets: Record<string, number>;
  vote_skip_count: number;
  nominee: string | null;
  confirm_yes: number;
  confirm_no: number;
  winner: string | null;
  public_status: string;
  day_skip_count: number;
  day_skip_threshold: number;
  contractor_can_act: boolean;
  contractor_targets: ContractorTarget[];
  contractor_guess_roles: string[];
}

export type ActionType =
  | "night_action"
  | "day_vote"
  | "confirm_vote"
  | "skip_vote"
  | "contractor_action"
  | "hacker_action"
  | "vigilante_action"
  | "psychologist_action"
  | "hypnotist_action";

export interface ActionRequest {
  guild_id: string;
  action: ActionType;
  target_id?: string;
  secondary_target_id?: string;
  confirm?: boolean;
  contract_target_ids?: [string, string];
  contract_roles?: [string, string];
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
