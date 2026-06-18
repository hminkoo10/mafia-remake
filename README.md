# Mafia Discord Bot

디스코드에서 마피아 게임을 진행해 주는 봇입니다.

참가자 모집, 역할 배정, 밤 행동, 투표, 익명 채팅, 전적 기록까지 게임 진행에 필요한 기능을 봇이 처리합니다.

## 실행 방법

### 1. 프론트엔드 빌드

Activity 기능을 사용하려면 먼저 프론트엔드를 빌드해야 합니다.

```bash
cd activity
npm install
npm run build
cd ..
```

빌드 결과물은 `activity/dist/`에 생성됩니다. `.env`의 `ACTIVITY_STATIC_DIR`이 이 경로를 가리켜야 합니다.

### 2. 봇 실행

```bash
cargo run
```

프로덕션 배포 시에는 `--release` 플래그를 추가합니다.

```bash
cargo run --release
```

`.env.example`을 복사해 `.env`를 만들고 값을 채웁니다.

```bash
cp .env.example .env
```

```env
DISCORD_TOKEN=your_bot_token_here

# 웹 설정 서버 (선택)
WEB_SETTINGS_HOST=0.0.0.0
WEB_SETTINGS_PORT=8800
# 리버스 프록시/도메인을 쓴다면 사용자에게 보여줄 기본 URL을 직접 지정할 수 있습니다.
# WEB_SETTINGS_BASE_URL=https://your-domain.example.com

# Discord Activity 서버 (선택)
ACTIVITY_PORT=2053
ACTIVITY_STATIC_DIR=/path/to/activity/dist
# HTTPS를 쓰려면 Cloudflare Origin Certificate 등의 인증서 경로를 지정합니다.
# ACTIVITY_TLS_CERT=/path/to/cert.pub
# ACTIVITY_TLS_KEY=/path/to/cert.key
DISCORD_CLIENT_ID=your_client_id_here
DISCORD_CLIENT_SECRET=your_client_secret_here
```

Activity 프론트엔드(`activity/`)도 별도의 `.env`가 필요합니다.

```bash
cp activity/.env.example activity/.env
```

```env
VITE_CLIENT_ID=your_client_id_here
VITE_MOCK_GUILD_ID=your_guild_id_here  # 로컬 개발용
```

## 설정

기본 설정은 `config.json`에서 관리합니다. 파일이 없으면 `config.example.json`을 복사해 자동으로 만듭니다.
`config.json`은 서버별 실제 설정이라 Git에는 올리지 않습니다.
게임 안에서는 `/마피아설정` 명령어로 인원, 특수 직업, 익명 모드 같은 옵션을 바꿀 수 있고,
`/마피아웹설정` 명령어로 브라우저에서 같은 항목들을 편집할 수도 있습니다.

### 웹 관리/상태 페이지

`/마피아웹설정`을 실행하면(관리자 역할 보유자만) 봇 프로세스 안에서 함께 떠 있는
작은 웹 서버(같은 서버, 기본 포트 `8800`)의 설정 편집 페이지로 연결되는 1회용 링크를
본인에게만 보이는 메시지로 보내줍니다. 이 링크는

- 명령어를 실행한 본인만 사용할 수 있고,
- 발급 후 10분 이내에 1번만 사용할 수 있으며,
- 저장하거나 시간이 지나면 즉시 만료됩니다.

일반 유저는 웹에서 봇 상태를 볼 수 있습니다.

- `http://서버주소:8800/status` 공개 상태판
- `http://서버주소:8800/leaderboard` 공개 리더보드
- `http://서버주소:8800/api/docs` API 문서
- `http://서버주소:8800/api/status` 상태 JSON API
- `http://서버주소:8800/api/games` 진행 중 게임 API
- `http://서버주소:8800/api/settings` 공개 설정 요약 API
- `http://서버주소:8800/api/stats` 전적 요약 API
- `http://서버주소:8800/api/leaderboard/{기준}` 리더보드 API (`rating`, `wins`, `winrate`, `games`, `mafia`, `playtime`)

외부에서 접속하려면 방화벽/리버스 프록시(nginx 등)로 `WEB_SETTINGS_PORT`를 노출하고,
필요하면 `WEB_SETTINGS_BASE_URL`로 사용자에게 보여줄 주소를 지정하세요.

## 주요 명령어

- `/마피아시작` 게임 모집 시작
- `/마피아중지` 진행 중인 게임 중지
- `/마피아설정` 게임 설정 변경
- `/마피아웹설정` 브라우저에서 게임 설정을 편집할 수 있는 1회용 링크 발급 (관리자 전용)
- `/역할설명` 전체 역할 안내
- `/직업정보` 특정 직업 안내
- `/상태` 현재 게임 상태 확인
- `/내정보` 내 전적 확인
- `/리더보드` 전적 순위 확인

## 참고

봇 초대 시 `Server Members Intent`와 메시지 관련 권한이 필요합니다.
