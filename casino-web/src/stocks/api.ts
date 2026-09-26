// 웹 증권 화면 API (/casino/api/stocks/...). 카지노 개인 링크의 토큰으로 인증한다.
import { request } from "../api";
import type { ActionResponse, CandleRange, CandleResponse, CompanyAction, OrderResult, Side, StockState } from "./types";

const base = () => `${window.location.origin}/casino/api/stocks`;

async function getJson<T>(url: URL, token: string): Promise<T> {
  const { raw } = await request(url.toString(), { headers: { Authorization: `Bearer ${token}` } });
  return JSON.parse(raw) as T;
}

async function postJson<T>(path: string, token: string, body: unknown): Promise<T> {
  const { raw } = await request(`${base()}${path}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return JSON.parse(raw) as T;
}

export function fetchStockState(token: string, code: string | null): Promise<StockState> {
  const url = new URL(`${base()}/state`);
  if (code) url.searchParams.set("code", code);
  return getJson<StockState>(url, token);
}

export function fetchCandles(token: string, code: string, range: CandleRange): Promise<CandleResponse> {
  const url = new URL(`${base()}/candles`);
  url.searchParams.set("code", code);
  url.searchParams.set("range", range);
  return getJson<CandleResponse>(url, token);
}

export function placeOrder(
  token: string,
  order: { code: string; side: Side; qty: number; price?: number },
): Promise<ActionResponse<OrderResult>> {
  return postJson("/order", token, order);
}

export function cancelOrder(token: string, orderId: number, code: string | null): Promise<ActionResponse<null>> {
  return postJson("/cancel", token, { order_id: orderId, code });
}

export function subscribeIpo(token: string, code: string, qty: number): Promise<ActionResponse<number>> {
  return postJson("/subscribe", token, { code, qty });
}

export function unsubscribeIpo(token: string, code: string): Promise<ActionResponse<number>> {
  return postJson("/unsubscribe", token, { code });
}

export function companyAction(token: string, action: CompanyAction): Promise<ActionResponse<null>> {
  return postJson("/company", token, action);
}
