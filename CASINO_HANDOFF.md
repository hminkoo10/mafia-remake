# 카지노 작업 인수인계 (2026-09-24 기준)

## 카지노 패널 (2026-09-24, Claude + Codex)

- `/카지노패널`(관리자)이 채널에 고정 패널을 올린다. 버튼: 테이블 입장, 내 정보, 홀덤/블랙잭 테이블 만들기, 테이블 설정(이름·방 설정), 테이블 닫기(확인 후). 슬래시 명령은 그대로 둔다. 커밋 `e0aab76`, 설명은 README `### 카지노 패널`.
- 라운드 중에 바꾼 방 설정은 `pending_settings`에 두었다가 그 라운드가 끝날 때(`CasinoTable::reconcile`) 적용한다. 설정 안내는 딜러 채팅으로만 보내 말풍선(인슈어런스 안내·결과)을 덮지 않는다.
- 이름 변경은 채널 이름도 바꾼다. Discord 제한(채널당 10분에 두 번)을 넘을 것 같으면 보내지 않고, 봇이 마지막으로 정한 채널 이름(`TableBinding.channel_name`)과 비교해 다음 설정 제출 때 다시 맞춘다.
- 패널 위치는 `casino.json`의 `panel`에 저장한다. 패널 메시지를 지우면 연결이 끊기고, `/카지노패널`을 다시 쓰면 새로 올리고 옛 패널을 지운다.
- 구현: Codex(Tier 2)가 명세로 초안을 썼고 90분 제한에 걸려 중단됐다. 나머지(테스트·README)와 검토 수정은 Claude가 했다. 검토는 워크플로 세 번(71개, 10개, 3개 에이전트, 읽기 전용)으로 반박 검증했고 찾은 문제는 모두 고쳤다.
- 검증: Rust 테스트 392개, 웹 빌드 통과. 실제 Discord에서는 아직 눌러 보지 않았다 → 배포 후 D 항목과 함께 패널 버튼을 모두 확인.

## 최신 수정 (2026-09-23 밤, Claude)

모두 커밋·푸시됨. Rust 테스트 374개(라이브러리 238, 바이너리 130, 통합 6) 통과. 배포는 아직 안 했다. 보안 수정은 빌드하지 않는 검토자 세 명이 반박하는 관점으로 다시 읽었고, 찾은 문제는 아래 커밋에 반영했다.

- **블랙잭 승리 표기** (`7169320`): 이긴 베팅이 돌려주는 총액(원금 포함)을 보여 준다. 5,000 블랙잭 → `12,500원 이기셨습니다`. 홀덤은 가져간 팟. `SeatResult.paid`.
- **/코인선물 대상 금액** (`d01ab68`): 서버 안에서 다른 멤버에게 코인을 준다. 봇·자기 자신 거부, 진행 중인 마피아 게임에 건 코인(`Data.bet_locks`)은 줄 수 없다. 관리 로그에 기록.
- **딜러 영상** (`ae1466a`): 이 PC에서 로컬 모델로 만든 소피아 대기·딜링·플립 영상. 영상 사이는 450ms 교차 전환. 만든 방법과 한계는 `casino-web/public/dealers/GENERATION.md`. 아래 "실제 딜링 동영상 생성은 미완료" 항목은 이걸로 끝났다.
- **저장 안전성**: stats.json 저장 직렬화(`5c047b1`, 다른 세션), config.json·API 키·리플레이도 `.tmp` 작성 → fsync → 한 번의 rename(`2a2b6ec`, `src/atomic_file.rs`).
- **보안 점검 수정** (공격자가 할 수 있던 일 → 수정):
  - `26bc6f9` 카지노 웹 정적 파일 경로 조작으로 서버 파일 읽기, 웹소켓 대용량 입력 → 경로 검사·크기 제한.
  - `90c236a` Activity에 guild_id 0을 보내 봇 전체 종료(panic=abort), 다른 서버 게임 조회, 익명 게임의 실제 ID 노출 → ID 검증·세션 서버 고정·별칭만 전송.
  - `f2b2620` 위 커밋의 IP별 인증 제한이 Discord 프록시 뒤라 게임 시작 때 참가자들을 막던 문제 → 전역 4칸 대기열(최대 10초) + Discord 429 뒤 Retry-After만큼 전체 대기.
  - `b1d13c6`, `d4229e7`, `526fc71` 익명 게임에서 /메모·별칭·같은 직업 목록·관전 권한으로 실제 사람이 드러나던 문제.
  - `d1ef42b` 공개 채널 `발언 종료` 버튼 ID에 대상자 실제 ID가 들어 있던 문제.
  - `06870b1` 역할 배정이 마피아팀부터 채워 플레이어 목록 앞쪽이 늘 마피아였던 문제 → 배정 뒤 섞음. `5b4fe8d` 시한부 보유자가 마피아팀·교주팀 모두 살아 있으면 예전처럼 마피아팀 승리(순서에 기대던 규칙을 명시).
  - `d2fe341` → `659f025` 관전 → 나가기(역할 회수 실패) → 참가를 빠르게 누르면 관전자 역할을 가진 채 참가하던 문제. 봇이 준 관전자 역할을 모집 중에 기억해 두고, 참가 때 그 경우만 회수한다(관전자 역할이 봇보다 위에 있는 서버에서 아무도 참가 못 하던 회귀를 고침).
  - `f24a127` 다른 서버 관리자가 공유 코인·설정을 바꾸던 문제 → 관리 명령은 본 서버에서만. 깨진 stats/casino 파일로 빈 상태 시작 → 시작 거부. `85a66c0` 원본은 그대로 두고 복사본만 남겨 재시작해도 계속 거부. `edccca8` 끊어진 심볼릭 링크·권한 오류를 "파일 없음"으로 보고 빈 상태로 시작하던 문제(모든 상태 파일 로더).
  - `cc471fd` 테이블을 닫으면 이미 정산된 베팅까지 환불되던 문제.
  - `202d25d` 설정 웹 연결을 붙잡아 서버를 멈추게 하던 문제 → 읽기·쓰기 시간 제한, 동시 연결 제한. `c934726` Content-Length 넘침으로 봇 전체 종료.
- **배포 전에 할 일**:
  - 봇이 여러 서버에 있으면 `.env`에 `HOME_GUILD_ID=<본 서버 ID>`를 넣는다. 없으면 관리 명령(코인 관리·설정·카지노 테이블 생성 등)이 모든 서버에서 막힌다. 서버 하나에만 있으면 첫 시작 때 자동으로 정해 config.json에 저장한다.
  - 사용자가 aarch64로 빌드해 instance4 바이너리를 교체한다.
- **배포 후 Discord에서 확인할 것**: 아래 D 항목, /코인선물, 참가자 여럿이 동시에 Activity를 열 때 인증 실패가 없는지, 익명 게임의 `발언 종료` 버튼, 카지노 패널의 모든 버튼.

## 최신 수정 (2026-09-23 오후, Claude)

사용자 요청 5가지(스플릿, 슈에서 카드 딜링, 상대 카드 표시와 내 자리 강조, 칩 드래그 베팅, 테이블 방 설정)와 이전 에이전트의 딜러 영상 작업을 이어서 마쳤다.

- **방 설정** (`c67015b`): `/카지노테이블생성`에 선택 옵션 `최소베팅` `최대베팅` `사이드최대`(0이면 사이드베팅 없음) `빅블라인드` `최소바이인` `최대바이인` `제한시간`. `TableSettings::build`가 검증하고 테이블에 저장한다(`CasinoTable.settings`, 예전 저장본은 기본값). 베팅·블라인드·바이인·턴 시간·웹 규칙·채널 상태/목록이 모두 이 값을 따른다. 생성 후에는 카지노 패널의 **테이블 설정**으로 바꾼다(라운드 중이면 그 라운드가 끝날 때 적용).
- **딜링 시간**: 히트·더블·스플릿 카드도 첫 딜처럼 380ms 공개 시각을 가진다. 그동안 액션이 막히고, 다음 차례·딜러 공개가 그 뒤로 밀린다. 스플릿 두 핸드는 카드마다 공개 시각을 유지한다.
- **웹** (`a354c25`, `419f900`): 카드가 딜러 사진 속 슈 입구(사진 비율 0.79, 0.60)에서 뒷면으로 날아와 놓이며 앞면이 된다. 블랙잭 좌석마다 모든 참가자의 핸드·점수(소프트 7/17)·결과(WIN +1,000 등)가 보이고 스플릿 핸드는 나란히 놓인다. 내 자리는 금빛 원·큰 카드로 강조. 칩 트레이에서 베팅 원(PP·메인·21+3)이나 내 좌석 자리로 끌어 놓기(마우스·터치). 칩 단위는 테이블 한도에 맞춰 바뀐다(최소 베팅을 나누어떨어지게 하는 칩부터 6개). 홀덤 레이즈 빠른 선택(최소·½·¾·팟). 칩 더미가 모두 초록으로 보이던 기존 CSS 결함 수정. 계산 함수는 `casino-web/src/table-helpers.ts`(노드 테스트 있음).
- **서버**: 웹 에셋 바이트 범위 응답(`70a2de3`). Safari/iOS·Discord 앱 내 브라우저에서 딜러 영상 재생에 필요.
- **검증**: Rust 317개, 웹 테스트 10개, 웹 빌드 통과. 개발 서버에서 3인 블랙잭(다른 참가자 카드 표시), 스플릿 두 핸드 진행·정산, 히트 카드가 슈에서 날아옴, 드래그 베팅(실제 마우스 이벤트, 좌석 자리 포함), 베팅 마감 중 드래그 정리, 고액 테이블(500~25,000) 칩 단위·한도 거부, 홀덤 레이즈 빠른 선택 금액, 1440px·390px 캡처로 겹침 확인. 캡처는 헤드리스 Chrome을 DevTools 프로토콜로 조종했다(스크래치패드 `cdp.mjs`, 앱 내 브라우저 창이 가려져 있으면 페이지가 hidden이 되어 시계·ResizeObserver가 멈춘다).

## 최신 수정 (2026-09-23)

- 채팅 기본 탭을 열고, 최근 참가자 채팅 3개(모바일 2개)를 테이블 위에도 표시. 채팅 자동 스크롤은 채팅 창 내부로 제한하고, 이전 메시지를 읽는 동안 강제 이동하지 않는다. 40개 메시지 상한 이후에도 마지막 메시지 ID로 갱신한다.
- 전체화면/종료 버튼, Esc 종료, Fullscreen API 미지원 시 화면 확장 대체 처리 추가.
- 대기 중 250ms 타이머 제거. 턴은 초 경계, 카드 공개/셔플 중에만 빠르게 갱신. 같은 서버 응답은 재렌더하지 않고, 늦은 응답의 상태 되돌림을 차단. 버전이 같아도 카드 공개·채팅이 바뀌면 반영. 카드/채팅/딜러를 memo로 분리.
- 이전 `DealerHands.tsx`·CSS 및 사진 줌/흔들림 제거. 내장 image_gen으로 팔에 연결된 손·슈가 있는 실사 `casino-web/public/dealers/sophia-table.png` 생성.
- **실제 딜링 동영상 생성은 미완료**: 현재 연결된 생성 도구는 정지 이미지만 지원한다. 영상 도구/API 정보 요청 상태. 사용할 이미지·영상 프롬프트는 `casino-web/public/dealers/GENERATION.md` 참고.
- 영상 재생 컴포넌트: OpenClaw Tier 2 Codex(gpt-5.6-luna), 새 `DealerBackdrop.tsx`·CSS만 위임(223초). 검토에서 준비된 영상의 무드 전환 결함을 수정하고 테스트 추가. WebM/MP4 MIME·교체 가능한 딜러 에셋 캐시도 수정.
- 검증: Rust 전체 310개, 웹 회귀 테스트 5개·웹 빌드 통과. 개발 서버에서 채팅 기본 열기·전송·테이블 표시, 전체화면 버튼 왕복, 홀덤 2인 프리플롭→리버→쇼다운·비공개 카드·칩 보존 확인. FPS 수치 측정은 하지 않았다. 실제 영상 재생 검증은 영상 생성 후 필요.
- 사용자 aarch64 빌드·실서버 교체 및 Discord 실동작 확인은 여전히 남아 있다.

## 후속 반영 완료 (2026-09-22)

- A 완료: `won`으로 이긴 베팅의 이익을 별도 집계. 웹 배너·기록·Discord 결과에 표시하고 순손익은 보조 줄로 유지. 커밋 `f66ba2c`.
- B 완료: 8덱 슈 유지, 컷 카드, 다음 판 셔플, 슈 게이지·셔플 오버레이, 명시 덱 격리 및 고갈 복구. 커밋 `4bc7a32`.
- C 완료: `components/DealerHands.tsx`와 CSS 추가·마운트. 대기/딜/플립, 1인 테이블 반복, 모션 줄이기 지원. OpenClaw Tier 2 Codex(gpt-5.6-luna)가 새 파일 초안 작성(287초), 메인 Codex가 검토·수정·빌드·화면 검증.
- 검증: Rust 전체 308개(라이브러리 211, 바이너리 91, 통합 6), 웹 빌드 통과. 개발 서버에서 블랙잭 3자리 베팅→딜→스탠드→정산, 슈 416→412→411 감소 확인. 2인 홀덤 프리플롭→리버→쇼다운, 비공개 카드 가림, 총 칩 20,000 보존 확인. 390px·1440px 가로 넘침 없음, 브라우저 콘솔 오류 없음.
- 브라우저는 OS의 모션 줄이기 설정 상태에서 확인했다. 일반 모션의 전체 재생과 실제 Discord 확인(D)은 추가 확인 대상.
- 임시 개발 서버: `http://localhost:8811`, 상태 파일은 `C:/temp/mafia-casino-qa-20260922/`. 운영 데이터와 분리됨.
- 남은 배포: 사용자 aarch64 빌드·instance4 바이너리 교체 후 D 확인. 아래 1~3절은 최초 인수인계 당시 상태와 구현 명세를 보존한 것이다.

다음 에이전트가 이어서 하기 위한 문서다. 현재 상태, 남은 작업의 정확한 명세, 환경·검증·배포 방법을 담았다.
읽는 순서: 1 → 2 → 3(남은 작업) → 4(환경) → 5(검증) → 6(배포·규칙).

---

## 1. 현재 상태 (origin/main = `76ae2c9`)

모두 커밋·푸시되어 있고 `cargo test`(205개) 통과, `casino-web` 빌드 통과.

| 영역 | 상태 |
|---|---|
| Discord 연동 | `/카지노테이블생성 종류 이름`(관리자) `/카지노테이블닫기` `/카지노테이블목록` `/카지노입장 [테이블]` `/카지노상태`. 테이블마다 `#카지노-<이름>` 채널, 고정 상태 임베드(+ **테이블 입장** 버튼 → 12시간 개인 링크), 웹훅 채팅 양방향 중계(딜러 얼굴 아바타, 참가자 Discord 아바타), 핸드 결과 게시(손익·사이드베팅 메모). 실제 Discord 서버에서는 아직 눌러보지 않았다(코드·테스트만). |
| 코인 연동 | 바이인 5,000~20,000, 퇴장·유휴 30분·테이블 닫기 시 코인 반환. 좌석 이름은 항상 Discord 표시 이름(서버 강제). |
| 엔진 (`src/casino/`) | 홀덤(블라인드 50/100, 사이드팟, 30초 턴), 블랙잭 **에볼루션 규칙**(8덱, S17, 딜러 피크, 스플릿 1회, 에이스 스플릿 1장, 서렌더 없음, 인슈어런스 2:1·10초, 퍼펙트 페어 6/12/25, 21+3 5/10/30/40/100, 베팅 15초·전원 베팅 시 즉시 딜). 카드 공개 페이싱(홀 카드 한 장씩, 스트리트 pause, 쇼다운 순차 공개, 딜러 홀 카드 플립 후 한 장씩 드로우) — 연출 중 액션 거부(`REVEALING`), 테스트에서는 지연 0. 딜러 명단(소피아·미아·하나·리나, 초상 있는 딜러만 12판마다 교대 — 현재 소피아만 초상 있음). |
| 웹 (`casino-web/`) | noir-casino 원본 화면 이식(원본 CSS `noir.css` 그대로, 추가는 `overrides.css`). 브랜드 **CASINO73**. 3D 딜 인/플립, 시간차 표시, 딜링 잠금, 결과 배너(다음 라운드까지 유지), 족보 이름·카드 강조, 내 자리 강조, 에볼루션식 칩 쌓기 베팅(메인·PP·21+3 자리, 되돌리기/지우기/다시 베팅/더블/확정, 마감 2초 전 자동 확정), 인슈어런스 프롬프트, 액션 말풍선, 칩 이동(베팅→팟, 팟→승자), 승자 글로우, 차례 링, 팟 강조, 딜러 숨결/딜링 움직임, 합성 효과음(토글), 딜러 클립 상태(idle/deal/flip, 파일 있으면 재생), 상세 규칙 다이얼로그(배당표). |
| 문서 | `README.md` 카지노 절, `casino-web/public/dealers/README.md`(딜러 에셋 규격), `.env.example`. |
| 인프라 | instance4(`168.138.39.183`, ubuntu, `/home/ubuntu/mafia_rust`)에 Cloudflare Origin 인증서(`*.milky.kr`) 적용, Activity 2053·설정 웹 8443 모두 Cloudflare 프록시 HTTPS로 정상. 서버 바이너리(`./mafia`, aarch64)는 **사용자가 직접 빌드**한다. 서버의 바이너리는 9/22 08:15 빌드라 카지노가 없다 → 최신 main으로 다시 빌드해 배포해야 한다. |

정리할 것: `.claude/worktrees/agent-*` 는 중단된 하위 에이전트의 작업 트리다. `git worktree remove --force .claude/worktrees/<이름>` 으로 지워도 된다(내용은 미완성이라 버려도 됨). `.gitignore`에 이미 제외되어 있다.

---

## 2. 코드 지도

```
src/casino/cards.rs      덱·족보·best_hand(현재 족보+핵심 카드)·perfect_pairs·twenty_one_plus_three
src/casino/table.rs      CasinoTable/Round/Seat/BjHand/HandResult/SeatResult, CasinoCommand, apply_command, tick,
                         상수(TURN_MS, BET_WINDOW_MS, DEAL_CARD_MS…, SIDE_BET_*, INSURANCE_MS, DEALERS, DEALER_SHIFT_HANDS)
src/casino/holdem.rs     start_poker/advance/settle_poker (reveal 스케줄 포함)
src/casino/blackjack.rs  start/place_bet/deal/settle_side_bets/insurance_decision/resolve_insurance/settle/legal
src/casino/view.rs       table_view(table, viewer, now): 비밀 카드 가림, reveal 시각, legal(can_bet/can_insure…), DealerView
src/casino/tests.rs      엔진 테스트(덱을 위에서부터 지정: deck_from_top; 블랙잭 딜 순서 = 참가자1장씩→딜러1장, 반복)
src/casino_hub.rs        테이블 저장소(casino.json), 세션(12h), 코인↔칩, tick_all(250ms), 웹훅/아바타 캐시
src/casino_web.rs        /casino/api (state, command, ws) + SPA 서빙, dealer_avatar_png, `mafia --casino-dev`
src/commands/casino_cmds.rs  슬래시 명령, 상태 임베드(+버튼), 결과 게시(render_hand_result), 웹훅 중계, 채널→테이블 채팅
casino-web/src/App.tsx   화면 전체(원본 page.tsx 이식본). types.ts(API 타입), ui.tsx(Tabs/Dialog/Sheet/Slider/Toast),
                         sounds.ts(효과음), noir.css(원본, 수정 금지), base.css, overrides.css(모든 추가 스타일)
build.rs                 activity/, casino-web/ 를 npm으로 빌드해 바이너리에 내장 (MAFIA_SKIP_CASINO_BUILD로 생략)
```

---

## 3. 남은 작업 (우선순위 순)

### A. 승패 표기를 "딴 금액" 기준으로 (사용자 요청)
지금 결과 배너는 순손익(net)만 보여서 메인은 이기고 사이드를 잃으면 패배처럼 보인다.
- `SeatResult`에 `won: i64` 추가(`#[serde(default)]`): 이번 라운드에 **이긴 베팅의 이익 합**.
  - 블랙잭: 핸드마다 `max(payout - bet, 0)` + 사이드베팅 적중 시 `stake * odds` + 인슈어런스 적중 시 `stake * 2`.
  - 홀덤: `max(returned - wagered, 0)`.
  - `Seat`에 `side_won: i64`(라운드마다 초기화)를 두고 `settle_side_bets`/`resolve_insurance`/`settle_blackjack`에서 누적.
- 웹 배너·기록: 내 좌석 헤드라인 `이기셨습니다 +{won}`(금색) / `won == 0 && net < 0` → `패배 −{-net}`(빨강) / 그 외 `푸시`. 다른 참가자는 `{이름} 이김 +{won}`. 기존 net·label·notes 줄은 그 아래 작게.
- Discord `render_hand_result`에도 `이김 +{won}` 표기.
- 예: 메인 2,000 승 + 사이드 3,000 패 → `이기셨습니다 +2,000`(net −1,000은 작은 글씨). 사이드 3,000을 4:1로 이기고 메인 패 → `+12,000`(원금 3,000은 따로 돌아옴). 다 지면 패배.
- 테스트 추가(덱 크래프팅 예시는 `blackjack_side_bets_are_settled_on_the_deal` 참고).

### B. 8덱 슈 유지 + 빨간 컷 카드 + 셔플 연출 (사용자 요청)
지금은 라운드마다 8덱을 새로 섞는다(`start_blackjack`의 `shuffled_deck(8)`). 에볼루션처럼 슈를 이어서 쓰고 컷 카드가 나오면 다음 라운드 전에 섞는다.
- `CasinoTable`에 `shoe: Vec<String>`(남은 카드, `draw`가 pop하므로 끝이 위), `shoe_cut: usize`(바닥에서 몇 장 지점에 컷 카드), `shoe_total: usize`, `shuffled_at: i64` (모두 serde default).
- 새 슈: `shuffled_deck(8)`(416장), 컷 카드는 위에서 70~80% 깊이(= `shoe_cut` 20~30%, `system_random` 사용).
- `start_blackjack`: 명시 덱(테스트)이 오면 예전처럼 그대로 쓰고 슈는 건드리지 않는다. 아니면 슈가 비었거나 `shoe.len() <= shoe_cut`이면 새 슈를 만들고 `shuffled_at = now`, 딜러가 "컷 카드가 나왔어요. 새 슈를 섞습니다."(첫 슈는 "새 슈를 준비합니다."). `round.deck = std::mem::take(&mut table.shoe)`; 라운드가 끝나면(`settle_blackjack` 끝, `deal_blackjack`의 베팅 없음 경로) `table.shoe = std::mem::take(&mut round.deck)`. 덱이 도중에 바닥나면 패닉 대신 새 덱을 붙인다.
- 홀덤은 지금처럼 핸드마다 1덱.
- View: `TableView.shoe: { remaining, total, cut_at, shuffled_at, reshuffle_due }` + TS 타입.
- 웹: 테이블 상단에 슈 게이지(딜된 만큼 채움, 컷 카드 위치에 빨간 세로선, `슈 {remaining}/{total}`, 셔플 예정이면 `다음 라운드 전 셔플`). `shuffled_at`이 서버 시각(`serverNow`) 기준 2.5초 이내면 카드 뒷면 8~10장이 부채꼴로 섞이는 오버레이(CSS keyframes, `새 슈를 섞는 중`) 2.2초. `overrides.css`에만 추가, `prefers-reduced-motion` 존중.
- 테스트: 연속 두 라운드가 한 슈를 공유(남은 장수 감소), 컷 지점 이하면 다음 Start에서 재셔플(`shuffled_at` 갱신, 안내문에 "섞"), 명시 덱 테스트는 그대로 통과.

### C. 딜러 손 딜링/카드 오픈 오버레이 애니메이션 (사용자: "사진만 있어 허전")
영상 에셋이 없으므로 코드로 움직임을 만든다. 새 파일만 만들고 App.tsx에는 한 줄로 마운트.
- `casino-web/src/components/DealerHands.tsx` + `dealer-hands.css`: `mood: "idle"|"deal"|"flip"`, `targets: [x%,y%][]`(좌석 위치 `SEAT_POS`), `center?`, `reducedMotion?`.
  - 인라인 SVG 손·소매(피부 #e8c4a8, 검정 소매 #1b1b1b + 금테 #c8ab62, 치파오와 맞춤), 우측(약 84%,62%)에 카드 슈(짙은 목재 #2c1c12 + 금테) 상시 표시.
  - deal: 900ms 주기로 오른손이 슈에서 뒷면 카드를 꺼내 `targets`를 순환하며 밀어주고 목적지 근처에서 사라짐(실제 카드는 앱이 그림). flip: 두 손이 `center`에서 카드를 600ms rotateY로 뒤집기(1.6초 반복). idle: 4초 주기 미세한 숨결. 무드 전환 250ms 크로스페이드.
  - `.game-table` 안 `.table-shade` 바로 다음에 마운트, `position:absolute; inset:0; pointer-events:none; z-index:0`.
- App.tsx: `mood`는 이미 `DealerBackdrop`에 넘기는 값(`revealing ? (complete/딜러 reveal ? "flip" : "deal") : "idle"`)을 그대로 쓴다.
- 실제 딜러 영상 클립이 생기면 `casino-web/public/dealers/README.md` 규격대로 넣으면 자동 재생된다(이건 Codex/사용자 에셋 작업).

### D. 실서버 Discord 확인 (배포 후)
- `/카지노테이블생성` → 채널·고정 임베드·**테이블 입장** 버튼 → 개인 링크 → 웹 참여.
- 웹 채팅 → 채널 웹훅(딜러 아바타/참가자 아바타), 채널 채팅 → 웹.
- 핸드 결과 게시에 손익·사이드베팅 메모.

---

## 4. 환경·빌드·테스트 (이 Windows PC)

cargo 명령 전에 항상:
```bash
export CARGO_TARGET_DIR="C:/temp/gcc-tmp/claude/mafia-target"
export PATH="$HOME/.rustup/toolchains/1.96-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/gcc-ld:$PATH"
export RUSTFLAGS="-C link-arg=-fuse-ld=lld"
```
- 테스트: `cargo test` (전부 통과해야 커밋). "file-system error deleting outdated file" 메시지는 무시.
- 포맷: `cargo fmt` 뒤 반드시 `git checkout -- src/http_pool.rs` (fmt가 그 파일을 망가뜨림, 커밋 금지).
- 웹: `cd casino-web && npm run build` (TS strict). 파이프로 grep하면 실패가 가려지니 `npm run build > log || { cat log; exit 1; }` 식으로 확인.
- 웹 편집 시 파이썬 스크립트로 앵커 치환을 쓰면 편하다. 한글이 든 스크립트는 파일로 써서 실행(bash heredoc은 cp949로 깨짐). 같은 스크립트를 두 번 돌리면 앵커가 `new`의 접두어일 때 중복 삽입되니 주의(types.ts에서 실제로 발생했음).
- 개발 모드(Discord 없이 웹만): `cargo build --bin mafia` 후
  `CASINO_STATIC_DIR=<repo>/casino-web/dist CASINO_DEV_PORT=8811 <target>/debug/mafia.exe --casino-dev`
  → 테스트 계정 3개(각 50,000코인)와 홀덤·블랙잭 테이블 링크를 출력. 다시 빌드하려면 먼저 서버를 종료해야 exe 교체가 된다(복사본 `mafia-dev.exe`로 띄우면 편함).
- 브라우저 수동 검증 팁: 턴 타이머가 30초라 액션을 한 배치에서 연속으로 눌러야 한다(사이 대기가 길면 자동 폴드/스탠드). 결과 배너·연출은 서버 시각 기준(`serverNow = clock + offset`).

---

## 5. 검증 체크리스트 (남은 작업 반영 후)

- [ ] `cargo test` 전부 통과, `npm run build` 통과, `cargo fmt` + http_pool.rs 복원.
- [ ] 개발 모드 블랙잭: 칩 3자리 쌓기 → 확정 → 시간차 딜 → 액션 → 딜러 플립·드로우 연출 → 결과 배너에 `이기셨습니다 +N`/사이드 메모.
- [ ] 슈 게이지 감소, 컷 카드 지점 이후 다음 라운드 전 셔플 애니메이션과 안내.
- [ ] 홀덤 2인: 홀 카드 시간차, 딜링 잠금, 스트리트 오픈, 쇼다운 순차 공개, 승자 글로우·칩 이동.
- [ ] 콘솔 오류 없음, 모바일(390px)에서 레이아웃 깨짐 없음.

---

## 6. 배포·규칙

- 배포: 사용자가 aarch64로 빌드해 instance4의 `/home/ubuntu/mafia_rust/mafia`를 교체하고 tmux `mafia` 세션에서 재시작한다. 서버 `.env`는 이미 8443/https로 맞춰져 있고 인증서도 새것이다. `CASINO_BASE_URL`은 비워도 된다(설정 웹 호스트 + 2053으로 생성).
- 커밋 규칙: 항목마다 따로 커밋, 커밋 후 항상 `git push origin main`, **Co-Authored-By 트레일러 금지**, 역할/능력 변경 시 웹 가이드와 Discord 설명 동기화(카지노 작업엔 해당 없음).
- 절대 커밋하지 말 것: `cert.*`, `.env`, `casino.json`, `casino-dev*.json`, `.claude/worktrees/`(모두 .gitignore됨).
- 위임 규칙: `~/.claude/CLAUDE.md`(OpenClaw) 참고 — 기능 구현은 `openclaw agent --message-file <경로>`(Codex), 단순 변환은 로컬 모델. 결과는 반드시 직접 테스트로 검증한다. 클로드 하위 에이전트(Agent 툴)는 사용자가 원치 않는다(클로드 토큰 소모).
- 원본 디자인 원칙: `noir.css`는 손대지 않고 `overrides.css`에만 추가. 문구는 한국어, 브랜드는 CASINO73.
