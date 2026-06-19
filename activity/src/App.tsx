import { useCallback, useEffect, useMemo, useState } from "react";
import { createWebSocket, fetchState, sendAction, setSession } from "./api";
import { authenticateWithDiscord } from "./discord";
import type { ActionRequest, GameState, Phase, PlayerDto, RoleTeam } from "./types";

type AuthStatus = "loading" | "ready" | "error";
type ConnectionStatus = "connecting" | "live" | "offline";
type PlayerFilter = "all" | "alive" | "dead" | "marked";
type PlayerMark = "none" | "trust" | "suspect" | "watch";

const PHASE_META: Record<Phase, { label: string; tone: string; summary: string }> = {
  Night: { label: "밤", tone: "night", summary: "비공개 행동" },
  Day: { label: "낮", tone: "day", summary: "토론과 정보 정리" },
  Vote: { label: "투표", tone: "vote", summary: "처형 대상 지목" },
  FinalDefense: { label: "최후변론", tone: "defense", summary: "지목자 방어" },
  ConfirmVote: { label: "처형 확인", tone: "confirm", summary: "찬반 결정" },
  Ended: { label: "게임 종료", tone: "ended", summary: "결과 확인" },
};

const TEAM_META: Record<RoleTeam, { label: string; className: string }> = {
  Citizen: { label: "시민팀", className: "team-citizen" },
  Mafia: { label: "마피아팀", className: "team-mafia" },
  Cult: { label: "교주팀", className: "team-cult" },
  Neutral: { label: "중립", className: "team-neutral" },
};

const MARK_META: Record<PlayerMark, { label: string; short: string; className: string }> = {
  none: { label: "표시 없음", short: "-", className: "mark-none" },
  trust: { label: "신뢰", short: "신", className: "mark-trust" },
  suspect: { label: "의심", short: "의", className: "mark-suspect" },
  watch: { label: "관찰", short: "관", className: "mark-watch" },
};

const ROLE_HELP: Record<string, string> = {
  시민: "발언, 투표, 공개 정보를 묶어 마피아 후보를 좁히세요.",
  마피아: "낮에는 시민처럼 보이고 밤에는 킬 우선순위를 정하세요.",
  경찰: "조사 결과와 발언 모순을 함께 기록하세요.",
  의사: "킬 가능성이 높은 확직과 핵심 발언자를 우선 보호하세요.",
  요원: "조사 결과를 낮 토론 흐름과 맞춰 공개하세요.",
  자경단원: "확신 없는 처단은 시민 수를 크게 줄입니다.",
  사립탐정: "대상 이동과 밤 행동 루트를 메모하세요.",
  기자: "공개 타이밍이 판 흐름을 바꿉니다.",
  해커: "상대 행동 정보는 다음 낮 지목 근거가 됩니다.",
  영매: "사망자 정보와 산 사람 발언을 연결하세요.",
  성직자: "정화 대상과 위험 직업을 분리해 보세요.",
  교주: "포교 성공 후 팀 구도를 빠르게 다시 계산하세요.",
  광신도: "교주 생존과 포교 정보 보존이 중요합니다.",
  조커: "처형 유도와 과한 의심 사이 균형이 핵심입니다.",
  청부업자: "두 대상과 직업 추측을 확신할 때 제출하세요.",
  심리학자: "낮 행동 대상의 태도 변화를 함께 보세요.",
  도둑: "훔칠 직업 가치와 생존 가능성을 비교하세요.",
  군인: "방탄 가능성을 노출할 타이밍을 아끼세요.",
  간호사: "의사 위치와 치료 흐름을 보조하세요.",
  마담: "핵심 투표권을 묶어 낮 구도를 흔드세요.",
  마녀: "저주 대상이 팀 계산을 바꿀 수 있습니다.",
  과학자: "소생 타이밍은 공개 정보와 같이 봐야 합니다.",
  대부: "조사 회피를 믿고 과감하게 라인을 만들 수 있습니다.",
  스파이: "마피아 접선 전까지 정보 손실을 줄이세요.",
  테러리스트: "처형과 폭발 대상의 교환 가치를 계산하세요.",
  정치인: "2표 영향력이 큰 최종 투표를 관리하세요.",
  판사: "찬반 동률과 막판 뒤집기를 의식하세요.",
};

export default function App() {
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>("connecting");
  const [errorMsg, setErrorMsg] = useState("");
  const [guildId, setGuildId] = useState("");
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [playerFilter, setPlayerFilter] = useState<PlayerFilter>("alive");
  const [playerMarks, setPlayerMarks] = useState<Record<string, PlayerMark>>({});
  const [notes, setNotes] = useState("");

  useEffect(() => {
    (async () => {
      try {
        const auth = await authenticateWithDiscord();
        setSession(auth.sessionToken, auth.guildId);
        setGuildId(auth.guildId);
        setAuthStatus("ready");
      } catch (e) {
        setErrorMsg(e instanceof Error ? e.message : JSON.stringify(e));
        setAuthStatus("error");
      }
    })();
  }, []);

  const refreshState = useCallback(async () => {
    const state = await fetchState();
    setGameState(state);
    setConnectionStatus("live");
  }, []);

  useEffect(() => {
    if (authStatus !== "ready") return;

    setConnectionStatus("connecting");
    refreshState().catch((error) => {
      console.error(error);
      setConnectionStatus("offline");
    });

    const socket = createWebSocket((state) => {
      setGameState(state);
      setConnectionStatus("live");
    });
    socket.addEventListener("open", () => setConnectionStatus("live"));
    socket.addEventListener("close", () => setConnectionStatus("offline"));
    socket.addEventListener("error", () => setConnectionStatus("offline"));

    return () => socket.close();
  }, [authStatus, refreshState]);

  useEffect(() => {
    if (!guildId) return;
    setPlayerMarks(readJson<Record<string, PlayerMark>>(`mafia-activity:${guildId}:marks`, {}));
    setNotes(localStorage.getItem(`mafia-activity:${guildId}:notes`) ?? "");
  }, [guildId]);

  useEffect(() => {
    if (!guildId) return;
    localStorage.setItem(`mafia-activity:${guildId}:marks`, JSON.stringify(playerMarks));
  }, [guildId, playerMarks]);

  useEffect(() => {
    if (!guildId) return;
    localStorage.setItem(`mafia-activity:${guildId}:notes`, notes);
  }, [guildId, notes]);

  const handleActionSent = useCallback(() => {
    refreshState().catch((error) => {
      console.error(error);
      setConnectionStatus("offline");
    });
  }, [refreshState]);

  const setMark = useCallback((playerId: string, mark: PlayerMark) => {
    setPlayerMarks((prev) => ({ ...prev, [playerId]: mark }));
  }, []);

  if (authStatus === "loading") return <LoadingScreen text="Discord 연결 중" />;
  if (authStatus === "error") return <ErrorScreen msg={errorMsg} />;
  if (!gameState) return <LoadingScreen text="게임 정보 로딩 중" />;
  if (!gameState.in_game) {
    return (
      <div className="activity-shell is-empty">
        <section className="empty-state">
          <div className="empty-mark">M</div>
          <h1>마피아 게임</h1>
          <p>진행 중인 게임 없음</p>
        </section>
      </div>
    );
  }

  const phase = PHASE_META[gameState.phase];
  const aliveCount = gameState.players.filter((p) => p.alive).length;
  const deadCount = gameState.players.length - aliveCount;
  const me = gameState.players.find((p) => p.is_you);

  return (
    <div className={`activity-shell phase-${phase.tone}`}>
      <TopBar
        state={gameState}
        connectionStatus={connectionStatus}
        aliveCount={aliveCount}
        deadCount={deadCount}
        onRefresh={handleActionSent}
      />

      <main className="activity-layout">
        <section className="primary-column">
          <RoleFocus player={me} state={gameState} />
          <ActionConsole
            state={gameState}
            selectedTarget={selectedTarget}
            onSelectTarget={setSelectedTarget}
            onActionSent={handleActionSent}
          />
          <VoteIntel state={gameState} />
          <PublicStatus text={gameState.public_status} />
        </section>

        <section className="secondary-column">
          <PlayerDesk
            state={gameState}
            selectedTarget={selectedTarget}
            filter={playerFilter}
            marks={playerMarks}
            onFilter={setPlayerFilter}
            onSelectTarget={setSelectedTarget}
            onMark={setMark}
          />
          <NotesPanel notes={notes} onNotes={setNotes} marks={playerMarks} />
        </section>
      </main>
    </div>
  );
}

function TopBar({
  state,
  connectionStatus,
  aliveCount,
  deadCount,
  onRefresh,
}: {
  state: GameState;
  connectionStatus: ConnectionStatus;
  aliveCount: number;
  deadCount: number;
  onRefresh: () => void;
}) {
  const phase = PHASE_META[state.phase];
  const remaining = useRemainingSeconds(state.phase_ends_at);
  const actionText = state.can_act || state.contractor_can_act ? "행동 가능" : "대기";

  return (
    <header className="top-bar">
      <div className="phase-block">
        <div className="phase-label">{phase.label}</div>
        <div className="phase-sub">
          {state.day_number}일차 · {phase.summary}
        </div>
      </div>

      <div className="timer-block">
        <span className={remaining !== null && remaining <= 10 ? "timer danger" : "timer"}>
          {remaining === null ? "--:--" : formatClock(remaining)}
        </span>
      </div>

      <div className="top-stats">
        <Stat label="생존" value={aliveCount} />
        <Stat label="사망" value={deadCount} />
        <Stat label="상태" value={actionText} />
      </div>

      <button className="icon-command" onClick={onRefresh} title="상태 새로고침" type="button">
        ↻
      </button>

      <div className={`connection-dot ${connectionStatus}`} title={`연결: ${connectionStatus}`} />
    </header>
  );
}

function RoleFocus({ player, state }: { player?: PlayerDto; state: GameState }) {
  const team = state.my_team;
  const teamMeta = team ? TEAM_META[team] : null;
  const role = state.my_role ?? player?.role ?? "관전자";
  const guide = ROLE_HELP[role] ?? "공개 정보, 투표 흐름, 발언 모순을 같이 보세요.";
  const result = state.my_action_result;

  return (
    <section className={`panel role-focus ${teamMeta?.className ?? "team-unknown"}`}>
      <div className="section-kicker">내 정보</div>
      <div className="role-main">
        <div>
          <h1>{role}</h1>
          <p>{teamMeta?.label ?? "관전자"}</p>
        </div>
        <div className="role-badge">{player?.alive === false ? "사망" : "생존"}</div>
      </div>
      <div className="role-guide">{guide}</div>
      {state.my_night_target && (
        <div className="mini-alert">
          밤 대상: {state.players.find((p) => p.id === state.my_night_target)?.name ?? "알 수 없음"}
        </div>
      )}
      {result && <div className="result-alert">{result}</div>}
    </section>
  );
}

function ActionConsole({
  state,
  selectedTarget,
  onSelectTarget,
  onActionSent,
}: {
  state: GameState;
  selectedTarget: string | null;
  onSelectTarget: (id: string | null) => void;
  onActionSent: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [contractTargets, setContractTargets] = useState<[string, string]>(["", ""]);
  const [contractRoles, setContractRoles] = useState<[string, string]>(["", ""]);
  const me = state.players.find((p) => p.is_you);
  const aliveTargets = state.players.filter((p) => p.alive && !p.is_you);
  const selectedPlayer = state.players.find((p) => p.id === selectedTarget);

  async function run(req: Omit<ActionRequest, "guild_id">, successText: string) {
    setBusy(true);
    setMessage("");
    try {
      const res = await sendAction(req);
      setMessage(res.ok ? successText : res.message ?? "요청 실패");
      if (res.ok) onActionSent();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "요청 실패");
    } finally {
      setBusy(false);
    }
  }

  function setContractTarget(slot: 0 | 1, value: string) {
    setContractTargets((prev) => (slot === 0 ? [value, prev[1]] : [prev[0], value]));
  }

  function setContractRole(slot: 0 | 1, value: string) {
    setContractRoles((prev) => (slot === 0 ? [value, prev[1]] : [prev[0], value]));
  }

  const canNightAction = state.phase === "Night" && me?.alive && state.can_act;
  const canSkip = state.phase === "Day" && me?.alive;
  const canVote = (state.phase === "Vote" || state.phase === "FinalDefense") && me?.alive;
  const canConfirm = state.phase === "ConfirmVote" && me?.alive;
  const nominee = state.players.find((p) => p.id === state.nominee);

  return (
    <section className="panel action-console">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">행동 콘솔</div>
          <h2>{actionHeadline(state)}</h2>
        </div>
        {selectedPlayer && <span className="selected-chip">{selectedPlayer.name}</span>}
      </div>

      {canNightAction && (
        <div className="action-group">
          <TargetGrid
            players={aliveTargets}
            selectedTarget={selectedTarget}
            voteTargets={state.vote_targets}
            onSelect={onSelectTarget}
          />
          <div className="command-row">
            <button
              className="primary-command"
              disabled={busy || !selectedTarget}
              onClick={() =>
                run({ action: "night_action", target_id: selectedTarget ?? undefined }, "밤 행동 제출 완료")
              }
              type="button"
            >
              대상 제출
            </button>
            <button
              className="secondary-command"
              disabled={busy}
              onClick={() => run({ action: "night_action" }, "밤 행동 스킵 완료")}
              type="button"
            >
              스킵
            </button>
          </div>
        </div>
      )}

      {state.contractor_can_act && (
        <div className="contractor-grid">
          {[0, 1].map((slot) => (
            <div className="contract-slot" key={slot}>
              <span>청부 {slot + 1}</span>
              <select
                value={contractTargets[slot as 0 | 1]}
                onChange={(e) => setContractTarget(slot as 0 | 1, e.target.value)}
              >
                <option value="">대상</option>
                {state.contractor_targets
                  .filter((target) => target.id !== contractTargets[slot === 0 ? 1 : 0])
                  .map((target) => (
                    <option key={target.id} value={target.id}>
                      {target.name}
                    </option>
                  ))}
              </select>
              <select
                value={contractRoles[slot as 0 | 1]}
                onChange={(e) => setContractRole(slot as 0 | 1, e.target.value)}
              >
                <option value="">직업</option>
                {state.contractor_guess_roles.map((role) => (
                  <option key={role} value={role}>
                    {role}
                  </option>
                ))}
              </select>
            </div>
          ))}
          <button
            className="primary-command wide"
            disabled={
              busy ||
              !contractTargets[0] ||
              !contractTargets[1] ||
              !contractRoles[0] ||
              !contractRoles[1] ||
              contractTargets[0] === contractTargets[1]
            }
            onClick={() =>
              run(
                {
                  action: "contractor_action",
                  contract_target_ids: contractTargets,
                  contract_roles: contractRoles,
                },
                "청부 제출 완료",
              )
            }
            type="button"
          >
            청부 제출
          </button>
        </div>
      )}

      {canSkip && (
        <div className="skip-strip">
          <ProgressBar value={state.day_skip_count} max={state.day_skip_threshold} />
          <button
            className="secondary-command"
            disabled={busy}
            onClick={() => run({ action: "skip_vote" }, "스킵 투표 완료")}
            type="button"
          >
            낮 스킵
          </button>
        </div>
      )}

      {canVote && (
        <div className="action-group">
          <TargetGrid
            players={aliveTargets}
            selectedTarget={selectedTarget}
            voteTargets={state.vote_targets}
            onSelect={onSelectTarget}
          />
          <div className="command-row">
            <button
              className="danger-command"
              disabled={busy || !selectedTarget}
              onClick={() =>
                run({ action: "day_vote", target_id: selectedTarget ?? undefined }, "투표 완료")
              }
              type="button"
            >
              지목
            </button>
            <button
              className="secondary-command"
              disabled={busy}
              onClick={() => run({ action: "day_vote" }, "기권 완료")}
              type="button"
            >
              기권
            </button>
          </div>
        </div>
      )}

      {canConfirm && (
        <div className="confirm-box">
          <div>
            <span className="confirm-target">{nominee?.name ?? "대상 없음"}</span>
            <small>
              찬성 {state.confirm_yes} · 반대 {state.confirm_no}
            </small>
          </div>
          <div className="command-row compact">
            <button
              className="primary-command"
              disabled={busy}
              onClick={() => run({ action: "confirm_vote", confirm: true }, "찬성 완료")}
              type="button"
            >
              찬성
            </button>
            <button
              className="danger-command"
              disabled={busy}
              onClick={() => run({ action: "confirm_vote", confirm: false }, "반대 완료")}
              type="button"
            >
              반대
            </button>
          </div>
        </div>
      )}

      {!canNightAction && !state.contractor_can_act && !canSkip && !canVote && !canConfirm && (
        <div className="idle-box">{state.phase === "Ended" ? "게임 종료" : "제출할 행동 없음"}</div>
      )}

      {message && <div className={message.includes("완료") ? "toast ok" : "toast error"}>{message}</div>}
    </section>
  );
}

function TargetGrid({
  players,
  selectedTarget,
  voteTargets,
  onSelect,
}: {
  players: PlayerDto[];
  selectedTarget: string | null;
  voteTargets: Record<string, number>;
  onSelect: (id: string | null) => void;
}) {
  return (
    <div className="target-grid">
      {players.map((player) => (
        <button
          key={player.id}
          className={selectedTarget === player.id ? "target-button selected" : "target-button"}
          onClick={() => onSelect(selectedTarget === player.id ? null : player.id)}
          type="button"
        >
          <span>{player.name}</span>
          {player.role && <small>{player.role}</small>}
          {(voteTargets[player.id] ?? 0) > 0 && <b>{voteTargets[player.id]}</b>}
        </button>
      ))}
    </div>
  );
}

function PlayerDesk({
  state,
  selectedTarget,
  filter,
  marks,
  onFilter,
  onSelectTarget,
  onMark,
}: {
  state: GameState;
  selectedTarget: string | null;
  filter: PlayerFilter;
  marks: Record<string, PlayerMark>;
  onFilter: (filter: PlayerFilter) => void;
  onSelectTarget: (id: string | null) => void;
  onMark: (id: string, mark: PlayerMark) => void;
}) {
  const [query, setQuery] = useState("");
  const players = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return state.players
      .filter((player) => {
        if (filter === "alive" && !player.alive) return false;
        if (filter === "dead" && player.alive) return false;
        if (filter === "marked" && (!marks[player.id] || marks[player.id] === "none")) return false;
        if (!normalized) return true;
        return `${player.name} ${player.role ?? ""}`.toLowerCase().includes(normalized);
      })
      .sort((a, b) => Number(b.is_you) - Number(a.is_you) || Number(b.alive) - Number(a.alive));
  }, [filter, marks, query, state.players]);

  return (
    <section className="panel player-desk">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">플레이어 보드</div>
          <h2>{state.players.length}명</h2>
        </div>
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="검색" />
      </div>
      <div className="segmented">
        {(["alive", "all", "dead", "marked"] as PlayerFilter[]).map((item) => (
          <button
            key={item}
            className={filter === item ? "active" : ""}
            onClick={() => onFilter(item)}
            type="button"
          >
            {filterLabel(item)}
          </button>
        ))}
      </div>
      <div className="player-list">
        {players.map((player) => (
          <PlayerRow
            key={player.id}
            player={player}
            votes={state.vote_targets[player.id] ?? 0}
            selected={selectedTarget === player.id}
            mark={marks[player.id] ?? "none"}
            onSelect={() => onSelectTarget(player.alive && !player.is_you ? player.id : null)}
            onMark={(mark) => onMark(player.id, mark)}
          />
        ))}
      </div>
    </section>
  );
}

function PlayerRow({
  player,
  votes,
  selected,
  mark,
  onSelect,
  onMark,
}: {
  player: PlayerDto;
  votes: number;
  selected: boolean;
  mark: PlayerMark;
  onSelect: () => void;
  onMark: (mark: PlayerMark) => void;
}) {
  const team = player.role_team ? TEAM_META[player.role_team] : null;
  const markMeta = MARK_META[mark];

  return (
    <div className={`player-row ${selected ? "selected" : ""} ${player.alive ? "" : "dead"}`}>
      <button className="player-main" onClick={onSelect} type="button">
        <span className={`status-pin ${team?.className ?? "team-unknown"}`} />
        <span className="player-name">
          {player.name}
          {player.is_you && <small>나</small>}
        </span>
        {player.role && <span className="role-tag">{player.role}</span>}
        {votes > 0 && <span className="vote-chip">{votes}표</span>}
      </button>
      <div className="mark-controls">
        {(["trust", "suspect", "watch"] as PlayerMark[]).map((item) => (
          <button
            key={item}
            className={mark === item ? MARK_META[item].className : ""}
            onClick={() => onMark(mark === item ? "none" : item)}
            title={MARK_META[item].label}
            type="button"
          >
            {MARK_META[item].short}
          </button>
        ))}
      </div>
      {mark !== "none" && <span className={`mark-badge ${markMeta.className}`}>{markMeta.label}</span>}
    </div>
  );
}

function VoteIntel({ state }: { state: GameState }) {
  const entries = Object.entries(state.vote_targets)
    .map(([id, votes]) => ({ player: state.players.find((p) => p.id === id), votes }))
    .filter((entry): entry is { player: PlayerDto; votes: number } => Boolean(entry.player))
    .sort((a, b) => b.votes - a.votes);
  const maxVotes = Math.max(1, ...entries.map((entry) => entry.votes));

  if (state.phase === "ConfirmVote") {
    const total = Math.max(1, state.confirm_yes + state.confirm_no);
    return (
      <section className="panel vote-intel">
        <div className="section-kicker">찬반 현황</div>
        <ProgressBar value={state.confirm_yes} max={total} label={`찬성 ${state.confirm_yes}`} />
        <ProgressBar value={state.confirm_no} max={total} label={`반대 ${state.confirm_no}`} danger />
      </section>
    );
  }

  return (
    <section className="panel vote-intel">
      <div className="section-kicker">투표 흐름</div>
      {entries.length === 0 ? (
        <div className="muted-line">표 없음</div>
      ) : (
        entries.slice(0, 5).map(({ player, votes }) => (
          <ProgressBar key={player.id} value={votes} max={maxVotes} label={`${player.name} ${votes}표`} danger />
        ))
      )}
    </section>
  );
}

function PublicStatus({ text }: { text: string }) {
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

  return (
    <section className="panel public-status">
      <div className="section-kicker">공개 상태</div>
      {lines.length === 0 ? (
        <div className="muted-line">공개 정보 없음</div>
      ) : (
        <div className="status-lines">
          {lines.map((line, index) => (
            <p key={`${line}-${index}`}>{line}</p>
          ))}
        </div>
      )}
    </section>
  );
}

function NotesPanel({
  notes,
  marks,
  onNotes,
}: {
  notes: string;
  marks: Record<string, PlayerMark>;
  onNotes: (value: string) => void;
}) {
  const markedCount = Object.values(marks).filter((mark) => mark !== "none").length;
  return (
    <section className="panel notes-panel">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">판 메모</div>
          <h2>표시 {markedCount}</h2>
        </div>
        <button className="icon-command" onClick={() => onNotes("")} title="메모 지우기" type="button">
          ×
        </button>
      </div>
      <textarea value={notes} onChange={(e) => onNotes(e.target.value)} placeholder="메모" />
    </section>
  );
}

function ProgressBar({
  value,
  max,
  label,
  danger,
}: {
  value: number;
  max: number;
  label?: string;
  danger?: boolean;
}) {
  const width = `${Math.min(100, Math.round((value / Math.max(1, max)) * 100))}%`;
  return (
    <div className="progress-line">
      {label && <span>{label}</span>}
      <div className="progress-track">
        <div className={danger ? "progress-fill danger" : "progress-fill"} style={{ width }} />
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="stat">
      <span>{label}</span>
      <b>{value}</b>
    </div>
  );
}

function LoadingScreen({ text }: { text: string }) {
  return (
    <div className="activity-shell is-empty">
      <section className="empty-state">
        <div className="loading-ring" />
        <h1>{text}</h1>
      </section>
    </div>
  );
}

function ErrorScreen({ msg }: { msg: string }) {
  return (
    <div className="activity-shell is-empty">
      <section className="empty-state error">
        <div className="empty-mark">!</div>
        <h1>인증 실패</h1>
        <p>{msg}</p>
      </section>
    </div>
  );
}

function useRemainingSeconds(phaseEndsAt: number | null) {
  const [remaining, setRemaining] = useState<number | null>(null);

  useEffect(() => {
    if (!phaseEndsAt) {
      setRemaining(null);
      return;
    }
    const update = () => setRemaining(Math.max(0, Math.ceil((phaseEndsAt - Date.now()) / 1000)));
    update();
    const id = window.setInterval(update, 500);
    return () => window.clearInterval(id);
  }, [phaseEndsAt]);

  return remaining;
}

function actionHeadline(state: GameState) {
  if (state.contractor_can_act) return "청부 제출";
  if (state.phase === "Night" && state.can_act) return "밤 행동";
  if (state.phase === "Day") return "낮 진행";
  if (state.phase === "Vote") return "투표";
  if (state.phase === "FinalDefense") return "최후변론";
  if (state.phase === "ConfirmVote") return "처형 찬반";
  return "대기";
}

function filterLabel(filter: PlayerFilter) {
  const labels: Record<PlayerFilter, string> = {
    all: "전체",
    alive: "생존",
    dead: "사망",
    marked: "표시",
  };
  return labels[filter];
}

function formatClock(seconds: number) {
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return value ? (JSON.parse(value) as T) : fallback;
  } catch {
    return fallback;
  }
}
