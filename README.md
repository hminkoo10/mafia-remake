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

# 본 서버 ID (봇이 여러 서버에 있으면 필수). 관리 명령은 이 서버에서만 받습니다.
# config.json의 home_guild_id보다 우선하고, 설정을 저장하면 config.json에도 기록됩니다.
# 둘 다 비어 있고 봇이 서버 하나에만 있으면 시작할 때 그 서버로 정합니다.
# HOME_GUILD_ID=your_home_guild_id_here

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

## 카지노

Discord 안에서 **홀덤**과 **블랙잭** 테이블을 열고 봇의 코인으로 플레이하는 기능입니다. 게임 화면은 웹(개인 링크)이고, Discord 채널은 상태판과 채팅 중계를 맡습니다.

### 흐름

1. 관리자가 `/카지노테이블생성`으로 테이블을 만들면 `#카지노-<이름>` 채널이 생기고 상태 임베드가 고정됩니다
2. 참가자는 임베드의 **테이블 입장** 버튼이나 `/카지노입장`으로 본인 전용 링크(`/casino/<토큰>?table=<id>`, 12시간 유효)를 받습니다
3. 웹에서 좌석(6석)을 고르고 코인으로 **바이인**(기본 5,000~20,000, 100 단위, 테이블마다 다를 수 있음)을 하면 테이블 칩이 됩니다. 한 번에 한 테이블에만 앉을 수 있습니다
4. 퇴장하거나(라운드 중이면 핸드가 끝난 뒤), 라운드 없이 30분 방치되거나, 테이블이 닫히면 남은 칩이 코인으로 돌아옵니다

### 명령어

| 명령어 | 설명 | 권한 |
|--------|------|------|
| `/카지노테이블생성 [종류] [이름] [방 설정…]` | 홀덤/블랙잭 테이블 생성 + 전용 채널 (이름 2~24자, 최대 12개). 방 설정은 아래 표 | 관리자 |
| `/카지노테이블닫기 [테이블]` | 테이블을 닫고 칩을 코인으로 돌려준 뒤 채널 삭제 | 관리자 |
| `/카지노테이블목록` | 열려 있는 테이블과 판돈·인원·단계 | 누구나 |
| `/카지노입장 [테이블]` | 개인 링크 발급 (비우면 첫 테이블, 본인에게만 표시) | 누구나 |
| `/카지노상태` | 내 코인, 앉은 테이블, 하우스 누적 | 누구나 |

### 방 설정 (테이블 생성 옵션)

모두 선택 항목입니다. 비우면 기본값이나 다른 값에 맞춘 자동 값을 씁니다. 게임과 상관없는 항목은 무시합니다. 만든 뒤에는 바꿀 수 없으니 바꾸려면 테이블을 닫고 다시 만듭니다.

| 옵션 | 게임 | 기본값 | 범위 |
|---|---|---|---|
| `최소베팅` | 블랙잭 | 100 | 100 이상, 100 단위 |
| `최대베팅` | 블랙잭 | 최소 베팅×50 (최소 5,000) | 최소 베팅 이상, 100 단위 |
| `사이드최대` | 블랙잭 | 최대 베팅의 절반 | 0(사이드베팅 없음) 또는 100~최대 베팅 |
| `빅블라인드` | 홀덤 | 100 (스몰은 절반) | 10~50,000 짝수 |
| `최소바이인` | 공통 | 홀덤 빅 블라인드×50, 블랙잭 최소 베팅×50 (최소 5,000) | 홀덤 빅 블라인드×10 이상, 블랙잭 최소 베팅 이상, 100 단위 |
| `최대바이인` | 공통 | 홀덤 빅 블라인드×200, 블랙잭 최대 베팅×4 | 최소 바이인 이상, 100 단위 |
| `제한시간` | 공통 | 30초 | 10~120초 |

예: `/카지노테이블생성 종류:블랙잭 이름:하이리밋 최소베팅:1000` → 베팅 1,000~50,000, 사이드 최대 25,000, 바이인 50,000~200,000. 설정은 `casino.json`에 테이블과 함께 저장되고, 예전 저장본은 기본값으로 읽습니다.

### Discord 채널 중계

- 고정 상태 임베드: 판돈·바이인, 단계, 팟/딜러 카드, 좌석별 칩·베팅, 딜러 안내. 변경이 있으면 테이블당 1.5초 간격으로 갱신
- 핸드/라운드 결과는 별도 임베드로 게시 (참가자별 순손익, 보드/딜러 카드)
- 채팅은 양방향: 웹 채팅은 `Mafia Casino` 웹훅(참가자 아바타, 딜러는 딜러 얼굴)으로 채널에, 채널 메시지는 테이블 채팅으로 전달 (240자)
- 테이블 생성/닫기는 관리 로그 채널에 기록됩니다. 봇에 **채널 관리·웹훅 관리** 권한이 필요합니다

### 홀덤 규칙

- 2~6인 노 리밋 텍사스 홀덤, 블라인드 기본 **50/100**(방 설정), 헤즈업은 버튼이 스몰 블라인드
- 액션 제한 기본 **30초**(방 설정). 시간 초과 시 체크 가능하면 체크, 아니면 폴드. 2회 연속 초과하면 자리 비움(웹에서 복귀)
- 레이즈는 슬라이더 또는 최소 · ½ 팟 · ¾ 팟 · 팟 · 올인 버튼
- 사이드팟 정산, 나눠지지 않는 칩은 버튼 왼쪽부터

### 블랙잭 규칙 (에볼루션 스타일)

- 8덱 슈를 이어서 쓰고 빨간 컷 카드가 나오면 다음 라운드 전에 섞음. 딜러는 소프트 17 스탠드(S17), 블랙잭 3:2, 딜러 피크(에이스면 인슈어런스 후 확인)
- 베팅창 **15초** (기본 100~5,000, 100 단위, 방 설정). 베팅 가능한 참가자가 모두 걸면 바로 딜. 액션 기본 30초, 초과 시 스탠드
- 베팅: 칩을 끌어서 퍼펙트 페어·메인·21+3 원(또는 내 좌석의 자리)에 놓거나, 칩을 고른 뒤 자리를 누름. 마감 2초 전에 쌓인 칩은 자동 확정. 칩 단위는 테이블 한도에 맞춰 바뀜
- 더블은 처음 두 장에서(스플릿 후에도 가능), 스플릿은 1회, 에이스 스플릿은 한 장씩만, 서렌더 없음
- 인슈어런스: 딜러 에이스일 때 **10초** 안에 결정, 베팅의 절반, 딜러 블랙잭이면 2:1
- 사이드베팅(기본 100~2,500, 100 단위, 방 설정에서 끌 수 있음)은 딜 직후 정산

| 사이드베팅 | 조건 | 배당 |
|---|---|---|
| 퍼펙트 페어 | 믹스 페어 / 컬러 페어 / 퍼펙트 페어 | 6:1 / 12:1 / 25:1 |
| 21+3 | 플러시 / 스트레이트 / 트리플 / 스트레이트 플러시 / 수티드 트립스 | 5:1 / 10:1 / 30:1 / 40:1 / 100:1 |

### 연출·딜러

- 카드는 시간차로 열리고 그동안 액션이 막힙니다 (`src/casino/table.rs` 상수, 테스트에서는 0): 카드당 380ms(히트·더블·스플릿 카드 포함), 스트리트 전 900ms, 쇼다운 좌석당 800ms, 딜러 드로우 900ms, 정산 전 700ms
- 카드는 딜러 사진 속 슈에서 뒷면으로 날아와 자리에 놓이며 앞면이 보입니다. 블랙잭은 모든 참가자의 핸드(스플릿은 두 핸드)와 점수·결과가 좌석 앞에 보이고, 내 자리는 금색으로 강조됩니다
- 딜러는 12판마다 교대합니다 (`DEALERS` 중 초상이 있는 딜러만). 초상·클립 규격은 [`casino-web/public/dealers/README.md`](casino-web/public/dealers/README.md)

### 웹·설정

- 프론트엔드 `casino-web/`(Vite + React)는 Activity 서버와 같은 포트에서 `/casino/...`로 서비스됩니다. `build.rs`가 `casino-web/dist`를 바이너리에 내장합니다 (npm이 있으면 자동 빌드, `MAFIA_SKIP_CASINO_BUILD`로 생략)
- API: `GET /casino/api/state`, `POST /casino/api/command`, `WS /casino/api/ws` (개인 링크 토큰을 Bearer로 사용)
- 테이블 상태와 하우스 누적은 `casino.json`에, 코인은 `stats.json`에 저장됩니다. 봇을 재시작하면 테이블은 남지만 개인 링크는 다시 받아야 합니다

```env
# 개인 링크 공개 주소 (비우면 WEB_SETTINGS_BASE_URL 호스트 + ACTIVITY_PORT)
# CASINO_BASE_URL=https://example.com:2053
# 내장된 카지노 웹 대신 디스크의 dist를 쓰려면
# CASINO_STATIC_DIR=/path/to/casino-web/dist
```

UI만 점검할 때는 Discord 없이 개발 서버를 띄울 수 있습니다:

```bash
cargo run -- --casino-dev     # http://localhost:8811/casino/ (CASINO_DEV_PORT로 변경)
cd casino-web && npm run dev  # Vite 5174, /casino/api 는 localhost:2053으로 프록시
```

테스트 계정 3개(각 50,000코인)와 홀덤·블랙잭 테이블이 하나씩 만들어지고 개인 링크가 출력됩니다. 상태는 `casino-dev.json`에 저장됩니다.

---

## Discord 설정

### 봇 권한

봇 초대 시 다음 권한과 인텐트가 필요합니다:

- **Privileged Intents**: Server Members Intent, Message Content Intent, Presence Intent
- **권한**: 채널 관리, 역할 관리, 메시지 전송, 임베드 전송, 웹훅 관리

### 레이트리밋 분산 (워커 봇 토큰)

Discord 레이트리밋 버킷(전역 50/s 포함)은 **봇 토큰 단위**로 집계됩니다. 게임 진행 중 페이즈 전환 때 채널 권한 오버라이트가 대량으로 몰리면 단일 토큰의 예산이 금방 소진되어 봇이 느려집니다. `DISCORD_WORKER_TOKENS`에 추가 봇 토큰을 넣으면 봇 정체성과 무관한 길드 관리용 REST 호출이 여러 토큰으로 라운드로빈 분산되어 예산이 **(N+1)배**가 됩니다.

```env
# 쉼표/공백/줄바꿈으로 여러 개 구분
DISCORD_WORKER_TOKENS=worker_token_1,worker_token_2,worker_token_3
```

- **분산되는 작업**: 채널 권한 오버라이트 생성/삭제, 채널 생성/삭제/수정(슬로우모드·토픽), 멤버 역할 부여/회수, 익명 채팅 웹훅 생성
- **항상 메인 토큰이 처리**: 슬래시/버튼 인터랙션 응답(인터랙션 토큰은 메인 앱 전용), 상태 메시지 생성/수정(메시지는 작성한 봇만 수정 가능), 웹훅 실행(이미 전역 레이트리밋 면제)

**워커 봇 준비**

1. 각 워커 봇을 게임 서버에 초대 — 권한: **채널 관리, 역할 관리, 웹훅 관리**
2. 워커 봇의 역할을 관리 대상(참가자/사망자/관전자 역할, 게임 채널)보다 **위**에 배치
3. 봇 토큰만 있으면 됩니다. 워커는 REST 호출을 처리하고, 동시에 최소 게이트웨이 연결로 **온라인 상태**(시청 중: 마피아 게임)를 표시합니다. 이벤트/명령은 처리하지 않습니다

기동 시 각 워커 토큰을 검증하고, 검증 실패한 토큰은 버립니다. 런타임에 워커 호출이 실패하면(권한 부족·미초대 등) 해당 호출은 메인 토큰으로 자동 폴백하므로, 워커 설정이 잘못되어도 게임은 기존(메인 토큰 단독) 동작으로 강등될 뿐 중단되지 않습니다.

> 참고: 채널 이름/토픽 수정은 Discord가 **채널당 10분에 2회**로 토큰과 무관하게 서버측에서 제한합니다. 이 한도는 토큰을 늘려도 우회되지 않습니다(코드가 토픽을 캐시해 불필요한 수정을 이미 억제합니다).

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
| `show_confirmation_vote_counts` | 찬반투표 집계 공개 여부 | true |
| `anonymous_mode` | 익명 모드 활성화 | false |
| `enable_inspector` | 형사 활성화 | true |
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
| 형사 | 밤에 1명을 수사해 같은 팀이면 직업 확인, 같은 팀 대상에게 형사 정체 전달 |
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
| 테러리스트 | 밤에는 다른 팀 지목 반격, 투표 처형 시 최후 반론에서 고른 접선 완료 마피아팀 습격 |

### 마피아팀

| 직업 | 설명 |
|------|------|
| 마피아 | 매 밤 공동으로 시민 1명 제거 |
| 대부 | 마피아팀 리더, 경찰 조사 무력화 |
| 건달 | 대상을 위협해 밤 행동 봉쇄 |
| 스파이 | 시민팀으로 위장, 정보 수집 |
| 마담 | 대상을 유혹해 밤 행동 봉쇄 |
| 마녀 | 저주로 대상에게 디버프 부여 |
| 과학자 | 처음부터 마피아팀. 첫 사망 전에는 미접선 보조처럼 위장 판정을 받고, 첫 사망 후 접선되어 다음 밤 부활 |

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
| `/코인선물 [대상] [금액]` | 내 코인을 다른 멤버에게 선물 (봇·자기 자신 불가, 진행 중인 게임에 걸린 배팅액은 정산 전까지 남겨 둠, 관리 로그에 기록) | 누구나 |
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

### 보호 API

웹 설정 페이지의 **API 키 관리**에서 서버별 API 키를 발급하거나 폐기할 수 있습니다. 키 원문은 발급 직후 한 번만 표시되며, 서버에는 해시만 저장됩니다. 키는 발급한 Discord 서버 범위에서만 사용할 수 있습니다.

보호 API는 `X-API-Key: <key>` 또는 `Authorization: Bearer <key>` 헤더가 필요합니다.

| 엔드포인트 | 설명 |
|-----|-----|
| `GET /api/v1/me` | 현재 API 키 정보 |
| `GET /api/v1/config` | 게임 설정 요약 |
| `GET /api/v1/stats` | 전적 요약 |
| `GET /api/v1/stats/leaderboard` | Laravel-friendly 리더보드 (`sort`, `limit`) |
| `GET /api/v1/stats/user/{user_id}` | Laravel-friendly 개인 전적 프로필 |
| `GET /api/v1/stats/user/{user_id}/games` | Laravel-friendly 개인 게임 이력 (`page`, `per_page`) |
| `GET /stats/leaderboard`, `/stats/user/{user_id}`, `/stats/user/{user_id}/games` | Laravel 명세 호환 alias. API 키 필요 |
| `GET /api/v1/leaderboard/{metric}` | 보호 리더보드 조회 |
| `GET /api/v1/games` | 키 발급 서버의 진행 중 게임 목록 |
| `GET /api/v1/games/recent` | Laravel-friendly 최근 종료 게임 목록 (`page`, `limit`/`per_page`) |
| `GET /api/v1/game/{game_key}` | Laravel-friendly 종료 게임 요약 |
| `GET /api/v1/game/{game_key}/result` | Laravel-friendly 종료 게임 결과 요약 |
| `GET /api/v1/game/{game_key}/events` | Laravel-friendly 리플레이 이벤트 타임라인 |
| `GET /games/recent`, `/game/{game_key}`, `/game/{game_key}/result`, `/game/{game_key}/events` | Laravel 명세 호환 alias. API 키 필요 |
| `GET /api/v1/games/{guild_id}` | 참가자·직업·단계·타이머를 포함한 게임 상세 |
| `GET /api/v1/games/{guild_id}/replay` | 참가자, 투표, 직업 행동, 단계 결과, 레이팅 로그를 포함한 전체 리플레이 |
| `POST /api/v1/games/{guild_id}/actions` | `skip_day`, `extend_day`, `stop` 작업 |
| `GET /api/v1/replays` | API 키 서버의 진행 중/최근 종료 리플레이 목록 |
| `GET /api/v1/replays/{game_key}` | `game_key`로 전체 리플레이 조회 |
| `GET /api/v1/recruitments/{guild_id}` | 모집 인원·역할 구성·관전자 상세 |
| `POST /api/v1/recruitments/{guild_id}/actions` | `start` 또는 `cancel` 작업 |

예시:

```bash
curl -H "X-API-Key: mfr_..." https://example.com/api/v1/games/123456789
curl -X POST -H "Authorization: Bearer mfr_..." -H "Content-Type: application/json" \
  -d '{"action":"skip_day"}' https://example.com/api/v1/games/123456789/actions
```

외부에서 접속하려면 방화벽/리버스 프록시로 `WEB_SETTINGS_PORT`를 노출하고, `WEB_SETTINGS_BASE_URL`로 공개 주소를 지정하세요.

---

## 레이팅 시스템

Elo 기반 레이팅을 사용합니다.

- 초기 레이팅: **1000**
- 상대 진영의 평균 레이팅을 기준으로 기대 승률을 계산해 점수를 가감합니다
- 낮은 레이팅은 승리 보상이 크고 패배 손실이 작습니다
- 높은 레이팅은 승리 보상이 작고 패배 손실이 큽니다
- 승패 Elo에 역할 성공·핵심 능력 미사용 보정을 합산합니다 (`role_delta` ±14, 전체 ±80)
- 패배팀은 역할 활약이 있어도 한 판 최대 `+5`까지만 상승합니다
- 레이팅 구간은 `C`, `B`, `A`, `S`, `SS`, `X` 랭크로 표시됩니다
- 랭크 기준: `C < 950`, `B 950~1099`, `A 1100~1299`, `S 1300~1549`, `SS 1550~1849`, `X >= 1850`

자세한 내용은 [`docs/rating_plan.md`](docs/rating_plan.md)를 참고하세요.
