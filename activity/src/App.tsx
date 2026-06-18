import { useEffect, useState, useCallback } from "react";
import { authenticateWithDiscord } from "./discord";
import { setSession, createWebSocket, fetchState } from "./api";
import type { GameState } from "./types";
import { PlayerList } from "./components/PlayerList";
import { PhaseTimer } from "./components/PhaseTimer";
import { RoleCard } from "./components/RoleCard";
import { VotePanel } from "./components/VotePanel";
import { ActionPanel } from "./components/ActionPanel";

type AuthStatus = "loading" | "auth" | "ready" | "error";

export default function App() {
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [errorMsg, setErrorMsg] = useState("");
  const [guildId, setGuildId] = useState("");
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [ws, setWs] = useState<WebSocket | null>(null);

  // Discord 인증
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

  // WebSocket 연결
  useEffect(() => {
    if (authStatus !== "ready") return;

    // 초기 상태 즉시 로드
    fetchState().then(setGameState).catch(console.error);

    // WebSocket으로 실시간 업데이트
    const socket = createWebSocket(setGameState);
    setWs(socket);

    return () => {
      socket.close();
    };
  }, [authStatus]);

  const handleActionSent = useCallback(() => {
    fetchState().then(setGameState).catch(console.error);
  }, []);

  if (authStatus === "loading") return <Loading text="Discord 연결 중..." />;
  if (authStatus === "error") return <ErrorScreen msg={errorMsg} />;
  if (!gameState) return <Loading text="게임 정보 불러오는 중..." />;

  if (!gameState.in_game) {
    return (
      <div style={styles.container}>
        <div style={styles.centered}>
          <div style={{ fontSize: 32 }}>🎭</div>
          <div style={{ fontSize: 18, fontWeight: 700, marginTop: 10 }}>마피아 게임</div>
          <div style={{ fontSize: 13, color: "#666", marginTop: 6 }}>
            현재 진행 중인 게임이 없습니다.
          </div>
          <div style={{ fontSize: 12, color: "#555", marginTop: 4 }}>
            Discord에서 /마피아시작 으로 시작하세요.
          </div>
        </div>
      </div>
    );
  }

  const showVotePanel = ["Vote", "FinalDefense", "ConfirmVote"].includes(gameState.phase);
  // ActionPanel은 낮/밤 행동 + 결과 배너 + 스킵 버튼을 모두 담당
  const showActionPanel = ["Night", "Day", "Vote", "FinalDefense", "ConfirmVote"].includes(gameState.phase);
  const isEnded = gameState.phase === "Ended";

  return (
    <div style={styles.container}>
      {/* 헤더 */}
      <PhaseTimer
        phase={gameState.phase}
        dayNumber={gameState.day_number}
        phaseEndsAt={gameState.phase_ends_at}
      />

      {/* 게임 종료 배너 */}
      {isEnded && gameState.winner && (
        <div style={{
          textAlign: "center",
          padding: "10px 14px",
          borderRadius: 10,
          background: "rgba(255,215,0,0.12)",
          border: "1px solid rgba(255,215,0,0.3)",
          fontSize: 16,
          fontWeight: 700,
          color: "#fdd835",
        }}>
          🏆 {winnerLabel(gameState.winner)} 승리!
        </div>
      )}

      {/* 내 역할 카드 */}
      <RoleCard role={gameState.my_role} team={gameState.my_team} />

      {/* 밤 행동 패널 */}
      {showActionPanel && (
        <ActionPanel state={gameState} onActionSent={handleActionSent} />
      )}

      {/* 투표 패널 */}
      {showVotePanel && (
        <VotePanel state={gameState} onActionSent={handleActionSent} />
      )}

      {/* 플레이어 목록 */}
      <div style={{ flex: 1, overflow: "auto" }}>
        <div style={{ fontSize: 12, color: "#888", marginBottom: 6, fontWeight: 600 }}>
          플레이어 현황
        </div>
        <PlayerList
          players={gameState.players}
          highlightVotes={gameState.phase === "Vote" ? gameState.vote_targets : undefined}
        />
      </div>

      {/* 공개 상태 텍스트 */}
      <div style={{
        fontSize: 11,
        color: "#555",
        borderTop: "1px solid rgba(255,255,255,0.06)",
        paddingTop: 8,
        lineHeight: 1.6,
        whiteSpace: "pre-line",
      }}>
        {gameState.public_status}
      </div>
    </div>
  );
}

function Loading({ text }: { text: string }) {
  return (
    <div style={{ ...styles.container, ...styles.centered }}>
      <div style={{ fontSize: 28, marginBottom: 10 }}>⏳</div>
      <div style={{ fontSize: 14, color: "#888" }}>{text}</div>
    </div>
  );
}

function ErrorScreen({ msg }: { msg: string }) {
  return (
    <div style={{ ...styles.container, ...styles.centered }}>
      <div style={{ fontSize: 28, marginBottom: 10 }}>⚠️</div>
      <div style={{ fontSize: 14, color: "#f44336" }}>인증 실패</div>
      <div style={{ fontSize: 11, color: "#666", marginTop: 6, maxWidth: 250, textAlign: "center" }}>{msg}</div>
    </div>
  );
}

function winnerLabel(winner: string): string {
  const map: Record<string, string> = {
    Mafia: "마피아팀",
    Citizen: "시민팀",
    Cult: "교주팀",
    Joker: "조커",
    Prophet: "예언자",
  };
  return map[winner] ?? winner;
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    height: "100vh",
    display: "flex",
    flexDirection: "column",
    gap: 10,
    padding: "12px 14px",
    overflow: "hidden",  },
  centered: {
    alignItems: "center",
    justifyContent: "center",
    textAlign: "center",
  },
};

