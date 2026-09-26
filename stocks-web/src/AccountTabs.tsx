// 내 계좌: 자산 요약, 잔고, 미체결, 체결, 공모·증자(청약·청약 취소·신주인수), 내 회사, 순위, 시장 뉴스,
// 관리(관리자만).
import { useState, type ReactNode } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui";
import { AdminTab } from "./AdminTab";
import { cancelOrder, companyAction, subscribeIpo, unsubscribeIpo } from "./api";
import { CompanyDesk } from "./CompanyDesk";
import { NewsList } from "./NewsList";
import { RankingTab } from "./RankingTab";
import { bpText, dateTimeText, parseAmount, pctText, relativeText, signedWon, tone, won } from "./format";
import type { Act, OfferingView, RightsClaimView, StockState, SubscriptionView } from "./types";

function Stat({ label, value, className }: { label: string; value: ReactNode; className?: string }) {
  return (
    <div>
      <span>{label}</span>
      <strong className={className}>{value}</strong>
    </div>
  );
}

export function AccountTabs({
  token,
  state,
  now,
  busy,
  act,
  onSelect,
}: {
  token: string;
  state: StockState;
  now: number;
  busy: boolean;
  act: Act;
  onSelect: (code: string) => void;
}) {
  const [tab, setTab] = useState("positions");
  const a = state.account;
  const openRights = a.rights.filter((claim) => claim.granted > claim.exercised).length;
  const ipoCount = state.offerings.length + state.rights_offerings.length;
  const total = state.me.coins + a.stock_value + a.pending;
  const selectedCode = state.selected?.summary.code ?? null;
  return (
    <section className="card account">
      <div className="summary">
        <Stat label="총자산" value={won(total)} className="accent" />
        <Stat label="보유 코인" value={won(state.me.coins)} />
        <Stat label="주식 평가액" value={won(a.stock_value)} />
        <Stat label="주문·청약에 묶인 코인" value={won(a.pending)} />
        <Stat label="평가손익" value={signedWon(a.unrealized)} className={tone(a.unrealized)} />
        <Stat label="실현손익" value={signedWon(a.realized)} className={tone(a.realized)} />
      </div>
      <Tabs value={tab} onValueChange={setTab}>
        <TabsList label="내 계좌">
          <TabsTrigger value="positions">잔고 {a.positions.length || ""}</TabsTrigger>
          <TabsTrigger value="orders">미체결 {a.orders.length || ""}</TabsTrigger>
          <TabsTrigger value="fills">체결</TabsTrigger>
          <TabsTrigger value="ipo">공모·증자 {ipoCount || ""}</TabsTrigger>
          <TabsTrigger value="company">내 회사</TabsTrigger>
          <TabsTrigger value="ranking">순위</TabsTrigger>
          <TabsTrigger value="news">시장 뉴스</TabsTrigger>
          {state.me.admin && <TabsTrigger value="admin">관리</TabsTrigger>}
        </TabsList>
        <TabsContent value="positions">
          {openRights > 0 && (
            <div className="banner">
              <span>받은 신주인수권이 {openRights}건 있습니다. 기간 안에 인수해야 새 주식을 받습니다.</span>
              <button className="text-button" onClick={() => setTab("ipo")}>
                공모·증자 탭에서 인수
              </button>
            </div>
          )}
          {a.positions.length === 0 ? (
            <p className="empty">가진 주식이 없습니다. 호가창 옆 주문 칸에서 사 보세요.</p>
          ) : (
            <div className="table-wrap">
              <table className="table clickable">
                <thead>
                  <tr>
                    <th>종목</th>
                    <th>보유</th>
                    <th>매도 가능</th>
                    <th>평균 단가</th>
                    <th>현재가</th>
                    <th>평가 금액</th>
                    <th>평가 손익</th>
                    <th>수익률</th>
                  </tr>
                </thead>
                <tbody>
                  {a.positions.map((p) => (
                    <tr key={p.code} onClick={() => onSelect(p.code)} title="이 종목 보기">
                      <td>
                        <b>{p.name}</b>
                        <small>
                          {p.code}
                          {p.status !== "상장" ? ` · ${p.status}` : ""}
                        </small>
                      </td>
                      <td>{won(p.qty)}</td>
                      <td>
                        {won(p.available)}
                        {p.lockup_qty > 0 && <small>보호예수 {won(p.lockup_qty)}</small>}
                      </td>
                      <td>{won(p.avg_price)}</td>
                      <td>{won(p.price)}</td>
                      <td>{won(p.value)}</td>
                      <td className={tone(p.pnl)}>{signedWon(p.pnl)}</td>
                      <td className={tone(p.pnl_bp)}>{pctText(p.pnl_bp)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <p className="foot">
            낸 수수료·세금 누적 {won(a.fees)} · 비상장 주식은 주당 순자산으로, 상장폐지된 주식은 0으로 평가합니다.
          </p>
        </TabsContent>
        <TabsContent value="orders">
          {a.orders.length === 0 ? (
            <p className="empty">호가에 걸어 둔 주문이 없습니다.</p>
          ) : (
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>종목</th>
                    <th>구분</th>
                    <th>주문가</th>
                    <th>남은 / 주문</th>
                    <th>묶인 코인</th>
                    <th>주문</th>
                    <th>만료</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {a.orders.map((order) => (
                    <tr key={order.id}>
                      <td>
                        <button className="link-cell" onClick={() => onSelect(order.code)}>
                          {order.name}
                        </button>
                      </td>
                      <td className={order.side === "buy" ? "up" : "down"}>{order.side === "buy" ? "매수" : "매도"}</td>
                      <td>{won(order.limit)}</td>
                      <td>
                        {won(order.remaining)} / {won(order.original)}
                      </td>
                      <td>{order.reserved > 0 ? won(order.reserved) : "—"}</td>
                      <td>{dateTimeText(order.created_at)}</td>
                      <td>{relativeText(order.expires_at, now)}</td>
                      <td>
                        <button className="btn small" disabled={busy} onClick={() => void act((t) => cancelOrder(t, order.id, selectedCode))}>
                          취소
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </TabsContent>
        <TabsContent value="fills">
          {a.fills.length === 0 ? (
            <p className="empty">체결 기록이 없습니다.</p>
          ) : (
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>시각</th>
                    <th>종목</th>
                    <th>구분</th>
                    <th>수량</th>
                    <th>체결가</th>
                    <th>수수료·세금</th>
                  </tr>
                </thead>
                <tbody>
                  {a.fills.map((fill, index) => (
                    <tr key={`${fill.at}-${index}`}>
                      <td>{dateTimeText(fill.at)}</td>
                      <td>{state.market.companies.find((c) => c.code === fill.code)?.name ?? fill.code}</td>
                      <td className={fill.side === "buy" ? "up" : "down"}>{fill.side === "buy" ? "매수" : "매도"}</td>
                      <td>{won(fill.qty)}</td>
                      <td>{won(fill.price)}</td>
                      <td>{won(fill.cost)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </TabsContent>
        <TabsContent value="ipo">
          <div className="offer-list">
            <MySubscriptions subscriptions={a.subscriptions} now={now} busy={busy} act={act} onSelect={onSelect} />
            <Offerings state={state} now={now} busy={busy} act={act} onSelect={onSelect} />
            <RightsSection state={state} now={now} busy={busy} act={act} onSelect={onSelect} />
          </div>
        </TabsContent>
        <TabsContent value="company">
          <CompanyDesk state={state} now={now} busy={busy} act={act} onSelect={onSelect} />
        </TabsContent>
        <TabsContent value="ranking">
          <RankingTab token={token} />
        </TabsContent>
        <TabsContent value="news">
          <NewsList items={state.news} now={now} empty="아직 뉴스가 없습니다." onSelect={onSelect} />
        </TabsContent>
        {state.me.admin && (
          <TabsContent value="admin">
            <AdminTab state={state} busy={busy} act={act} />
          </TabsContent>
        )}
      </Tabs>
    </section>
  );
}

// ------------------------------------------------------------ 청약·신주인수

function MySubscriptions({
  subscriptions,
  now,
  busy,
  act,
  onSelect,
}: {
  subscriptions: SubscriptionView[];
  now: number;
  busy: boolean;
  act: Act;
  onSelect: (code: string) => void;
}) {
  if (subscriptions.length === 0) return null;
  return (
    <div className="desk-card">
      <h3>내 청약</h3>
      <p className="desk-hint">마감 전에는 청약을 취소하고 증거금을 모두 돌려받을 수 있습니다.</p>
      {subscriptions.map((sub) => (
        <div key={sub.code} className="rights-row">
          <div>
            <button className="link-cell" onClick={() => onSelect(sub.code)}>
              {sub.name}
            </button>
            <small>
              {won(sub.qty)}주 청약 · 증거금 {won(sub.deposit)}
              {sub.closes_at > 0 ? ` · ${relativeText(sub.closes_at, now)} 마감` : ""}
            </small>
          </div>
          <button className="btn small" disabled={busy} onClick={() => void act((t) => unsubscribeIpo(t, sub.code))}>
            청약 취소
          </button>
        </div>
      ))}
    </div>
  );
}

function RightsSection({
  state,
  now,
  busy,
  act,
  onSelect,
}: {
  state: StockState;
  now: number;
  busy: boolean;
  act: Act;
  onSelect: (code: string) => void;
}) {
  return (
    <div className="desk-card rights">
      <h3>유상증자·신주인수</h3>
      <p className="desk-hint">
        상장사가 유상증자를 하면 그때의 주주에게 보유 주식 비율대로 신주인수권이 배정되고, 1게임일 안에 발행가를 내면 새 주식을 받습니다.
      </p>
      {state.rights_offerings.length === 0 && <p className="empty">진행 중인 유상증자가 없습니다.</p>}
      {state.rights_offerings.map((offer) => {
        const claim = state.account.rights.find((item) => item.code === offer.code);
        return claim ? (
          <RightsRow key={offer.code} claim={claim} coins={state.me.coins} now={now} busy={busy} act={act} />
        ) : (
          <div key={offer.code} className="rights-row">
            <div>
              <button className="link-cell" onClick={() => onSelect(offer.code)}>
                {offer.name}
              </button>
              <small>
                발행가 {won(offer.price)} · 신주 {won(offer.shares)}주 중 {won(offer.exercised)}주 인수 · {relativeText(offer.until, now)} 마감
              </small>
            </div>
            <span className="desk-done">주주가 아니라 배정받은 인수권이 없습니다</span>
          </div>
        );
      })}
    </div>
  );
}

function RightsRow({ claim, coins, now, busy, act }: { claim: RightsClaimView; coins: number; now: number; busy: boolean; act: Act }) {
  const left = claim.granted - claim.exercised;
  const [qtyText, setQtyText] = useState(() => won(left));
  const qty = parseAmount(qtyText);
  const cost = qty * claim.price;
  return (
    <div className="rights-row">
      <div>
        <b>{claim.name}</b>
        <small>
          발행가 {won(claim.price)} · 배정 {won(claim.granted)}주 · 인수 {won(claim.exercised)}주 · {relativeText(claim.until, now)} 마감
        </small>
      </div>
      {left > 0 ? (
        <div className="inline-form">
          <input inputMode="numeric" value={qtyText} onChange={(e) => setQtyText(e.target.value)} aria-label="인수할 주식 수" />
          <button
            className="btn primary small"
            disabled={busy || qty <= 0 || qty > left || cost > coins}
            onClick={() => void act((t) => companyAction(t, { action: "exercise", code: claim.code, qty }))}
          >
            {won(cost)}으로 인수
          </button>
        </div>
      ) : (
        <span className="desk-done">모두 인수함</span>
      )}
    </div>
  );
}

// ------------------------------------------------------------ 공모주

function Offerings({
  state,
  now,
  busy,
  act,
  onSelect,
}: {
  state: StockState;
  now: number;
  busy: boolean;
  act: Act;
  onSelect: (code: string) => void;
}) {
  if (state.offerings.length === 0 && state.account.subscriptions.length === 0) {
    return (
      <p className="empty">
        지금 청약을 받는 공모주가 없습니다. 새 시스템 회사는 가끔 상장하고, 플레이어 회사는 대표가 공모를 열면 여기에 나옵니다.
      </p>
    );
  }
  return (
    <div className="offer-list">
      {state.offerings.map((offering) => (
        <OfferingCard
          key={offering.code}
          offering={offering}
          mine={state.account.subscriptions.find((sub) => sub.code === offering.code)}
          coins={state.me.coins}
          now={now}
          busy={busy}
          act={act}
          onSelect={onSelect}
        />
      ))}
      <p className="foot">
        플레이어 회사 공모는 실제처럼 기관이 공모가를 보고 일부를 받아 갑니다 (주당 순자산 이하면 공모 주식의 30%, 비쌀수록 줄어 2배면 없음).
        기관 몫은 상장 뒤 시장에서 거래되고, 플레이어는 나머지(일반 청약분)를 나눠 받습니다. 청약이 일반 청약분보다 많으면 그 절반은
        청약자에게 고르게(균등 배정), 나머지는 남은 청약 수량에 비례해 나눠 받고, 받지 못한 만큼의 증거금은 돌려받습니다. 상장 첫날 가격은
        공모가의 60~400% 안에서 움직입니다.
      </p>
    </div>
  );
}

function OfferingCard({
  offering,
  mine,
  coins,
  now,
  busy,
  act,
  onSelect,
}: {
  offering: OfferingView;
  mine: SubscriptionView | undefined;
  coins: number;
  now: number;
  busy: boolean;
  act: Act;
  onSelect: (code: string) => void;
}) {
  const [qtyText, setQtyText] = useState("");
  const qty = parseAmount(qtyText);
  const cost = qty * offering.price;
  const institutions = offering.institutions ?? 0;
  const retail = offering.shares - institutions;
  const ratio = retail > 0 ? offering.requested / retail : 0;
  return (
    <div className="desk-card offer">
      <header>
        <div>
          <h3>{offering.name}</h3>
          <span>
            {offering.code} · {offering.sector}
            {offering.player ? ` · 플레이어 회사${offering.founder_name ? ` (${offering.founder_name})` : ""}` : ""}
          </span>
        </div>
        <button className="btn small" onClick={() => onSelect(offering.code)}>
          회사 보기
        </button>
      </header>
      <dl className="facts">
        <div>
          <dt>공모가</dt>
          <dd>{won(offering.price)}</dd>
        </div>
        <div>
          <dt>공모 주식</dt>
          <dd>
            {won(offering.shares)}주{institutions > 0 && <small> (일반 {won(retail)} · 기관 {won(institutions)})</small>}
          </dd>
        </div>
        <div>
          <dt>청약 경쟁률</dt>
          <dd>
            {ratio.toFixed(2)} : 1 <small>({won(offering.requested)}주)</small>
          </dd>
        </div>
        <div>
          <dt>마감</dt>
          <dd>{relativeText(offering.closes_at, now)}</dd>
        </div>
      </dl>
      {offering.min_fill_bp > 0 && (
        <p className="desk-hint">청약이 공모 주식의 {bpText(offering.min_fill_bp)}에 못 미치면 공모가 무산되고 증거금은 모두 돌려받습니다.</p>
      )}
      {mine && (
        <div className="rights-row">
          <div>
            <b>내 청약 {won(mine.qty)}주</b>
            <small>증거금 {won(mine.deposit)} 냄</small>
          </div>
          <button className="btn small" disabled={busy} onClick={() => void act((t) => unsubscribeIpo(t, offering.code))}>
            청약 취소
          </button>
        </div>
      )}
      <div className="inline-form">
        <input inputMode="numeric" placeholder="청약 수량" value={qtyText} onChange={(e) => setQtyText(e.target.value)} aria-label="청약 수량" />
        <button
          className="btn primary small"
          disabled={busy || qty <= 0 || qty > offering.shares || cost > coins}
          onClick={async () => {
            const done = await act((t) => subscribeIpo(t, offering.code, qty));
            if (done) setQtyText("");
          }}
        >
          {qty > 0 ? `증거금 ${won(cost)} 내고 청약` : "청약"}
        </button>
      </div>
      {cost > coins && <p className="order-warning">코인이 부족합니다 (가진 코인 {won(coins)}).</p>}
    </div>
  );
}
