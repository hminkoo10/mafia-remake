import type { ActionRequest, GameState } from "./types";

let sessionToken = "";
let guildId = "";

export function setSession(token: string, guild: string) {
  sessionToken = token;
  guildId = guild;
}

function headers(): HeadersInit {
  return {
    "Content-Type": "application/json",
    "X-Session-Token": sessionToken,
  };
}

export async function fetchState(): Promise<GameState> {
  const res = await fetch(
    `/activity/api/state?guild_id=${guildId}`,
    { headers: headers() }
  );
  if (!res.ok) throw new Error(`state fetch failed: ${res.status}`);
  return res.json();
}

export async function sendAction(req: Omit<ActionRequest, "guild_id">): Promise<{ ok: boolean; message?: string }> {
  const res = await fetch("/activity/api/action", {
    method: "POST",
    headers: headers(),
    body: JSON.stringify({ ...req, guild_id: guildId }),
  });
  return res.json();
}

export function createWebSocket(onMessage: (state: GameState) => void): WebSocket {
  const protocol = location.protocol === "https:" ? "wss" : "ws";
  // 브라우저는 WebSocket 업그레이드 시 커스텀 헤더를 보낼 수 없으므로 토큰을 쿼리 파라미터로 전달
  const ws = new WebSocket(
    `${protocol}://${location.host}/activity/api/ws?guild_id=${guildId}&token=${encodeURIComponent(sessionToken)}`
  );

  ws.addEventListener("message", (event) => {
    try {
      const state: GameState = JSON.parse(event.data);
      onMessage(state);
    } catch {}
  });

  return ws;
}
