// noir-casino app/page.tsx 의 이식본. 화면 구조·클래스·문구는 원본을 그대로 따르고,
// 데이터 소스만 봇의 /casino/api (개인 링크 세션, 여러 테이블, Discord 코인)로 바꿨다.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowUpRight,
  AudioLines,
  Check,
  CircleHelp,
  Club,
  Copy,
  Diamond,
  Grid2X2,
  History,
  LogOut,
  Plus,
  RefreshCw,
  Send,
  ShieldCheck,
  Spade,
  Users,
  Volume2,
  VolumeX,
  Wallet,
  WifiOff,
} from "lucide-react";
import { CasinoApiError, fetchState, readLink, sendCommand, wsUrl } from "./api";
import type { CasinoCommand, GameKind, StateResponse, TableRules, TableView } from "./types";
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

const fmt = (n: number) => n.toLocaleString("en-US");
const phases: Record<string, string> = {
  preflop: "프리플롭",
  flop: "플롭",
  turn: "턴",
  river: "리버",
  betting: "베팅 접수",
  playing: "플레이 중",
  complete: "라운드 종료",
};
const suits: Record<string, string> = { s: "♠", h: "♥", d: "♦", c: "♣" };
const DEALER_IMAGE = `${import.meta.env.BASE_URL}dealer.png`;
const POLL_MS = 1200;
const RECONNECT_MS = 2000;

function PlayingCard({ card, small = false }: { card?: string; small?: boolean }) {
  return (
    <span
      className={`playing-card ${small ? "small" : ""} ${!card ? "empty" : card === "??" ? "back" : ""} ${
        card && /[hd]$/.test(card) ? "red" : ""
      }`}
      aria-label={!card ? "카드 대기" : card === "??" ? "비공개 카드" : `${card[0] === "T" ? "10" : card[0]} ${suits[card[1]]}`}
    >
      {!card ? (
        <Spade />
      ) : card === "??" ? (
        <Club />
      ) : (
        <>
          <b>
            {card[0] === "T" ? "10" : card[0]}
            <i>{suits[card[1]]}</i>
          </b>
          <span>{suits[card[1]]}</span>
        </>
      )}
    </span>
  );
}

const defaultRules: TableRules = {
  small_blind: 50,
  big_blind: 100,
  min_buy_in: 5000,
  max_buy_in: 20000,
  buy_in_step: 100,
  min_bet: 100,
  max_bet: 5000,
  bet_step: 100,
  seat_count: 6,
  turn_ms: 30000,
  bet_window_ms: 15000,
};

const blankTable = (): TableView => ({
  id: "",
  kind: "holdem",
  name: "테이블 없음",
  version: 0,
  button: -1,
  my_seat: -1,
  narration: "열려 있는 테이블이 없어요. 관리자가 Discord에서 /카지노테이블생성 으로 열 수 있어요.",
  seats: Array(6).fill(null),
  round: null,
  legal: { poker: null, blackjack: null, can_bet: false, can_start: false },
  messages: [],
  history: [],
  rules: defaultRules,
});

const kindTitle = (kind: GameKind) => (kind === "holdem" ? "Texas Hold’em" : "Blackjack");
const kindRoom = (kind: GameKind) => (kind === "holdem" ? "THE SIGNATURE ROOM" : "THE BLACKJACK SALON");
const kindStakes = (kind: GameKind, rules: TableRules) =>
  kind === "holdem" ? `${fmt(rules.small_blind)} / ${fmt(rules.big_blind)}` : `${fmt(rules.min_bet)} – ${fmt(rules.max_bet)}`;
const floorStep = (value: number, step: number) => Math.floor(value / step) * step;

export default function Casino() {
  const [{ token, table: linkTable }] = useState(readLink);
  const [tableId, setTableId] = useState<string | null>(linkTable);
  const [data, setData] = useState<StateResponse | null>(null);
  const [rules, setRules] = useState(false),
    [history, setHistory] = useState(false),
    [lobby, setLobby] = useState(false),
    [profile, setProfile] = useState(false);
  const [muted, setMuted] = useState(true),
    [join, setJoin] = useState<number | null>(null),
    [buyin, setBuyin] = useState(10000);
  const [pending, setPending] = useState(false),
    [connected, setConnected] = useState(false),
    [expired, setExpired] = useState(!token),
    [error, setError] = useState(token ? "" : "개인 링크가 아닙니다. Discord에서 /카지노입장 으로 링크를 받아 주세요.");
  const [wager, setWager] = useState(100),
    [raise, setRaise] = useState(200),
    [chat, setChat] = useState(""),
    [clock, setClock] = useState(0),
    [offset, setOffset] = useState(0);
  const stateRef = useRef<StateResponse | null>(null),
    tableRef = useRef<string | null>(linkTable),
    pendingRef = useRef(false),
    expiredRef = useRef(!token),
    socketRef = useRef<WebSocket | null>(null),
    socketOpenRef = useRef(false),
    lastNarration = useRef("");
  const chatBottom = useRef<HTMLDivElement>(null);

  const accept = useCallback((value: StateResponse) => {
    const wanted = tableRef.current;
    // 보고 있던 테이블이 아직 있는데 다른 테이블 상태가 오면(전환 직후 늦은 푸시) 무시한다.
    if (value.table && wanted && value.table.id !== wanted && value.tables.some((t) => t.id === wanted)) {
      return;
    }
    if (value.table && value.table.id !== wanted) {
      tableRef.current = value.table.id;
      setTableId(value.table.id);
    } else if (!value.table && wanted) {
      tableRef.current = null;
      setTableId(null);
    }
    stateRef.current = value;
    setData(value);
    setOffset(value.server_time - Date.now());
    setConnected(true);
    setError("");
  }, []);
  const fail = useCallback((e: unknown) => {
    setConnected(false);
    if (e instanceof CasinoApiError && e.status === 401) {
      expiredRef.current = true;
      setExpired(true);
      setError(e.message);
      socketRef.current?.close();
      return;
    }
    setError(e instanceof Error ? e.message : "연결을 확인해 주세요.");
  }, []);
  const refresh = useCallback(async () => {
    if (!token || pendingRef.current || expiredRef.current) return;
    try {
      accept(await fetchState(token, tableRef.current));
    } catch (e) {
      fail(e);
    }
  }, [token, accept, fail]);

  // 첫 로드 + 웹소켓이 끊긴 동안의 폴링 (원본과 같은 1.2초).
  useEffect(() => {
    if (!token) return;
    void refresh();
    const interval = setInterval(() => {
      if (document.visibilityState === "visible" && !socketOpenRef.current) void refresh();
    }, POLL_MS);
    const visible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [token, refresh]);

  // 웹소켓 푸시: 1초마다 + 테이블이 바뀔 때 즉시.
  useEffect(() => {
    if (!token) return;
    let closed = false;
    let timer: number | null = null;
    const connect = () => {
      if (closed || expiredRef.current) return;
      const socket = new WebSocket(wsUrl(token, tableRef.current));
      socketRef.current = socket;
      socket.onopen = () => {
        socketOpenRef.current = true;
      };
      socket.onmessage = (event) => {
        try {
          accept(JSON.parse(event.data as string) as StateResponse);
        } catch {
          // 잘못된 페이로드는 무시한다.
        }
      };
      socket.onclose = () => {
        socketOpenRef.current = false;
        socketRef.current = null;
        if (!closed && !expiredRef.current) timer = window.setTimeout(connect, RECONNECT_MS);
      };
      socket.onerror = () => socket.close();
    };
    connect();
    return () => {
      closed = true;
      if (timer) clearTimeout(timer);
      socketRef.current?.close();
      socketRef.current = null;
      socketOpenRef.current = false;
    };
  }, [token, accept]);

  useEffect(() => {
    const timer = setInterval(() => setClock(Date.now()), 250);
    return () => clearInterval(timer);
  }, []);

  // 링크의 ?table= 을 현재 테이블과 맞춘다.
  useEffect(() => {
    const url = new URL(location.href);
    if (tableId) url.searchParams.set("table", tableId);
    else url.searchParams.delete("table");
    window.history.replaceState(null, "", url);
  }, [tableId]);

  const tables = data?.tables ?? [];
  const table = data?.table ?? blankTable();
  const noTable = !data?.table;
  const round = table.round,
    me = table.my_seat >= 0 ? table.seats[table.my_seat] : null;
  const anySeat = Boolean(data?.me.seated_table),
    isTurn = round?.turn === table.my_seat && table.my_seat >= 0;
  const seconds = round?.deadline ? Math.max(0, Math.ceil((round.deadline - (clock + offset)) / 1000)) : 0;
  const legal = table.legal.poker,
    bj = table.legal.blackjack;
  const balance = data?.me.coins ?? 0,
    name = data?.me.name ?? "플레이어";
  const kind = table.kind;
  const tableRules = table.rules;
  const maxBuyin = Math.max(tableRules.min_buy_in, Math.min(tableRules.max_buy_in, floorStep(balance, tableRules.buy_in_step)));

  useEffect(() => {
    if (legal) setRaise(Math.min(legal.min_raise_to, legal.max_raise_to));
  }, [legal?.min_raise_to, legal?.max_raise_to, round?.id, round?.phase]);
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
    chatBottom.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [table.messages.length]);
  useEffect(() => {
    document.title = noTable ? "NOIR — Texas Hold’em & Blackjack" : `${table.name} · NOIR`;
  }, [noTable, table.name]);

  const chooseTable = useCallback(
    (id: string) => {
      if (!id || id === tableRef.current) return;
      tableRef.current = id;
      setTableId(id);
      lastNarration.current = "";
      const socket = socketRef.current;
      if (socket && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ table: id }));
      else void refresh();
    },
    [refresh],
  );
  const act = useCallback(
    async (command: CasinoCommand) => {
      if (pendingRef.current || !stateRef.current?.table || !token) return false;
      const current = stateRef.current.table;
      pendingRef.current = true;
      setPending(true);
      try {
        accept(await sendCommand(token, current.id, command.action === "chat" ? null : current.version, command));
        return true;
      } catch (e) {
        if (e instanceof CasinoApiError && e.code === "STALE_STATE") {
          toast.error("테이블 상태가 바뀌어 다시 불러왔습니다. 다시 시도해 주세요.");
        } else if (e instanceof CasinoApiError && e.status === 401) {
          fail(e);
        } else {
          toast.error(e instanceof Error ? e.message : "연결이 끊겼습니다. 상태를 다시 확인합니다.");
        }
        return false;
      } finally {
        pendingRef.current = false;
        setPending(false);
        void refresh();
      }
    },
    [token, accept, fail, refresh],
  );

  const disabled = pending || !connected;
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
  const cards = kind === "holdem" ? (round?.board ?? []) : (round?.dealer ?? []);
  const seatedElsewhere = tables.find((t) => t.id === data?.me.seated_table && t.id !== table.id) ?? null;

  return (
    <div className="casino-app">
      <Toaster position="top-center" richColors />
      <header className="topbar">
        <a className="brand" href={location.pathname} aria-label="NOIR 홈">
          <Club fill="currentColor" />
          <span>
            NOIR<small>LIVE CASINO</small>
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
            <button onClick={() => setRules(true)}>
              <CircleHelp size={17} />
              게임 규칙
            </button>
            <button className="icon-button" aria-label="핸드 기록 열기" onClick={() => setHistory(true)}>
              <History size={18} />
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
        {error && (
          <div className="error-banner" role="status">
            <WifiOff size={17} />
            <span>{error}</span>
            {expired ? (
              <span>Discord에서 /카지노입장</span>
            ) : (
              <button onClick={() => void refresh()}>
                <RefreshCw size={15} />
                다시 연결
              </button>
            )}
          </div>
        )}
        <div className="play-layout">
          <section className="table-column">
            <div className={`game-table ${kind}`}>
              <img className="dealer-backdrop" src={DEALER_IMAGE} alt="에메랄드 테이블의 AI 딜러 소피아" />
              <div className="table-shade" />
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
                SOPHIA <span>YOUR DEALER</span>
              </div>
              <div className="board">
                <div className="board-label">{kind === "holdem" ? "TEXAS HOLD’EM" : "BLACKJACK PAYS 3 TO 2"}</div>
                {kind === "holdem" && round && (
                  <div className="pot-pill">
                    POT <b>{fmt(round.pot)}</b>
                  </div>
                )}
                <div className="community-cards">
                  {Array.from({ length: Math.max(kind === "holdem" ? 5 : 2, cards.length) }, (_, i) => (
                    <PlayingCard key={`${round?.id}-${i}-${cards[i] ?? ""}`} card={cards[i]} />
                  ))}
                </div>
                {kind === "blackjack" && round && round.dealer_total !== null && round.dealer.length !== 0 && (
                  <span className="dealer-total">딜러 {round.dealer_total}</span>
                )}
                <p className="board-status">
                  {round
                    ? round.phase === "complete"
                      ? "라운드 종료 · 다음 핸드를 시작하세요"
                      : round.phase === "betting"
                        ? `베팅 마감까지 ${seconds}초`
                        : isTurn
                          ? "당신의 차례입니다"
                          : `${table.seats[round.turn]?.name ?? "딜러"}님 차례`
                    : noTable
                      ? "열려 있는 테이블이 없습니다"
                      : "테이블에 앉아 게임을 시작하세요"}
                </p>
              </div>
              {table.seats.map((seat, i) =>
                seat ? (
                  <div
                    key={i}
                    className={`seat occupied seat-${i} ${seat.mine ? "mine" : ""} ${round?.turn === i ? "active-seat" : ""} ${
                      seat.folded ? "folded" : ""
                    }`}
                  >
                    <div className="seat-cards">
                      {kind === "holdem" ? (
                        seat.cards.map((card, k) => <PlayingCard key={k} card={card} small />)
                      ) : seat.hands.some((h) => h.cards.length > 0) ? (
                        <span className="hand-score">
                          {seat.hands.map((h, k) => (
                            <span key={h.id} className={round?.turn === i && round.hand === k ? "active-score" : ""}>
                              {h.total > 21 ? "BUST" : h.total}
                              {h.result?.includes("블랙잭") ? " BJ" : ""}
                            </span>
                          ))}
                        </span>
                      ) : null}
                    </div>
                    <span className="player-avatar">
                      {seat.name.slice(0, 1)}
                      {kind === "holdem" && table.button === i && <i>D</i>}
                    </span>
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
                    ) : round?.turn === i ? (
                      <em>{seconds}s</em>
                    ) : (
                      seat.bet > 0 && <em>{fmt(seat.bet)}</em>
                    )}
                  </div>
                ) : (
                  <button key={i} className={`seat seat-${i}`} aria-label={`${i + 1}번 빈 좌석 참여`} onClick={() => openJoin(i)} disabled={noTable}>
                    <span className="empty-avatar">
                      <Plus size={20} />
                    </span>
                    <span>빈 좌석</span>
                  </button>
                ),
              )}
              <span className="felt-mark">N O I R</span>
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
                  <div className="my-hand-row">
                    <div className="my-hand-info">
                      <span className="eyebrow">{kind === "holdem" ? "YOUR HAND" : "YOUR HANDS"}</span>
                      <div className="my-cards">
                        {kind === "holdem" ? (
                          me.cards.length ? (
                            me.cards.map((c, i) => <PlayingCard key={i} card={c} small />)
                          ) : (
                            <span>다음 핸드 대기</span>
                          )
                        ) : me.hands.length ? (
                          me.hands.map((h, i) => (
                            <div className={`bj-hand ${round?.hand === i && isTurn ? "selected-hand" : ""}`} key={h.id}>
                              <div>
                                {h.cards.map((c, k) => (
                                  <PlayingCard key={k} card={c} small />
                                ))}
                              </div>
                              <span>
                                {h.total > 21 ? "버스트" : h.cards.length ? `${h.soft ? "소프트 " : ""}${h.total}` : "베팅 완료"} · {h.result ?? fmt(h.bet)}
                              </span>
                            </div>
                          ))
                        ) : (
                          <span>베팅을 준비해 주세요</span>
                        )}
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
                      <div className="action-buttons">
                        <button className="fold-button" disabled={disabled} onClick={() => void act({ action: "fold" })}>
                          폴드
                        </button>
                        <button className="secondary-button" disabled={disabled} onClick={() => void act({ action: legal.can_check ? "check" : "call" })}>
                          {legal.can_check ? "체크" : `콜 ${fmt(legal.to_call)}`}
                        </button>
                        <button className="gold-button" disabled={disabled || !legal.can_raise} onClick={() => void act({ action: "raise", amount: raise })}>
                          {raise === legal.max_raise_to ? "올인" : "레이즈"} {fmt(raise)}
                        </button>
                      </div>
                    </div>
                  ) : kind === "blackjack" && bj ? (
                    <div className="blackjack-actions">
                      <button className="gold-button" disabled={disabled} onClick={() => void act({ action: "hit" })}>
                        <Plus size={16} />
                        히트
                      </button>
                      <button className="secondary-button" disabled={disabled} onClick={() => void act({ action: "stand" })}>
                        <Check size={16} />
                        스탠드
                      </button>
                      <button className="secondary-button" disabled={disabled || !bj.can_double} onClick={() => void act({ action: "double" })}>
                        더블 ×2
                      </button>
                      <button className="secondary-button" disabled={disabled || !bj.can_split} onClick={() => void act({ action: "split" })}>
                        스플릿
                      </button>
                      <button className="text-button" disabled={disabled || !bj.can_surrender} onClick={() => void act({ action: "surrender" })}>
                        서렌더
                      </button>
                    </div>
                  ) : table.legal.can_bet ? (
                    <div className="betting-controls">
                      <div className="chip-options">
                        {[100, 500, 1000, 2500, 5000].map((v) => (
                          <button
                            key={v}
                            className={`chip chip-${v} ${wager === v ? "chosen" : ""}`}
                            disabled={disabled || v > me.stack}
                            onClick={() => setWager(v)}
                            aria-label={`${fmt(v)} 칩 선택`}
                          >
                            {v >= 1000 ? `${v / 1000}K` : v}
                          </button>
                        ))}
                      </div>
                      <button className="gold-button" disabled={disabled || wager > me.stack || seconds <= 0} onClick={() => void act({ action: "bet", amount: wager })}>
                        {fmt(wager)} 베팅 <span className="button-timer">{seconds}s</span>
                      </button>
                    </div>
                  ) : (
                    <div className="action-row">
                      <div>
                        <span className="eyebrow">{round ? phases[round.phase] : "READY WHEN YOU ARE"}</span>
                        <p>
                          {me.leaving
                            ? "핸드 종료 후 남은 칩이 코인으로 돌아갑니다."
                            : table.legal.can_start
                              ? "준비되셨나요? 다음 라운드를 시작하세요."
                              : round && round.phase !== "complete"
                                ? "다른 플레이어의 진행을 기다리고 있어요."
                                : kind === "holdem"
                                  ? "홀덤은 플레이어 2명부터 시작합니다."
                                  : "칩이 부족합니다. 퇴장 후 다시 참여해 주세요."}
                        </p>
                      </div>
                      {table.legal.can_start ? (
                        <button className="gold-button" disabled={disabled} onClick={() => void act({ action: "start" })}>
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
              <span>NOIR ORIGINALS</span>
            </div>
          </section>
          <aside className="side-panel">
            <Tabs defaultValue="table" className="panel-tabs">
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
                    <span>{kind === "holdem" ? "블라인드" : "최소 베팅"}</span>
                    <strong>{kind === "holdem" ? kindStakes(kind, tableRules) : fmt(tableRules.min_bet)}</strong>
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
                    <img src={DEALER_IMAGE} alt="" />
                  </span>
                  <div>
                    <strong>
                      소피아 <span>AUTO</span>
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
                        {t.kind_text} · {t.seated}/{t.seat_count}
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
                    <span>함께 즐기는 NOIR 테이블.</span>
                  </p>
                </div>
              </TabsContent>
              <TabsContent value="chat">
                <div className="chat-messages" role="log" aria-label="테이블 채팅">
                  {table.messages.length === 0 ? (
                    <div className="chat-empty">
                      <AudioLines />
                      <p>
                        소피아와 함께하는 테이블.
                        <br />
                        첫 인사를 건네보세요.
                      </p>
                    </div>
                  ) : (
                    table.messages.map((m) => (
                      <div key={m.id} className={`chat-message ${m.dealer ? "from-dealer" : ""}`}>
                        <strong>
                          {m.name}
                          {m.dealer && <span>DEALER</span>}
                          {m.from_discord && <span>DISCORD</span>}
                        </strong>
                        <p>{m.text}</p>
                      </div>
                    ))
                  )}
                  <div ref={chatBottom} />
                </div>
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
                <p className="chat-notice">소피아는 게임 상황에 맞춰 자동으로 안내합니다. 채팅은 Discord 테이블 채널과 연결됩니다.</p>
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
          <DialogDescription>NOIR HOUSE RULES · V1 · 마피아73 코인</DialogDescription>
          {kind === "holdem" ? (
            <div className="rules-copy">
              <p>2–6인 노 리밋 홀덤. 스몰 블라인드 50 / 빅 블라인드 100. 레이크 없음.</p>
              <p>개인 카드 2장과 공용 카드 5장으로 가장 강한 5장 조합을 만듭니다. 프리플롭 → 플롭 → 턴 → 리버 순서로 베팅합니다.</p>
              <p>
                레이즈 금액은 해당 라운드의 <b>총 베팅액</b>입니다. 최소 레이즈는 직전의 완전한 레이즈 폭을 따릅니다. 짧은 올인은 레이즈 권한을 자동으로 다시 열지
                않습니다.
              </p>
              <p>올인 시 사이드팟을 각각 정산합니다. 무승부는 분배하고, 남는 1칩은 버튼 왼쪽부터 지급합니다.</p>
              <p>제한시간 30초. 시간 초과 시 무료 체크 또는 폴드. 2회 연속 시간 초과 시 다음 핸드부터 자리 비움.</p>
            </div>
          ) : (
            <div className="rules-copy">
              <p>6덱 · 매 라운드 새 셔플 · 딜러는 소프트 17 포함 17 이상에서 스탠드.</p>
              <p>베팅 100–5,000, 100 단위. 블랙잭 3:2, 일반 승리 1:1, 무승부 원금 반환. 딜러 블랙잭은 플레이어 액션 전에 확인합니다.</p>
              <p>첫 2장에서 더블 가능. 같은 값 카드 스플릿, 최대 4핸드. 스플릿 후 더블 가능. 에이스 스플릿은 1장만 추가, 재스플릿 불가. 스플릿 21은 일반 승리 배당.</p>
              <p>첫 2장, 스플릿 전 서렌더 시 베팅 절반 반환. 보험·사이드베팅 없음. 베팅창 15초, 액션 30초. 시간 초과는 스탠드.</p>
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
                    <time>{new Date(h.at).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" })}</time>
                  </div>
                  <p>{h.summary}</p>
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
          <DialogTitle>NOIR 테이블</DialogTitle>
          <DialogDescription>{tables.length ? "원하는 테이블을 선택하세요." : "열려 있는 테이블이 없습니다. 관리자가 Discord에서 /카지노테이블생성 으로 열 수 있어요."}</DialogDescription>
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
                  {t.kind_text} · {t.seated} / {t.seat_count} 참여 중{t.phase_text ? ` · ${t.phase_text}` : ""}
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
