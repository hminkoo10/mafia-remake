# Mafia Discord Bot

디스코드에서 마피아 게임을 진행해 주는 봇입니다.

참가자 모집, 역할 배정, 밤 행동, 낮 투표, 익명 채팅, 전적/레이팅 기록까지 게임 진행에 필요한 기능을 모두 봇이 처리합니다. Discord Activity(임베디드 앱)를 통해 Discord 앱 내에서 직접 게임 상태를 확인하고 행동을 제출할 수 있습니다.

---

## 기술 스택

- **봇/서버**: Rust (poise, serenity, axum, tokio)
- **Activity 프론트엔드**: React + TypeScript (Vite, Discord Embedded App SDK)
- **배포**: Fly.io (테스트), OCI VPS + Cloudflare (프로덕션)

---

## 프로젝트 구조

```
mafia-remake/
├── src/                   # Rust 봇 소스
│   ├── main.rs            # 진입점, 봇 초기화
│   ├── activity.rs        # Discord Activity REST API + WebSocket 서버
│   ├── runner.rs          # 게임 루프 (밤/낮/투표 진행)
│   ├── commands.rs        # Discord 슬래시 커맨드
│   ├── channel.rs         # Discord 채널/권한 관리
│   ├── game/              # 게임 로직
│   │   ├── mod.rs         # 게임 상태, 플레이어 관리
│   │   ├── actions.rs     # 밤 행동 처리
│   │   ├── resolve.rs     # 밤 결과 정산
│   │   ├── vote.rs        # 투표 처리
│   │   └── actors.rs      # 행동 가능 직업 목록
│   ├── model.rs           # 역할/페이즈 등 데이터 모델
│   ├── web_settings.rs    # 웹 설정 페이지 서버
│   ├── config.rs          # 설정 파일 구조
│   └── stats.rs           # 전적/레이팅 기록
├── activity/              # Discord Activity 프론트엔드 (React)
│   ├── src/
│   │   ├── App.tsx        # 메인 컴포넌트
│   │   ├── discord.ts     # Discord SDK 인증
│   │   ├── api.ts         # 서버 API 클라이언트
│   │   ├── types.ts       # TypeScript 타입 정의
│   │   └── components/
│   │       ├── ActionPanel.tsx   # 밤 행동 / 청부업자 / 스킵 UI
│   │       ├── VotePanel.tsx     # 낮 투표 / 처형 찬반 UI
│   │       ├── PlayerList.tsx    # 플레이어 목록
│   │       ├── RoleCard.tsx      # 내 역할 카드
│   │       └── PhaseTimer.tsx    # 페이즈/타이머 헤더
│   └── package.json
├── Dockerfile             # 멀티스테이지 빌드 (Node → Rust → 최종)
├── fly.toml               # Fly.io 배포 설정
├── config.example.json    # 게임 설정 예시
├── .env.example           # 환경변수 예시
└── docs/                  # 기획 문서
```

---

## 시작하기

### 사전 요구사항

- [Rust](https://rustup.rs/) 1.80+
- [Node.js](https://nodejs.org/) 20+
- Discord 봇 토큰 및 애플리케이션 클라이언트 ID/Secret

### 1. 환경변수 설정

```bash
cp .env.example .env
```

`.env` 파일을 열어 값을 채웁니다:

```env
# 필수
DISCORD_TOKEN=your_bot_token_here
DISCORD_CLIENT_ID=your_client_id_here
DISCORD_CLIENT_SECRET=your_client_secret_here

# 웹 설정 서버 (선택, 기본값 사용 가능)
WEB_SETTINGS_HOST=0.0.0.0
WEB_SETTINGS_PORT=8800

# Discord Activity 서버
ACTIVITY_PORT=2053
ACTIVITY_STATIC_DIR=/path/to/activity/dist

# HTTPS (Cloudflare Origin Certificate 등)
# ACTIVITY_TLS_CERT=/path/to/cert.pub
# ACTIVITY_TLS_KEY=/path/to/cert.key
```

Activity 프론트엔드용 `.env`도 설정합니다:

```bash
cp activity/.env.example activity/.env
```

```env
VITE_CLIENT_ID=your_client_id_here
VITE_MOCK_GUILD_ID=your_guild_id_here   # 로컬 개발용 Mock 서버 ID
```

### 2. Activity 프론트엔드 빌드

```bash
cd activity
npm install
npm run build
cd ..
```

빌드 결과물은 `activity/dist/`에 생성됩니다. `.env`의 `ACTIVITY_STATIC_DIR`이 이 경로를 가리켜야 합니다.

### 3. 봇 실행

```bash
cargo run
```

프로덕션 배포 시:

```bash
cargo run --release
```

---

## 배포

### Fly.io

```bash
fly launch          # 최초 설정
fly secrets set DISCORD_TOKEN=... DISCORD_CLIENT_ID=... DISCORD_CLIENT_SECRET=...
fly deploy
```

`fly.toml`에서 포트(`internal_port = 2053`)와 리전을 조정할 수 있습니다.

### VPS (OCI 등)

Discord Activity는 HTTPS가 필수입니다. Cloudflare를 도메인 앞단에 두고 Cloudflare Origin Certificate를 사용하는 것을 권장합니다.

1. Cloudflare에 도메인 등록
2. Cloudflare Origin Certificate 발급 → `cert.pub`, `cert.key`로 저장
3. `.env`에 인증서 경로 설정 후 실행:

```bash
cargo run --release
```

Cloudflare의 **URL Mappings** 설정에서 Activity 경로를 프록시 도메인으로 매핑해야 합니다.

### Docker

```bash
docker build -t mafia-bot .
docker run --env-file .env mafia-bot
```

---

## Discord 설정

### 봇 권한

봇 초대 시 다음 권한과 인텐트가 필요합니다:

- **Privileged Intents**: Server Members Intent, Message Content Intent, Presence Intent
- **권한**: 채널 관리, 역할 관리, 메시지 전송, 임베드 전송, 웹훅 관리

### Activity 설정

[Discord Developer Portal](https://discord.com/developers/applications) → 앱 선택:

1. **OAuth2 → Redirect URIs**에 Activity URL 추가 (예: `https://1513505888667828224.discordsays.com`)
2. **Activities → URL Mappings**에서 `/` → 서버 도메인으로 매핑
3. 출시 전에는 **App Testers**에 테스터를 등록해야 Activity를 사용할 수 있습니다

---

## 게임 설정

`config.json`으로 관리합니다. 파일이 없으면 `config.example.json`을 복사해 자동 생성합니다.

Discord에서 `/마피아설정` 또는 `/마피아웹설정`으로 게임 중에도 변경할 수 있습니다.

주요 설정 항목:

| 항목 | 설명 | 기본값 |
|------|------|--------|
| `default_mafia_count` | 마피아 기본 인원 | 3 |
| `night_seconds` | 밤 행동 시간(초) | 40 |
| `discussion_seconds` | 낮 토론 시간(초) | 60 |
| `vote_seconds` | 투표 시간(초) | 20 |
| `reveal_death_roles` | 사망 시 역할 공개 여부 | false |
| `anonymous_mode` | 익명 모드 활성화 | false |
| `enable_cult_team` | 교주팀 활성화 | false |

---

## 직업 목록

### 시민팀

| 직업 | 설명 |
|------|------|
| 시민 | 특수 능력 없음 |
| 경찰 | 매 밤 1명을 조사해 마피아 여부 확인 |
| 요원 | 경찰과 유사, 조사 방식 상이 |
| 의사 | 매 밤 1명을 보호 |
| 간호사 | 의사 사망 후 보호 능력 승계 |
| 자경단원 | 매 밤 1명을 처단 (오판 시 패널티) |
| 사립탐정 | 매 밤 1명을 추적해 접선 정보 확인 |
| 기자 | 매 밤 취재로 정보 수집 |
| 해커 | 특정 플레이어의 정보 탈취 |
| 군인 | 마피아 공격을 1회 버팀 |
| 예언자 | 특수 능력 보유 |
| 영매 | 사망자와 교신 |
| 성직자 | 사망자 소생 시도 |
| 심리학자 | 심리 분석 |
| 도둑 | 대상의 역할을 훔쳐 사용 |
| 연인 | 대상과 운명 공동체 |
| 테러리스트 | 지목 중인 상대와 함께 사망 |
| 과학자 | 사망자를 소생시킴 |

### 마피아팀

| 직업 | 설명 |
|------|------|
| 마피아 | 매 밤 공동으로 시민 1명 제거 |
| 대부 | 마피아팀 리더, 경찰 조사 무력화 |
| 건달 | 대상을 위협해 밤 행동 봉쇄 |
| 스파이 | 시민팀으로 위장, 정보 수집 |
| 마담 | 대상을 유혹해 밤 행동 봉쇄 |
| 마녀 | 저주로 대상에게 디버프 부여 |

### 교주팀

| 직업 | 설명 |
|------|------|
| 교주 | 매 밤 시민을 포섭해 광신도로 전환 |
| 광신도 | 포섭된 시민, 교주팀으로 활동 |

### 중립

| 직업 | 설명 |
|------|------|
| 조커 | 처형당하면 승리 |
| 청부업자 | 2명의 역할을 맞추면 암살 (2일 차 밤부터) |
| 악인 | 독자적인 승리 조건 |
| 판사 | 처형 찬반투표 결과를 조작 |
| 정치인 | 처형당해도 부활 |
| 도굴꾼 | 사망자의 역할 정보 탈취 |
| 개구리 | 특수 상태 직업 |

---

## 주요 명령어

| 명령어 | 설명 | 권한 |
|--------|------|------|
| `/마피아시작` | 게임 모집 시작 | 관리자 |
| `/마피아중지` | 진행 중인 게임 강제 종료 | 관리자 |
| `/마피아설정` | 게임 설정 변경 | 관리자 |
| `/마피아웹설정` | 브라우저 설정 페이지 링크 발급 | 관리자 |
| `/마피아활성화` / `/마피아비활성화` | 게임 기능 on/off | 관리자 |
| `/블랙리스트추가` / `/블랙리스트제거` | 참가 차단 관리 | 관리자 |
| `/역할설명` | 전체 역할 목록 안내 | 누구나 |
| `/직업정보 [직업명]` | 특정 직업 상세 안내 | 누구나 |
| `/능력설명` | 직업별 능력 안내 | 누구나 |
| `/상태` | 현재 게임 상태 확인 | 누구나 |
| `/내정보` | 내 전적 및 레이팅 확인 | 누구나 |
| `/리더보드 [기준]` | 전적 순위 확인 | 누구나 |
| `/메모` | 게임 중 메모 작성 | 참가자 |

---

## Discord Activity

Discord 앱 내 임베디드 UI에서 게임을 진행할 수 있습니다.

### 지원 기능

- 밤 행동 대상 지목 (전 직업)
- 청부업자 전용 UI (대상 2명 + 역할 추측 드롭다운)
- 낮 투표 / 처형 찬반 투표
- 낮 스킵 투표 (과반수 현황 표시)
- 밤 행동 결과 배너 (조사 결과, 추적 결과 등 — 낮에 표시)
- 플레이어 목록 (생존/사망, 득표수)
- 게임 종료 시 승자 표시

### Activity API 엔드포인트

서버는 기본 포트 `2053`에서 실행됩니다.

| 엔드포인트 | 설명 |
|-----------|------|
| `GET /activity/api/auth?code=&guild_id=` | OAuth2 코드 → 세션 토큰 교환 |
| `GET /activity/api/state?guild_id=` | 현재 게임 상태 조회 |
| `POST /activity/api/action` | 게임 행동 제출 |
| `WS /activity/api/ws?guild_id=&token=` | 실시간 게임 상태 스트림 (1초 폴링) |

---

## 웹 관리 페이지

봇과 함께 기본 포트 `8800`에서 웹 서버가 실행됩니다.

| URL | 설명 |
|-----|------|
| `/status` | 공개 상태판 |
| `/leaderboard` | 공개 리더보드 |
| `/api/docs` | API 문서 |
| `/api/status` | 봇 상태 JSON |
| `/api/games` | 진행 중 게임 목록 |
| `/api/stats` | 전적 요약 |
| `/api/leaderboard/{기준}` | 리더보드 (`rating`, `wins`, `winrate`, `games`, `mafia`, `playtime`) |

`/마피아웹설정` 명령어로 관리자 전용 설정 편집 페이지의 1회용 링크를 발급받을 수 있습니다 (10분 유효, 1회 사용).

외부에서 접속하려면 방화벽/리버스 프록시로 `WEB_SETTINGS_PORT`를 노출하고, `WEB_SETTINGS_BASE_URL`로 공개 주소를 지정하세요.

---

## 레이팅 시스템

Elo 기반 레이팅을 사용합니다.

- 초기 레이팅: **1000**
- 배치 판수: **10판** (배치 중에는 리더보드에 "배치중" 표시)
- 상대 진영의 평균 레이팅을 기준으로 기대 승률을 계산해 점수를 가감합니다
- 승패 외에도 역할 핵심 능력 수행 여부가 보정 요소로 반영됩니다

자세한 내용은 [`docs/rating_plan.md`](docs/rating_plan.md)를 참고하세요.
