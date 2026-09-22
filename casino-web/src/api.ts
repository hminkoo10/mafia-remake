import type { ApiError, CasinoCommand, StateResponse } from "./types";

export class CasinoApiError extends Error {
  code: string;
  status: number;
  constructor(status: number, error: ApiError) {
    super(error.message);
    this.code = error.code;
    this.status = status;
  }
}

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

export async function fetchState(token: string, table: string | null): Promise<StateResponse> {
  const url = new URL(`${apiBase()}/state`);
  if (table) {
    url.searchParams.set("table", table);
  }
  const response = await fetch(url.toString(), {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok) {
    throw await parseError(response);
  }
  return (await response.json()) as StateResponse;
}

export async function sendCommand(
  token: string,
  tableId: string,
  version: number | null,
  command: CasinoCommand,
): Promise<StateResponse> {
  const response = await fetch(`${apiBase()}/command`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ table_id: tableId, version, command }),
  });
  if (!response.ok) {
    throw await parseError(response);
  }
  return (await response.json()) as StateResponse;
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
