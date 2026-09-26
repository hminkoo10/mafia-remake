// 관리자 탭: 시장 집계, 종목 거래정지·재개, 게임일 넘기기, 시세판·뉴스 채널 연결.
// 관리자가 Discord에서 `/주식 증권`으로 받은 링크에서만 보인다 (서버도 다시 확인한다).
import { useState } from "react";
import { adminAction } from "./api";
import { signedWon, won } from "./format";
import type { Act, StockState } from "./types";

export function AdminTab({ state, busy, act }: { state: StockState; busy: boolean; act: Act }) {
  const admin = state.admin;
  const companies = state.market.companies;
  const [code, setCode] = useState(() => companies[0]?.code ?? "");
  const [days, setDays] = useState(1);
  const [panel, setPanel] = useState(() => (admin && admin.panel_channel !== "0" ? admin.panel_channel : ""));
  const [news, setNews] = useState(() => (admin && admin.news_channel !== "0" ? admin.news_channel : ""));
  if (!admin) return <p className="empty">관리자만 볼 수 있습니다.</p>;
  const stats = admin.stats;
  const made = stats.lp_sold - stats.lp_bought + stats.dividends - stats.ipo_burned;
  const nameOf = (target: string) => companies.find((company) => company.code === target)?.name ?? target;

  const skip = () => {
    if (window.confirm(`게임일을 ${days}일 넘길까요? 시장 시계가 앞당겨지고 그 사이 시세·청약·배당이 바로 처리됩니다.`)) {
      void act((t) => adminAction(t, { action: "skip", days }));
    }
  };

  return (
    <div className="desk">
      <div className="desk-card">
        <h3>시장 집계</h3>
        <dl className="facts">
          <div>
            <dt>시장조성자에게서 산 금액 (코인 사라짐)</dt>
            <dd>{won(stats.lp_bought)}</dd>
          </div>
          <div>
            <dt>시장조성자에게 판 금액 (코인 생김)</dt>
            <dd>{won(stats.lp_sold)}</dd>
          </div>
          <div>
            <dt>플레이어끼리 체결</dt>
            <dd>{won(stats.p2p)}</dd>
          </div>
          <div>
            <dt>금고로 간 수수료 / 거래세</dt>
            <dd>
              {won(stats.fees)} / {won(stats.taxes)}
            </dd>
          </div>
          <div>
            <dt>시스템 회사 배당 (생김)</dt>
            <dd>{won(stats.dividends)}</dd>
          </div>
          <div>
            <dt>시스템 공모 대금 (사라짐)</dt>
            <dd>{won(stats.ipo_burned)}</dd>
          </div>
          <div>
            <dt>플레이어 회사 분기 실적</dt>
            <dd>{signedWon(stats.company_earnings)}</dd>
          </div>
          <div>
            <dt>시장이 만든 코인</dt>
            <dd className={made > 0 ? "up" : made < 0 ? "down" : ""}>{signedWon(made)}</dd>
          </div>
        </dl>
      </div>

      <div className="desk-grid">
        <div className="desk-form">
          <h4>종목 거래정지·재개</h4>
          <p className="desk-hint">거래정지한 종목은 재개할 때까지 아무도 사고팔 수 없습니다. 뉴스 채널에 공시로 올라갑니다.</p>
          <label className="desk-field">
            <span>종목</span>
            <select value={code} onChange={(e) => setCode(e.target.value)}>
              {companies.map((company) => (
                <option key={company.code} value={company.code}>
                  {company.name} ({company.code})
                </option>
              ))}
            </select>
          </label>
          <div className="inline-form">
            <button className="btn danger small" disabled={busy || !code} onClick={() => void act((t) => adminAction(t, { action: "halt", code }))}>
              거래정지
            </button>
            <button className="btn small" disabled={busy || !code} onClick={() => void act((t) => adminAction(t, { action: "resume", code }))}>
              거래재개
            </button>
          </div>
          {admin.halted.length > 0 && (
            <div className="halted-list">
              <span>거래정지 중:</span>
              {admin.halted.map((halted) => (
                <button key={halted} className="btn small" disabled={busy} onClick={() => void act((t) => adminAction(t, { action: "resume", code: halted }))}>
                  {nameOf(halted)} 재개
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="desk-form">
          <h4>게임일 넘기기</h4>
          <p className="desk-hint">
            시장 시계를 다음 게임일로 옮기고 그 사이의 시세·주문·청약·배당·보호예수를 바로 처리합니다 (한 번에 24일까지).
            {state.time_shift_ms > 0 && ` 지금 시장 시계는 실제보다 ${Math.round(state.time_shift_ms / 60_000)}분 앞서 있습니다.`}
          </p>
          <label className="desk-field">
            <span>넘길 게임일</span>
            <input type="number" min={1} max={24} value={days} onChange={(e) => setDays(Math.min(24, Math.max(1, Number(e.target.value) || 1)))} />
          </label>
          <button className="btn primary small" disabled={busy} onClick={skip}>
            {days}일 넘기기
          </button>
        </div>

        <div className="desk-form">
          <h4>채널 연결</h4>
          <p className="desk-hint">
            Discord 채널 ID를 적습니다 (채널 우클릭 → ID 복사). 시세판은 1분 안에 새 채널에 올라가고 옛 것은 지웁니다. 비우면 끊습니다.
          </p>
          <label className="desk-field">
            <span>시세판 채널</span>
            <input inputMode="numeric" placeholder="없음" value={panel} onChange={(e) => setPanel(e.target.value.trim())} />
          </label>
          <label className="desk-field">
            <span>뉴스 채널</span>
            <input inputMode="numeric" placeholder="없음" value={news} onChange={(e) => setNews(e.target.value.trim())} />
          </label>
          <button
            className="btn primary small"
            disabled={busy}
            onClick={() => void act((t) => adminAction(t, { action: "channels", panel_channel: panel, news_channel: news }))}
          >
            채널 저장
          </button>
        </div>
      </div>
      <p className="foot">수수료·거래세·가격제한폭·보유 한도·설립 조건 같은 주식 설정은 Discord `/마피아웹설정`의 주식 카테고리에서 바꿉니다.</p>
    </div>
  );
}
