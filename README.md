# Mafia Discord Bot

디스코드에서 마피아 게임을 진행해 주는 봇입니다.

참가자 모집, 역할 배정, 밤 행동, 투표, 익명 채팅, 전적 기록까지 게임 진행에 필요한 기능을 봇이 처리합니다.

## 실행 방법

Rust 고속 런타임입니다.

```powershell
$env:CARGO_HOME='C:\rust-mafia\cargo'
$env:RUSTUP_HOME='C:\rust-mafia\rustup'
$env:Path="$env:CARGO_HOME\bin;$env:Path"
cargo run --release
```

이미 빌드된 실행 파일을 바로 실행할 수도 있습니다.

```powershell
.\target\release\mafia-remake.exe
```

기존 Python 구현은 비교/백업용 legacy 코드로 남아 있습니다.

```powershell
pip install -r requirements.txt
python bot.py
```

`.env` 파일에 봇 토큰을 넣어야 합니다.

```env
DISCORD_TOKEN=your_bot_token_here

# /마피아웹설정 명령어가 발급하는 설정 편집 페이지 관련 옵션 (선택)
# WEB_SETTINGS_HOST=0.0.0.0
# WEB_SETTINGS_PORT=8800
# 리버스 프록시/도메인을 쓴다면 사용자에게 보여줄 기본 URL을 직접 지정할 수 있습니다.
# WEB_SETTINGS_BASE_URL=https://your-domain.example.com
```

## 설정

기본 설정은 `config.json`에서 관리합니다. 파일이 없으면 `config.example.json`을 복사해 자동으로 만듭니다.
`config.json`은 서버별 실제 설정이라 Git에는 올리지 않습니다.
게임 안에서는 `/마피아설정` 명령어로 인원, 특수 직업, 익명 모드 같은 옵션을 바꿀 수 있고,
`/마피아웹설정` 명령어로 브라우저에서 같은 항목들을 편집할 수도 있습니다.

### 웹 설정 페이지

`/마피아웹설정`을 실행하면(관리자 역할 보유자만) 봇 프로세스 안에서 함께 떠 있는
작은 웹 서버(같은 서버, 기본 포트 `8800`)의 설정 편집 페이지로 연결되는 1회용 링크를
본인에게만 보이는 메시지로 보내줍니다. 이 링크는

- 명령어를 실행한 본인만 사용할 수 있고,
- 발급 후 10분 이내에 1번만 사용할 수 있으며,
- 저장하거나 시간이 지나면 즉시 만료됩니다.

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
