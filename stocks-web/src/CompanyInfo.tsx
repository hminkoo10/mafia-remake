// 고른 종목의 정보: 개요·진행 중인 일(공모·유상증자·자사주·배당·청산), 분기 실적, 주주, 뉴스.
import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui";
import { NewsList } from "./NewsList";
import { bpText, compactWon, dateTimeText, pctText, relativeText, RISK_TEXT, tone, won } from "./format";
import type { CompanyDetail, NewsItem } from "./types";

export function CompanyInfo({ detail, news, now }: { detail: CompanyDetail; news: NewsItem[]; now: number }) {
  const [tab, setTab] = useState("overview");
  const s = detail.summary;
  const pbr = detail.bvps > 0 ? s.price / detail.bvps : null;
  const notices: string[] = [];
  if (detail.ipo) {
    notices.push(
      `공모 청약 중: 공모가 ${won(detail.ipo.price)}, ${won(detail.ipo.shares)}주 중 ${won(detail.ipo_requested)}주 청약 · ${relativeText(detail.ipo.closes_at, now)} 마감`,
    );
  }
  if (detail.rights) {
    notices.push(
      `유상증자: 신주 ${won(detail.rights.shares)}주, 발행가 ${won(detail.rights.price)} · ${relativeText(detail.rights.until, now)} 청약 마감 (주주에게 신주인수권 배정)`,
    );
  }
  if (detail.buyback) {
    const total = Math.max(detail.buyback.total ?? 0, detail.buyback.budget);
    notices.push(
      `자사주 매입: 예산 ${won(total)} 중 ${won(total - detail.buyback.budget)} 사용 · ${won(detail.buyback.bought)}주 매입 · ${relativeText(detail.buyback.until, now)} 종료 (회사가 현재가에 매수 호가를 냅니다)`,
    );
  }
  if (detail.pending_dividend) {
    notices.push(`배당 예정: 주당 ${won(detail.pending_dividend.per_share)} · ${relativeText(detail.pending_dividend.pay_at, now)} 지급`);
  }
  if (detail.liquidation) {
    notices.push(`${detail.liquidation[1]}: ${relativeText(detail.liquidation[0], now)} 남은 자본을 주주에게 나누고 상장폐지`);
  }
  if (detail.delisted) {
    notices.push(`${dateTimeText(detail.delisted[0])} 상장폐지 (${detail.delisted[1]})`);
  }
  if (s.managed) {
    notices.push("관리종목: 자본잠식 등으로 지정됐습니다. 개선되지 않으면 상장폐지될 수 있습니다.");
  }

  return (
    <div className="card info">
      <Tabs value={tab} onValueChange={setTab}>
        <TabsList label="종목 정보">
          <TabsTrigger value="overview">종목 정보</TabsTrigger>
          <TabsTrigger value="earnings">실적</TabsTrigger>
          <TabsTrigger value="holders">주주</TabsTrigger>
          <TabsTrigger value="news">뉴스 {news.length > 0 ? news.length : ""}</TabsTrigger>
        </TabsList>
        <TabsContent value="overview">
          {notices.length > 0 && (
            <ul className="notices">
              {notices.map((text) => (
                <li key={text}>{text}</li>
              ))}
            </ul>
          )}
          <p className="desc">{detail.description || "회사 소개가 아직 없습니다."}</p>
          <dl className="facts">
            <div>
              <dt>업종</dt>
              <dd>{s.sector}</dd>
            </div>
            <div>
              <dt>{s.player ? "대표" : "구분"}</dt>
              <dd>{s.player ? s.founder_name ?? "—" : "시스템 운영 회사"}</dd>
            </div>
            <div>
              <dt>발행 주식</dt>
              <dd>{won(detail.shares)}주</dd>
            </div>
            <div>
              <dt>자본총계</dt>
              <dd>{compactWon(detail.equity)}</dd>
            </div>
            <div>
              <dt>주당 순자산</dt>
              <dd>
                {won(detail.bvps)}
                {pbr !== null && <small> · PBR {pbr.toFixed(2)}</small>}
              </dd>
            </div>
            <div>
              <dt>거래대금</dt>
              <dd>{compactWon(detail.turnover)}</dd>
            </div>
            {!s.player && (
              <div>
                <dt>배당수익률</dt>
                <dd>{bpText(detail.dividend_yield_bp)}</dd>
              </div>
            )}
            {s.player && (
              <div>
                <dt>사업 위험도</dt>
                <dd>{RISK_TEXT[detail.risk] ?? detail.risk}</dd>
              </div>
            )}
            <div>
              <dt>다음 실적</dt>
              <dd>{detail.next_earnings_at > 0 ? relativeText(detail.next_earnings_at, now) : "—"}</dd>
            </div>
            {detail.consensus !== null && (
              <div>
                <dt>실적 예상 (순이익)</dt>
                <dd className={tone(detail.consensus)}>
                  {detail.consensus > 0 ? "+" : ""}
                  {compactWon(detail.consensus)}
                </dd>
              </div>
            )}
            <div>
              <dt>{detail.listed_at > 0 ? "상장" : "설립"}</dt>
              <dd>{dateTimeText(detail.listed_at > 0 ? detail.listed_at : detail.founded_at)}</dd>
            </div>
          </dl>
        </TabsContent>
        <TabsContent value="earnings">
          {detail.quarters.length === 0 ? (
            <p className="empty">아직 발표한 실적이 없습니다. 실적은 실제 1주마다 발표됩니다.</p>
          ) : (
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>분기</th>
                    <th>발표</th>
                    <th>순이익</th>
                    <th>예상</th>
                    <th>차이</th>
                    <th>주당 배당</th>
                    <th>자본총계</th>
                  </tr>
                </thead>
                <tbody>
                  {[...detail.quarters].reverse().map((q) => {
                    const surprise = q.consensus !== 0 ? Math.round(((q.profit - q.consensus) * 10000) / Math.abs(q.consensus)) : 0;
                    return (
                      <tr key={q.quarter}>
                        <td>{q.quarter}분기</td>
                        <td>{dateTimeText(q.at)}</td>
                        <td className={tone(q.profit)}>{compactWon(q.profit)}</td>
                        <td>{compactWon(q.consensus)}</td>
                        <td className={tone(surprise)}>{q.consensus !== 0 ? pctText(surprise) : "—"}</td>
                        <td>{q.dividend > 0 ? won(q.dividend) : "—"}</td>
                        <td>{compactWon(q.equity)}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </TabsContent>
        <TabsContent value="holders">
          {detail.holders.length === 0 ? (
            <p className="empty">플레이어 주주가 아직 없습니다.</p>
          ) : (
            <ol className="holders">
              {detail.holders.map((holder, index) => (
                <li key={`${holder.name}-${index}`}>
                  <span>{holder.name}</span>
                  <em>{won(holder.qty)}주</em>
                  <strong>{bpText(holder.share_bp)}</strong>
                </li>
              ))}
            </ol>
          )}
        </TabsContent>
        <TabsContent value="news">
          <NewsList items={news} now={now} empty="이 종목의 뉴스가 아직 없습니다." />
        </TabsContent>
      </Tabs>
    </div>
  );
}
