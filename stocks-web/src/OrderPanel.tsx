// 호가창(10단계)과 주문 입력. 호가를 누르면 그 가격이 지정가로 들어간다.
import { useEffect, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { affordableQty, floorTick, orderEstimate, parseAmount, pctText, ppmText, relativeText, tickDown, tickUp, tone, won } from "./format";
import type { BookLevel, CompanyDetail, OrderView, Side, StockState } from "./types";

// ------------------------------------------------------------ 호가창

function emptyBookText(detail: CompanyDetail): string {
  const s = detail.summary;
  if (s.status === "비상장") return "비상장 회사입니다. 대표가 공모 청약을 열면 상장됩니다.";
  if (s.status === "공모 청약") return "공모 청약 중입니다. 아래 공모주 탭에서 청약할 수 있습니다.";
  if (s.status === "청산 대기") return "청산을 기다리는 중이라 거래가 멈췄습니다.";
  if (s.status === "상장폐지") return "상장폐지된 회사입니다.";
  return "지금은 호가가 없습니다.";
}

export function OrderBook({
  detail,
  levels,
  myOrders,
  onPick,
}: {
  detail: CompanyDetail;
  /** 위아래로 보여 줄 호가 수 (휴대폰은 5단계). */
  levels: number;
  myOrders: OrderView[];
  onPick: (price: number) => void;
}) {
  const s = detail.summary;
  const asks = detail.book.asks.slice(0, levels).reverse();
  const bids = detail.book.bids.slice(0, levels);
  const max = Math.max(1, ...asks.map((level) => level.qty), ...bids.map((level) => level.qty));
  const mine = new Map<number, number>();
  for (const order of myOrders) mine.set(order.limit, (mine.get(order.limit) ?? 0) + order.remaining);
  const askTotal = asks.reduce((sum, level) => sum + level.qty, 0);
  const bidTotal = bids.reduce((sum, level) => sum + level.qty, 0);

  const row = (level: BookLevel, side: "ask" | "bid") => {
    const bp = s.prev_close > 0 ? Math.round(((level.price - s.prev_close) * 10000) / s.prev_close) : 0;
    const bar = (
      <>
        <i style={{ width: `${(level.qty / max) * 100}%` }} />
        <em>{won(level.qty)}</em>
        {level.players > 0 && (
          <b className="book-players" title={`그중 플레이어 주문 ${won(level.players)}주`}>
            P
          </b>
        )}
      </>
    );
    const own = mine.get(level.price);
    return (
      <button
        key={`${side}-${level.price}`}
        className={`book-row ${side} ${level.price === s.price ? "current" : ""}`}
        onClick={() => onPick(level.price)}
        title="이 가격으로 지정가 주문"
      >
        <span className="book-qty">{side === "ask" && bar}</span>
        <span className={`book-price ${tone(level.price - s.prev_close)}`}>
          <strong>{won(level.price)}</strong>
          <small>{pctText(bp)}</small>
          {own !== undefined && (
            <b className="book-mine" title={`내 주문 ${won(own)}주`}>
              내 {won(own)}
            </b>
          )}
        </span>
        <span className="book-qty">{side === "bid" && bar}</span>
      </button>
    );
  };

  return (
    <div className="card book">
      <div className="card-title">
        <h2>호가</h2>
        <span className="muted">누르면 지정가로</span>
      </div>
      {asks.length + bids.length === 0 ? (
        <p className="empty">{emptyBookText(detail)}</p>
      ) : (
        <>
          <div className="book-head">
            <span>매도잔량</span>
            <span>호가</span>
            <span>매수잔량</span>
          </div>
          <div className="book-rows">
            {asks.map((level) => row(level, "ask"))}
            {bids.map((level) => row(level, "bid"))}
          </div>
          <div className="book-foot">
            <span>{won(askTotal)}</span>
            <span>잔량 합계</span>
            <span>{won(bidTotal)}</span>
          </div>
        </>
      )}
    </div>
  );
}

// ------------------------------------------------------------ 주문

export interface OrderInput {
  code: string;
  side: Side;
  qty: number;
  price?: number;
}

const PERCENTS = [
  { label: "10%", part: 0.1 },
  { label: "25%", part: 0.25 },
  { label: "50%", part: 0.5 },
  { label: "최대", part: 1 },
];

export function OrderForm({
  detail,
  state,
  pick,
  busy,
  onSubmit,
}: {
  detail: CompanyDetail;
  state: StockState;
  pick: { price: number; n: number } | null;
  busy: boolean;
  onSubmit: (order: OrderInput) => Promise<unknown>;
}) {
  const s = detail.summary;
  const rules = state.rules;
  const [side, setSide] = useState<Side>("buy");
  const [kind, setKind] = useState<"limit" | "market">("limit");
  const [priceText, setPriceText] = useState(() => won(s.price));
  const [qtyText, setQtyText] = useState("");
  const lastPick = useRef(pick?.n ?? 0);

  useEffect(() => {
    if (pick && pick.n !== lastPick.current) {
      lastPick.current = pick.n;
      setKind("limit");
      setPriceText(won(pick.price));
    }
  }, [pick]);

  const price = parseAmount(priceText);
  const qty = parseAmount(qtyText);
  const position = state.account.positions.find((p) => p.code === s.code);
  const available = position?.available ?? 0;
  const bestAsk = detail.book.asks[0]?.price ?? s.price;
  const bestBid = detail.book.bids[0]?.price ?? s.price;
  const unit = kind === "limit" ? price : side === "buy" ? bestAsk : bestBid;
  const maxQty = side === "buy" ? affordableQty(state.me.coins, unit, rules.fee_ppm) : available;
  const estimate = orderEstimate(side, unit, qty, rules);
  const tradable = rules.enabled && detail.book.asks.length + detail.book.bids.length > 0 && !s.halted;
  const outOfBand = kind === "limit" && price > 0 && (price < detail.lower_limit || price > detail.upper_limit);

  let warning = "";
  if (!rules.enabled) warning = "지금은 주식 시장이 닫혀 있습니다.";
  else if (!tradable) warning = s.halted ? "거래가 잠시 멈춘 종목입니다." : `지금은 거래할 수 없는 종목입니다 (${s.status}).`;
  else if (kind === "limit" && price <= 0) warning = "가격을 입력하세요.";
  else if (outOfBand) warning = `오늘 가격제한폭(${won(detail.lower_limit)} ~ ${won(detail.upper_limit)}) 밖입니다.`;
  else if (side === "sell" && qty > available) warning = `팔 수 있는 주식은 ${won(available)}주입니다.`;
  else if (side === "buy" && qty > 0 && estimate.total > state.me.coins)
    warning = kind === "limit" ? "코인이 모자라 살 수 있는 만큼만 주문됩니다." : "가진 코인만큼만 체결됩니다.";

  const blocked = !tradable || qty <= 0 || outOfBand || (kind === "limit" && price <= 0) || (side === "sell" && qty > available);

  const stepPrice = (up: boolean) => {
    const base = price > 0 ? price : s.price;
    setPriceText(won(up ? tickUp(base) : tickDown(base)));
  };
  const stepQty = (delta: number) => setQtyText(won(Math.max(0, qty + delta)));

  const submit = async () => {
    const order: OrderInput = { code: s.code, side, qty };
    if (kind === "limit") order.price = floorTick(price);
    const done = await onSubmit(order);
    if (done) setQtyText("");
  };

  return (
    <div className={`card order ${side}`}>
      <div className="order-sides" role="tablist" aria-label="매수·매도">
        <button role="tab" aria-selected={side === "buy"} className={side === "buy" ? "on buy" : ""} onClick={() => setSide("buy")}>
          매수
        </button>
        <button role="tab" aria-selected={side === "sell"} className={side === "sell" ? "on sell" : ""} onClick={() => setSide("sell")}>
          매도
        </button>
      </div>
      <div className="seg order-kind" role="group" aria-label="주문 종류">
        <button className={kind === "limit" ? "on" : ""} onClick={() => setKind("limit")}>
          지정가
        </button>
        <button className={kind === "market" ? "on" : ""} onClick={() => setKind("market")}>
          시장가
        </button>
      </div>
      <label className="field">
        <span>가격</span>
        <div className="stepper">
          <button type="button" aria-label="한 호가 아래" disabled={kind === "market"} onClick={() => stepPrice(false)}>
            <Minus size={15} />
          </button>
          <input
            inputMode="numeric"
            disabled={kind === "market"}
            value={kind === "market" ? "시장가" : priceText}
            onChange={(e) => setPriceText(e.target.value)}
            onBlur={() => price > 0 && setPriceText(won(floorTick(price)))}
          />
          <button type="button" aria-label="한 호가 위" disabled={kind === "market"} onClick={() => stepPrice(true)}>
            <Plus size={15} />
          </button>
        </div>
      </label>
      <label className="field">
        <span>수량</span>
        <div className="stepper">
          <button type="button" aria-label="1주 빼기" onClick={() => stepQty(-1)}>
            <Minus size={15} />
          </button>
          <input inputMode="numeric" placeholder="0" value={qtyText} onChange={(e) => setQtyText(e.target.value)} />
          <button type="button" aria-label="1주 더하기" onClick={() => stepQty(1)}>
            <Plus size={15} />
          </button>
        </div>
      </label>
      <div className="percents">
        {PERCENTS.map((p) => (
          <button key={p.label} type="button" disabled={maxQty <= 0} onClick={() => setQtyText(won(Math.max(p.part === 1 ? maxQty : 0, Math.floor(maxQty * p.part))))}>
            {p.label}
          </button>
        ))}
      </div>
      <p className="order-hint">
        {side === "buy" ? `최대 ${won(maxQty)}주 (코인 ${won(state.me.coins)})` : `팔 수 있는 주식 ${won(available)}주`}
        {position && position.qty > 0 && ` · 보유 ${won(position.qty)}주 · 평균 ${won(position.avg_price)}`}
        {position && position.lockup_qty > 0 && ` · 보호예수 ${won(position.lockup_qty)}주 (${relativeText(position.lockup_until, state.server_time)} 풀림)`}
      </p>
      <dl className="order-sum">
        <div>
          <dt>{kind === "market" ? "예상 주문 금액" : "주문 금액"}</dt>
          <dd>{won(estimate.notional)}</dd>
        </div>
        <div>
          <dt>수수료 {ppmText(rules.fee_ppm)}</dt>
          <dd>{won(estimate.fee)}</dd>
        </div>
        {side === "sell" && (
          <div>
            <dt>거래세 {ppmText(rules.tax_ppm)}</dt>
            <dd>{won(estimate.tax)}</dd>
          </div>
        )}
        <div className="total">
          <dt>{side === "buy" ? "낼 코인" : "받을 코인"}</dt>
          <dd>
            {kind === "market" ? "약 " : ""}
            {won(estimate.total)}
          </dd>
        </div>
      </dl>
      {warning && <p className="order-warning">{warning}</p>}
      <button className={`order-submit ${side}`} disabled={busy || blocked} onClick={() => void submit()}>
        {busy ? "처리 중…" : `${s.name} ${qty > 0 ? `${won(qty)}주 ` : ""}${side === "buy" ? "매수" : "매도"}`}
      </button>
      <p className="order-note">
        {kind === "market"
          ? "시장가는 호가를 차례로 체결하고, 남은 수량은 취소됩니다."
          : "지정가에 바로 체결되지 않은 수량은 호가에 남아 있다가 조건이 맞으면 체결됩니다."}
      </p>
    </div>
  );
}
