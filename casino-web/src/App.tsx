// noir-casino app/page.tsx 의 이식본. 화면 구조·클래스·문구는 원본을 그대로 따르고,
// 데이터 소스만 봇의 /casino/api (개인 링크 세션, 여러 테이블, Discord 코인)로 바꿨다.
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight,
  AudioLines,
  Bell,
  BellOff,
  Check,
  CircleHelp,
  Club,
  Copy,
  Diamond,
  Grid2X2,
  History,
  LogOut,
  Maximize,
  Minimize,
  Plus,
  RefreshCw,
  Send,
  ShieldCheck,
  Spade,
  Users,
  Video,
  VideoOff,
  Volume2,
  VolumeX,
  Wallet,
  WifiOff,
} from "lucide-react";
import { CasinoApiError, fetchState, readLink, sendCommand, wsUrl } from "./api";
import { confirmServerState, planServerClock, sampleServerTime, touchServerClock, useServerValue } from "./clock";
import { installAudioUnlock, setSoundEnabled, sfx, soundEnabled, unlockAudio } from "./sounds";
import { ChatMessages, TableChatPreview } from "./components/TableChat";
import { DealerBackdrop, hasDealerVideo } from "./components/DealerBackdrop";
import { BoardCards, BurnPile, Countdown, DockCards, PlayingCard, SeatCards, TurnRing } from "./components/TableCards";
import { cardTiming, dealClipAt, dealClipKey, dealerMoodAt, parseDealClipKey, flipTimes, revealStage, secondsLeft, SHUFFLE_MS, tableEvents, type CardTiming } from "./schedule";
import { deviceNow, isStaleSnapshot, rawStateKey, readServerTime, shareStable, snapshotKey } from "./state-sync";
import { ALL_DENOMS, blackjackValue, chipLabel, fmt, potRaiseTo, signed, tableDenoms } from "./table-helpers";
import type { CasinoCommand, GameKind, SeatResult, SeatView, StateResponse, TableRules, TableView } from "./types";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  Slider,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Toaster,
  toast,
} from "./ui";

const resultTone = (r: SeatResult) => r.won > 0 ? "won" : r.net < 0 ? "lost" : "even";
// 에볼루션처럼 이긴 베팅이 돌려준 총액을 보여 준다 (블랙잭 5,000 → 12,500). 예전 기록은 이익만 있다.
const resultHeadline = (r: SeatResult, mine: boolean) => r.won > 0
  ? `${mine ? "이기셨습니다" : `${r.name} 이김`} ${r.paid ? fmt(r.paid) : signed(r.won)}`
  : `${mine ? "" : `${r.name} `}${r.net < 0 ? `패배 ${signed(r.net)}` : "푸시"}`;
const phases: Record<string, string> = {
  preflop: "프리플롭",
  flop: "플롭",
  turn: "턴",
  river: "리버",
  betting: "베팅 접수",
  playing: "플레이 중",
  complete: "라운드 종료",
};
// 카드 연출 중 중립 안내: 단계 이름이나 결과를 드러내지 않는다.
const REVEAL_TEXT = { dealing: "카드를 나누는 중…", showdown: "카드를 확인하는 중…" } as const;
const REVEAL_EYEBROW = { dealing: "DEALING", showdown: "SHOWDOWN" } as const;
// 기록 시각: 부를 때마다 포매터를 만들면 기록이 많을 때 렌더가 수백 ms 걸린다.
const historyTime = new Intl.DateTimeFormat("ko-KR", { hour: "2-digit", minute: "2-digit" });
const DEALER_IMAGE = `${import.meta.env.BASE_URL}dealer.png`;
const dealerPortrait = (id: string) => (id === "sophia" ? DEALER_IMAGE : `${import.meta.env.BASE_URL}dealers/${id}.png`);
const POLL_MS = 1200;
/** 서버는 바뀐 것이 없어도 1초마다 신호를 보낸다. 이만큼 조용하면 열려 있어도 끊긴 소켓으로 본다. */
const SOCKET_SILENCE_MS = 4000;
const RECONNECT_BASE_MS = 1000;
const RECONNECT_MAX_MS = 10000;
const NO_CARDS: readonly string[] = [];

/** 카드가 출발하는 딜러 사진 속 슈 입구 (사진 가로·세로 비율). */
const SHOE_IN_PHOTO: [number, number] = [0.8, 0.59];

const defaultRules: TableRules = {
  small_blind: 50,
  big_blind: 100,
  min_buy_in: 5000,
  max_buy_in: 20000,
  buy_in_step: 100,
  min_bet: 100,
  max_bet: 5000,
  bet_step: 100,
  side_bet_min: 100,
  side_bet_max: 2500,
  insurance_ms: 10000,
  seat_count: 6,
  turn_ms: 30000,
  bet_window_ms: 15000,
};

const BLANK_TABLE: TableView = {
  id: "",
  kind: "holdem",
  name: "테이블 없음",
  version: 0,
  button: -1,
  my_seat: -1,
  narration: "열려 있는 테이블이 없어요. 관리자가 Discord 카지노 패널에서 열 수 있어요.",
  seats: Array(6).fill(null),
  round: null,
  legal: { poker: null, blackjack: null, can_bet: false, can_start: false, can_insure: false, insurance_cost: 0 },
  messages: [],
  history: [],
  rules: defaultRules,
  dealer: { id: "sophia", name: "소피아", tagline: "YOUR DEALER" },
  shoe: null,
};

const kindTitle = (kind: GameKind) => (kind === "holdem" ? "Texas Hold’em" : "Blackjack");
const kindRoom = (kind: GameKind) => (kind === "holdem" ? "THE SIGNATURE ROOM" : "THE BLACKJACK SALON");
const kindStakes = (kind: GameKind, rules: TableRules) =>
  kind === "holdem" ? `${fmt(rules.small_blind)} / ${fmt(rules.big_blind)}` : `${fmt(rules.min_bet)} – ${fmt(rules.max_bet)}`;
const floorStep = (value: number, step: number) => Math.floor(value / step) * step;
type BetSpot = "main" | "pairs" | "plus3";
interface Placement {
  spot: BetSpot;
  value: number;
}
const SPOT_LABEL: Record<BetSpot, string> = { main: "메인", pairs: "퍼펙트 페어", plus3: "21+3" };
/** 금액을 큰 칩부터 쌓은 모양으로 나눈다 (표시용). */
function chipsFor(amount: number): number[] {
  const out: number[] = [];
  let rest = amount;
  for (const value of [...ALL_DENOMS].reverse()) {
    while (rest >= value && out.length < 14) {
      out.push(value);
      rest -= value;
    }
  }
  return out;
}
/** 좌석 옆에 쌓인 칩 더미. onClick이 있으면 베팅 자리(칩 놓기)로 동작한다. */
function ChipStack({
  chips,
  total,
  onClick,
  active = false,
  hint,
  spot,
  over = false,
}: {
  chips: number[];
  total: number;
  onClick?: () => void;
  active?: boolean;
  hint?: string;
  /** 칩을 끌어다 놓을 수 있는 베팅 자리. */
  spot?: BetSpot;
  over?: boolean;
}) {
  const shown = chips.slice(-10);
  return (
    <button
      type="button"
      className={`chip-stack ${active ? "active" : ""} ${chips.length === 0 ? "empty-spot" : ""} ${over ? "drop-over" : ""}`}
      onClick={onClick}
      disabled={!onClick}
      data-bet-spot={spot}
      aria-label={total > 0 ? `베팅 ${fmt(total)}` : "베팅 자리"}
      title={hint}
    >
      <span className="chip-pile">
        {shown.map((value, index) => (
          <i key={`${index}-${value}`} className={`chip-coin chip-${value}`} style={{ bottom: index * 3 }}>
            {index === shown.length - 1 ? chipLabel(value) : ""}
          </i>
        ))}
      </span>
      <b>{total > 0 ? fmt(total) : hint ?? "BET"}</b>
    </button>
  );
}
/** 베팅 독의 베팅 원: 쌓인 칩과 금액. 칩을 끌어다 놓거나, 칩을 고른 뒤 누르면 올라간다. */
function BetCircle({
  spot,
  label,
  chips,
  total,
  limit,
  active,
  over,
  disabled,
  onClick,
}: {
  spot: BetSpot;
  label: string;
  chips: number[];
  total: number;
  limit: number;
  active: boolean;
  over: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  const shown = chips.slice(-9);
  return (
    <button
      type="button"
      data-bet-spot={spot}
      className={`bet-circle ${spot} ${active ? "active" : ""} ${over ? "drop-over" : ""} ${total > 0 ? "filled" : ""}`}
      onClick={onClick}
      disabled={disabled}
      aria-label={`${label} 베팅 ${fmt(total)}`}
    >
      <span className="bet-circle-label">{label}</span>
      <span className="chip-pile">
        {shown.map((value, index) => (
          <i key={`${index}-${value}`} className={`chip-coin chip-${value}`} style={{ bottom: index * 3 }}>
            {index === shown.length - 1 ? chipLabel(value) : ""}
          </i>
        ))}
      </span>
      <b>{total > 0 ? fmt(total) : "BET"}</b>
      <small>최대 {fmt(limit)}</small>
    </button>
  );
}
/** 좌석의 메인 베팅 합 (스플릿·더블 포함). */
const mainBet = (seat: SeatView) => seat.hands.reduce((sum, hand) => sum + hand.bet, 0) || seat.bet;
/** 화면 자리별 테이블 위 위치 (noir.css의 .seat-N과 같은 값, % 단위). */
const SEAT_POS: Array<[number, number]> = [
  [12, 63],
  [22, 82],
  [40, 89],
  [60, 89],
  [78, 82],
  [88, 63],
];
/**
 * 좌석 번호 → 화면 자리. 실제 카지노처럼 1번 좌석(첫 번째로 카드를 받는 퍼스트 베이스)이 슈 옆,
 * 딜러의 왼쪽(화면 오른쪽)에 앉는다. 번호가 오를수록 화면 오른쪽에서 왼쪽으로(시계 방향) 돈다.
 */
const seatSlot = (index: number, count = SEAT_POS.length) => count - 1 - index;
const seatPos = (index: number, count?: number) => SEAT_POS[seatSlot(index, count)];
const POT_POS: [number, number] = [50, 50];
interface Flight {
  id: number;
  from: [number, number];
  to: [number, number];
  amount: number;
}
/**
 * 칩 더미가 테이블 위를 날아가는 연출 (베팅 → 팟, 팟 → 승자).
 * 테이블 크기의 틀을 transform으로만 옮긴다: 레이아웃·그림자를 매 프레임 다시 그리지 않는다.
 */
function ChipFlight({ flight, onDone }: { flight: Flight; onDone: (id: number) => void }) {
  const [go, setGo] = useState(false);
  useEffect(() => {
    const raf = requestAnimationFrame(() => requestAnimationFrame(() => setGo(true)));
    const timer = window.setTimeout(() => onDone(flight.id), 900);
    return () => {
      cancelAnimationFrame(raf);
      window.clearTimeout(timer);
    };
    // 한 번 날리면 끝이다.
  }, [flight.id]);
  const chips = chipsFor(flight.amount).slice(-6);
  return (
    <div
      className={`chip-flight-track ${go ? "go" : ""}`}
      style={{ "--dx": flight.to[0] - flight.from[0], "--dy": flight.to[1] - flight.from[1] } as React.CSSProperties}
      aria-hidden="true"
    >
      <div className="chip-flight" style={{ left: `${flight.from[0]}%`, top: `${flight.from[1]}%` }}>
        <span className="chip-pile">
          {chips.map((value, index) => (
            <i key={index} className={`chip-coin chip-${value}`} style={{ bottom: index * 3 }} />
          ))}
        </span>
        <b>{fmt(flight.amount)}</b>
      </div>
    </div>
  );
}

/** 딜러 영상: 카드 일정에서 동작(딜·뒤집기·대기)과 슈를 떠난 카드 수를 읽는다. 이 칸만 시계를 구독한다. */
const DealerStage = memo(function DealerStage({
  table,
  timing,
  motion,
  poster,
}: {
  table: TableView;
  timing: CardTiming;
  motion: boolean;
  poster: string;
}) {
  const mood = useServerValue((now) => dealerMoodAt(table.round, table.seats, timing, now));
  // 클립은 문자열 키로 구독한다 (객체는 매번 새로 만들어져 구독할 수 없다). 키가 바뀔 때만 다시 만든다.
  const dealKey = useServerValue((now) => dealClipKey(dealClipAt(table.round, table.seats, timing, now)));
  const deal = useMemo(() => parseDealClipKey(dealKey), [dealKey]);
  // 쇼다운에서 뒤집을 카드가 남아 있으면 뒤집는 동작이 끝나도 다시 한다.
  const flipsAhead = useServerValue((now) => flipTimes(table.round, table.seats).some((time) => time > now));
  return (
    <DealerBackdrop
      id={table.dealer.id}
      name={table.dealer.name}
      mood={mood}
      deal={deal}
      flipsAhead={flipsAhead}
      poster={poster}
      motionEnabled={motion}
    />
  );
});

/** 버전이 바뀐 사이에도 같은 차례가 이어지는지 (콜·레이즈를 조용히 한 번 더 보낼 수 있는지). */
const sameTurn = (before: TableView, after: TableView) =>
  !!before.round &&
  !!after.round &&
  before.round.id === after.round.id &&
  before.round.phase === after.round.phase &&
  before.round.turn === after.round.turn &&
  before.round.hand === after.round.hand &&
  after.my_seat >= 0 &&
  after.round.turn === after.my_seat;

interface Received {
  raw: string;
  receivedAt: number;
}

/** 라운드 동안 족보 이름과 족보 카드를 붙잡아 둔다: 카드를 여는 동안 서버가 잠깐 비워 보내도 깜빡이지 않는다. */
function useSteadyHands(table: TableView) {
  const memo = useRef<{ round: string | null; seats: Map<number, { name: string; cards: string[] }> }>({ round: null, seats: new Map() });
  return useMemo(() => {
    const kept = memo.current;
    const round = table.round?.id ?? null;
    if (kept.round !== round || !round) kept.seats = new Map();
    kept.round = round;
    const out = new Map<number, { name: string | null; cards: readonly string[] }>();
    table.seats.forEach((seat, index) => {
      if (!seat) {
        kept.seats.delete(index);
        return;
      }
      if (seat.hand_name) kept.seats.set(index, { name: seat.hand_name, cards: seat.hand_cards });
      else if (!seat.cards.length) kept.seats.delete(index);
      const steady = kept.seats.get(index);
      out.set(index, steady ? steady : { name: null, cards: NO_CARDS });
    });
    return out;
  }, [table]);
}

export default function Casino() {
  const appRef = useRef<HTMLDivElement>(null);
  const [fullscreen, setFullscreen] = useState<"native" | "expanded" | null>(null);
  // 연출: 카드·칩 애니메이션과 딜러 영상. 기기의 '동작 줄이기'와 상관없이 켜 두고, 버튼으로 끌 수 있다.
  const [motion, setMotion] = useState(() => {
    try {
      const saved = localStorage.getItem("casino-motion");
      if (saved !== null) return saved === "true";
    } catch { /* Storage can be unavailable in embedded browsers. */ }
    return true;
  });
  const [{ token, table: linkTable }] = useState(readLink);
  const [tableId, setTableId] = useState<string | null>(linkTable);
  const [data, setData] = useState<StateResponse | null>(null);
  const [rules, setRules] = useState(false),
    [history, setHistory] = useState(false),
    [lobby, setLobby] = useState(false),
    [profile, setProfile] = useState(false);
  const [muted, setMuted] = useState(true),
    [join, setJoin] = useState<number | null>(null),
    [buyin, setBuyin] = useState(10000),
    [dismissedResult, setDismissedResult] = useState<string | null>(null),
    [bubble, setBubble] = useState<{ id: string; seat: number; text: string } | null>(null),
    [flights, setFlights] = useState<Flight[]>([]),
    [winners, setWinners] = useState<number[]>([]),
    [potBump, setPotBump] = useState(false),
    [sound, setSound] = useState(soundEnabled);
  const [pending, setPending] = useState(false),
    [connected, setConnected] = useState(false),
    [expired, setExpired] = useState(!token),
    [error, setError] = useState(token ? "" : "개인 링크가 아닙니다. Discord에서 /카지노입장 으로 링크를 받아 주세요.");
  const [wager, setWager] = useState(100),
    [chip, setChip] = useState(500),
    [spot, setSpot] = useState<BetSpot>("main"),
    [placements, setPlacements] = useState<Placement[]>([]),
    [lastPlacements, setLastPlacements] = useState<Placement[]>([]),
    [raise, setRaise] = useState(200),
    [chat, setChat] = useState("");
  const stateRef = useRef<StateResponse | null>(null),
    snapshotRef = useRef(""),
    refreshingRef = useRef(false),
    refreshQueued = useRef(false),
    refreshRef = useRef<() => Promise<void>>(async () => {}),
    tableRef = useRef<string | null>(linkTable),
    pendingRef = useRef(false),
    expiredRef = useRef(!token),
    socketRef = useRef<WebSocket | null>(null),
    socketOpenRef = useRef(false),
    lastMessageAt = useRef(0),
    connectRef = useRef<(() => void) | null>(null),
    hiddenState = useRef<Received | null>(null),
    hiddenHeartbeat = useRef<number | null>(null),
    autoBetRound = useRef<string | null>(null),
    bubbleSeen = useRef<string | null>(null),
    prevBets = useRef<{ round: string | null; bets: number[]; pot: number }>({ round: null, bets: [], pot: 0 }),
    winnersShown = useRef<string | null>(null),
    flightId = useRef(1),
    wasMyTurn = useRef(false),
    resultSounded = useRef<string | null>(null),
    lastNarration = useRef("");
  /** 웹소켓이 열려 있고 최근에 소식이 왔는지. */
  const socketAlive = () => socketOpenRef.current && Date.now() - lastMessageAt.current <= SOCKET_SILENCE_MS;
  /** 서버 상태를 받아들인다. key는 내용 비교용 키. */
  const accept = useCallback((value: StateResponse, key: string) => {
    const wanted = tableRef.current;
    // 보고 있던 테이블이 아직 있는데 다른 테이블 상태가 오면(전환 직후 늦은 푸시) 무시한다.
    if (value.table && wanted && value.table.id !== wanted && value.tables.some((t) => t.id === wanted)) return;
    if (isStaleSnapshot(stateRef.current, value)) return;
    if (value.table && value.table.id !== wanted) {
      tableRef.current = value.table.id;
      setTableId(value.table.id);
    } else if (!value.table && wanted) {
      tableRef.current = null;
      setTableId(null);
    }
    if (typeof value.server_time === "number") confirmServerState(value.server_time);
    const next = shareStable(stateRef.current, value);
    stateRef.current = next;
    if (snapshotRef.current !== key) {
      snapshotRef.current = key;
      // 이 상태를 그리는 렌더가 최신 서버 시각을 쓰게 한다.
      touchServerClock();
      setData(next);
    }
    setConnected(true);
    setError("");
  }, []);
  /**
   * 받은 원문 한 건: 먼저 시계 표본을 잡고(파싱 전), 하트비트면 거기서 끝, 전과 같은 내용이면 파싱도 건너뛴다.
   * 처리 뒤 최신 상태를 돌려준다 (하트비트면 null).
   */
  const ingest = useCallback(
    (raw: string, receivedAt: number): StateResponse | null => {
      const serverTime = readServerTime(raw);
      if (serverTime !== null) sampleServerTime(serverTime, receivedAt);
      const key = rawStateKey(raw);
      if (key === "{}") {
        if (serverTime !== null) confirmServerState(serverTime);
        // 하트비트: 바뀐 것은 없지만 연결은 살아 있다 (잠깐의 오류 안내를 걷는다).
        if (stateRef.current && !expiredRef.current) {
          setConnected(true);
          setError("");
        }
        return null;
      }
      if (key !== null && key === snapshotRef.current) {
        if (serverTime !== null) confirmServerState(serverTime);
        setConnected(true);
        setError("");
        return stateRef.current;
      }
      let value: StateResponse;
      try {
        value = JSON.parse(raw) as StateResponse;
      } catch {
        return null; // 잘못된 페이로드는 무시한다.
      }
      if (!value || typeof value !== "object") return null;
      if (serverTime === null && typeof value.server_time === "number") sampleServerTime(value.server_time, receivedAt);
      if (!value.me) return null; // me가 없으면 하트비트: 시계와 연결 확인에만 쓴다.
      accept(value, key ?? snapshotKey(value));
      return stateRef.current;
    },
    [accept],
  );
  const fail = useCallback((e: unknown) => {
    if (e instanceof CasinoApiError && e.status === 401) {
      setConnected(false);
      expiredRef.current = true;
      setExpired(true);
      setError(e.message);
      socketRef.current?.close();
      return;
    }
    // 웹소켓으로 상태가 잘 오고 있으면 잠깐의 네트워크 오류로 버튼을 막거나 경고를 띄우지 않는다 (socketAlive는 ref만 읽는다).
    if (!(e instanceof CasinoApiError) && socketAlive()) return;
    setConnected(false);
    setError(e instanceof Error ? e.message : "연결을 확인해 주세요.");
  }, []);
  /** 미뤄 둔 새로 받기를 실행한다 (진행 중인 요청이 끝난 뒤). */
  const flushQueuedRefresh = useCallback(() => {
    if (!refreshQueued.current || refreshingRef.current || pendingRef.current) return;
    refreshQueued.current = false;
    void refreshRef.current();
  }, []);
  const refresh = useCallback(async () => {
    if (!token || pendingRef.current || expiredRef.current || refreshingRef.current) return;
    refreshingRef.current = true;
    try {
      const got = await fetchState(token, tableRef.current);
      ingest(got.raw, got.receivedAt);
    } catch (e) {
      fail(e);
    } finally {
      refreshingRef.current = false;
      flushQueuedRefresh();
    }
  }, [token, ingest, fail, flushQueuedRefresh]);
  refreshRef.current = refresh;
  /** 진행 중인 요청이 있어도 잃지 않고, 끝난 뒤 한 번 더 새로 받는다 (테이블 전환 등). */
  const requestRefresh = useCallback(() => {
    refreshQueued.current = true;
    flushQueuedRefresh();
  }, [flushQueuedRefresh]);

  // 첫 로드 + 웹소켓이 끊긴 동안의 폴링 (원본과 같은 1.2초). 열려 있는데 조용한 소켓은 다시 연결한다.
  useEffect(() => {
    if (!token) return;
    void refresh();
    const interval = setInterval(() => {
      if (document.visibilityState !== "visible") return;
      if (socketOpenRef.current && Date.now() - lastMessageAt.current > SOCKET_SILENCE_MS) {
        connectRef.current?.();
        void refresh();
      } else if (!socketOpenRef.current) {
        void refresh();
      }
    }, POLL_MS);
    const wake = () => {
      if (document.visibilityState !== "visible") return;
      // 숨어 있는 동안 받아 둔 마지막 상태를 먼저 그리고, 최신 상태를 다시 받는다.
      const buffered = hiddenState.current;
      hiddenState.current = null;
      if (buffered) ingest(buffered.raw, buffered.receivedAt);
      const heartbeat = hiddenHeartbeat.current;
      hiddenHeartbeat.current = null;
      if (heartbeat !== null) confirmServerState(heartbeat);
      connectRef.current?.();
      void refresh();
    };
    const online = () => {
      connectRef.current?.();
      void refresh();
    };
    document.addEventListener("visibilitychange", wake);
    window.addEventListener("online", online);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", wake);
      window.removeEventListener("online", online);
    };
  }, [token, refresh, ingest]);

  // 웹소켓 푸시: 바뀔 때 즉시 + 1초마다 하트비트.
  useEffect(() => {
    if (!token) return;
    let closed = false;
    let attempts = 0;
    let retry: number | null = null;
    const drop = (socket: WebSocket) => {
      socket.onopen = null;
      socket.onmessage = null;
      socket.onclose = null;
      socket.onerror = null;
      try {
        socket.close();
      } catch {
        // 이미 닫혔다.
      }
      if (socketRef.current === socket) {
        socketRef.current = null;
        socketOpenRef.current = false;
      }
    };
    const later = () => {
      if (closed || expiredRef.current || retry !== null) return;
      // 서버가 다시 뜨는 동안 모두가 같은 박자로 두드리지 않게 점점 늦추고 흩뜨린다.
      const delay = Math.min(RECONNECT_MAX_MS, RECONNECT_BASE_MS * 2 ** attempts) * (0.7 + Math.random() * 0.6);
      attempts = Math.min(attempts + 1, 6);
      retry = window.setTimeout(() => {
        retry = null;
        connect();
      }, delay);
    };
    const connect = () => {
      if (closed || expiredRef.current) return;
      if (retry !== null) {
        window.clearTimeout(retry);
        retry = null;
      }
      const old = socketRef.current;
      if (old) drop(old);
      let socket: WebSocket;
      try {
        socket = new WebSocket(wsUrl(token, tableRef.current));
      } catch {
        later();
        return;
      }
      socketRef.current = socket;
      socket.onopen = () => {
        socketOpenRef.current = true;
        lastMessageAt.current = Date.now();
        // 연결하는 사이에 테이블을 바꿨을 수 있다: 지금 보는 테이블을 다시 알린다.
        if (tableRef.current) socket.send(JSON.stringify({ table: tableRef.current }));
      };
      socket.onmessage = (event) => {
        // 받은 시각은 JSON을 풀기 전에 단조 시계로 잰다 (시계 표본).
        const receivedAt = deviceNow();
        lastMessageAt.current = Date.now();
        attempts = 0;
        if (typeof event.data !== "string") return;
        if (document.hidden) {
          // 숨은 탭은 그리지 않고 마지막 상태만 들고 있다가 돌아오면 그린다.
          const serverTime = readServerTime(event.data);
          if (serverTime !== null) sampleServerTime(serverTime, receivedAt);
          if (rawStateKey(event.data) !== "{}") {
            hiddenState.current = { raw: event.data, receivedAt };
          } else if (serverTime !== null) {
            if (hiddenState.current) hiddenHeartbeat.current = Math.max(hiddenHeartbeat.current ?? 0, serverTime);
            else confirmServerState(serverTime);
          }
          return;
        }
        ingest(event.data, receivedAt);
      };
      socket.onclose = () => {
        // 이미 바꿔 끼운 옛 소켓이 늦게 닫혀도 새 소켓을 건드리지 않는다.
        if (socketRef.current !== socket) return;
        socketRef.current = null;
        socketOpenRef.current = false;
        later();
      };
      socket.onerror = () => socket.close();
    };
    connectRef.current = () => {
      const socket = socketRef.current;
      if (socket && socket.readyState === WebSocket.CONNECTING) return;
      if (socket && socket.readyState === WebSocket.OPEN && Date.now() - lastMessageAt.current <= SOCKET_SILENCE_MS) return;
      attempts = 0;
      connect();
    };
    connect();
    return () => {
      closed = true;
      connectRef.current = null;
      if (retry !== null) window.clearTimeout(retry);
      const socket = socketRef.current;
      if (socket) drop(socket);
    };
  }, [token, ingest]);

  // 링크의 ?table= 을 현재 테이블과 맞춘다.
  useEffect(() => {
    const url = new URL(location.href);
    if (tableId) url.searchParams.set("table", tableId);
    else url.searchParams.delete("table");
    window.history.replaceState(null, "", url);
  }, [tableId]);

  const tables = data?.tables ?? [];
  const table = data?.table ?? BLANK_TABLE;
  const noTable = !data?.table;
  const round = table.round,
    me = table.my_seat >= 0 ? table.seats[table.my_seat] : null;
  const anySeat = Boolean(data?.me.seated_table),
    isTurn = round?.turn === table.my_seat && table.my_seat >= 0;
  const tableRules = table.rules;
  const timing = useMemo(() => cardTiming(tableRules, motion), [tableRules, motion]);
  // 시간에 따라 바뀌는 값 중 루트가 쓰는 것은 바뀌는 순간에만 다시 그린다 (시계 틱마다가 아니다).
  // 카드 연출(딜·오픈) 중에는 액션을 막고 결과도 미룬다.
  const revealing = useServerValue(
    (now, confirmedAt) => round !== null && (now < round.reveal_until || confirmedAt < round.reveal_until),
  );
  // 연출 중 안내 (카드를 나누는 중 / 확인하는 중). 결과를 드러내는 말은 뒤집는 시각 전에는 쓰지 않는다.
  const stage = useServerValue((now, confirmedAt) => {
    if (!round) return null;
    const pending = now < round.reveal_until || confirmedAt < round.reveal_until;
    const current = revealStage(round, table.seats, now);
    return pending ? current ?? "dealing" : null;
  });
  // 베팅 마감 직전 3초만 초 단위로 본다 (자동 베팅 확정).
  const betSeconds = useServerValue((now) => (round?.phase === "betting" && round.deadline ? Math.min(3, secondsLeft(round.deadline, now)) : 3));
  const legal = table.legal.poker,
    bj = table.legal.blackjack;
  const balance = data?.me.coins ?? 0,
    name = data?.me.name ?? "플레이어";
  const kind = table.kind;
  const lastBlackjackDock = useRef<{ roundId: string; kind: "blackjack" | "neutral" } | null>(null);
  useEffect(() => {
    if (!round || stage || kind !== "blackjack") return;
    lastBlackjackDock.current = { roundId: round.id, kind: bj ? "blackjack" : "neutral" };
  }, [bj, kind, round?.id, stage]);
  const rememberedBlackjackDock = stage && round && lastBlackjackDock.current?.roundId === round.id
    ? lastBlackjackDock.current.kind
    : null;
  const shoe = table.shoe;
  const shuffling = useServerValue((now) => !!shoe && shoe.total > 0 && now >= shoe.shuffled_at && now - shoe.shuffled_at < SHUFFLE_MS);
  const steadyHands = useSteadyHands(table);
  const myHandCards = (me && steadyHands.get(table.my_seat)?.cards) || NO_CARDS;
  const denoms = tableDenoms(tableRules);
  const sideBetsOpen = tableRules.side_bet_max > 0;
  // 서버 시계에 다음 예정 사건(카드 출발·도착, 뒤집기, 연출 끝)과 마감을 알려 준다. 시계는 그때만 깬다.
  useEffect(() => {
    const current = data?.table ?? null;
    planServerClock(tableEvents(current, timing), current?.round?.deadline ?? 0, `${current?.id ?? ""}:${current?.round?.id ?? ""}`);
  }, [data, timing]);
  useEffect(() => {
    // 다른 한도의 테이블로 옮기면 트레이에 있는 칩으로 다시 고른다.
    if (!denoms.includes(chip)) setChip(denoms[Math.min(1, denoms.length - 1)] ?? tableRules.min_bet);
  }, [denoms.join(","), chip]);
  // 카드가 출발할 슈 위치: 딜러 사진 속 슈 입구를 테이블 좌표(%)로 옮긴다.
  const tableEl = useRef<HTMLDivElement>(null);
  const [shoePos, setShoePos] = useState<[number, number]>([79, 34]);
  useLayoutEffect(() => {
    const box = tableEl.current;
    if (!box) return;
    const img = box.querySelector<HTMLImageElement>(".dealer-backdrop-poster");
    const measure = () => {
      const outer = box.getBoundingClientRect();
      if (!img || !img.naturalWidth || !outer.width || !outer.height) return;
      const rect = img.getBoundingClientRect();
      const style = getComputedStyle(img);
      const [px, py] = style.objectPosition.split(" ").map((value) => parseFloat(value) / 100);
      const scale = (style.objectFit === "contain" ? Math.min : Math.max)(rect.width / img.naturalWidth, rect.height / img.naturalHeight);
      const width = img.naturalWidth * scale,
        height = img.naturalHeight * scale;
      const x = rect.left - outer.left + (rect.width - width) * (Number.isFinite(px) ? px : 0.5) + SHOE_IN_PHOTO[0] * width;
      const y = rect.top - outer.top + (rect.height - height) * (Number.isFinite(py) ? py : 0.5) + SHOE_IN_PHOTO[1] * height;
      const next: [number, number] = [(100 * x) / outer.width, (100 * y) / outer.height];
      setShoePos((current) => (Math.abs(current[0] - next[0]) < 0.05 && Math.abs(current[1] - next[1]) < 0.05 ? current : next));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    img?.addEventListener("load", measure);
    return () => {
      observer.disconnect();
      img?.removeEventListener("load", measure);
    };
  }, [table.id, table.dealer.id]);

  // 효과음용 오디오는 첫 클릭 때 미리 준비한다 (딜 도중에 만들면 화면이 멈춘다).
  useEffect(() => installAudioUnlock(), []);
  useEffect(() => {
    const change = () => setFullscreen((current) => document.fullscreenElement === appRef.current ? "native" : current === "native" ? null : current);
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (document.fullscreenElement === appRef.current) void document.exitFullscreen().catch(() => {});
      setFullscreen((current) => current === "expanded" ? null : current);
    };
    document.addEventListener("fullscreenchange", change);
    window.addEventListener("keydown", escape);
    return () => { document.removeEventListener("fullscreenchange", change); window.removeEventListener("keydown", escape); };
  }, []);
  useEffect(() => {
    if (fullscreen !== "expanded") return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => { document.body.style.overflow = previous; };
  }, [fullscreen]);
  const toggleFullscreen = async () => {
    if (fullscreen) {
      if (document.fullscreenElement === appRef.current) await document.exitFullscreen();
      setFullscreen(null);
      return;
    }
    try {
      if (!document.fullscreenEnabled || !appRef.current?.requestFullscreen) throw new Error("unavailable");
      await appRef.current.requestFullscreen();
      setFullscreen("native");
    } catch {
      setFullscreen("expanded");
    }
  };
  const maxBuyin = Math.max(tableRules.min_buy_in, Math.min(tableRules.max_buy_in, floorStep(balance, tableRules.buy_in_step)));
  // 블랙잭 베팅: 최소~최대 사이, 단위에 맞고, 테이블 칩을 넘지 않아야 한다.
  const maxWager = me ? Math.min(tableRules.max_bet, floorStep(me.stack, tableRules.bet_step)) : tableRules.max_bet;
  const clampWager = (value: number) =>
    Math.min(Math.max(maxWager, tableRules.min_bet), Math.max(tableRules.min_bet, floorStep(Number.isFinite(value) ? value : tableRules.min_bet, tableRules.bet_step)));
  const validWager = wager >= tableRules.min_bet && wager <= maxWager && wager % tableRules.bet_step === 0;
  const chipsAt = (which: BetSpot, list: Placement[] = placements) => list.filter((p) => p.spot === which).map((p) => p.value);
  const chipStack = chipsAt("main");
  const stackTotal = chipStack.reduce((sum, value) => sum + value, 0);
  const pairsTotal = chipsAt("pairs").reduce((sum, value) => sum + value, 0);
  const plus3Total = chipsAt("plus3").reduce((sum, value) => sum + value, 0);
  const grandTotal = stackTotal + pairsTotal + plus3Total;
  const seatChips = me?.stack ?? 0;
  // 메인은 테이블 한도, 사이드는 사이드 한도, 전부 합쳐 보유 칩을 넘지 않아야 한다.
  const canPlaceAt = (which: BetSpot, value: number) => {
    if (grandTotal + value > seatChips) return false;
    if (which === "main") return stackTotal + value <= tableRules.max_bet;
    const current = which === "pairs" ? pairsTotal : plus3Total;
    return current + value <= tableRules.side_bet_max;
  };
  const canPlaceChip = (value: number) => canPlaceAt(spot, value);
  const placeChip = (value: number = chip, which: BetSpot = spot) => {
    if (!table.legal.can_bet || disabled || !canPlaceAt(which, value)) return;
    sfx.chip();
    setPlacements((list) => [...list, { spot: which, value }]);
  };
  const placeableAnywhere = (value: number) => (["main", "pairs", "plus3"] as BetSpot[]).some((which) => canPlaceAt(which, value));
  /** 칩을 못 놓는 이유 (끌어 놓기 실패 안내). */
  const placeRefusal = (which: BetSpot, value: number) =>
    grandTotal + value > seatChips
      ? "테이블 칩이 부족해요."
      : which === "main"
        ? `메인 베팅은 최대 ${fmt(tableRules.max_bet)}까지예요.`
        : `사이드베팅은 자리마다 최대 ${fmt(tableRules.side_bet_max)}까지예요.`;
  // 칩 끌어 놓기: 트레이의 칩을 베팅 자리(독의 베팅 원, 내 좌석의 칩 자리)로 옮긴다.
  // 손가락을 따라가는 칩은 ref로 직접 옮기고(프레임당 한 번), 화면 상태는 올려 둘 자리가 바뀔 때만 바꾼다.
  const [drag, setDrag] = useState<{ value: number; x: number; y: number; over: BetSpot | null } | null>(null);
  const dragRef = useRef<{
    value: number;
    startX: number;
    startY: number;
    x: number;
    y: number;
    moved: boolean;
    pointer: number;
    over: BetSpot | null;
    frame: number | null;
  } | null>(null);
  const ghostRef = useRef<HTMLDivElement>(null);
  const suppressClick = useRef(false);
  const spotAt = (x: number, y: number): BetSpot | null => {
    const target = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-bet-spot]");
    const which = target?.dataset.betSpot as BetSpot | undefined;
    return which && (which === "main" || sideBetsOpen) ? which : null;
  };
  const endDrag = () => {
    const current = dragRef.current;
    if (current?.frame != null) cancelAnimationFrame(current.frame);
    dragRef.current = null;
    setDrag(null);
  };
  const chipDrag = (value: number) => ({
    onPointerDown: (event: React.PointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0 || event.currentTarget.disabled) return;
      dragRef.current = {
        value,
        startX: event.clientX,
        startY: event.clientY,
        x: event.clientX,
        y: event.clientY,
        moved: false,
        pointer: event.pointerId,
        over: null,
        frame: null,
      };
      try {
        // 손가락·마우스가 칩 밖으로 나가도 끝까지 따라가게 잡아 둔다.
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // 이미 끝난 포인터면 캡처 없이 진행한다.
      }
    },
    onPointerMove: (event: React.PointerEvent<HTMLButtonElement>) => {
      const current = dragRef.current;
      if (!current || current.pointer !== event.pointerId) return;
      current.x = event.clientX;
      current.y = event.clientY;
      if (!current.moved) {
        if (Math.hypot(current.x - current.startX, current.y - current.startY) < 6) return;
        current.moved = true;
        setDrag({ value, x: current.x, y: current.y, over: null });
      }
      if (current.frame !== null) return;
      current.frame = requestAnimationFrame(() => {
        const live = dragRef.current;
        if (!live) return;
        live.frame = null;
        if (ghostRef.current) ghostRef.current.style.transform = `translate3d(${live.x}px, ${live.y}px, 0)`;
        const over = spotAt(live.x, live.y);
        if (over !== live.over) {
          live.over = over;
          setDrag((d) => (d ? { ...d, over } : d));
        }
      });
    },
    onPointerUp: (event: React.PointerEvent<HTMLButtonElement>) => {
      const current = dragRef.current;
      if (!current || current.pointer !== event.pointerId) return;
      endDrag();
      if (!current.moved) return;
      // 끌기가 끝난 뒤 따라오는 클릭은 칩을 한 번 더 놓지 않게 무시한다.
      suppressClick.current = true;
      window.setTimeout(() => (suppressClick.current = false), 0);
      const target = spotAt(event.clientX, event.clientY);
      if (!target) return;
      if (!canPlaceAt(target, value)) {
        toast.info(placeRefusal(target, value));
        return;
      }
      setChip(value);
      setSpot(target);
      placeChip(value, target);
    },
    onPointerCancel: () => endDrag(),
  });
  const dragging = drag !== null;
  useEffect(() => {
    // 끄는 도중 베팅이 마감돼 칩 트레이가 사라지면 포인터 캡처도 풀린다. 창에서 끝을 받아 정리한다.
    if (!dragging) return;
    if (!table.legal.can_bet) endDrag();
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
    return () => {
      window.removeEventListener("pointerup", endDrag);
      window.removeEventListener("pointercancel", endDrag);
    };
    // endDrag는 ref와 setState만 쓴다.
  }, [dragging, table.legal.can_bet]);
  const potRaise = (fraction: number) => (legal && round ? potRaiseTo(round, legal, fraction) : 0);
  const sidesValid = (pairsTotal === 0 || pairsTotal >= tableRules.side_bet_min) && (plus3Total === 0 || plus3Total >= tableRules.side_bet_min);
  const lockBet = async () => {
    if (!table.legal.can_bet || stackTotal < tableRules.min_bet || stackTotal > maxWager || !sidesValid) return false;
    const list = placements;
    const ok = await act({ action: "bet", amount: stackTotal, pairs: pairsTotal, plus3: plus3Total });
    if (ok) {
      sfx.lock();
      setLastPlacements(list);
      setPlacements([]);
    }
    return ok;
  };

  useEffect(() => {
    if (legal) setRaise(Math.min(legal.min_raise_to, legal.max_raise_to));
  }, [legal?.min_raise_to, legal?.max_raise_to, round?.id, round?.phase]);
  useEffect(() => {
    // 새 베팅 라운드마다 지난 베팅액을 유지하되 현재 칩 안으로 맞춘다.
    if (table.legal.can_bet) setWager((value) => clampWager(value));
  }, [table.legal.can_bet, round?.id, maxWager]);
  useEffect(() => {
    // 새 라운드가 열리면 쌓던 칩을 비운다.
    setPlacements([]);
    setSpot("main");
    autoBetRound.current = null;
  }, [round?.id]);
  useEffect(() => {
    // 베팅 마감 2초 전: 쌓아 둔 칩이 있으면 자동으로 확정한다 (에볼루션의 "No more bets").
    // 실패하면(다른 요청 중, 연결 문제) 다음 초에 한 번 더 시도한다.
    if (!table.legal.can_bet || !round || stackTotal < tableRules.min_bet) return;
    if (betSeconds > 2 || betSeconds <= 0 || autoBetRound.current === round.id || pendingRef.current) return;
    const id = round.id;
    autoBetRound.current = id;
    void lockBet().then((ok) => {
      if (!ok && autoBetRound.current === id) autoBetRound.current = null;
    });
  }, [betSeconds, table.legal.can_bet, round?.id, stackTotal]);
  // 액션 말풍선: 딜러 안내 "이름님, 콜." 을 좌석 위에 잠깐 띄운다. 채팅이 40개로 꽉 차도 마지막 메시지로 따라간다.
  const lastMessageId = table.messages.length ? table.messages[table.messages.length - 1].id : null;
  useEffect(() => {
    const last = [...table.messages].reverse().find((m) => m.dealer);
    if (!last || bubbleSeen.current === last.id) return;
    bubbleSeen.current = last.id;
    const match = /^(.+?)님, (.+?)\.$/.exec(last.text);
    // 액션 문구만 띄운다 (인사말 같은 긴 안내는 제외).
    if (!match || match[2].length > 14 || !/(체크|콜|폴드|레이즈|올인|히트|스탠드|더블|스플릿|서렌더|베팅)/.test(match[2])) return;
    const seat = table.seats.find((s) => s?.name === match[1]);
    if (!seat) return;
    setBubble({ id: last.id, seat: seat.seat, text: match[2] });
  }, [lastMessageId, table.id]);
  useEffect(() => {
    // 숨기는 타이머는 말풍선마다 따로: 다른 메시지가 와도 끊기지 않는다.
    if (!bubble) return;
    const id = bubble.id;
    const timer = window.setTimeout(() => setBubble((current) => (current?.id === id ? null : current)), 1700);
    return () => window.clearTimeout(timer);
  }, [bubble?.id]);
  // 칩 이동: 스트리트가 넘어가 베팅이 팟으로 모일 때, 그리고 결과가 나와 팟이 승자에게 갈 때.
  useEffect(() => {
    const bets = table.seats.map((s) => s?.bet ?? 0);
    const previous = prevBets.current;
    const pot = round?.pot ?? 0;
    if (round && previous.round === round.id && round.phase !== "complete") {
      const moved: Flight[] = [];
      previous.bets.forEach((bet, index) => {
        if (bet > 0 && bets[index] === 0) {
          moved.push({ id: flightId.current++, from: seatPos(index, table.seats.length), to: POT_POS, amount: bet });
        }
      });
      if (moved.length && motion) setFlights((current) => [...current, ...moved]);
    }
    if (round && pot !== previous.pot && previous.round === round.id) {
      setPotBump(true);
      window.setTimeout(() => setPotBump(false), 450);
    }
    prevBets.current = { round: round?.id ?? null, bets, pot };
  }, [table.seats, round?.id, round?.phase, round?.pot]);
  useEffect(() => {
    if (!muted && table.narration !== lastNarration.current && "speechSynthesis" in window) {
      speechSynthesis.cancel();
      const speech = new SpeechSynthesisUtterance(table.narration);
      speech.lang = "ko-KR";
      speech.rate = 0.97;
      speechSynthesis.speak(speech);
    }
    lastNarration.current = table.narration;
  }, [table.narration, muted]);
  useEffect(() => {
    document.title = noTable ? "CASINO73 — Texas Hold’em & Blackjack" : `${table.name} · CASINO73`;
  }, [noTable, table.name]);

  const chooseTable = useCallback(
    (id: string) => {
      if (!id || id === tableRef.current) return;
      tableRef.current = id;
      setTableId(id);
      lastNarration.current = "";
      const socket = socketRef.current;
      if (socket && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ table: id }));
      // 소켓이 아직이면 받아 오기: 진행 중인 요청이 있어도 끝난 뒤 새 테이블을 받는다.
      else requestRefresh();
    },
    [requestRefresh],
  );
  const act = useCallback(
    async (command: CasinoCommand) => {
      if (pendingRef.current || !stateRef.current?.table || !token) return false;
      const current = stateRef.current.table;
      pendingRef.current = true;
      setPending(true);
      // 콜·레이즈만 테이블 버전을 확인한다 (그사이 금액이 바뀌었으면 거절돼야 한다). 나머지는 서버가 직접 검증한다.
      const versioned = command.action === "call" || command.action === "raise";
      try {
        const sent = await sendCommand(token, current.id, versioned ? current.version : null, command);
        ingest(sent.raw, sent.receivedAt);
        return true;
      } catch (first) {
        let e: unknown = first;
        if (versioned && first instanceof CasinoApiError && first.code === "STALE_STATE") {
          // 채팅·입장 같은 다른 변화로 버전만 올랐고 내 차례가 그대로면 최신 버전으로 조용히 한 번 더 보낸다.
          try {
            const fresh = await fetchState(token, current.id);
            ingest(fresh.raw, fresh.receivedAt);
            const next = stateRef.current?.table;
            if (next && next.id === current.id && next.version !== current.version && sameTurn(current, next)) {
              const sent = await sendCommand(token, next.id, next.version, command);
              ingest(sent.raw, sent.receivedAt);
              return true;
            }
          } catch (second) {
            e = second;
          }
        }
        if (e instanceof CasinoApiError && e.code === "STALE_STATE") {
          toast.error("테이블 상태가 바뀌어 다시 불러왔습니다. 다시 시도해 주세요.");
        } else if (e instanceof CasinoApiError && e.status === 401) {
          fail(e);
        } else {
          toast.error(e instanceof Error ? e.message : "연결이 끊겼습니다. 상태를 다시 확인합니다.");
        }
        // 명령이 서버에 닿았을 수도 있다: 끝난 뒤 상태를 한 번 확인한다.
        refreshQueued.current = true;
        return false;
      } finally {
        pendingRef.current = false;
        setPending(false);
        flushQueuedRefresh();
      }
    },
    [token, ingest, fail, flushQueuedRefresh],
  );

  const disabled = pending || !connected;
  /** 카드를 나누거나 여는 동안에는 게임 버튼을 그대로 두고 누르지만 못하게 한다 (독 높이가 바뀌지 않는다). */
  const busy = disabled || revealing;
  const openJoin = (seat?: number) => {
    if (me || noTable) return;
    if (anySeat) {
      toast.info("현재 테이블에서 퇴장한 후 참여해 주세요.");
      return;
    }
    if (!connected) {
      toast.info(expired ? "링크가 만료됐어요. Discord에서 /카지노입장 으로 새 링크를 받아 주세요." : "서버 연결을 확인하고 있습니다.");
      return;
    }
    const index = seat ?? table.seats.findIndex((s) => !s);
    if (index < 0) {
      toast.info("빈 좌석이 없습니다.");
      return;
    }
    setBuyin((value) => Math.max(tableRules.min_buy_in, Math.min(maxBuyin, floorStep(value, tableRules.buy_in_step))));
    setJoin(index);
  };
  const invite = async () => {
    const command = `/카지노입장 테이블:${table.name}`;
    try {
      await navigator.clipboard.writeText(command);
      toast.success("Discord 명령을 복사했습니다. 서버 채널에 붙여넣으면 개인 링크를 받아요.");
    } catch {
      toast.info(`Discord에서 ${command} 을 입력하면 개인 링크를 받아요.`);
    }
  };
  // 라운드가 끝나면 결과를 테이블 위에 띄운다. 다음 라운드가 시작되거나 닫을 때까지 남는다.
  const latestResult = table.history[0] ?? null;
  const showResult =
    round !== null &&
    round.phase === "complete" &&
    stage === null &&
    latestResult !== null &&
    latestResult.id === round.id &&
    dismissedResult !== latestResult.id;
  const dealerFinal =
    kind === "blackjack" && round ? round.dealer_total ?? (round.dealer.length ? blackjackValue(round.dealer.filter((c) => c !== "??")).total : null) : null;
  useEffect(() => {
    // 결과가 뜨면 팟이 승자에게 날아가고 승자 좌석이 빛난다.
    if (!showResult || !latestResult || winnersShown.current === latestResult.id) {
      if (!showResult) setWinners([]);
      return;
    }
    winnersShown.current = latestResult.id;
    const won = latestResult.results.filter((r) => r.won > 0);
    setWinners(won.map((r) => r.seat));
    if (won.length && motion) {
      const flightsToWinners = won.map((r) => ({
        id: flightId.current++,
        from: kind === "holdem" ? POT_POS : ([50, 30] as [number, number]),
        to: seatPos(r.seat, table.seats.length) ?? POT_POS,
        amount: r.net + r.wagered,
      }));
      setFlights((current) => [...current, ...flightsToWinners]);
    }
  }, [showResult, latestResult?.id]);
  // 효과음: 카드 소리는 카드가 슈를 떠날 때 카드마다 낸다 (TableCards). 여기서는 내 차례와 결과.
  useEffect(() => {
    const mine = isTurn && !revealing;
    if (mine && !wasMyTurn.current) sfx.turn();
    wasMyTurn.current = mine;
  }, [isTurn, revealing]);
  useEffect(() => {
    if (!showResult || !latestResult || resultSounded.current === latestResult.id) return;
    resultSounded.current = latestResult.id;
    const mine = latestResult.results.find((r) => r.seat === table.my_seat);
    if (!mine) return;
    if (mine.won > 0) sfx.win();
    else if (mine.net < 0) sfx.lose();
  }, [showResult, latestResult?.id]);
  const seatedElsewhere = tables.find((t) => t.id === data?.me.seated_table && t.id !== table.id) ?? null;
  const posterUrl =
    table.dealer.id !== "sophia"
      ? dealerPortrait(table.dealer.id)
      : motion && hasDealerVideo("sophia")
        ? // 영상 첫 프레임: 영상이 뜨기 전후로 딜러 얼굴이 바뀌지 않게 한다.
          `${import.meta.env.BASE_URL}dealers/sophia-video.jpg`
        : `${import.meta.env.BASE_URL}dealers/sophia-video.jpg`;
  const onFlightDone = useCallback((id: number) => setFlights((current) => current.filter((f) => f.id !== id)), []);

  return (
    <div
      ref={appRef}
      className={`casino-app ${fullscreen ? "is-fullscreen" : ""} ${fullscreen === "expanded" ? "expanded-screen" : ""} ${drag ? "chip-dragging" : ""} ${
        motion ? "" : "calm"
      }`}
    >
      <Toaster position="top-center" richColors />
      {drag && (
        <div ref={ghostRef} className="drag-chip-track" style={{ transform: `translate3d(${drag.x}px, ${drag.y}px, 0)` }} aria-hidden="true">
          <div className={`drag-chip chip-${drag.value} ${drag.over ? "over" : ""}`}>{chipLabel(drag.value)}</div>
        </div>
      )}
      <header className="topbar">
        <a className="brand" href={location.pathname} aria-label="CASINO73 홈">
          <Club fill="currentColor" />
          <span>
            CASINO73<small>LIVE CASINO</small>
          </span>
        </a>
        <Tabs value={tableId ?? ""} onValueChange={chooseTable}>
          <TabsList className="game-nav">
            {tables.map((t) => (
              <TabsTrigger key={t.id} value={t.id} aria-label={`${t.name} (${t.kind_text})`} title={t.name}>
                {t.kind === "holdem" ? <Spade /> : <Diamond />}
                {t.name}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        <div className="top-right">
          <span className="practice-label">PLAY MONEY</span>
          <button className="balance" aria-label="내 코인 잔액" onClick={() => setProfile(true)}>
            <Wallet size={17} />
            <strong>{data ? fmt(balance) : "—"}</strong>
            <span>COINS</span>
          </button>
          <button className="profile" aria-label="내 프로필" onClick={() => setProfile(true)}>
            {name.slice(0, 1)}
          </button>
        </div>
      </header>
      <aside className="rail">
        <button className="rail-active" aria-label="테이블 목록" title="테이블 목록" onClick={() => setLobby(true)}>
          <Grid2X2 />
        </button>
        <button aria-label="핸드 기록" title="핸드 기록" onClick={() => setHistory(true)}>
          <History />
        </button>
        <button aria-label="게임 규칙 보기" title="게임 규칙" onClick={() => setRules(true)}>
          <CircleHelp />
        </button>
        <div className="rail-bottom">
          <ShieldCheck size={20} />
          <span>
            FAIR
            <br />
            PLAY
          </span>
        </div>
      </aside>
      <main className="workspace">
        <div className="breadcrumb">
          LIVE CASINO <span>/</span> {kindRoom(kind)}
        </div>
        <div className="page-heading">
          <div>
            <div className="title-line">
              <h1>{kindTitle(kind)}</h1>
              <span className="live-tag">
                <i /> LIVE TABLE
              </span>
            </div>
            <p>
              {kind === "holdem" ? "노 리밋 홀덤" : "클래식 블랙잭"}
              <span>·</span> AI 딜러 <span>·</span> 코인 테이블
            </p>
          </div>
          <div className="heading-actions">
            <button
              className="icon-button"
              aria-label={motion ? "연출 끄기" : "연출 켜기"}
              aria-pressed={motion}
              title={motion ? "연출 끄기 (카드·칩 애니메이션, 딜러 영상)" : "연출 켜기 (카드·칩 애니메이션, 딜러 영상)"}
              onClick={() => {
                setMotion(!motion);
                try {
                  localStorage.setItem("casino-motion", String(!motion));
                } catch {
                  /* Optional preference. */
                }
              }}
            >
              {motion ? <Video size={18} /> : <VideoOff size={18} />}
            </button>
            <button className="icon-button" aria-label={fullscreen ? "전체화면 종료" : "전체화면"} title={fullscreen ? "전체화면 종료 (Esc)" : "전체화면"} onClick={() => void toggleFullscreen()}>
              {fullscreen ? <Minimize size={18} /> : <Maximize size={18} />}
            </button>
            <button onClick={() => setRules(true)}>
              <CircleHelp size={17} />
              게임 규칙
            </button>
            <button className="icon-button" aria-label="핸드 기록 열기" onClick={() => setHistory(true)}>
              <History size={18} />
            </button>
            <button
              className="icon-button"
              aria-label={sound ? "효과음 끄기" : "효과음 켜기"}
              onClick={() => {
                setSoundEnabled(!sound);
                setSound(!sound);
                if (!sound) {
                  unlockAudio();
                  sfx.chip();
                }
              }}
            >
              {sound ? <Bell size={18} /> : <BellOff size={18} />}
            </button>
            <button
              className="icon-button"
              aria-label={muted ? "딜러 음성 켜기" : "딜러 음성 끄기"}
              onClick={() => {
                if (!muted && "speechSynthesis" in window) speechSynthesis.cancel();
                setMuted(!muted);
              }}
            >
              {muted ? <VolumeX size={19} /> : <Volume2 size={19} />}
            </button>
          </div>
        </div>
        {/* 연결 안내는 화면 흐름 밖에 띄운다: 떴다 사라져도 테이블이 밀리지 않는다. */}
        <div className="error-slot">
          {error && (
            <div className="error-banner" role="status">
              <WifiOff size={17} />
              <span>{error}</span>
              {expired ? (
                <span>Discord에서 /카지노입장</span>
              ) : (
                <button
                  onClick={() => {
                    connectRef.current?.();
                    requestRefresh();
                  }}
                >
                  <RefreshCw size={15} />
                  다시 연결
                </button>
              )}
            </div>
          )}
        </div>
        <div className="play-layout">
          <section className="table-column">
            <div ref={tableEl} className={`game-table ${kind} ${revealing ? "dealing" : ""} ${round && round.phase !== "complete" ? "in-play" : ""}`}>
              <DealerStage table={table} timing={timing} motion={motion} poster={posterUrl} />
              <div className="table-shade" />
              <span className="shoe-anchor" aria-hidden="true" style={{ left: `${shoePos[0]}%`, top: `${shoePos[1]}%` }} />
              <TableChatPreview messages={table.messages} />
              <div className="table-topline">
                <span className="room-id">
                  {noTable ? "NO TABLE" : table.name}
                  <small>{kindStakes(kind, tableRules)}</small>
                </span>
                <span className="ai-tag">
                  <AudioLines size={14} /> AI DEALER
                </span>
              </div>
              <div className="dealer-name">
                {table.dealer.id.toUpperCase()} <span>{table.dealer.tagline}</span>
              </div>
              {kind === "blackjack" && shoe && (
                <div className={`shoe-status ${shoe.reshuffle_due ? "cut-reached" : ""}`}>
                  <div><span>슈 {shoe.remaining}/{shoe.total}</span><small>{shoe.reshuffle_due ? "다음 라운드 전 셔플" : shoe.total ? "8 DECKS" : "라운드 시작 시 준비"}</small></div>
                  <div className="shoe-gauge" role="meter" aria-label="슈에서 딜된 카드" aria-valuemin={0} aria-valuemax={shoe.total || 416} aria-valuenow={Math.max(0, shoe.total - shoe.remaining)}>
                    <span style={{ width: `${shoe.total ? 100 * (1 - shoe.remaining / shoe.total) : 0}%` }} />
                    {shoe.total > 0 && <i title="빨간 컷 카드" style={{ left: `${100 * (1 - shoe.cut_at / shoe.total)}%` }} />}
                  </div>
                </div>
              )}
              {shuffling && shoe && (
                <div key={`${table.id}-${shoe.shuffled_at}`} className="shoe-shuffle" role="status">
                  <div className="shoe-fan" aria-hidden="true">{Array.from({ length: 9 }, (_, i) => <span key={i} style={{ "--fan": i - 4 } as React.CSSProperties} />)}</div>
                  <strong>새 슈를 섞는 중</strong><small>8 DECKS · 416 CARDS</small>
                </div>
              )}
              <div className="board">
                <div className="board-label">{kind === "holdem" ? "TEXAS HOLD’EM" : "BLACKJACK PAYS 3 TO 2"}</div>
                {kind === "holdem" && round && (
                  <div className={`pot-pill ${potBump ? "bump" : ""}`}>
                    POT <b>{fmt(round.pot)}</b>
                  </div>
                )}
                <BoardCards kind={kind} round={round} timing={timing} litCards={myHandCards} />
                <p className="board-status">
                  {round
                    ? stage
                      ? REVEAL_TEXT[stage]
                      : round.phase === "complete"
                      ? "라운드 종료 · 다음 핸드를 시작하세요"
                      : round.phase === "insurance"
                        ? <>인슈어런스 접수 · <Countdown deadline={round.deadline} />초</>
                        : round.phase === "betting"
                        ? <>베팅 마감까지 <Countdown deadline={round.deadline} />초</>
                        : isTurn
                          ? "당신의 차례입니다"
                           : round.turn >= 0
                             ? `${table.seats[round.turn]?.name ?? "좌석"}님 차례`
                             : REVEAL_TEXT.dealing
                    : noTable
                      ? "열려 있는 테이블이 없습니다"
                      : "테이블에 앉아 게임을 시작하세요"}
                </p>
              </div>
              {round?.burn_at && round.burn_at.length > 0 && <BurnPile roundId={round.id} times={round.burn_at} timing={timing} />}
              {table.seats.map((seat, i) =>
                seat ? (
                  <div
                    key={i}
                    className={`seat occupied seat-${seatSlot(i, table.seats.length)} ${seat.mine ? "mine" : ""} ${round?.turn === i && !revealing ? "active-seat" : ""} ${
                      seat.folded ? "folded" : ""
                    } ${winners.includes(i) ? "seat-winner" : ""}`}
                  >
                    <SeatCards
                      kind={kind}
                      seat={seat}
                      roundId={round?.id ?? "none"}
                      timing={timing}
                      litCards={steadyHands.get(i)?.cards ?? NO_CARDS}
                      handName={steadyHands.get(i)?.name ?? null}
                      activeHand={round?.phase === "playing" && round.turn === i && !revealing ? round.hand : -1}
                    />
                    <span className="player-avatar">
                      {seat.name.slice(0, 1)}
                      {kind === "holdem" && table.button === i && <i>D</i>}
                      {round && round.turn === i && round.phase !== "complete" && !revealing && round.deadline > 0 && (
                        <TurnRing key={`${round.id}-${round.turn}-${round.hand}-${round.deadline}`} deadline={round.deadline} turnMs={tableRules.turn_ms} />
                      )}
                    </span>
                    {kind === "blackjack" &&
                      (seat.mine && table.legal.can_bet ? (
                        <>
                          <ChipStack
                            chips={chipStack}
                            total={stackTotal}
                            active={spot === "main"}
                            hint={`${chipLabel(chip)} 놓기`}
                            spot="main"
                            over={drag?.over === "main"}
                            onClick={() => {
                              setSpot("main");
                              placeChip(chip, "main");
                            }}
                          />
                          {sideBetsOpen && (
                            <div className="side-spots">
                              {(["pairs", "plus3"] as BetSpot[]).map((which) => (
                                <ChipStack
                                  key={which}
                                  chips={chipsAt(which)}
                                  total={which === "pairs" ? pairsTotal : plus3Total}
                                  active={spot === which}
                                  hint={which === "pairs" ? "PP" : "21+3"}
                                  spot={which}
                                  over={drag?.over === which}
                                  onClick={() => {
                                    setSpot(which);
                                    placeChip(chip, which);
                                  }}
                                />
                              ))}
                            </div>
                          )}
                        </>
                      ) : (
                        mainBet(seat) > 0 && (
                          <>
                            <ChipStack chips={chipsFor(mainBet(seat))} total={mainBet(seat)} />
                            {(seat.side_pairs > 0 || seat.side_plus3 > 0 || seat.insurance > 0) && (
                              <div className="side-spots">
                                {seat.side_pairs > 0 && <ChipStack chips={chipsFor(seat.side_pairs)} total={seat.side_pairs} hint="PP" />}
                                {seat.side_plus3 > 0 && <ChipStack chips={chipsFor(seat.side_plus3)} total={seat.side_plus3} hint="21+3" />}
                                {seat.insurance > 0 && <ChipStack chips={chipsFor(seat.insurance)} total={seat.insurance} hint="INS" />}
                              </div>
                            )}
                          </>
                        )
                      ))}
                    <span className="player-name">
                      {seat.name}
                      {seat.mine && <b>나</b>}
                    </span>
                    <strong>{fmt(seat.stack)}</strong>
                    {seat.leaving ? (
                      <em>퇴장 대기</em>
                    ) : seat.sit_out ? (
                      <em>자리 비움</em>
                    ) : seat.folded ? (
                      <em>FOLD</em>
                    ) : round?.turn === i && !revealing && round.deadline > 0 ? (
                      <em>
                        <Countdown deadline={round.deadline} />s
                      </em>
                    ) : (
                      seat.bet > 0 && <em>{fmt(seat.bet)}</em>
                    )}
                  </div>
                ) : (
                  <button
                    key={i}
                    className={`seat seat-${seatSlot(i, table.seats.length)}`}
                    aria-label={`${i + 1}번 빈 좌석 참여`}
                    onClick={() => openJoin(i)}
                    disabled={noTable}
                  >
                    <span className="empty-avatar">
                      <Plus size={20} />
                    </span>
                    <span>빈 좌석</span>
                  </button>
                ),
              )}
              {kind === "blackjack" && <span className="felt-rules">BLACKJACK PAYS 3 TO 2 · INSURANCE PAYS 2 TO 1</span>}
              <span className="felt-mark">C A S I N O 7 3</span>
              {bubble && seatPos(bubble.seat, table.seats.length) && (
                <div
                  key={bubble.id}
                  className="action-bubble"
                  style={{ left: `${seatPos(bubble.seat, table.seats.length)[0]}%`, top: `${seatPos(bubble.seat, table.seats.length)[1] - 16}%` }}
                >
                  {bubble.text}
                </div>
              )}
              {flights.map((flight) => (
                <ChipFlight key={flight.id} flight={flight} onDone={onFlightDone} />
              ))}
              {showResult && latestResult && (
                <div className="result-banner" role="status" aria-live="polite">
                  <div className="result-head">
                    <span className="eyebrow">{kind === "holdem" ? "HAND RESULT" : "ROUND RESULT"}</span>
                    <button type="button" aria-label="결과 닫기" onClick={() => setDismissedResult(latestResult.id)}>
                      ✕
                    </button>
                  </div>
                  {dealerFinal !== null && <p className="result-dealer">딜러 {dealerFinal > 21 ? "버스트" : dealerFinal}</p>}
                  {latestResult.results.length > 0 ? (
                    <ul>
                      {latestResult.results.map((r) => (
                        <li
                          key={`${r.seat}-${r.user_id}`}
                          className={`${resultTone(r)} ${r.seat === table.my_seat ? "me" : ""}`}
                        >
                          <b className="result-name">{resultHeadline(r, r.seat === table.my_seat)}</b>
                          <small>
                            순손익 {signed(r.net)} · {r.label}
                            {r.notes.length > 0 ? ` · ${r.notes.join(" · ")}` : ""}
                          </small>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <p className="result-dealer">{latestResult.summary}</p>
                  )}
                  <p className="result-note">{me ? "라운드 시작을 누르면 다음 핸드가 시작됩니다." : "다음 핸드를 기다리는 중입니다."}</p>
                </div>
              )}
            </div>
            <div className={`action-dock ${me ? "seated-dock" : ""}`}>
              {!me ? (
                <>
                  <div>
                    <span className="eyebrow">TAKE YOUR SEAT</span>
                    <h2>당신의 다음 핸드가 기다립니다.</h2>
                    <p>
                      {seatedElsewhere
                        ? `${seatedElsewhere.name} 테이블에 앉아 있어요. 퇴장 후 이 테이블에 참여할 수 있어요.`
                        : "좌석을 선택하고 테이블에 참여하세요."}
                    </p>
                  </div>
                  {seatedElsewhere ? (
                    <button className="secondary-button" onClick={() => chooseTable(seatedElsewhere.id)}>
                      <ArrowUpRight size={16} />
                      {seatedElsewhere.name} 보기
                    </button>
                  ) : (
                    <button className="gold-button" onClick={() => openJoin()} disabled={disabled || noTable}>
                      <Plus size={18} />
                      테이블 참여
                    </button>
                  )}
                </>
              ) : (
                <>
                  {stage && (
                    // 카드를 나누는 동안에도 버튼은 제자리에 두고(누를 수만 없게), 안내는 독 위 테두리에 띄운다.
                    <span className="dock-status" role="status">
                      {stage === "showdown" ? "SHOWDOWN · 카드를 확인하는 중" : `DEALING · ${table.dealer.name}가 카드를 나누는 중`}
                    </span>
                  )}
                  <div className="my-hand-row">
                    <div className="my-hand-info">
                      <span className="eyebrow">
                        {kind === "holdem" ? "YOUR HAND" : "YOUR HANDS"}
                        {kind === "holdem" && steadyHands.get(table.my_seat)?.name && <b className="hand-now">{steadyHands.get(table.my_seat)?.name}</b>}
                      </span>
                      <div className="my-cards">
                        <DockCards
                          kind={kind}
                          seat={me}
                          roundId={round?.id ?? "none"}
                          litCards={myHandCards}
                           selectedHand={isTurn && !revealing && round ? round.hand : -1}
                        />
                      </div>
                    </div>
                    <div className="stack-info">
                      <small>내 테이블 칩</small>
                      <strong>{fmt(me.stack)}</strong>
                      <button aria-label="테이블 퇴장" disabled={disabled || me.leaving} onClick={() => void act({ action: "leave" })}>
                        <LogOut size={13} />
                        {me.leaving ? "퇴장 대기" : "퇴장"}
                      </button>
                    </div>
                  </div>
                  {me.sit_out && !isTurn ? (
                    <div className="action-row">
                      <p>시간 초과로 자리 비움 상태입니다.</p>
                      <button className="gold-button" disabled={disabled} onClick={() => void act({ action: "resume" })}>
                        복귀하기
                      </button>
                    </div>
                  ) : kind === "holdem" && legal ? (
                    <div className="poker-actions">
                      <div className="raise-control">
                        <span>
                          레이즈 총액 <strong>{fmt(raise)}</strong>
                        </span>
                        <Slider
                          aria-label="레이즈 총액"
                          value={[raise]}
                          min={Math.min(legal.min_raise_to, legal.max_raise_to)}
                          max={legal.max_raise_to}
                          step={1}
                          disabled={!legal.can_raise || disabled}
                          onValueChange={(v) => setRaise(v[0])}
                        />
                        <button disabled={!legal.can_raise || disabled} onClick={() => setRaise(legal.max_raise_to)}>
                          ALL IN
                        </button>
                      </div>
                      <div className="raise-presets" role="group" aria-label="레이즈 금액 빠른 선택">
                        {([
                          ["최소", 0],
                          ["½ 팟", 0.5],
                          ["¾ 팟", 0.75],
                          ["팟", 1],
                        ] as Array<[string, number]>).map(([label, fraction]) => {
                          const value = fraction === 0 ? Math.min(legal.min_raise_to, legal.max_raise_to) : potRaise(fraction);
                          return (
                            <button
                              key={label}
                              type="button"
                              className={raise === value ? "chosen" : ""}
                              disabled={!legal.can_raise || disabled}
                              onClick={() => setRaise(value)}
                            >
                              {label}
                              <b>{fmt(value)}</b>
                            </button>
                          );
                        })}
                      </div>
                      <div className="action-buttons">
                        <button className="fold-button" disabled={busy} onClick={() => void act({ action: "fold" })}>
                          폴드
                        </button>
                        <button className="secondary-button" disabled={busy} onClick={() => void act({ action: legal.can_check ? "check" : "call" })}>
                          {legal.can_check ? "체크" : `콜 ${fmt(legal.to_call)}`}
                        </button>
                        <button className="gold-button" disabled={busy || !legal.can_raise} onClick={() => void act({ action: "raise", amount: raise })}>
                          {raise === legal.max_raise_to ? "올인" : "레이즈"} {fmt(raise)}
                        </button>
                      </div>
                    </div>
                  ) : kind === "blackjack" && round?.phase === "insurance" && !stage ? (
                    // 인슈어런스 안내는 딜이 끝난 뒤에 (그 전에는 아래 중립 안내: 딜러 에이스를 미리 알리지 않는다).
                    <div className="action-row insurance-row">
                      <div>
                        <span className="eyebrow">INSURANCE</span>
                        <p>
                          {table.legal.can_insure
                            ? `딜러가 에이스를 보입니다. 인슈어런스 ${fmt(table.legal.insurance_cost)}(베팅의 절반)을 걸면 딜러 블랙잭일 때 2:1로 받습니다.`
                            : me.insurance_decided
                              ? me.insurance > 0
                                ? `인슈어런스 ${fmt(me.insurance)}을 걸었어요. 딜러 카드를 확인합니다.`
                                : "인슈어런스를 거절했어요. 딜러 카드를 확인합니다."
                              : "다른 플레이어의 인슈어런스 결정을 기다리는 중입니다."}{" "}
                          <span className="button-timer">
                            <Countdown deadline={round.deadline} />s
                          </span>
                        </p>
                      </div>
                      {table.legal.can_insure && (
                        <div className="action-buttons insurance-buttons">
                          <button className="gold-button" disabled={busy} onClick={() => void act({ action: "insure", accept: true })}>
                            인슈어런스 받기
                          </button>
                          <button className="secondary-button" disabled={busy} onClick={() => void act({ action: "insure", accept: false })}>
                            거절
                          </button>
                        </div>
                      )}
                    </div>
                  ) : kind === "blackjack" && (bj || rememberedBlackjackDock === "blackjack") ? (
                    <div className="blackjack-actions">
                      <button className="gold-button" disabled={busy} onClick={() => void act({ action: "hit" })}>
                        <Plus size={16} />
                        히트
                      </button>
                      <button className="secondary-button" disabled={busy} onClick={() => void act({ action: "stand" })}>
                        <Check size={16} />
                        스탠드
                      </button>
                      <button className="secondary-button" disabled={busy || !bj?.can_double} onClick={() => void act({ action: "double" })}>
                        더블 ×2
                      </button>
                      <button className="secondary-button" disabled={busy || !bj?.can_split} onClick={() => void act({ action: "split" })}>
                        스플릿
                      </button>
                    </div>
                  ) : table.legal.can_bet ? (
                    <div className="betting-controls evo">
                      <div className="bet-spots">
                        {(sideBetsOpen ? (["pairs", "main", "plus3"] as BetSpot[]) : (["main"] as BetSpot[])).map((which) => (
                          <BetCircle
                            key={which}
                            spot={which}
                            label={SPOT_LABEL[which]}
                            chips={chipsAt(which)}
                            total={which === "main" ? stackTotal : which === "pairs" ? pairsTotal : plus3Total}
                            limit={which === "main" ? maxWager : tableRules.side_bet_max}
                            active={spot === which}
                            over={drag?.over === which}
                            disabled={disabled}
                            onClick={() => {
                              setSpot(which);
                              if (canPlaceAt(which, chip)) placeChip(chip, which);
                              else toast.info(placeRefusal(which, chip));
                            }}
                          />
                        ))}
                      </div>
                      <div className="chip-options" role="group" aria-label="칩 고르기. 끌어서 베팅 자리에 놓을 수 있어요.">
                        {denoms.map((v) => (
                          <button
                            key={v}
                            type="button"
                            className={`chip chip-${v} ${chip === v ? "chosen" : ""}`}
                            disabled={disabled || !placeableAnywhere(v)}
                            {...chipDrag(v)}
                            onClick={() => {
                              if (suppressClick.current) return;
                              setChip(v);
                              if (canPlaceChip(v)) placeChip(v);
                            }}
                            aria-label={`${fmt(v)} 칩`}
                            title={`${fmt(v)} 칩 · 누르면 ${SPOT_LABEL[spot]}에 놓고, 끌어서 원하는 자리에 놓을 수 있어요`}
                          >
                            {chipLabel(v)}
                          </button>
                        ))}
                      </div>
                      <div className="bet-tools">
                        <span className="bet-total">
                          합계 <strong>{fmt(grandTotal)}</strong>
                          <small>
                            {" "}
                            / 메인 {fmt(tableRules.min_bet)}~{fmt(maxWager)}
                            {sideBetsOpen ? ` · 사이드 최대 ${fmt(tableRules.side_bet_max)}` : ""}
                          </small>
                        </span>
                        <button type="button" className="text-button" disabled={disabled || placements.length === 0} onClick={() => setPlacements((list) => list.slice(0, -1))}>
                          되돌리기
                        </button>
                        <button type="button" className="text-button" disabled={disabled || placements.length === 0} onClick={() => setPlacements([])}>
                          지우기
                        </button>
                        <button
                          type="button"
                          className="text-button"
                          disabled={
                            disabled || placements.length > 0 || lastPlacements.length === 0 || lastPlacements.reduce((s, p) => s + p.value, 0) > seatChips
                          }
                          onClick={() => setPlacements(lastPlacements)}
                        >
                          다시 베팅
                        </button>
                        <button
                          type="button"
                          className="text-button"
                          disabled={disabled || placements.length === 0 || grandTotal * 2 > seatChips || stackTotal * 2 > tableRules.max_bet}
                          onClick={() => setPlacements((list) => [...list, ...list])}
                        >
                          더블
                        </button>
                        <button
                          className="gold-button"
                          disabled={disabled || stackTotal < tableRules.min_bet || stackTotal > maxWager || !sidesValid || betSeconds <= 0}
                          onClick={() => void lockBet()}
                        >
                          베팅 확정{" "}
                          <span className="button-timer">
                            <Countdown deadline={round?.deadline ?? 0} />s
                          </span>
                        </button>
                      </div>
                      <p className="bet-hint">
                        칩을 끌어서 베팅 자리에 놓거나, 칩을 고른 뒤 자리를 누르세요.{" "}
                        {sideBetsOpen
                          ? `사이드베팅은 ${fmt(tableRules.side_bet_min)}~${fmt(tableRules.side_bet_max)}.`
                          : "이 테이블은 사이드베팅이 없습니다."}{" "}
                        마감 2초 전에 쌓인 칩은 자동으로 확정됩니다.
                      </p>
                    </div>
                  ) : (
                    // 연출 중에는 단계 이름·시작 가능 여부에 따른 안내 대신 중립 안내를 띄운다 (결과를 미리 알리지 않는다).
                    // 줄 모양은 그대로 두고 버튼도 늘 초대 버튼 하나로 둔다.
                    <div className="action-row">
                      <div>
                        <span className="eyebrow">{stage ? REVEAL_EYEBROW[stage] : round ? phases[round.phase] : "READY WHEN YOU ARE"}</span>
                        <p>
                          {me.leaving
                            ? "핸드 종료 후 남은 칩이 코인으로 돌아갑니다."
                            : stage
                              ? REVEAL_TEXT[stage]
                              : table.legal.can_start
                                ? "준비되셨나요? 다음 라운드를 시작하세요."
                                : round && round.phase !== "complete"
                                  ? "다른 플레이어의 진행을 기다리고 있어요."
                                  : kind === "holdem"
                                    ? "홀덤은 플레이어 2명부터 시작합니다."
                                    : "칩이 부족합니다. 퇴장 후 다시 참여해 주세요."}
                        </p>
                      </div>
                      {table.legal.can_start && !stage ? (
                        <button className="gold-button" disabled={busy} onClick={() => void act({ action: "start" })}>
                          라운드 시작
                        </button>
                      ) : (
                        <button className="secondary-button" onClick={() => void invite()}>
                          <Copy size={15} />
                          초대 명령 복사
                        </button>
                      )}
                    </div>
                  )}
                </>
              )}
            </div>
            <div className="table-footer">
              <span>
                <ShieldCheck size={14} />
                서버 검증 게임
              </span>
              <span>모든 게임은 마피아73 코인으로 진행됩니다.</span>
              <span>CASINO73 ORIGINALS</span>
            </div>
          </section>
          <aside className="side-panel">
            <Tabs defaultValue="chat" className="panel-tabs">
              <TabsList className="panel-tab-list">
                <TabsTrigger value="table">테이블</TabsTrigger>
                <TabsTrigger value="chat">
                  채팅 <span>{table.messages.filter((m) => !m.dealer).length || ""}</span>
                </TabsTrigger>
              </TabsList>
              <TabsContent value="table">
                <div className="panel-title">
                  <h2>테이블 정보</h2>
                  <Users size={17} />
                </div>
                <div className="table-stats">
                  <div>
                    <span>참가자</span>
                    <strong>
                      {table.seats.filter(Boolean).length} <em>/ {table.seats.length}</em>
                    </strong>
                  </div>
                  <div>
                    <span>{kind === "holdem" ? "블라인드" : "베팅 한도"}</span>
                    <strong>{kindStakes(kind, tableRules)}</strong>
                  </div>
                  <div>
                    <span>바이인</span>
                    <strong>
                      {fmt(tableRules.min_buy_in)} – {fmt(tableRules.max_buy_in)}
                    </strong>
                  </div>
                </div>
                <div className="dealer-note" aria-live="polite">
                  <span className="dealer-avatar">
                    <img
                      src={
                        motion && table.dealer.id === "sophia" && hasDealerVideo("sophia")
                          ? // 영상 속 딜러와 같은 얼굴 (같은 구도라 얼굴 확대 위치도 같다)
                            `${import.meta.env.BASE_URL}dealers/sophia-video.jpg`
                          : dealerPortrait(table.dealer.id)
                      }
                      onError={(e) => ((e.currentTarget as HTMLImageElement).src = DEALER_IMAGE)}
                      alt=""
                    />
                  </span>
                  <div>
                    <strong>
                      {table.dealer.name} <span>AUTO</span>
                    </strong>
                    <p>{table.narration}</p>
                  </div>
                </div>
                <div className="panel-divider" />
                <div className="panel-title">
                  <h2>다른 테이블</h2>
                  <span className="small-count">{String(tables.length).padStart(2, "0")}</span>
                </div>
                {tables.map((t) => (
                  <button key={t.id} className={`room-card ${table.id === t.id ? "selected" : ""}`} onClick={() => chooseTable(t.id)}>
                    <div className={`room-symbol ${t.kind === "blackjack" ? "ruby" : ""}`}>
                      {t.kind === "holdem" ? <Spade fill="currentColor" /> : <Diamond fill="currentColor" />}
                    </div>
                    <div>
                      <strong>{t.name}</strong>
                      <span>
                        {t.kind_text} · {t.stakes} · {t.seated}/{t.seat_count}
                        {t.id === data?.me.seated_table ? " · 착석 중" : ""}
                      </span>
                    </div>
                    <ArrowUpRight size={17} />
                  </button>
                ))}
                <div className="table-etiquette">
                  <Club size={19} />
                  <p>
                    좋은 플레이, 좋은 매너.
                    <br />
                    <span>함께 즐기는 CASINO73 테이블.</span>
                  </p>
                </div>
              </TabsContent>
              <TabsContent value="chat">
                <ChatMessages key={table.id} messages={table.messages} dealerName={table.dealer.name} />
                <form
                  className="chat-form"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    if (await act({ action: "chat", message: chat })) setChat("");
                  }}
                >
                  <input
                    aria-label="채팅 메시지"
                    placeholder={me ? "메시지 입력…" : "참여 후 채팅 가능"}
                    maxLength={240}
                    value={chat}
                    onChange={(e) => setChat(e.target.value)}
                    disabled={!me || disabled}
                  />
                  <button type="submit" aria-label="메시지 보내기" disabled={!me || disabled || !chat.trim()}>
                    <Send size={17} />
                  </button>
                </form>
                <p className="chat-notice">{table.dealer.name}는 게임 상황에 맞춰 자동으로 안내합니다. 채팅은 Discord 테이블 채널과 연결됩니다.</p>
              </TabsContent>
            </Tabs>
            <div className={`connection ${!connected ? "offline" : ""}`}>
              <i />
              {connected ? "테이블 연결됨" : expired ? "링크 만료" : "테이블 연결 중"}
              <span>코인 연동</span>
            </div>
          </aside>
        </div>
      </main>
      <Dialog
        open={join !== null}
        onOpenChange={(v) => {
          if (!v) setJoin(null);
        }}
      >
        <DialogContent className="noir-dialog">
          <DialogTitle>당신의 자리가 준비됐어요.</DialogTitle>
          <DialogDescription>
            {table.name} · {join !== null ? join + 1 : ""}번 좌석
          </DialogDescription>
          <form
            className="join-form"
            onSubmit={async (e) => {
              e.preventDefault();
              if (join !== null && (await act({ action: "join", seat: join, amount: buyin, name }))) setJoin(null);
            }}
          >
            <label>
              닉네임
              <input value={name} readOnly title="Discord 이름으로 표시됩니다." />
            </label>
            <label>
              바이인 <strong>{fmt(buyin)} 칩</strong>
            </label>
            <Slider
              aria-label="바이인 칩"
              min={tableRules.min_buy_in}
              max={maxBuyin}
              step={tableRules.buy_in_step}
              value={[buyin]}
              disabled={balance < tableRules.min_buy_in}
              onValueChange={(v) => setBuyin(v[0])}
            />
            <div className="range-labels">
              <span>{fmt(tableRules.min_buy_in)}</span>
              <span>{fmt(maxBuyin)}</span>
            </div>
            <p className="dialog-note">
              {balance < tableRules.min_buy_in
                ? `코인이 부족해요. 최소 바이인은 ${fmt(tableRules.min_buy_in)} 코인이에요. Discord에서 /출석 으로 코인을 받아 주세요.`
                : `보유 ${fmt(balance)} 코인 · 퇴장 시 남은 칩이 코인으로 돌아갑니다.`}
            </p>
            <button className="gold-button" type="submit" disabled={disabled || buyin > balance || buyin < tableRules.min_buy_in}>
              좌석 참여 · {fmt(buyin)}
            </button>
          </form>
        </DialogContent>
      </Dialog>
      <Dialog open={rules} onOpenChange={setRules}>
        <DialogContent className="noir-dialog rules-dialog">
          <DialogTitle>{kindTitle(kind)} 테이블 규칙</DialogTitle>
          <DialogDescription>CASINO73 HOUSE RULES · V1 · 마피아73 코인</DialogDescription>
          {kind === "holdem" ? (
            <div className="rules-copy">
              <h3>기본</h3>
              <ul>
                <li>
                  2~6인 노 리밋 텍사스 홀덤. 스몰 블라인드 {fmt(tableRules.small_blind)} / 빅 블라인드 {fmt(tableRules.big_blind)}. 레이크(수수료) 없음.
                </li>
                <li>
                  바이인 {fmt(tableRules.min_buy_in)}~{fmt(tableRules.max_buy_in)} 칩. 나가면 남은 칩이 코인으로 돌아옵니다.
                </li>
                <li>개인 카드 2장 + 공용 카드 5장 중 가장 강한 5장이 내 패입니다.</li>
              </ul>
              <h3>진행</h3>
              <ul>
                <li>프리플롭 → 플롭(3장) → 턴(1장) → 리버(1장). 각 단계마다 베팅.</li>
                <li>프리플롭은 빅 블라인드 다음 사람부터, 그 뒤 단계는 버튼 다음 사람부터 행동합니다. 2인이면 버튼이 스몰 블라인드를 내고 프리플롭에 먼저 행동합니다.</li>
                <li>
                  레이즈 금액은 이번 단계의 <b>총 베팅액</b>입니다. 최소 레이즈 폭은 직전 레이즈 폭 이상(처음엔 빅 블라인드). 최소 · ½ 팟 · ¾ 팟 ·
                  팟 버튼으로 금액을 바로 고를 수 있습니다.
                </li>
                <li>올인이 있으면 각자 낸 만큼만 걸린 사이드팟으로 나눠 정산합니다. 무승부는 팟을 나누고 남는 1칩은 버튼 왼쪽부터 받습니다.</li>
                <li>남은 사람이 한 명이면 카드를 공개하지 않고 팟을 가져갑니다.</li>
              </ul>
              <h3>족보 (높은 순)</h3>
              <table className="rules-table">
                <tbody>
                  <tr><th>로열 플러시</th><td>A K Q J 10 같은 무늬</td></tr>
                  <tr><th>스트레이트 플러시</th><td>연속 5장 같은 무늬</td></tr>
                  <tr><th>포 카드</th><td>같은 숫자 4장</td></tr>
                  <tr><th>풀 하우스</th><td>트리플 + 페어</td></tr>
                  <tr><th>플러시</th><td>같은 무늬 5장</td></tr>
                  <tr><th>스트레이트</th><td>연속 5장 (A는 맨 위·맨 아래 모두 가능)</td></tr>
                  <tr><th>트리플</th><td>같은 숫자 3장</td></tr>
                  <tr><th>투 페어</th><td>페어 둘</td></tr>
                  <tr><th>원 페어</th><td>같은 숫자 2장</td></tr>
                  <tr><th>하이 카드</th><td>아무 조합도 없으면 가장 높은 카드</td></tr>
                </tbody>
              </table>
              <h3>시간</h3>
              <ul>
                <li>차례마다 {tableRules.turn_ms / 1000}초. 시간이 지나면 체크할 수 있으면 체크, 아니면 폴드.</li>
                <li>2회 연속 시간 초과 → 다음 핸드부터 자리 비움(복귀 버튼으로 돌아옴). 30분 동안 아무 행동이 없으면 자동 퇴장.</li>
              </ul>
            </div>
          ) : (
            <div className="rules-copy">
              <h3>기본 (에볼루션 라이브 블랙잭 규칙)</h3>
              <ul>
                <li>8덱 슈를 이어서 씁니다. 빨간 컷 카드가 나오면 다음 라운드 전에 새로 섞습니다. 딜러는 소프트 17을 포함해 17 이상이면 스탠드.</li>
                <li>
                  메인 베팅 {fmt(tableRules.min_bet)}~{fmt(tableRules.max_bet)} ({fmt(tableRules.bet_step)} 단위). 바이인{" "}
                  {fmt(tableRules.min_buy_in)}~{fmt(tableRules.max_buy_in)} 칩.
                </li>
                <li>칩을 끌어서 메인 · PP · 21+3 자리에 놓거나, 칩을 고른 뒤 자리를 눌러 쌓습니다.</li>
                <li>딜러가 에이스를 보이면 먼저 인슈어런스를 받고, 그다음 딜러 블랙잭을 확인합니다. 10 계열을 보이면 바로 확인합니다. 딜러 블랙잭이면 그 자리에서 정산합니다.</li>
              </ul>
              <h3>배당</h3>
              <table className="rules-table">
                <tbody>
                  <tr><th>블랙잭 (처음 두 장 21)</th><td>3:2 — 베팅의 2.5배를 돌려받음. 5,000 베팅이면 12,500 (순이익 +7,500)</td></tr>
                  <tr><th>일반 승리</th><td>1:1 — 베팅의 2배를 돌려받음. 1,000 베팅이면 2,000 (순이익 +1,000)</td></tr>
                  <tr><th>푸시 (같은 점수)</th><td>원금 반환</td></tr>
                  <tr><th>딜러 블랙잭 vs 내 블랙잭</th><td>푸시</td></tr>
                  <tr><th>버스트 (22 이상)</th><td>베팅을 잃음 (딜러가 뒤에 버스트해도 동일)</td></tr>
                  <tr><th>스플릿 뒤 21</th><td>블랙잭이 아니라 일반 승리 1:1</td></tr>
                </tbody>
              </table>
              <h3>액션</h3>
              <ul>
                <li><b>히트</b> 카드 한 장 더. <b>스탠드</b> 멈춤.</li>
                <li><b>더블</b> 처음 두 장에서 베팅만큼 추가하고 딱 한 장만 받습니다. 스플릿 뒤에도 가능(에이스 스플릿은 불가).</li>
                <li><b>스플릿</b> 같은 값 두 장(10·J·Q·K는 서로 같은 값)을 두 핸드로 나누고, 두 번째 핸드에 같은 베팅을 겁니다. 한 번만 가능(최대 2핸드). 에이스 스플릿은 핸드마다 한 장씩만 받습니다.</li>
                <li>서렌더는 없습니다.</li>
              </ul>
              <h3>인슈어런스</h3>
              <ul>
                <li>딜러 앞면 카드가 에이스일 때, 메인 베팅의 절반을 걸 수 있습니다 (10초 안에 결정, 무응답은 거절).</li>
                <li>딜러가 블랙잭이면 인슈어런스 2:1 — 500 베팅 · 인슈어런스 250이면 메인 −500, 인슈어런스 +500으로 본전.</li>
                <li>딜러가 블랙잭이 아니면 인슈어런스를 잃고 게임을 계속합니다.</li>
              </ul>
              <h3>
                {sideBetsOpen
                  ? `사이드베팅 (${fmt(tableRules.side_bet_min)}~${fmt(tableRules.side_bet_max)}, 딜 직후 정산)`
                  : "사이드베팅 (이 테이블은 받지 않음)"}
              </h3>
              <table className="rules-table">
                <tbody>
                  <tr><th colSpan={2}>퍼펙트 페어 — 내 처음 두 장이 같은 숫자</th></tr>
                  <tr><th>믹스 페어</th><td>같은 숫자, 다른 색 — 6:1</td></tr>
                  <tr><th>컬러 페어</th><td>같은 숫자, 같은 색, 다른 무늬 — 12:1</td></tr>
                  <tr><th>퍼펙트 페어</th><td>같은 숫자, 같은 무늬 — 25:1</td></tr>
                  <tr><th colSpan={2}>21+3 — 내 두 장 + 딜러 앞면 카드로 3장 족보</th></tr>
                  <tr><th>플러시</th><td>같은 무늬 3장 — 5:1</td></tr>
                  <tr><th>스트레이트</th><td>연속 3장 (A 2 3, Q K A 포함) — 10:1</td></tr>
                  <tr><th>트리플</th><td>같은 숫자 3장 — 30:1</td></tr>
                  <tr><th>스트레이트 플러시</th><td>연속 3장 같은 무늬 — 40:1</td></tr>
                  <tr><th>수티드 트립스</th><td>같은 숫자·같은 무늬 3장 — 100:1</td></tr>
                </tbody>
              </table>
              <p>예: 21+3에 100을 걸고 트리플이 나오면 3,100을 돌려받습니다(순이익 +3,000). 사이드베팅은 메인 결과와 상관없이 딜 직후 정산됩니다.</p>
              <p>결과 창의 "이기셨습니다" 금액은 이긴 베팅이 돌려준 돈(원금 포함)이고, 그 아래 줄이 이번 라운드 순손익입니다.</p>
              <h3>시간</h3>
              <ul>
                <li>베팅 15초 (베팅할 수 있는 사람이 모두 확정하면 바로 딜). 마감 2초 전에 쌓아 둔 칩은 자동 확정.</li>
                <li>
                  인슈어런스 {tableRules.insurance_ms / 1000}초, 액션 {tableRules.turn_ms / 1000}초. 시간이 지나면 스탠드. 2회 연속 초과 → 자리 비움. 30분
                  무응답 → 자동 퇴장.
                </li>
              </ul>
            </div>
          )}
          <div className="rules-foot">
            <ShieldCheck size={19} />
            <p>카드와 정산은 서버가 결정합니다. 소피아는 생성 이미지와 규칙 기반 진행·음성 안내를 사용하는 자동 딜러입니다. 테이블 칩은 마피아73 코인과 실시간으로 연동됩니다.</p>
          </div>
        </DialogContent>
      </Dialog>
      <Sheet open={history} onOpenChange={setHistory}>
        <SheetContent className="history-sheet">
          <SheetHeader>
            <SheetTitle>핸드 기록</SheetTitle>
            <SheetDescription>{table.name} · 최근 20개 라운드</SheetDescription>
          </SheetHeader>
          <div className="history-list">
            {table.history.length ? (
              table.history.map((h, i) => (
                <article key={h.id}>
                  <div>
                    <span>HAND {table.history.length - i}</span>
                    <time>{historyTime.format(new Date(h.at))}</time>
                  </div>
                  <p>{h.summary}</p>
                  {h.results.length > 0 && (
                    <ul className="history-results">
                      {h.results.map((r) => (
                        <li key={r.user_id} className={resultTone(r)}>
                          <b>{resultHeadline(r, String(r.user_id) === data?.me.user_id)}</b>
                          <small>
                            순손익 {signed(r.net)} · {r.label}
                            {r.notes.length > 0 ? ` · ${r.notes.join(" · ")}` : ""}
                          </small>
                        </li>
                      ))}
                    </ul>
                  )}
                  <div className="history-cards">
                    {h.board.map((c, k) => (
                      <PlayingCard key={k} card={c} small />
                    ))}
                  </div>
                  <small>{h.id.slice(0, 8)}</small>
                </article>
              ))
            ) : (
              <div className="empty-history">
                <History />
                <p>아직 완료된 핸드가 없습니다.</p>
                <span>첫 라운드가 끝나면 여기에 기록됩니다.</span>
              </div>
            )}
          </div>
        </SheetContent>
      </Sheet>
      <Dialog open={lobby} onOpenChange={setLobby}>
        <DialogContent className="noir-dialog">
          <DialogTitle>CASINO73 테이블</DialogTitle>
          <DialogDescription>{tables.length ? "원하는 테이블을 선택하세요." : "열려 있는 테이블이 없습니다. 관리자가 Discord 카지노 패널에서 열 수 있어요."}</DialogDescription>
          {tables.map((t) => (
            <button
              key={t.id}
              className="lobby-table"
              onClick={() => {
                chooseTable(t.id);
                setLobby(false);
              }}
            >
              {t.kind === "holdem" ? <Spade /> : <Diamond />}
              <div>
                <strong>{t.name}</strong>
                <span>
                  {t.kind_text} · {t.stakes} · {t.seated} / {t.seat_count} 참여 중{t.phase_text ? ` · ${t.phase_text}` : ""}
                </span>
              </div>
              <ArrowUpRight />
            </button>
          ))}
        </DialogContent>
      </Dialog>
      <Dialog open={profile} onOpenChange={setProfile}>
        <DialogContent className="noir-dialog">
          <DialogTitle>{name}</DialogTitle>
          <DialogDescription>마피아73 코인 지갑</DialogDescription>
          <div className="wallet-summary">
            <Wallet />
            <span>
              사용 가능한 코인<strong>{fmt(balance)}</strong>
            </span>
          </div>
          <p className="dialog-note">
            코인은 Discord 봇과 실시간으로 연동됩니다. 현재 테이블에 놓인 칩은 별도 보관되며, 퇴장 시 코인으로 돌아갑니다. 코인은 /출석 과 게임 결과로 얻을 수 있고, 현금 가치·입출금
            기능은 없습니다.
          </p>
          {seatedElsewhere && (
            <button
              className="secondary-button"
              onClick={() => {
                chooseTable(seatedElsewhere.id);
                setProfile(false);
              }}
            >
              참여 중인 {seatedElsewhere.name} 보기
            </button>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
