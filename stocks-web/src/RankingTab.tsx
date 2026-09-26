// 주식 재산 순위 (주식 평가액 + 주문·청약에 묶인 코인). 탭을 여는 동안 30초마다 새로 받는다.
import { useEffect, useState } from "react";
import { fetchRanking } from "./api";
import { signedWon, tone, won } from "./format";
import type { RankingResponse } from "./types";

export function RankingTab({ token }: { token: string }) {
  const [ranking, setRanking] = useState<RankingResponse | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const next = await fetchRanking(token);
        if (!cancelled) {
          setRanking(next);
          setError("");
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : "순위를 불러오지 못했습니다.");
      }
    };
    void load();
    const id = window.setInterval(() => {
      if (!document.hidden) void load();
    }, 30_000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [token]);

  if (error) return <p className="empty">{error}</p>;
  if (!ranking) return <p className="empty">순위를 불러오는 중…</p>;
  if (ranking.rows.length === 0) return <p className="empty">아직 주식을 가진 사람이 없습니다.</p>;
  const mineOutside = ranking.mine && !ranking.rows.some((row) => row.me) ? ranking.mine : null;
  return (
    <div className="table-wrap">
      <table className="table ranking">
        <thead>
          <tr>
            <th>순위</th>
            <th>이름</th>
            <th>주식 재산</th>
            <th>손익</th>
          </tr>
        </thead>
        <tbody>
          {ranking.rows.map((row) => (
            <tr key={row.rank} className={row.me ? "me" : ""}>
              <td>{row.rank}</td>
              <td>{row.name}</td>
              <td>{won(row.value)}</td>
              <td className={tone(row.profit)}>{signedWon(row.profit)}</td>
            </tr>
          ))}
          {mineOutside && (
            <tr className="me">
              <td>{mineOutside.rank}</td>
              <td>{mineOutside.name}</td>
              <td>{won(mineOutside.value)}</td>
              <td className={tone(mineOutside.profit)}>{signedWon(mineOutside.profit)}</td>
            </tr>
          )}
        </tbody>
      </table>
      <p className="foot">
        주식 재산은 보유 주식 평가액과 주문·청약에 묶인 코인의 합이고, 손익은 평가 손익과 실현 손익의 합입니다. 전체 {won(ranking.total)}명.
      </p>
    </div>
  );
}
