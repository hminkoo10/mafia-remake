// 마피아증권: 종목 목록, 차트, 호가·주문, 잔고·미체결·체결, 공모주, 회사 경영.
// 개인 링크(/stocks/<토큰>)로 들어오며, 상태는 2초마다 새로 받는다.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock, FastForward, RefreshCw, Search, Wallet, WifiOff } from "lucide-react";
import { AccountTabs } from "./AccountTabs";
import { ApiError, fetchCandles, fetchStockState, placeOrder, readToken } from "./api";
import { CandleChart } from "./CandleChart";
import { CompanyInfo } from "./CompanyInfo";
import { OrderBook, OrderForm } from "./OrderPanel";
import { arrow, compactWon, countdownText, durationText, msToNextGameDay, pctText, relativeText, timeText, tone, won } from "./format";
import type { ActionResponse, Candle, CandleRange, CompanyDetail, CompanySummary, StockState } from "./types";
import { Sheet, Toaster, toast } from "./ui";

const POLL_MS = 2000;
/** 탭이 가려져 있을 때는 드물게 받는다. */
const HIDDEN_POLL_MS = 15000;

/** 서버 시각 기준 지금 (1초마다 다시 그린다). */
function useServerNow(offset: number): number {
  const [now, setNow] = useState(() => Date.now() + offset);
  useEffect(() => {
    setNow(Date.now() + offset);
    const id = window.setInterval(() => setNow(Date.now() + offset), 1000);
    return () => window.clearInterval(id);
  }, [offset]);
  return now;
}

function useMedia(query: string): boolean {
  const [match, setMatch] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const list = window.matchMedia(query);
    const update = () => setMatch(list.matches);
    update();
    list.addEventListener("change", update);
    return () => list.removeEventListener("change", update);
  }, [query]);
  return match;
}

/** 고른 종목을 주소에 남긴다 (새로 고쳐도 같은 종목). */
function rememberCode(code: string) {
  const url = new URL(window.location.href);
  url.searchParams.set("code", code);
  window.history.replaceState(null, "", url.toString());
}

export function Logo() {
  return (
    <svg viewBox="0 0 64 64" aria-hidden="true" className="logo">
      <rect width="64" height="64" rx="16" fill="currentColor" />
      <path d="M14 42 L26 30 L35 37 L50 20" fill="none" stroke="#fff" strokeWidth="6" strokeLinecap="round" strokeLinejoin="round" />
      <circle cx="50" cy="20" r="4.5" fill="#fff" />
    </svg>
  );
}

export default function App() {
  const [token] = useState(readToken);
  const [code, setCode] = useState<string | null>(() => new URLSearchParams(window.location.search).get("code"));
  const [state, setState] = useState<StockState | null>(null);
  const [offset, setOffset] = useState(0);
  const [error, setError] = useState(token ? "" : "개인 링크가 아닙니다. Discord에서 /주식 증권 으로 링크를 받아 주세요.");
  const [expired, setExpired] = useState(!token);
  const [busy, setBusy] = useState(false);
  const [listOpen, setListOpen] = useState(false);
  const [range, setRange] = useState<CandleRange>("minute");
  const [chartIndex, setChartIndex] = useState(false);
  const [candles, setCandles] = useState<{ key: string; list: Candle[] } | null>(null);
  const [pick, setPick] = useState<{ price: number; n: number } | null>(null);
  const codeRef = useRef(code);
  const polling = useRef(false);
  const pollQueued = useRef(false);
  const busyRef = useRef(false);
  /** 요청 순번: 늦게 도착한 옛 응답이 새 상태를 덮지 않게 한다. */
  const requestSeq = useRef(0);
  const appliedSeq = useRef(0);
  const phone = useMedia("(max-width: 760px)");
  const now = useServerNow(offset);

  const selectCode = useCallback((next: string) => {
    codeRef.current = next;
    setCode(next);
    rememberCode(next);
  }, []);

  const apply = useCallback(
    (next: StockState, seq: number) => {
      if (seq < appliedSeq.current) return;
      appliedSeq.current = seq;
      setState(next);
      setOffset(next.server_time - Date.now());
      const shown = next.selected?.summary.code;
      if (shown && shown !== codeRef.current) {
        selectCode(shown);
      }
    },
    [selectCode],
  );

  const fail = useCallback((e: unknown, loud: boolean) => {
    const message = e instanceof Error ? e.message : "요청을 처리하지 못했습니다.";
    if (e instanceof ApiError && e.status === 401) {
      setExpired(true);
      setError(message);
      return;
    }
    if (loud) toast.error(message);
    else setError(message);
  }, []);

  const load = useCallback(async () => {
    if (!token) return;
    if (polling.current) {
      pollQueued.current = true;
      return;
    }
    polling.current = true;
    const requested = codeRef.current;
    const seq = ++requestSeq.current;
    try {
      const next = await fetchStockState(token, requested);
      // 받는 사이에 다른 종목을 골랐으면 버린다 (바로 다시 받는다).
      if (requested !== codeRef.current) {
        pollQueued.current = true;
      } else {
        apply(next, seq);
        setError("");
      }
    } catch (e) {
      fail(e, false);
    } finally {
      polling.current = false;
      if (pollQueued.current) {
        pollQueued.current = false;
        void load();
      }
    }
  }, [token, apply, fail]);

  useEffect(() => {
    if (!token || expired) return;
    let last = 0;
    const tick = () => {
      const due = document.hidden ? HIDDEN_POLL_MS : POLL_MS;
      if (Date.now() - last >= due) {
        last = Date.now();
        void load();
      }
    };
    tick();
    const id = window.setInterval(tick, 500);
    const onVisible = () => {
      if (!document.hidden) {
        last = 0;
        tick();
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(id);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [token, expired, load]);

  const choose = useCallback(
    (next: string) => {
      setListOpen(false);
      setChartIndex(false);
      if (next === codeRef.current) return;
      selectCode(next);
      void load();
    },
    [selectCode, load],
  );

  /** 코인이 오가는 작업: 응답의 새 상태를 바로 쓰고 안내를 띄운다. */
  const act = useCallback(
    async <T,>(work: (token: string) => Promise<ActionResponse<T>>): Promise<ActionResponse<T> | null> => {
      if (!token || busyRef.current) return null;
      busyRef.current = true;
      setBusy(true);
      const seq = ++requestSeq.current;
      try {
        const response = await work(token);
        apply(response.state, seq);
        toast.success(response.message);
        return response;
      } catch (e) {
        fail(e, true);
        return null;
      } finally {
        busyRef.current = false;
        setBusy(false);
      }
    },
    [token, apply, fail],
  );

  // 봉: 분봉은 15초, 일봉은 1분마다 새로 받는다 (그 사이 마지막 봉은 현재가로 고친다).
  const chartCode = chartIndex ? "INDEX" : code;
  const candleKey = chartCode ? `${chartCode}:${range}` : "";
  useEffect(() => {
    if (!token || !chartCode || expired) return;
    let cancelled = false;
    const key = `${chartCode}:${range}`;
    const pull = async () => {
      try {
        const response = await fetchCandles(token, chartCode, range);
        if (!cancelled) setCandles({ key, list: response.candles });
      } catch {
        // 다음 차례에 다시 받는다.
      }
    };
    void pull();
    const id = window.setInterval(() => {
      if (!document.hidden) void pull();
    }, range === "minute" ? 15000 : 60000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [token, chartCode, range, expired]);

  const selected = state?.selected && state.selected.summary.code === code ? state.selected : null;
  // 거래되는 종목만 마지막 봉을 현재가로 고친다 (비상장·청약 중인 회사에 없는 봉을 만들지 않게).
  const trading = selected ? ["상장", "정리매매"].includes(selected.summary.status) : false;
  const livePrice = chartIndex ? (state ? Math.round(state.market.index * 100) : null) : trading ? selected!.summary.price : null;
  const liveCandles = useMemo(() => {
    if (!candles || candles.key !== candleKey) return [];
    const list = candles.list;
    if (!livePrice || !state) return list;
    const bucketMs = range === "minute" ? 60_000 : state.rules.day_ms;
    const bucket = Math.floor(state.server_time / bucketMs) * bucketMs;
    const last = list[list.length - 1];
    if (last && last.t > bucket) return list;
    if (last && last.t === bucket) {
      if (last.c === livePrice) return list;
      return [...list.slice(0, -1), { ...last, c: livePrice, h: Math.max(last.h, livePrice), l: Math.min(last.l, livePrice) }];
    }
    const open = last?.c ?? livePrice;
    return [...list, { t: bucket, o: open, h: Math.max(open, livePrice), l: Math.min(open, livePrice), c: livePrice, v: 0 }];
  }, [candles, candleKey, livePrice, range, state]);

  const market = state?.market;
  const indexDelta = market ? market.index - market.index_prev : 0;
  const indexBp = market && market.index_prev > 0 ? Math.round((indexDelta / market.index_prev) * 10000) : 0;
  const marketHalted = market ? market.halted_until > now : false;

  return (
    <div className="app">
      <Toaster />
      <header className="topbar">
        <div className="topbar-inner">
          <a className="brand" href={window.location.pathname} aria-label="마피아증권 처음 화면">
            <Logo />
            <span>마피아증권</span>
          </a>
          {market && (
            <button className="index-ticker" onClick={() => setChartIndex(true)} title="종합지수 차트 보기">
              <span>마피아 종합</span>
              <strong className={tone(indexDelta)}>{market.index.toFixed(2)}</strong>
              <em className={tone(indexDelta)}>
                {arrow(indexDelta)} {Math.abs(indexDelta).toFixed(2)} ({pctText(indexBp)})
              </em>
            </button>
          )}
          <div className="topbar-right">
            {state && state.time_shift_ms > 0 && (
              <div className="chip shift-chip" title={`관리자가 게임일을 넘겨 시장 시계가 실제보다 ${durationText(state.time_shift_ms)} 앞서 있습니다.`}>
                <FastForward size={14} />
                <span>시장 시각</span>
                <strong>{timeText(now)}</strong>
              </div>
            )}
            {state && (
              <div
                className="chip day-chip"
                title={`게임 하루 = 실제 ${Math.round(state.rules.day_ms / 60000)}분. 날이 바뀌면 가격제한폭 기준(전일 종가)이 새로 잡힙니다.`}
              >
                <Clock size={14} />
                <span>게임일 마감까지</span>
                <strong>{countdownText(msToNextGameDay(now, state.rules.day_ms))}</strong>
              </div>
            )}
            <div className="chip coin-chip" aria-label="주식에 쓸 수 있는 코인">
              <Wallet size={15} />
              <strong>{state ? won(state.me.coins) : "—"}</strong>
              <span>코인</span>
            </div>
            {state && <div className="chip user-chip">{state.me.name}</div>}
          </div>
        </div>
      </header>
      <main className="page">
        {error && (
          <div className="banner error">
            <WifiOff size={17} />
            <span>{error}</span>
            {!expired && (
              <button className="text-button" onClick={() => void load()}>
                <RefreshCw size={14} /> 다시 시도
              </button>
            )}
          </div>
        )}
        {state && !state.rules.enabled && <div className="banner">지금은 주식 시장이 닫혀 있습니다. 시세는 볼 수 있지만 주문할 수 없습니다.</div>}
        {marketHalted && market && <div className="banner warn">서킷브레이커 발동: 모든 거래가 {relativeText(market.halted_until, now)} 다시 열립니다.</div>}
        {!state && !error && <div className="loading">시세를 불러오는 중…</div>}
        {state && (
          <>
            <div className="layout">
              <aside className="card list-card">
                <StockList companies={state.market.companies} selected={code} onSelect={choose} />
              </aside>
              <section className="col-main">
                {selected ? (
                  <QuoteHeader detail={selected} onOpenList={() => setListOpen(true)} />
                ) : (
                  <div className="card quote loading">종목을 불러오는 중…</div>
                )}
                <div className="card chart-card">
                  <div className="chart-bar">
                    <div className="seg" role="group" aria-label="차트 대상">
                      <button className={chartIndex ? "" : "on"} onClick={() => setChartIndex(false)}>
                        {selected?.summary.name ?? "종목"}
                      </button>
                      <button className={chartIndex ? "on" : ""} onClick={() => setChartIndex(true)}>
                        종합지수
                      </button>
                    </div>
                    <div className="seg" role="group" aria-label="봉 단위">
                      <button className={range === "minute" ? "on" : ""} onClick={() => setRange("minute")} title="실제 1분 봉">
                        분봉
                      </button>
                      <button className={range === "day" ? "on" : ""} onClick={() => setRange("day")} title="게임 하루(실제 1시간) 봉">
                        일봉
                      </button>
                    </div>
                  </div>
                  <CandleChart
                    candles={liveCandles}
                    range={range}
                    scale={chartIndex ? 100 : 1}
                    decimals={chartIndex ? 2 : 0}
                    prevClose={chartIndex ? (market ? Math.round(market.index_prev * 100) : undefined) : selected?.summary.prev_close}
                    height={phone ? 250 : 330}
                  />
                </div>
                {selected && <CompanyInfo detail={selected} news={state.selected_news} now={now} />}
              </section>
              <section className="col-trade">
                {selected && (
                  <>
                    <OrderBook
                      detail={selected}
                      levels={phone ? 5 : 10}
                      myOrders={state.account.orders.filter((order) => order.code === selected.summary.code)}
                      onPick={(price) => setPick((prev) => ({ price, n: (prev?.n ?? 0) + 1 }))}
                    />
                    <OrderForm
                      key={selected.summary.code}
                      detail={selected}
                      state={state}
                      pick={pick}
                      busy={busy}
                      onSubmit={(order) => act((t) => placeOrder(t, order))}
                    />
                  </>
                )}
              </section>
            </div>
            <AccountTabs state={state} now={now} busy={busy} act={act} onSelect={choose} />
          </>
        )}
      </main>
      <Sheet open={listOpen} onClose={() => setListOpen(false)} title="종목" description="종목을 누르면 시세와 주문 화면이 바뀝니다.">
        {state && <StockList companies={state.market.companies} selected={code} onSelect={choose} />}
      </Sheet>
    </div>
  );
}

// ------------------------------------------------------------ 종목 목록

function StockList({
  companies,
  selected,
  onSelect,
}: {
  companies: CompanySummary[];
  selected: string | null;
  onSelect: (code: string) => void;
}) {
  const [query, setQuery] = useState("");
  const q = query.trim();
  const shown = q ? companies.filter((c) => c.name.includes(q) || c.code.includes(q)) : companies;
  const system = shown.filter((c) => !c.player);
  const players = shown.filter((c) => c.player);
  const row = (c: CompanySummary) => (
    <button key={c.code} className={`stock-row ${c.code === selected ? "selected" : ""}`} onClick={() => onSelect(c.code)}>
      <span className="name">
        <b>{c.name}</b>
        <small>
          {c.code} · {c.sector}
          {c.status !== "상장" ? ` · ${c.status}` : ""}
          {c.managed ? " · 관리" : ""}
          {c.halted ? " · 정지" : ""}
        </small>
      </span>
      <span className={`price ${tone(c.change_bp)}`}>
        <b>{won(c.price)}</b>
        <small>{pctText(c.change_bp)}</small>
      </span>
    </button>
  );
  return (
    <div className="stock-list">
      <label className="search">
        <Search size={15} />
        <input value={query} placeholder="종목명·코드 검색" onChange={(e) => setQuery(e.target.value)} />
      </label>
      <div className="stock-rows">
        {system.map(row)}
        {players.length > 0 && <h3>플레이어 회사</h3>}
        {players.map(row)}
        {shown.length === 0 && <p className="empty">검색 결과가 없습니다</p>}
      </div>
    </div>
  );
}

// ------------------------------------------------------------ 시세 머리

const limitText = (value: number) => (value >= 1e15 ? "제한 없음" : won(value));

function QuoteHeader({ detail, onOpenList }: { detail: CompanyDetail; onOpenList: () => void }) {
  const s = detail.summary;
  const delta = s.price - s.prev_close;
  return (
    <div className="card quote">
      <div className="quote-title">
        <button className="list-toggle" onClick={onOpenList} aria-label="종목 목록 열기">
          <Search size={15} /> 종목
        </button>
        <h1>{s.name}</h1>
        <span className="code">{s.code}</span>
        <span className="tag">{s.sector}</span>
        {s.player && <span className="tag player">플레이어 회사{s.founder_name ? ` · ${s.founder_name}` : ""}</span>}
        {s.status !== "상장" && <span className="tag warn">{s.status}</span>}
        {s.managed && <span className="tag warn">관리종목</span>}
        {s.halted && <span className="tag warn">거래정지</span>}
      </div>
      <div className="quote-price">
        <strong className={tone(delta)}>{won(s.price)}</strong>
        <span className={tone(delta)}>
          {arrow(delta)} {won(Math.abs(delta))} ({pctText(s.change_bp)})
        </span>
      </div>
      <dl className="quote-stats">
        <div>
          <dt>시가</dt>
          <dd>{won(detail.open)}</dd>
        </div>
        <div>
          <dt>고가</dt>
          <dd className="up">{won(detail.high)}</dd>
        </div>
        <div>
          <dt>저가</dt>
          <dd className="down">{won(detail.low)}</dd>
        </div>
        <div>
          <dt>전일 종가</dt>
          <dd>{won(s.prev_close)}</dd>
        </div>
        <div>
          <dt>상한가</dt>
          <dd className="up">{limitText(detail.upper_limit)}</dd>
        </div>
        <div>
          <dt>하한가</dt>
          <dd className="down">{won(detail.lower_limit)}</dd>
        </div>
        <div>
          <dt>거래량</dt>
          <dd>{won(s.volume)}</dd>
        </div>
        <div>
          <dt>시가총액</dt>
          <dd>{compactWon(s.market_cap)}</dd>
        </div>
      </dl>
    </div>
  );
}
