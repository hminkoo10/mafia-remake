import { DiscordSDK, DiscordSDKMock } from "@discord/embedded-app-sdk";

// 개발 환경에선 Mock SDK 사용
const isEmbedded = window.location.href.includes("discord.com") ||
  window.location.href.includes("discordsays.com");

export let discordSdk: DiscordSDK | DiscordSDKMock;

if (isEmbedded) {
  discordSdk = new DiscordSDK(import.meta.env.VITE_CLIENT_ID ?? "");
} else {
  // 개발용 Mock: VITE_MOCK_USER_ID, VITE_MOCK_GUILD_ID env 설정
  discordSdk = new DiscordSDKMock(
    import.meta.env.VITE_CLIENT_ID ?? "",
    import.meta.env.VITE_MOCK_GUILD_ID ?? null,
    null,
    null,
  );
}

export interface AuthResult {
  sessionToken: string;
  userId: string;
  username: string;
  guildId: string;
}

export async function authenticateWithDiscord(): Promise<AuthResult> {
  await discordSdk.ready();

  // OAuth2 Authorization Code
  const { code } = await discordSdk.commands.authorize({
    client_id: import.meta.env.VITE_CLIENT_ID ?? "",
    response_type: "code",
    state: "",
    prompt: "none",
    scope: ["identify", "guilds"],
  });

  const guild = discordSdk.guildId ?? "";

  // 백엔드에 코드 전달 → 세션 토큰 발급
  const res = await fetch(`/activity/api/auth?code=${code}&guild_id=${guild}`);
  if (!res.ok) throw new Error("Authentication failed");

  const { session_token, user_id, username } = await res.json();

  return {
    sessionToken: session_token,
    userId: user_id,
    username,
    guildId: guild,
  };
}
