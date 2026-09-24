// 테이블 위 카드: 슈에서 날아와 놓이는 카드, 보드·딜러 줄, 번 카드 더미, 좌석 카드, 내 카드 칸, 카운트다운, 차례 링.
// 시간에 따라 바뀌는 칸은 여기서 서버 시계를 직접 구독한다. 루트 화면은 시계 때문에 다시 그려지지 않는다.
import { memo, useEffect, useLayoutEffect, useRef, useState, type AnimationEvent, type CSSProperties } from "react";
import { Club, Spade } from "lucide-react";
import { liveServerNow, serverNow, useServerValue } from "../clock";
import { DEFAULT_FLIP_MS, faceDown, inPlayCount, landedCount, MIN_FLIGHT_MS, secondsLeft, type CardTiming } from "../schedule";
import { sfx } from "../sounds";
import { blackjackValue, fmt, handResultText, handScore, handTone } from "../table-helpers";
import type { GameKind, RoundView, SeatView } from "../types";

const suits: Record<string, string> = { s: "♠", h: "♥", d: "♦", c: "♣" };

type EntryState = "static" | "pending" | "flying" | "landed" | "appear" | "slide";
interface Entry {
  state: EntryState;
  /** 남은 비행 시간 (ms). */
  duration: number;
  /** 슈에서 카드 자리까지의 거리 (px). */
  from: [number, number];
}
const ENTRY_CLASS: Record<EntryState, string> = {
  static: "",
  pending: "from-shoe-pending",
  flying: "from-shoe",
  landed: "landed",
  appear: "appear",
  slide: "slide-in",
};

export const PlayingCard = memo(function PlayingCard({
  card,
  small = false,
  lit = false,
  landAt,
  flightMs = 0,
  flipMs = DEFAULT_FLIP_MS,
  enter,
  silent = false,
}: {
  card?: string;
  small?: boolean;
  lit?: boolean;
  /** 카드가 테이블에 놓이는 서버 시각. 주면 그 시각에 맞춰 슈에서 날아와 놓인다 (처음 그릴 때만 본다). */
  landAt?: number;
  /** 슈에서 날아오는 시간. 0이면 날지 않는다. */
  flightMs?: number;
  flipMs?: number;
  /** 날아오지 않는 카드가 나타나는 방식 (내 카드 칸은 살짝 떠오르고, 스플릿한 카드는 옆에서 밀려온다). */
  enter?: "appear" | "slide";
  /** 딜 소리를 내지 않는다 (테이블 카드를 내 카드 칸에 한 번 더 보여 줄 때). */
  silent?: boolean;
}) {
  const element = useRef<HTMLSpanElement>(null);
  const previous = useRef(card);
  const sounded = useRef(false);
  const [flipping, setFlipping] = useState(false);
  // 처음 그릴 때 정한다: 아직 날아가는 중이면 남은 시간만큼 슈에서 날아오고, 이미 놓였으면 그대로 놓인다.
  const [entry, setEntry] = useState<Entry>(() => {
    if (landAt === undefined || !card) return { state: "static", duration: 0, from: [0, 0] };
    const remaining = landAt - liveServerNow();
    if (flightMs > 0 && remaining >= MIN_FLIGHT_MS) return { state: "pending", duration: Math.min(flightMs, remaining), from: [0, 0] };
    return { state: enter ?? "landed", duration: 0, from: [0, 0] };
  });

  useLayoutEffect(() => {
    if (entry.state !== "pending") return;
    const el = element.current;
    const shoe = el?.closest(".game-table")?.querySelector<HTMLElement>(".shoe-anchor");
    if (!el || !shoe) {
      setEntry((current) => ({ ...current, state: "landed" }));
      return;
    }
    const start = shoe.getBoundingClientRect();
    const end = el.getBoundingClientRect();
    // 내 자리처럼 확대된 좌석 안이면 화면 거리를 카드 좌표로 되돌린다.
    const scale = el.offsetWidth ? end.width / el.offsetWidth : 1;
    const from: [number, number] = [
      (start.left + start.width / 2 - (end.left + end.width / 2)) / scale,
      (start.top + start.height / 2 - (end.top + end.height / 2)) / scale,
    ];
    setEntry((current) => ({ ...current, state: "flying", from }));
  }, [entry.state]);

  useEffect(() => {
    if (entry.state !== "flying") return;
    // 딜 소리는 카드가 슈를 떠나는 순간에 한 번.
    if (!silent && !sounded.current) {
      sounded.current = true;
      sfx.deal();
    }
    const timer = window.setTimeout(() => setEntry((current) => ({ ...current, state: "landed" })), entry.duration + 40);
    return () => window.clearTimeout(timer);
  }, [entry.state, entry.duration, silent]);

  useEffect(() => {
    // 연출을 꺼서 날지 않는 카드는 놓이는 순간에 소리를 낸다 (다시 연결해 예전 카드를 그릴 때는 조용히).
    if (silent || sounded.current || landAt === undefined || flightMs > 0 || !card) return;
    sounded.current = true;
    if (landAt - liveServerNow() > -250) sfx.deal();
  }, []);

  useEffect(() => {
    // 뒷면("??")이었다가 앞면이 되면 뒤집는다.
    const before = previous.current;
    previous.current = card;
    if (before === "??" && card && card !== "??") {
      setFlipping(true);
      // 나타나는 연출 중이었다면 끝난 것으로 둔다 (뒤집은 뒤 나타나는 연출이 다시 돌지 않게).
      setEntry((current) => (current.state === "appear" || current.state === "slide" ? { ...current, state: "landed" } : current));
    }
  }, [card]);

  useEffect(() => {
    if (!flipping) return;
    const timer = window.setTimeout(() => setFlipping(false), flipMs + 50);
    return () => window.clearTimeout(timer);
  }, [flipping, flipMs]);

  const settle = (event: AnimationEvent<HTMLSpanElement>) => {
    if (event.animationName === "card-appear" || event.animationName === "card-slide") {
      setEntry((current) => (current.state === "appear" || current.state === "slide" ? { ...current, state: "landed" } : current));
    }
  };

  const style: CSSProperties | undefined =
    entry.state === "flying"
      ? ({ "--from-x": `${entry.from[0]}px`, "--from-y": `${entry.from[1]}px`, "--flight-ms": `${entry.duration}ms` } as CSSProperties)
      : flipping
        ? ({ "--flip-ms": `${flipMs}ms` } as CSSProperties)
        : undefined;
  return (
    <span
      ref={element}
      className={`playing-card ${small ? "small" : ""} ${!card ? "empty" : card === "??" ? "back" : ""} ${
        card && /[hd]$/.test(card) ? "red" : ""
      } ${lit ? "lit" : ""} ${flipping ? "flip" : ""} ${ENTRY_CLASS[entry.state]}`}
      style={style}
      onAnimationEnd={entry.state === "appear" || entry.state === "slide" ? settle : undefined}
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
});

/** 아직 카드가 오지 않은 자리: 크기만 차지해 먼저 놓인 카드가 옆으로 밀리지 않게 한다. */
const CardSlot = ({ small = false }: { small?: boolean }) => <span className={`playing-card ${small ? "small" : ""} slot`} aria-hidden="true" />;

const at = (times: readonly number[] | undefined, index: number) => times?.[index];

/** 가운데 카드 줄: 홀덤은 보드 5칸, 블랙잭은 딜러 카드 (처음 두 장 자리에 고정하고, 더 뽑은 카드는 오른쪽으로 놓인다). */
export const BoardCards = memo(function BoardCards({
  kind,
  round,
  timing,
  litCards,
}: {
  kind: GameKind;
  round: RoundView | null;
  timing: CardTiming;
  litCards: readonly string[];
}) {
  const cards = (kind === "holdem" ? round?.board : round?.dealer) ?? [];
  const times = kind === "holdem" ? round?.board_reveal_at : round?.dealer_reveal_at;
  const flipAt = kind === "holdem" ? round?.board_flip_at : round?.dealer_flip_at;
  useServerValue((now) => `${inPlayCount(times, cards.length, now, timing.flight)}:${landedCount(times, cards.length, now)}:${faceDown(flipAt, now)}`);
  const now = serverNow();
  const shown = inPlayCount(times, cards.length, now, timing.flight);
  const hidden = faceDown(flipAt, now);
  const roundId = round?.id ?? "none";
  // 홀덤 플롭은 세 장이 뒷면으로 놓였다가 함께 뒤집히고, 블랙잭 홀 카드는 정산 때 뒤집힌다.
  const face = (index: number) => (hidden && (kind === "holdem" ? index < 3 : index === 1) ? "??" : cards[index]);
  const card = (index: number) => {
    const value = face(index);
    return (
      <PlayingCard
        key={`${roundId}-${index}-card`}
        card={value}
        landAt={at(times, index) ?? 0}
        flightMs={timing.flight}
        flipMs={timing.flip}
        lit={value !== "??" && litCards.includes(value)}
      />
    );
  };
  const slot = (index: number) => <PlayingCard key={`${roundId}-${index}-slot`} />;

  if (kind === "holdem") {
    return <div className="community-cards">{Array.from({ length: 5 }, (_, i) => (i < shown ? card(i) : slot(i)))}</div>;
  }
  const landed = landedCount(times, cards.length, now);
  const up = cards.slice(0, landed).filter((value, index) => value !== "??" && face(index) !== "??");
  const dealer = up.length ? blackjackValue(up).total : null;
  return (
    <div className="community-cards dealer-cards">
      <div className="dealer-hand">
        {/* 딜러 점수는 카드 왼쪽에 띄운다: 나타나도 아래 안내 문구가 밀리지 않는다. */}
        {dealer !== null && <span className="dealer-total">딜러 {dealer}</span>}
        {[0, 1].map((i) => (i < shown ? card(i) : slot(i)))}
        {shown > 2 && <div className="dealer-draws">{Array.from({ length: shown - 2 }, (_, i) => card(i + 2))}</div>}
      </div>
    </div>
  );
});

/**
 * 버린 카드(번 카드): 슈에서 뒷면으로 딜러 앞 머크 자리로 날아가 라운드가 끝날 때까지 작게 쌓인다.
 * 자리 상자(.burn-card)는 옮기기만 하고 돌리지 않는다: 비행 거리를 돌린 상자 안에서 재면 슈에서 벗어나 출발한다.
 * 기울기는 카드가 놓일 때 카드 자신에 준다 (overrides.css).
 */
export const BurnPile = memo(function BurnPile({ roundId, times, timing }: { roundId: string; times: readonly number[]; timing: CardTiming }) {
  const shown = useServerValue((now) => inPlayCount(times, times.length, now, timing.flight));
  if (!shown) return null;
  return (
    <div className="burn-pile" aria-label={`버린 카드 ${shown}장`}>
      {times.slice(0, shown).map((time, index) => (
        <span key={`${roundId}-${index}`} className="burn-card" style={{ "--burn": index } as CSSProperties}>
          <PlayingCard card="??" small landAt={time} flightMs={timing.flight} />
        </span>
      ))}
    </div>
  );
});

/** 좌석 앞 카드. 홀덤은 두 장 자리를 먼저 잡아 두고, 블랙잭 핸드는 왼쪽을 고정한 채 오른쪽으로 쌓인다. */
export const SeatCards = memo(function SeatCards({
  kind,
  seat,
  roundId,
  timing,
  litCards,
  handName,
  activeHand,
}: {
  kind: GameKind;
  seat: SeatView;
  roundId: string;
  timing: CardTiming;
  litCards: readonly string[];
  /** 홀덤 족보 이름: 카드 바로 위에 띄워 좌석이 움직이지 않게 한다. */
  handName: string | null;
  /** 지금 차례인 핸드 번호 (-1 = 없음). */
  activeHand: number;
}) {
  useServerValue((now) =>
    kind === "holdem"
      ? `${inPlayCount(seat.cards_reveal_at, seat.cards.length, now, timing.flight)}:${!seat.mine && faceDown(seat.showdown_at, now)}`
      : seat.hands.map((h) => `${inPlayCount(h.reveal_at, h.cards.length, now, timing.flight)}.${landedCount(h.reveal_at, h.cards.length, now)}`).join(","),
  );
  const now = serverNow();
  if (kind === "holdem") {
    const shown = inPlayCount(seat.cards_reveal_at, seat.cards.length, now, timing.flight);
    // 쇼다운 순서가 오기 전까지 다른 사람 카드는 뒷면으로 둔다.
    const hidden = !seat.mine && faceDown(seat.showdown_at, now);
    const slots = seat.cards.length ? Math.max(2, seat.cards.length) : 0;
    return (
      <div className="seat-cards">
        {Array.from({ length: slots }, (_, k) => {
          if (k >= shown) return <CardSlot key={`${roundId}-${k}-slot`} small />;
          const value = hidden ? "??" : seat.cards[k];
          return (
            <PlayingCard
              key={`${roundId}-${k}`}
              card={value}
              small
              landAt={at(seat.cards_reveal_at, k) ?? 0}
              flightMs={timing.flight}
              flipMs={timing.flip}
              lit={value !== "??" && litCards.includes(value)}
            />
          );
        })}
        {handName && slots > 0 && <span className="hand-name">{handName}</span>}
      </div>
    );
  }
  const single = seat.hands.length === 1;
  return (
    <div className="seat-cards bj-seat-hands">
      {seat.hands.map((h, k) => {
        const shown = inPlayCount(h.reveal_at, h.cards.length, now, timing.flight);
        if (!shown) return null;
        // 합계는 테이블에 놓인 카드로만 센다 (날아가는 카드는 아직 세지 않는다).
        const landed = landedCount(h.reveal_at, h.cards.length, now);
        const value = blackjackValue(h.cards.slice(0, landed));
        return (
          <div key={h.id} className={`seat-hand ${activeHand === k ? "active-hand" : ""} ${handTone(h.result)}`}>
            <div className="seat-hand-cards">
              {Array.from({ length: Math.max(2, shown) }, (_, n) =>
                n < shown ? (
                  <PlayingCard
                    key={`${h.id}-${n}-${h.cards[n]}`}
                    card={h.cards[n]}
                    small
                    landAt={at(h.reveal_at, n) ?? 0}
                    flightMs={timing.flight}
                    flipMs={timing.flip}
                    enter={k > 0 && n === 0 ? "slide" : undefined}
                  />
                ) : (
                  <CardSlot key={`${h.id}-slot-${n}`} small />
                ),
              )}
            </div>
            {landed > 0 && <span className="seat-hand-total">{handScore({ ...value, status: h.status, result: h.result }, landed, single)}</span>}
            {h.result ? (
              <span className="seat-hand-result">{handResultText(h)}</span>
            ) : (
              (seat.hands.length > 1 || h.bet !== seat.bet) && <span className="seat-hand-bet">{fmt(h.bet)}</span>
            )}
          </div>
        );
      })}
    </div>
  );
});

/** 액션 독의 내 카드: 테이블에 놓인 카드만 보여 준다. */
export const DockCards = memo(function DockCards({
  kind,
  seat,
  roundId,
  litCards,
  selectedHand,
}: {
  kind: GameKind;
  seat: SeatView;
  roundId: string;
  litCards: readonly string[];
  /** 지금 고른 핸드 (-1 = 없음). */
  selectedHand: number;
}) {
  useServerValue((now) =>
    kind === "holdem"
      ? landedCount(seat.cards_reveal_at, seat.cards.length, now)
      : seat.hands.map((h) => landedCount(h.reveal_at, h.cards.length, now)).join(","),
  );
  const now = serverNow();
  if (kind === "holdem") {
    if (!seat.cards.length) return <span>다음 핸드 대기</span>;
    const landed = landedCount(seat.cards_reveal_at, seat.cards.length, now);
    return (
      <>
        {seat.cards.slice(0, landed).map((c, i) => (
          <PlayingCard key={`${roundId}-${i}`} card={c} small lit={litCards.includes(c)} landAt={at(seat.cards_reveal_at, i) ?? 0} enter="appear" silent />
        ))}
      </>
    );
  }
  if (!seat.hands.length) return <span>베팅을 준비해 주세요</span>;
  return (
    <>
      {seat.hands.map((h, i) => {
        const landed = landedCount(h.reveal_at, h.cards.length, now);
        const { total, soft } = blackjackValue(h.cards.slice(0, landed));
        return (
          <div className={`bj-hand ${selectedHand === i ? "selected-hand" : ""}`} key={h.id}>
            <div>
              {h.cards.slice(0, landed).map((c, k) => (
                <PlayingCard key={`${k}-${c}`} card={c} small landAt={at(h.reveal_at, k) ?? 0} enter="appear" silent />
              ))}
            </div>
            <span>
              {total > 21 ? "버스트" : landed ? `${soft ? "소프트 " : ""}${total}` : "베팅 완료"} · {h.result ?? fmt(h.bet)}
            </span>
          </div>
        );
      })}
    </>
  );
});

/** 마감까지 남은 초. 이 숫자만 초마다 다시 그린다. */
export const Countdown = memo(function Countdown({ deadline }: { deadline: number }) {
  return <>{useServerValue((now) => secondsLeft(deadline, now))}</>;
});

/** 차례 링: 한 차례에 CSS 애니메이션 하나. 남은 시간만큼 앞당겨 시작해 초마다 다시 그리지 않고 부드럽게 줄어든다. */
export const TurnRing = memo(function TurnRing({ deadline, turnMs }: { deadline: number; turnMs: number }) {
  const [start] = useState(() => {
    const total = Math.max(1, turnMs);
    const left = Math.max(0, Math.min(total, deadline - liveServerNow()));
    return { elapsed: total - left, offset: 132 * (1 - left / total), total };
  });
  return (
    <svg className="turn-ring" viewBox="0 0 48 48" aria-hidden="true">
      <circle cx="24" cy="24" r="21" />
      <circle
        cx="24"
        cy="24"
        r="21"
        className="turn-ring-progress"
        style={{ strokeDashoffset: start.offset, animationDuration: `${start.total}ms`, animationDelay: `-${start.elapsed}ms` }}
      />
    </svg>
  );
});
