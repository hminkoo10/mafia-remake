// 증권 사이트 API (/stocks/api/...). 개인 링크(/stocks/<토큰>)의 토큰으로 인증한다.
import type { ActionResponse, CandleRange, CandleResponse, CompanyAction, OrderResult, Side, StockState } from "./types";

/** 서버가 거절한 요청 (안내 문구는 서버가 준다). */
export class ApiError extends Error {
  code: string;
  status: number;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

/** 서버에 닿지 못했거나 답이 너무 늦은 요청. */
export class NetworkError extends Error {
  constructor(timedOut: boolean) {
    super(timedOut ? "서버 응답이 늦어요. 잠시 후 다시 시도해 주세요." : "연결이 잠시 불안정해요. 다시 연결하는 중이에요.");
  }
}

/** 요청 제한 시간: 답이 없는 요청이 버튼을 계속 막지 않게 한다. */
const REQUEST_TIMEOUT_MS = 8000;

/** 개인 링크 /stocks/<토큰> 에서 토큰을 읽는다. */
export function readToken(): string | null {
  const parts = window.location.pathname.split("/").filter(Boolean);
  const index = parts.indexOf("stocks");
  return index >= 0 && parts.length > index + 1 ? parts[index + 1] : null;
}

const base = () => `${window.location.origin}/stocks/api`;

async function request<T>(path: string, token: string, init: RequestInit = {}): Promise<T> {
  const controller = new AbortController();
  let timedOut = false;
  const timer = window.setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, REQUEST_TIMEOUT_MS);
  try {
    const response = await fetch(`${base()}${path}`, {
      ...init,
      headers: { Authorization: `Bearer ${token}`, ...(init.body ? { "Content-Type": "application/json" } : {}) },
      signal: controller.signal,
    });
    const text = await response.text();
    if (!response.ok) {
      let code = "HTTP_ERROR";
      let message = `요청을 처리하지 못했습니다 (${response.status}).`;
      try {
        const body = JSON.parse(text);
        if (body?.error) {
          code = body.error.code;
          message = body.error.message;
        }
      } catch {
        // 본문이 JSON이 아니면 기본 문구를 쓴다.
      }
      throw new ApiError(response.status, code, message);
    }
    return JSON.parse(text) as T;
  } catch (e) {
    if (e instanceof ApiError) throw e;
    throw new NetworkError(timedOut);
  } finally {
    window.clearTimeout(timer);
  }
}

const post = <T>(path: string, token: string, body: unknown) => request<T>(path, token, { method: "POST", body: JSON.stringify(body) });

export function fetchStockState(token: string, code: string | null): Promise<StockState> {
  return request(code ? `/state?code=${encodeURIComponent(code)}` : "/state", token);
}

export function fetchCandles(token: string, code: string, range: CandleRange): Promise<CandleResponse> {
  return request(`/candles?code=${encodeURIComponent(code)}&range=${range}`, token);
}

export function placeOrder(
  token: string,
  order: { code: string; side: Side; qty: number; price?: number },
): Promise<ActionResponse<OrderResult>> {
  return post("/order", token, order);
}

export function cancelOrder(token: string, orderId: number, code: string | null): Promise<ActionResponse<null>> {
  return post("/cancel", token, { order_id: orderId, code });
}

export function subscribeIpo(token: string, code: string, qty: number): Promise<ActionResponse<number>> {
  return post("/subscribe", token, { code, qty });
}

export function unsubscribeIpo(token: string, code: string): Promise<ActionResponse<number>> {
  return post("/unsubscribe", token, { code });
}

export function companyAction(token: string, action: CompanyAction): Promise<ActionResponse<null>> {
  return post("/company", token, action);
}
