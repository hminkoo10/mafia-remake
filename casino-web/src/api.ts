import { deviceNow } from "./state-sync";
import type { ApiError, CasinoCommand } from "./types";

export class CasinoApiError extends Error {
  code: string;
  status: number;
  constructor(status: number, error: ApiError) {
    super(error.message);
    this.code = error.code;
    this.status = status;
  }
}

/** 서버에 닿지 못했거나(끊김) 답이 너무 늦은 요청. 명령이 서버에서 처리됐을 수도 있다. */
export class CasinoNetworkError extends Error {
  kind: "timeout" | "network";
  constructor(kind: "timeout" | "network") {
    super(kind === "timeout" ? "서버 응답이 늦어요. 잠시 후 상태를 다시 확인할게요." : "연결이 잠시 불안정해요. 다시 연결하는 중이에요.");
    this.kind = kind;
  }
}

/** 받은 상태의 원문과 받은 시각 (시계 맞추기는 JSON을 풀기 전에 잰 시각으로 한다). */
export interface RawResponse {
  raw: string;
  receivedAt: number;
}

/** 요청 제한 시간: 답이 없는 요청이 버튼을 계속 막거나 새로 받기를 멈추지 않게 한다. */
const REQUEST_TIMEOUT_MS = 8000;

/** 개인 링크 /casino/<토큰>?table=<id> 에서 토큰과 테이블을 읽는다. */
export function readLink(): { token: string | null; table: string | null } {
  const parts = window.location.pathname.split("/").filter(Boolean);
  const casinoIndex = parts.indexOf("casino");
  const token = casinoIndex >= 0 && parts.length > casinoIndex + 1 ? parts[casinoIndex + 1] : null;
  const table = new URLSearchParams(window.location.search).get("table");
  return { token, table };
}

function apiBase(): string {
  return `${window.location.origin}/casino/api`;
}

async function parseError(response: Response): Promise<CasinoApiError> {
  let error: ApiError = { code: "HTTP_ERROR", message: `요청 실패 (${response.status})` };
  try {
    const body = await response.json();
    if (body && body.error) {
      error = body.error as ApiError;
    }
  } catch {
    // 본문이 JSON이 아니면 기본 메시지를 쓴다.
  }
  return new CasinoApiError(response.status, error);
}

/** 제한 시간을 두고 요청해 원문을 받는다. 끊김·시간 초과는 한국어 안내가 담긴 CasinoNetworkError로 바꾼다. */
async function request(url: string, init: RequestInit): Promise<RawResponse> {
  const controller = new AbortController();
  let timedOut = false;
  const timer = window.setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, REQUEST_TIMEOUT_MS);
  try {
    const response = await fetch(url, { ...init, signal: controller.signal });
    // 헤더가 도착한 시각: 서버가 server_time을 찍은 뒤 가장 가까운 순간이다 (시계 표본은 단조 시계로 잰다).
    const receivedAt = deviceNow();
    if (!response.ok) {
      throw await parseError(response);
    }
    return { raw: await response.text(), receivedAt };
  } catch (e) {
    if (e instanceof CasinoApiError) throw e;
    throw new CasinoNetworkError(timedOut ? "timeout" : "network");
  } finally {
    window.clearTimeout(timer);
  }
}

export function fetchState(token: string, table: string | null): Promise<RawResponse> {
  const url = new URL(`${apiBase()}/state`);
  if (table) {
    url.searchParams.set("table", table);
  }
  return request(url.toString(), {
    headers: { Authorization: `Bearer ${token}` },
  });
}

export function sendCommand(token: string, tableId: string, version: number | null, command: CasinoCommand): Promise<RawResponse> {
  return request(`${apiBase()}/command`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ table_id: tableId, version, command }),
  });
}

export function wsUrl(token: string, table: string | null): string {
  const scheme = window.location.protocol === "https:" ? "wss" : "ws";
  const url = new URL(`${scheme}://${window.location.host}/casino/api/ws`);
  url.searchParams.set("token", token);
  if (table) {
    url.searchParams.set("table", table);
  }
  return url.toString();
}
