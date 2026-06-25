import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createWebSocket, fetchState, sendAction, setSession } from "./api";
import { authenticateWithDiscord } from "./discord";
import type {
  ActionRequest,
  ActionType,
  ActivitySpecialAction,
  GameState,
  Phase,
  PlayerDto,
  RoleTeam,
} from "./types";

type AuthStatus = "loading" | "ready" | "error";
type ConnectionStatus = "connecting" | "live" | "offline";
type PlayerFilter = "all" | "alive" | "dead" | "marked" | "voted";
type PlayerMark = "none" | "trust" | "suspect" | "watch";
type PlayerSort = "status" | "votes" | "name" | "mark";
type EventTone = "phase" | "vote" | "action";

interface ActivityEvent {
  id: string;
  at: string;
  text: string;
  tone: EventTone;
}

interface GameSnapshot {
  gameKey: string;
  phase: Phase;
  dayNumber: number;
  nominee: string | null;
  confirmYes: number;
  confirmNo: number;
  actionResult: string | null;
  votes: Record<string, number>;
  skipVotes: number;
}

const PHASE_META: Record<Phase, { label: string; tone: string; summary: string }> = {
  Night: { label: "밤", tone: "night", summary: "비공개 행동" },
  Day: { label: "낮", tone: "day", summary: "토론과 정보 정리" },
  Vote: { label: "투표", tone: "vote", summary: "처형 대상 지목" },
  FinalDefense: { label: "최후변론", tone: "defense", summary: "지목자 방어" },
  ConfirmVote: { label: "처형 확인", tone: "confirm", summary: "찬반 결정" },
  Ended: { label: "게임 종료", tone: "ended", summary: "결과 확인" },
};

const TEAM_META: Record<RoleTeam, { label: string; className: string }> = {
  Citizen: { label: "시민팀", className: "team-citizen" },
  Mafia: { label: "마피아팀", className: "team-mafia" },
  Cult: { label: "교주팀", className: "team-cult" },
  Neutral: { label: "중립", className: "team-neutral" },
};

const SPECIAL_ACTION_META: Record<
  ActivitySpecialAction,
  { label: string; action: ActionType; requiresPair: boolean; requiresTarget?: boolean }
> = {
  hacker: { label: "해킹", action: "hacker_action", requiresPair: false },
  vigilante: { label: "자경단원 조사", action: "vigilante_action", requiresPair: false },
  psychologist: { label: "심리학자 관찰", action: "psychologist_action", requiresPair: true },
  hypnotist: { label: "최면 해제", action: "hypnotist_action", requiresPair: false, requiresTarget: false },
  thief: { label: "도벽", action: "thief_action", requiresPair: false },
};

const MARK_META: Record<PlayerMark, { label: string; short: string; className: string }> = {
  none: { label: "표시 없음", short: "-", className: "mark-none" },
  trust: { label: "신뢰", short: "신", className: "mark-trust" },
  suspect: { label: "의심", short: "의", className: "mark-suspect" },
  watch: { label: "관찰", short: "관", className: "mark-watch" },
};

const SORT_LABELS: Record<PlayerSort, string> = {
  status: "상태순",
  votes: "득표순",
  name: "이름순",
  mark: "표시순",
};

const PHASE_CHECKS: Record<Phase, string[]> = {
  Night: ["밤 행동 제출", "대상 메모", "결과 대기"],
  Day: ["결과 확인", "발언 비교", "스킵 판단"],
  Vote: ["득표 선두 확인", "확정 정보 대조", "스킵/지목"],
  FinalDefense: ["변론 기록", "라인 재계산", "찬반 준비"],
  ConfirmVote: ["찬반 수 확인", "팀 손익 계산", "최종 제출"],
  Ended: ["승리팀 확인", "메모 정리", "다음 판 준비"],
};

const ROLE_HELP: Record<string, string> = {
  시민: "시민팀의 기본 역할입니다. 특별한 밤 능력은 없지만, 공개 정보와 발언 흐름을 조합해 마피아팀을 좁히는 핵심 역할입니다. 직업 주장, 투표 흐름, 사망자 역할, 밤 결과를 시간순으로 정리하면 시민도 강한 정보원이 됩니다.",
  마피아: "마피아팀의 중심 역할입니다. 밤마다 처치 대상을 선택하고, 낮에는 시민팀처럼 보이며 의심을 분산해야 합니다. 여러 마피아가 있으면 Activity와 마피아 비밀방의 선택 현황을 보며 처치 우선순위를 맞추는 것이 중요합니다.",
  경찰: "밤마다 한 명을 조사해 마피아 여부를 확인하는 수사직입니다. 조사 결과는 강력하지만, 대부의 조사 회피나 일부 보조직의 접선 상태처럼 예외가 있을 수 있습니다. 결과만 던지기보다 언제, 누구를, 왜 조사했는지 함께 설명해야 신뢰를 얻습니다.",
  의사: "밤마다 한 명을 보호해 마피아의 처치를 막을 수 있는 방어 역할입니다. 공개된 중요 직업을 지키는 안정 선택과, 마피아가 노릴 만한 발언자를 예측하는 심리전 사이에서 판단해야 합니다. 보호 성공은 시민팀 흐름을 크게 바꿉니다.",
  요원: "경찰 계열 수사직입니다. 밤 결과를 통해 마피아 후보를 좁히고, 낮 토론에서 그 결과를 어떻게 공개할지 결정해야 합니다. 너무 이른 공개는 표적이 될 수 있고, 너무 늦은 공개는 시민팀 판단을 늦출 수 있습니다.",
  자경단: "낮 조사와 밤 숙청을 통해 적극적으로 마피아팀을 압박하는 역할입니다. 처형 능력은 강력하지만 오판하면 시민 수를 직접 줄일 수 있습니다. 조사 결과, 투표 흐름, 다른 수사직 주장까지 묶어서 신중히 사용해야 합니다.",
  탐정: "밤에 대상의 행동 경로를 추적해 누가 누구에게 행동했는지 단서를 얻는 역할입니다. 직접적인 마피아 판정은 아니지만, 발언과 실제 행동이 어긋나는 사람을 잡아낼 수 있습니다. 연속된 밤의 이동 기록을 쌓을수록 가치가 커집니다.",
  기자: "특정 대상을 취재해 정보를 공개하는 역할입니다. 취재는 판 전체에 영향을 주는 공개 정보가 되므로, 공개 타이밍과 대상 선정이 매우 중요합니다. 이미 의심받는 사람보다 판을 확정할 수 있는 대상을 고르는 것이 좋습니다.",
  해커: "상대의 행동 정보를 얻어 낮 토론의 근거를 만드는 정보 역할입니다. 행동 대상, 타이밍, 발언을 함께 비교하면 거짓 직업 주장을 흔들 수 있습니다. 단독 결론보다 다른 조사 결과와 연결할 때 강합니다.",
  영매: "사망자와 관련된 정보를 활용하는 역할입니다. 죽은 사람의 직업, 발언, 밤 상황을 산 사람의 주장과 연결해 추론합니다. 사망자가 늘어날수록 정보량이 커지므로 기록 관리가 중요합니다.",
  성직자: "죽은 대상을 되살리거나 교주팀 위협을 정리하는 시민팀 보조 역할입니다. 부활은 판세를 뒤집을 수 있지만, 대상 선택을 잘못하면 정보 혼선을 만들 수 있습니다. 죽은 사람의 직업 가치와 현재 생존 구도를 같이 봐야 합니다.",
  교주: "교주팀의 핵심 역할입니다. 밤마다 포교를 통해 세력을 넓히고, 시민팀과 마피아팀 사이에서 독자 승리 조건을 노립니다. 포교 성공 후에는 생존자 수와 비교주팀 수를 계속 계산해야 합니다.",
  광신도: "교주팀 보조 역할입니다. 교주 생존 여부와 포교 흐름을 중심으로 움직입니다. 교주팀은 숫자 계산이 매우 중요하므로, 누가 포교되었는지와 누가 위협인지 빠르게 정리해야 합니다.",
  조커: "낮 투표로 처형되면 단독 승리를 노리는 중립 역할입니다. 너무 노골적으로 의심받으면 견제당하고, 너무 조용하면 처형 후보가 되기 어렵습니다. 의심과 설득 사이의 균형이 핵심입니다.",
  청부업자: "두 명의 대상과 각각의 직업을 추측해 청부를 시도하는 마피아팀 보조 역할입니다. 맞히면 큰 이득을 얻지만, 추측 실패는 행동 가치를 잃게 됩니다. 공개 직업, 발언 패턴, 밤 결과를 모아 확률이 높을 때 제출하는 것이 좋습니다.",
  심리학자: "낮에 두 명의 관계나 태도를 관찰해 정보 단서를 얻는 시민팀 역할입니다. 직접 판정형 역할은 아니지만, 서로의 주장 변화나 투표 라인을 해석하는 데 좋습니다. 같은 대상군을 반복 관찰하면 모순을 찾기 쉽습니다.",
  최면술사: "밤에 대상을 최면 상태로 누적시키고, 낮에 한 번에 깨워 팀 또는 직업 정보를 확인하는 시민팀 역할입니다. 깨우면 다음 밤에는 최면을 사용할 수 없으므로, 언제 정보를 공개할지 판단이 중요합니다. 여러 명을 모아 한 번에 확인하면 폭발적인 정보력을 얻습니다.",
  도둑: "밤에 다른 사람의 직업을 훔쳐 그 능력을 사용할 수 있는 마피아팀 역할입니다. 훔친 직업에 따라 행동 방식이 크게 달라지며, 경찰 계열을 훔치면 독립적으로 결과를 얻습니다. 훔칠 대상의 직업 가치와 자신의 생존 가능성을 같이 봐야 합니다.",
  군인: "마피아의 공격을 한 번 버티는 방어형 시민 역할입니다. 방탄이 발동되면 강한 생존 정보가 되지만, 너무 빨리 드러내면 이후 행동 가치가 줄어듭니다. 자신이 왜 살아남았는지 설명할 준비가 필요합니다.",
  간호사: "의사를 보조하고 의사와의 접선 정보를 활용하는 시민팀 역할입니다. 의사 위치를 파악하면 치료 흐름을 안정시키는 데 도움이 됩니다. 의사 주장자가 여러 명일 때는 접선 여부와 밤 결과를 함께 정리해야 합니다.",
  마담: "투표를 통해 상대를 유혹해 능력과 발언을 제한하는 마피아팀 보조 역할입니다. 핵심 시민 직업이나 결정적인 투표권을 묶으면 낮 구도를 크게 흔들 수 있습니다. 접선 이후에는 마피아 비밀방에서 밤 대화가 가능해집니다.",
  마녀: "밤에 대상을 개구리로 저주하는 마피아팀 보조 역할입니다. 저주 대상은 능력 사용과 낮 발언 방식에 제약을 받으므로, 중요한 수사직이나 투표 영향력이 큰 사람을 흔드는 데 좋습니다. 마피아와 접선했는지에 따라 정보 판정 해석도 달라집니다.",
  과학자: "사망 이후 소생 가능성을 가진 마피아팀 역할입니다. 죽었다고 끝난 것이 아니므로, 사망 타이밍과 공개 정보가 모두 전략 요소가 됩니다. 소생 후에는 의심이 커지기 쉬워 후속 발언 준비가 필요합니다.",
  대부: "마피아팀 특수 역할로, 조사 회피와 자동 접선 흐름을 이용해 라인을 만들 수 있습니다. 경찰에게 바로 잡히지 않는 장점이 있지만, 행동과 발언 모순까지 막아주지는 않습니다. 마피아팀의 장기 생존 축으로 운영하는 것이 좋습니다.",
  스파이: "밤에 첩보를 사용해 마피아와 접선하고 정보를 얻는 마피아팀 보조 역할입니다. 접선 전에는 정보 손실을 줄이고, 접선 후에는 마피아팀과 정보를 합쳐 시민팀 핵심 직업을 찾는 것이 중요합니다.",
  테러리스트: "처형이나 공격 상황에서 교환 가치를 만드는 시민팀 역할입니다. 자신이 죽을 때 누구를 함께 데려갈 수 있는지, 그 교환이 시민팀에 이득인지 계산해야 합니다. 마피아가 회피하도록 압박하는 존재감도 중요합니다.",
  정치인: "투표에서 2표 영향력을 가지는 시민팀 역할입니다. 단순 생존보다 최종 투표 구도에서 큰 힘을 발휘합니다. 자신의 표가 판정에 미치는 영향을 계산하고, 막판 표 이동을 주도해야 합니다.",
  판사: "찬반투표 동률이나 중요한 처형 판단에서 판세를 뒤집을 수 있는 시민팀 역할입니다. 공개 전에는 일반 시민처럼 보일 수 있지만, 결정 순간에 존재감이 커집니다. 찬반 수와 처형 대상의 팀 가치를 계속 계산해야 합니다.",
  건달: "밤에 한 명을 공갈해 다음 낮 투표권을 막는 시민팀 역할입니다. 투표권 하나가 승패를 바꿀 수 있는 후반에 특히 강합니다. 누구의 표를 막는 것이 시민팀에 이득인지 라인과 생존자 수를 보고 결정해야 합니다.",
  연인: "서로를 알고 밤 대화를 통해 정보를 맞출 수 있는 시민팀 특수 관계입니다. 둘 중 한 명의 정보가 다른 한 명의 신뢰를 보강할 수 있지만, 동시에 한쪽이 흔들리면 같이 의심받기 쉽습니다. 생존과 정보 공개 타이밍을 함께 맞춰야 합니다.",
  개구리: "마녀 저주로 인해 밤 능력을 사용할 수 없고 낮에는 제한된 방식으로만 말할 수 있는 상태입니다. 직접적인 정보 전달이 어려우므로, 짧은 표현으로 핵심 의심 대상과 결과를 남기는 것이 중요합니다.",
  악인: "마피아팀으로 승리하는 보조 성향 역할입니다. 정체가 너무 빨리 드러나면 시민팀에게 집중 견제를 받으므로, 접선 전까지는 시민처럼 정보를 정리하며 마피아팀과 연결될 기회를 봐야 합니다.",
};

const ROLE_TIPS: Record<string, string[]> = {
  시민: ["확정 정보와 추측을 분리해 메모", "투표 전 생존자 수와 과반 기준 확인", "직업 주장자 간 모순 우선 비교", "스킵이 이득인지 지목이 이득인지 계산"],
  마피아: ["마피아 비밀방 선택 현황 계속 확인", "수사직·의사·확정 시민 순서로 위협도 계산", "낮 발언은 시민 관점으로 일관성 유지", "팀원이 몰릴 때 표 분산과 라인 관리"],
  경찰: ["조사 대상, 결과, 일차를 같이 기록", "대부·접선 보조직 예외 가능성 확인", "결과 공개 전 의사 생존 여부 고려", "맞경 주장과 투표 흐름을 함께 비교"],
  의사: ["보호 대상이 공개 확직인지 핵심 발언자인지 구분", "마피아가 역으로 노릴 대상 예측", "연속 보호 가치와 대상 변경 가치를 비교", "치료 성공 시 즉시 판세 재계산"],
  요원: ["조사 결과를 낮 흐름과 연결", "공개 타이밍이 생존에 미치는 영향 확인", "다른 수사직 결과와 충돌 여부 확인", "결과가 없어도 대상 선정 이유 기록"],
  자경단: ["조사와 처형을 별도 판단", "오판 시 시민 수 손실 계산", "수사직 결과와 투표 라인 확인 후 처단", "후반 마피아 수 우위 조건 차단용으로 활용"],
  탐정: ["대상 이동 경로를 날짜별로 누적", "직업 주장과 실제 행동 가능성 비교", "같은 대상 반복 추적보다 핵심 라인 추적", "밤 행동 없는 직업의 거짓 주장 확인"],
  기자: ["취재 대상 공개 가치 계산", "이미 확정된 대상보다 애매한 핵심 대상 우선", "공개 후 투표 흐름 변화 예상", "마피아가 노릴 타이밍 전에 사용"],
  해커: ["행동 정보와 발언 모순 연결", "다음 낮 지목 근거로 정리", "수사직 주장자 검증에 활용", "단독 결론보다 다른 정보와 결합"],
  영매: ["사망자 역할과 생전 발언 연결", "죽은 사람 기준으로 투표 라인 복원", "사망자 채팅 정보와 공개 정보 구분", "후반에는 사망자 수가 곧 정보량"],
  성직자: ["부활 가치가 높은 역할 우선", "교주팀 관련 위험 대상 분리", "부활 후 공개될 정보량 계산", "한 번의 사용으로 판세가 바뀌는지 판단"],
  교주: ["포교 성공 후 팀 수 계산", "마피아와 시민 싸움 사이에서 생존 우선", "포교 대상의 공개 직업 가치 확인", "승리 조건 근접 시 과감한 표 조정"],
  광신도: ["교주 생존 여부 최우선 확인", "포교 정보 보존", "교주팀 숫자 우위 가능성 계산", "교주 노출 시 대체 표 흐름 준비"],
  조커: ["처형 유도와 과한 노출 사이 균형", "마피아로 확정되지 않게 의심 유지", "후반 과반 계산 이용", "찬반투표에서 처형 가능성 관리"],
  청부업자: ["두 대상 모두 공개 정보 충분할 때 제출", "직업 주장과 실제 행동 가능성 대조", "수사직과 공개 직업은 대상 제한 확인", "성공 시 접선/암살 가치까지 계산"],
  심리학자: ["두 대상의 상호 관계를 메모", "투표 전후 태도 변화 관찰", "반복 관찰로 라인 모순 찾기", "결과를 확정 판정처럼 과신하지 않기"],
  최면술사: ["최면 대상 누적 현황 기억", "깨우기 전까지 정보 공개 보류 가능", "낮에 깨우면 다음 밤 행동 불가", "여러 명을 한 번에 깨워 팀 구도 재계산"],
  도둑: ["훔친 직업의 당일 능력 확인", "경찰 계열은 독립 결과로 관리", "마피아 직업 도벽 시 접선 흐름 확인", "생존 가능성과 능력 가치 비교"],
  군인: ["방탄 발동 여부를 공개할 타이밍 판단", "공격받은 이유로 마피아 의도 추론", "거짓 군인 주장과 충돌 시 기록", "후반에는 생존 자체가 시민 수 방어"],
  간호사: ["의사 접선 여부 확인", "의사 주장자와 치료 흐름 비교", "의사 생존 추정에 도움", "치료 관련 공개 정보와 모순 점검"],
  마담: ["투표권이 큰 대상 우선 유혹", "수사직·의사·정치인 등 핵심 역할 견제", "접선 후 마피아 채팅 정보 공유", "유혹 지속 기간과 다음 투표 구도 계산"],
  마녀: ["저주 대상의 능력 가치 확인", "개구리 상태가 낮 발언에 미치는 영향 활용", "마피아 접선 여부와 경찰 판정 해석", "중요 수사직 저주로 정보 흐름 차단"],
  과학자: ["죽음 이후 소생 타이밍 고려", "사망 전 발언으로 후속 의심 대비", "소생 후 표적 가능성 대비", "마피아팀 수 계산에 자신 포함 여부 확인"],
  대부: ["조사 회피를 이용해 과감한 라인 형성", "자동 접선 타이밍 확인", "행동 모순은 조사 회피로도 숨길 수 없음", "후반 마피아 수 우위 조건 계산"],
  스파이: ["접선 전 정보 손실 최소화", "마피아 발견 시 추가 첩보 가치 확인", "수사직 위치 추정에 집중", "접선 후 마피아팀 목표와 정보 통합"],
  테러리스트: ["죽을 때 데려갈 대상 가치 계산", "마피아가 공격을 피하게 만드는 압박", "처형 후보가 됐을 때 교환 손익 설명", "확정 마피아와 교환하면 큰 이득"],
  정치인: ["자신의 2표가 결과를 바꾸는지 계산", "스킵/지목 동률 가능성 확인", "막판 표 이동을 주도", "공갈 대상이 되면 투표 영향 상실 주의"],
  판사: ["찬반 동률과 처형 기준 확인", "공개 전후 영향력 차이 계산", "처형 대상의 팀 가치를 따져 선택", "막판 뒤집기 가능성을 숨겨두기"],
  건달: ["막을 표의 가치 계산", "정치인·확정 마피아 후보 등 우선순위 지정", "다음 낮 투표 구도와 함께 사용", "공갈 성공 후 투표 결과 변화 확인"],
  연인: ["밤 대화로 서로의 정보 정합성 확인", "한쪽 공개가 다른 한쪽 신뢰에 미치는 영향 계산", "둘 다 살아야 정보 가치 상승", "동반 의심받지 않게 발언 일관성 유지"],
  개구리: ["짧은 표현으로 핵심 정보 전달", "능력 사용 불가 상태임을 고려", "저주한 마녀 후보 추론", "해제 후 이전 발언 보강"],
  악인: ["마피아팀 승리 조건 기준으로 움직임", "접선 전에는 시민처럼 정보 정리", "정체 노출 타이밍 조절", "마피아와 연결될 밤 행동 기회 확인"],
};

export default function App() {
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>("connecting");
  const [errorMsg, setErrorMsg] = useState("");
  const [guildId, setGuildId] = useState("");
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [focusedPlayerId, setFocusedPlayerId] = useState<string | null>(null);
  const [playerFilter, setPlayerFilter] = useState<PlayerFilter>("alive");
  const [playerSort, setPlayerSort] = useState<PlayerSort>("status");
  const [playerMarks, setPlayerMarks] = useState<Record<string, PlayerMark>>({});
  const [playerNotes, setPlayerNotes] = useState<Record<string, string>>({});
  const [activityLog, setActivityLog] = useState<ActivityEvent[]>([]);
  const [notes, setNotes] = useState("");
  const snapshotRef = useRef<GameSnapshot | null>(null);
  const gameStorageKey = useMemo(() => {
    if (!guildId || !gameState?.in_game || !gameState.game_key) return "";
    return `mafia-activity:${guildId}:${gameState.game_key}`;
  }, [gameState?.game_key, gameState?.in_game, guildId]);

  useEffect(() => {
    (async () => {
      try {
        const auth = await authenticateWithDiscord();
        setSession(auth.sessionToken, auth.guildId);
        setGuildId(auth.guildId);
        setAuthStatus("ready");
      } catch (e) {
        setErrorMsg(e instanceof Error ? e.message : JSON.stringify(e));
        setAuthStatus("error");
      }
    })();
  }, []);

  const refreshState = useCallback(async () => {
    const state = await fetchState();
    setGameState(state);
    setConnectionStatus("live");
  }, []);

  useEffect(() => {
    if (authStatus !== "ready") return;

    setConnectionStatus("connecting");
    refreshState().catch((error) => {
      console.error(error);
      setConnectionStatus("offline");
    });

    const socket = createWebSocket((state) => {
      setGameState(state);
      setConnectionStatus("live");
    });
    socket.addEventListener("open", () => setConnectionStatus("live"));
    socket.addEventListener("close", () => setConnectionStatus("offline"));
    socket.addEventListener("error", () => setConnectionStatus("offline"));

    return () => socket.close();
  }, [authStatus, refreshState]);

  useEffect(() => {
    if (!gameStorageKey) return;
    setPlayerMarks(readJson<Record<string, PlayerMark>>(`${gameStorageKey}:marks`, {}));
    setPlayerNotes(readJson<Record<string, string>>(`${gameStorageKey}:player-notes`, {}));
    setActivityLog(readJson<ActivityEvent[]>(`${gameStorageKey}:log`, []));
    setNotes(localStorage.getItem(`${gameStorageKey}:notes`) ?? "");
    setSelectedTarget(null);
    setFocusedPlayerId(null);
    snapshotRef.current = null;
  }, [gameStorageKey]);

  useEffect(() => {
    if (!gameStorageKey) return;
    localStorage.setItem(`${gameStorageKey}:marks`, JSON.stringify(playerMarks));
  }, [gameStorageKey, playerMarks]);

  useEffect(() => {
    if (!gameStorageKey) return;
    localStorage.setItem(`${gameStorageKey}:player-notes`, JSON.stringify(playerNotes));
  }, [gameStorageKey, playerNotes]);

  useEffect(() => {
    if (!gameStorageKey) return;
    localStorage.setItem(`${gameStorageKey}:log`, JSON.stringify(activityLog.slice(0, 32)));
  }, [activityLog, gameStorageKey]);

  useEffect(() => {
    if (!gameStorageKey) return;
    localStorage.setItem(`${gameStorageKey}:notes`, notes);
  }, [gameStorageKey, notes]);

  useEffect(() => {
    if (!gameStorageKey || !gameState?.in_game) {
      snapshotRef.current = null;
      return;
    }

    const next = snapshotGame(gameState);
    const previous = snapshotRef.current;
    if (!previous) {
      snapshotRef.current = next;
      return;
    }

    const events = diffGameEvents(previous, next, gameState);
    if (events.length > 0) {
      setActivityLog((prev) => [...events, ...prev].slice(0, 32));
    }
    snapshotRef.current = next;
  }, [gameState, gameStorageKey]);

  useEffect(() => {
    setSelectedTarget(null);
  }, [gameState?.game_key, gameState?.day_number, gameState?.phase]);

  useEffect(() => {
    if (!gameState || !selectedTarget) return;
    const selectableIds = new Set(selectableTargetIds(gameState));
    if (!selectableIds.has(selectedTarget)) {
      setSelectedTarget(null);
    }
  }, [gameState, selectedTarget]);

  const handleActionSent = useCallback(() => {
    refreshState().catch((error) => {
      console.error(error);
      setConnectionStatus("offline");
    });
  }, [refreshState]);

  const setMark = useCallback((playerId: string, mark: PlayerMark) => {
    setPlayerMarks((prev) => ({ ...prev, [playerId]: mark }));
  }, []);

  const setPlayerNote = useCallback((playerId: string, value: string) => {
    setPlayerNotes((prev) => {
      if (value) return { ...prev, [playerId]: value };
      const next = { ...prev };
      delete next[playerId];
      return next;
    });
  }, []);

  const selectTarget = useCallback((id: string | null) => {
    setSelectedTarget(id);
    if (id) setFocusedPlayerId(id);
  }, []);

  const focusPlayer = useCallback(
    (id: string) => {
      setFocusedPlayerId(id);
      const player = gameState?.players.find((item) => item.id === id);
      if (player?.alive && (gameState?.phase === "Vote" || !player.is_you)) {
        setSelectedTarget(id);
      } else if (selectedTarget === id) {
        setSelectedTarget(null);
      }
    },
    [gameState?.players, selectedTarget],
  );

  if (authStatus === "loading") return <LoadingScreen text="Discord 연결 중" />;
  if (authStatus === "error") return <ErrorScreen msg={errorMsg} />;
  if (!gameState) return <LoadingScreen text="게임 정보 로딩 중" />;
  if (!gameState.in_game) {
    return (
      <div className="activity-shell is-empty">
        <section className="empty-state">
          <div className="empty-mark">M</div>
          <h1>마피아 게임</h1>
          <p>진행 중인 게임 없음</p>
        </section>
      </div>
    );
  }

  const phase = PHASE_META[gameState.phase];
  const aliveCount = gameState.players.filter((p) => p.alive).length;
  const deadCount = gameState.players.length - aliveCount;
  const me = gameState.players.find((p) => p.is_you);
  const focusedPlayer = gameState.players.find((p) => p.id === focusedPlayerId) ?? me;

  return (
    <div className={`activity-shell phase-${phase.tone}`}>
      <TopBar
        state={gameState}
        connectionStatus={connectionStatus}
        aliveCount={aliveCount}
        deadCount={deadCount}
        onRefresh={handleActionSent}
      />

      <main className="activity-layout">
        <section className="primary-column">
          <RoleFocus player={me} state={gameState} />
          <RoundBrief
            state={gameState}
            focusedPlayer={focusedPlayer}
            selectedTarget={selectedTarget}
            marks={playerMarks}
            notes={playerNotes}
          />
          <ActionConsole
            state={gameState}
            selectedTarget={selectedTarget}
            onSelectTarget={selectTarget}
            onActionSent={handleActionSent}
          />
          <VoteIntel state={gameState} />
          <PublicStatus text={gameState.public_status} />
        </section>

        <section className="secondary-column">
          <PlayerDesk
            state={gameState}
            selectedTarget={selectedTarget}
            focusedPlayerId={focusedPlayer?.id ?? null}
            filter={playerFilter}
            sort={playerSort}
            marks={playerMarks}
            notes={playerNotes}
            onFilter={setPlayerFilter}
            onSort={setPlayerSort}
            onFocusPlayer={focusPlayer}
            onMark={setMark}
            onNote={setPlayerNote}
          />
          <NotesPanel notes={notes} onNotes={setNotes} marks={playerMarks} />
          <EventLog events={activityLog} onClear={() => setActivityLog([])} />
        </section>
      </main>
    </div>
  );
}

function TopBar({
  state,
  connectionStatus,
  aliveCount,
  deadCount,
  onRefresh,
}: {
  state: GameState;
  connectionStatus: ConnectionStatus;
  aliveCount: number;
  deadCount: number;
  onRefresh: () => void;
}) {
  const phase = PHASE_META[state.phase];
  const remaining = useRemainingSeconds(state.phase_ends_at);
  const actionText = state.can_act || state.contractor_can_act ? "행동 가능" : "대기";

  return (
    <header className="top-bar">
      <div className="phase-block">
        <div className="phase-label">{phase.label}</div>
        <div className="phase-sub">
          {state.day_number}일차 · {phase.summary}
        </div>
      </div>

      <div className="timer-block">
        <span className={remaining !== null && remaining <= 10 ? "timer danger" : "timer"}>
          {remaining === null ? "대기" : formatClock(remaining)}
        </span>
      </div>

      <div className="top-stats">
        <Stat label="생존" value={aliveCount} />
        <Stat label="사망" value={deadCount} />
        <Stat label="상태" value={actionText} />
      </div>

      <button className="icon-command" onClick={onRefresh} title="상태 새로고침" type="button">
        ↻
      </button>

      <div className={`connection-dot ${connectionStatus}`} title={`연결: ${connectionStatus}`} />
    </header>
  );
}

function RoleFocus({ player, state }: { player?: PlayerDto; state: GameState }) {
  const team = state.my_team;
  const teamMeta = team ? TEAM_META[team] : null;
  const role = state.my_role ?? player?.role ?? "관전자";
  const guide = ROLE_HELP[role] ?? "공개 정보, 투표 흐름, 발언 모순을 같이 보세요.";
  const tips = ROLE_TIPS[role] ?? ["생존자 수와 사망자 역할 확인", "득표 흐름과 메모를 같이 비교"];
  const result = state.my_action_result;

  return (
    <section className={`panel role-focus ${teamMeta?.className ?? "team-unknown"}`}>
      <div className="section-kicker">내 정보</div>
      <div className="role-main">
        <div>
          <h1>{role}</h1>
          <p>{teamMeta?.label ?? "관전자"}</p>
        </div>
        <div className="role-badge">{player?.alive === false ? "사망" : "생존"}</div>
      </div>
      <RoleGuide summary={guide} tips={tips} />
      {state.my_night_target && (
        <div className="mini-alert">
          밤 대상: {state.players.find((p) => p.id === state.my_night_target)?.name ?? "알 수 없음"}
        </div>
      )}
      {result && <div className="result-alert">{result}</div>}
    </section>
  );
}

function RoleGuide({ summary, tips }: { summary: string; tips: string[] }) {
  return (
    <div className="role-guide">
      <strong>운영 포인트</strong>
      <p>{summary}</p>
      <ul>
        {tips.map((tip) => (
          <li key={tip}>{tip}</li>
        ))}
      </ul>
    </div>
  );
}

function RoundBrief({
  state,
  focusedPlayer,
  selectedTarget,
  marks,
  notes,
}: {
  state: GameState;
  focusedPlayer?: PlayerDto;
  selectedTarget: string | null;
  marks: Record<string, PlayerMark>;
  notes: Record<string, string>;
}) {
  const leader = voteLeader(state);
  const checks = PHASE_CHECKS[state.phase];
  const targetName = state.players.find((player) => player.id === selectedTarget)?.name;
  const focusedMark = focusedPlayer ? MARK_META[marks[focusedPlayer.id] ?? "none"] : null;
  const focusedNote = focusedPlayer ? notes[focusedPlayer.id] : "";
  const voteLeaderText = state.phase === "Vote" && leader ? `${leader.player.name} ${leader.votes}표` : "진행 중 아님";
  const skipText = state.phase === "Day" ? `${state.day_skip_count}/${state.day_skip_threshold}` : "-";

  return (
    <section className="panel round-brief">
      <div className="brief-grid">
        <div className="brief-card is-primary">
          <span>현재 대상</span>
          <b>{targetName ?? "없음"}</b>
        </div>
        <div className="brief-card">
          <span>득표 선두</span>
          <b>{voteLeaderText}</b>
        </div>
        <div className="brief-card">
          <span>바로 투표</span>
          <b>{skipText}</b>
        </div>
      </div>

      <div className="phase-checks">
        {checks.map((item, index) => (
          <span key={item} className={index === 0 ? "active" : ""}>
            {item}
          </span>
        ))}
      </div>

      {focusedPlayer && (
        <div className="focus-strip">
          <div>
            <span className="section-kicker">선택</span>
            <strong>{focusedPlayer.name}</strong>
            <small>
              {focusedPlayer.alive ? "생존" : "사망"} · {focusedPlayer.role ?? "직업 미공개"}
            </small>
          </div>
          {focusedMark && focusedMark.label !== "표시 없음" && (
            <span className={`mark-badge inline ${focusedMark.className}`}>{focusedMark.label}</span>
          )}
          {focusedNote && <p>{focusedNote}</p>}
        </div>
      )}
    </section>
  );
}

function ActionConsole({
  state,
  selectedTarget,
  onSelectTarget,
  onActionSent,
}: {
  state: GameState;
  selectedTarget: string | null;
  onSelectTarget: (id: string | null) => void;
  onActionSent: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [contractTargets, setContractTargets] = useState<[string, string]>(["", ""]);
  const [contractRoles, setContractRoles] = useState<[string, string]>(["", ""]);
  const [psychologistTargets, setPsychologistTargets] = useState<[string, string]>(["", ""]);
  const me = state.players.find((p) => p.is_you);
  const voteTargets = state.players.filter((p) => p.alive);
  const nightTargets = state.players.filter((p) => state.night_target_ids.includes(p.id));
  const specialAction = state.special_action;
  const specialMeta = specialAction ? SPECIAL_ACTION_META[specialAction] : null;
  const specialTargets = state.players.filter((p) => state.special_action_target_ids.includes(p.id));
  const selectedPlayer = state.players.find((p) => p.id === selectedTarget);

  async function run(req: Omit<ActionRequest, "guild_id">, successText: string) {
    setBusy(true);
    setMessage("");
    try {
      const res = await sendAction(req);
      setMessage(res.ok ? res.message ?? successText : res.message ?? "요청 실패");
      if (res.ok) onActionSent();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "요청 실패");
    } finally {
      setBusy(false);
    }
  }

  function setContractTarget(slot: 0 | 1, value: string) {
    setContractTargets((prev) => (slot === 0 ? [value, prev[1]] : [prev[0], value]));
  }

  function setContractRole(slot: 0 | 1, value: string) {
    setContractRoles((prev) => (slot === 0 ? [value, prev[1]] : [prev[0], value]));
  }

  function setPsychologistTarget(slot: 0 | 1, value: string) {
    setPsychologistTargets((prev) => (slot === 0 ? [value, prev[1]] : [prev[0], value]));
  }

  const canNightAction = state.phase === "Night" && me?.alive && state.can_act;
  const nightTargetSelected = Boolean(selectedTarget && nightTargets.some((player) => player.id === selectedTarget));
  const specialTargetSelected = Boolean(selectedTarget && specialTargets.some((player) => player.id === selectedTarget));
  const canSkip = state.phase === "Day" && me?.alive;
  const canVote = state.phase === "Vote" && me?.alive;
  const canConfirm = state.phase === "ConfirmVote" && me?.alive;
  const nominee = state.players.find((p) => p.id === state.nominee);

  return (
    <section className="panel action-console">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">행동 콘솔</div>
          <h2>{actionHeadline(state)}</h2>
        </div>
        {selectedPlayer && <span className="selected-chip">{selectedPlayer.name}</span>}
      </div>

      {canNightAction && (
        <div className="action-group">
          <TargetGrid
            players={nightTargets}
            selectedTarget={selectedTarget}
            voteTargets={state.vote_targets}
            onSelect={onSelectTarget}
          />
          <div className="command-row">
            <button
              className="primary-command"
              disabled={busy || !nightTargetSelected}
              onClick={() =>
                run({ action: "night_action", target_id: selectedTarget ?? undefined }, "밤 행동 제출 완료")
              }
              type="button"
            >
              대상 제출
            </button>
            {state.night_action_can_skip && (
              <button
                className="secondary-command"
                disabled={busy}
                onClick={() => run({ action: "night_action" }, "밤 행동 스킵 완료")}
                type="button"
              >
                스킵
              </button>
            )}
          </div>
        </div>
      )}

      {state.contractor_can_act && (
        <div className="contractor-grid">
          {[0, 1].map((slot) => (
            <div className="contract-slot" key={slot}>
              <span>청부 {slot + 1}</span>
              <select
                value={contractTargets[slot as 0 | 1]}
                onChange={(e) => setContractTarget(slot as 0 | 1, e.target.value)}
              >
                <option value="">대상</option>
                {state.contractor_targets
                  .filter((target) => target.id !== contractTargets[slot === 0 ? 1 : 0])
                  .map((target) => (
                    <option key={target.id} value={target.id}>
                      {target.name}
                    </option>
                  ))}
              </select>
              <select
                value={contractRoles[slot as 0 | 1]}
                onChange={(e) => setContractRole(slot as 0 | 1, e.target.value)}
              >
                <option value="">직업</option>
                {state.contractor_guess_roles.map((role) => (
                  <option key={role} value={role}>
                    {role}
                  </option>
                ))}
              </select>
            </div>
          ))}
          <button
            className="primary-command wide"
            disabled={
              busy ||
              !contractTargets[0] ||
              !contractTargets[1] ||
              !contractRoles[0] ||
              !contractRoles[1] ||
              contractTargets[0] === contractTargets[1]
            }
            onClick={() =>
              run(
                {
                  action: "contractor_action",
                  contract_target_ids: contractTargets,
                  contract_roles: contractRoles,
                },
                "청부 제출 완료",
              )
            }
            type="button"
          >
            청부 제출
          </button>
        </div>
      )}

      {specialAction && specialMeta && (
        <div className="action-group">
          <div className="section-kicker">{specialMeta.label}</div>
          {specialMeta.requiresPair ? (
            <div className="contractor-grid">
              {[0, 1].map((slot) => (
                <div className="contract-slot" key={slot}>
                  <span>대상 {slot + 1}</span>
                  <select
                    value={psychologistTargets[slot as 0 | 1]}
                    onChange={(event) => setPsychologistTarget(slot as 0 | 1, event.target.value)}
                  >
                    <option value="">대상 선택</option>
                    {specialTargets
                      .filter((target) => target.id !== psychologistTargets[slot === 0 ? 1 : 0])
                      .map((target) => (
                        <option key={target.id} value={target.id}>
                          {target.name}
                        </option>
                      ))}
                  </select>
                </div>
              ))}
              <button
                className="primary-command wide"
                disabled={
                  busy ||
                  !psychologistTargets[0] ||
                  !psychologistTargets[1] ||
                  psychologistTargets[0] === psychologistTargets[1]
                }
                onClick={() =>
                  run(
                    {
                      action: specialMeta.action,
                      target_id: psychologistTargets[0],
                      secondary_target_id: psychologistTargets[1],
                    },
                    "관찰 완료",
                  )
                }
                type="button"
              >
                관찰 실행
              </button>
            </div>
          ) : specialMeta.requiresTarget === false ? (
            <div className="command-row">
              <button
                className="primary-command wide"
                disabled={busy}
                onClick={() => run({ action: specialMeta.action }, `${specialMeta.label} 완료`)}
                type="button"
              >
                {specialMeta.label}
              </button>
            </div>
          ) : (
            <>
              <TargetGrid
                players={specialTargets}
                selectedTarget={selectedTarget}
                voteTargets={state.vote_targets}
                onSelect={onSelectTarget}
              />
              <div className="command-row">
                <button
                  className="primary-command"
                  disabled={busy || !specialTargetSelected}
                  onClick={() =>
                    run(
                      { action: specialMeta.action, target_id: selectedTarget ?? undefined },
                      `${specialMeta.label} 완료`,
                    )
                  }
                  type="button"
                >
                  {specialMeta.label} 실행
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {canSkip && (
        <div className="skip-strip">
          <ProgressBar value={state.day_skip_count} max={state.day_skip_threshold} />
          <button
            className="secondary-command"
            disabled={busy}
            onClick={() => run({ action: "skip_vote" }, "바로 투표 완료")}
            type="button"
          >
            바로 투표
          </button>
        </div>
      )}

      {canVote && (
        <div className="action-group">
          <TargetGrid
            players={voteTargets}
            selectedTarget={selectedTarget}
            voteTargets={state.vote_targets}
            onSelect={onSelectTarget}
          />
          <div className="command-row">
            <button
              className="danger-command"
              disabled={busy || !selectedTarget}
              onClick={() =>
                run({ action: "day_vote", target_id: selectedTarget ?? undefined }, "투표 완료")
              }
              type="button"
            >
              지목
            </button>
            <button
              className="secondary-command"
              disabled={busy}
              onClick={() => run({ action: "day_vote" }, "스킵 완료")}
              type="button"
            >
              스킵
            </button>
          </div>
        </div>
      )}

      {canConfirm && (
        <div className="confirm-box">
          <div>
            <span className="confirm-target">{nominee?.name ?? "대상 없음"}</span>
            <small>
              찬성 {state.confirm_yes} · 반대 {state.confirm_no}
            </small>
          </div>
          <div className="command-row compact">
            <button
              className="primary-command"
              disabled={busy}
              onClick={() => run({ action: "confirm_vote", confirm: true }, "찬성 완료")}
              type="button"
            >
              찬성
            </button>
            <button
              className="danger-command"
              disabled={busy}
              onClick={() => run({ action: "confirm_vote", confirm: false }, "반대 완료")}
              type="button"
            >
              반대
            </button>
          </div>
        </div>
      )}

      {!canNightAction && !state.contractor_can_act && !specialAction && !canSkip && !canVote && !canConfirm && (
        <div className="idle-box">{state.phase === "Ended" ? "게임 종료" : "제출할 행동 없음"}</div>
      )}

      {message && <div className={message.includes("완료") ? "toast ok" : "toast error"}>{message}</div>}
    </section>
  );
}

function TargetGrid({
  players,
  selectedTarget,
  voteTargets,
  onSelect,
}: {
  players: PlayerDto[];
  selectedTarget: string | null;
  voteTargets: Record<string, number>;
  onSelect: (id: string | null) => void;
}) {
  return (
    <div className="target-grid">
      {players.map((player) => (
        <button
          key={player.id}
          className={selectedTarget === player.id ? "target-button selected" : "target-button"}
          onClick={() => onSelect(selectedTarget === player.id ? null : player.id)}
          type="button"
        >
          <span>{player.name}</span>
          {player.role && <small>{player.role}</small>}
          {(voteTargets[player.id] ?? 0) > 0 && <b>{voteTargets[player.id]}</b>}
        </button>
      ))}
    </div>
  );
}

function PlayerDesk({
  state,
  selectedTarget,
  focusedPlayerId,
  filter,
  sort,
  marks,
  notes,
  onFilter,
  onSort,
  onFocusPlayer,
  onMark,
  onNote,
}: {
  state: GameState;
  selectedTarget: string | null;
  focusedPlayerId: string | null;
  filter: PlayerFilter;
  sort: PlayerSort;
  marks: Record<string, PlayerMark>;
  notes: Record<string, string>;
  onFilter: (filter: PlayerFilter) => void;
  onSort: (sort: PlayerSort) => void;
  onFocusPlayer: (id: string) => void;
  onMark: (id: string, mark: PlayerMark) => void;
  onNote: (id: string, value: string) => void;
}) {
  const [query, setQuery] = useState("");
  const players = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return state.players
      .filter((player) => {
        if (filter === "alive" && !player.alive) return false;
        if (filter === "dead" && player.alive) return false;
        if (filter === "marked" && (!marks[player.id] || marks[player.id] === "none")) return false;
        if (filter === "voted" && (state.vote_targets[player.id] ?? 0) === 0) return false;
        if (!normalized) return true;
        return `${player.name} ${player.role ?? ""} ${notes[player.id] ?? ""}`.toLowerCase().includes(normalized);
      })
      .sort((a, b) => comparePlayers(a, b, sort, marks, state.vote_targets));
  }, [filter, marks, notes, query, sort, state.players, state.vote_targets]);

  return (
    <section className="panel player-desk">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">플레이어 보드</div>
          <h2>{state.players.length}명</h2>
        </div>
        <div className="desk-tools">
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="검색" />
          <select value={sort} onChange={(e) => onSort(e.target.value as PlayerSort)} title="정렬">
            {(["status", "votes", "name", "mark"] as PlayerSort[]).map((item) => (
              <option key={item} value={item}>
                {SORT_LABELS[item]}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="segmented">
        {(["alive", "all", "dead", "marked", "voted"] as PlayerFilter[]).map((item) => (
          <button
            key={item}
            className={filter === item ? "active" : ""}
            onClick={() => onFilter(item)}
            type="button"
          >
            {filterLabel(item)}
          </button>
        ))}
      </div>
      <div className="player-list">
        {players.map((player) => (
          <PlayerRow
            key={player.id}
            player={player}
            votes={state.vote_targets[player.id] ?? 0}
            selected={focusedPlayerId === player.id}
            actionSelected={selectedTarget === player.id}
            mark={marks[player.id] ?? "none"}
            note={notes[player.id] ?? ""}
            onSelect={() => onFocusPlayer(player.id)}
            onMark={(mark) => onMark(player.id, mark)}
            onNote={(value) => onNote(player.id, value)}
          />
        ))}
      </div>
    </section>
  );
}

function PlayerRow({
  player,
  votes,
  selected,
  actionSelected,
  mark,
  note,
  onSelect,
  onMark,
  onNote,
}: {
  player: PlayerDto;
  votes: number;
  selected: boolean;
  actionSelected: boolean;
  mark: PlayerMark;
  note: string;
  onSelect: () => void;
  onMark: (mark: PlayerMark) => void;
  onNote: (value: string) => void;
}) {
  const team = player.role_team ? TEAM_META[player.role_team] : null;
  const markMeta = MARK_META[mark];

  return (
    <div
      className={`player-row ${selected ? "selected" : ""} ${actionSelected ? "action-selected" : ""} ${
        player.alive ? "" : "dead"
      }`}
    >
      <button className="player-main" onClick={onSelect} type="button">
        <span className={`status-pin ${team?.className ?? "team-unknown"}`} />
        <span className="player-name">
          {player.name}
          {player.is_you && <small>나</small>}
        </span>
        {player.role && <span className="role-tag">{player.role}</span>}
        {votes > 0 && <span className="vote-chip">{votes}표</span>}
      </button>
      <div className="mark-controls">
        {(["trust", "suspect", "watch"] as PlayerMark[]).map((item) => (
          <button
            key={item}
            className={mark === item ? MARK_META[item].className : ""}
            onClick={() => onMark(mark === item ? "none" : item)}
            title={MARK_META[item].label}
            type="button"
          >
            {MARK_META[item].short}
          </button>
        ))}
      </div>
      {mark !== "none" && <span className={`mark-badge ${markMeta.className}`}>{markMeta.label}</span>}
      {(selected || note) && (
        <input
          className="row-note"
          value={note}
          onChange={(event) => onNote(event.target.value)}
          placeholder="개인 메모"
        />
      )}
    </div>
  );
}

function VoteIntel({ state }: { state: GameState }) {
  const entries = Object.entries(state.vote_targets)
    .map(([id, votes]) => ({ player: state.players.find((p) => p.id === id), votes }))
    .filter((entry): entry is { player: PlayerDto; votes: number } => Boolean(entry.player))
    .sort((a, b) => b.votes - a.votes);
  const maxVotes = Math.max(1, state.vote_skip_count, ...entries.map((entry) => entry.votes));

  if (state.phase !== "Vote" && state.phase !== "FinalDefense" && state.phase !== "ConfirmVote") {
    return null;
  }

  if (state.phase === "ConfirmVote") {
    const total = Math.max(1, state.confirm_yes + state.confirm_no);
    return (
      <section className="panel vote-intel">
        <div className="section-kicker">찬반 현황</div>
        <ProgressBar value={state.confirm_yes} max={total} label={`찬성 ${state.confirm_yes}`} />
        <ProgressBar value={state.confirm_no} max={total} label={`반대 ${state.confirm_no}`} danger />
      </section>
    );
  }

  if (state.phase === "FinalDefense") {
    const nominee = state.players.find((player) => player.id === state.nominee);
    return (
      <section className="panel vote-intel">
        <div className="section-kicker">최후변론</div>
        <div className="muted-line">{nominee ? `${nominee.name} 님 변론 중` : "지목 대상 확인 중"}</div>
      </section>
    );
  }

  return (
    <section className="panel vote-intel">
      <div className="section-kicker">투표 흐름</div>
      {entries.length === 0 && state.vote_skip_count === 0 ? (
        <div className="muted-line">표 없음</div>
      ) : (
        <>
          {entries.slice(0, 5).map(({ player, votes }) => (
            <ProgressBar key={player.id} value={votes} max={maxVotes} label={`${player.name} ${votes}표`} danger />
          ))}
          {state.vote_skip_count > 0 && (
            <ProgressBar value={state.vote_skip_count} max={maxVotes} label={`스킵 ${state.vote_skip_count}표`} />
          )}
        </>
      )}
    </section>
  );
}

function PublicStatus({ text }: { text: string }) {
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

  return (
    <section className="panel public-status">
      <div className="section-kicker">공개 상태</div>
      {lines.length === 0 ? (
        <div className="muted-line">공개 정보 없음</div>
      ) : (
        <div className="status-lines">
          {lines.map((line, index) => (
            <p key={`${line}-${index}`}>{line}</p>
          ))}
        </div>
      )}
    </section>
  );
}

function NotesPanel({
  notes,
  marks,
  onNotes,
}: {
  notes: string;
  marks: Record<string, PlayerMark>;
  onNotes: (value: string) => void;
}) {
  const markedCount = Object.values(marks).filter((mark) => mark !== "none").length;
  const appendNote = (label: string) => {
    const stamp = new Date().toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" });
    onNotes(`${notes}${notes ? "\n" : ""}[${stamp}] ${label}: `);
  };
  const copyNotes = () => {
    if (!navigator.clipboard) return;
    navigator.clipboard.writeText(notes).catch(() => undefined);
  };

  return (
    <section className="panel notes-panel">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">판 메모</div>
          <h2>표시 {markedCount}</h2>
        </div>
        <div className="note-actions">
          <button
            className="icon-command"
            onClick={copyNotes}
            title="복사"
            type="button"
          >
            ⧉
          </button>
          <button className="icon-command" onClick={() => onNotes("")} title="메모 지우기" type="button">
            ×
          </button>
        </div>
      </div>
      <div className="quick-notes">
        {["밤결과", "확정", "의심", "투표", "라인"].map((item) => (
          <button key={item} onClick={() => appendNote(item)} type="button">
            {item}
          </button>
        ))}
      </div>
      <textarea value={notes} onChange={(e) => onNotes(e.target.value)} placeholder="메모" />
    </section>
  );
}

function EventLog({ events, onClear }: { events: ActivityEvent[]; onClear: () => void }) {
  return (
    <section className="panel event-log">
      <div className="panel-heading">
        <div>
          <div className="section-kicker">최근 흐름</div>
          <h2>{events.length}개</h2>
        </div>
        <button className="icon-command" onClick={onClear} title="기록 지우기" type="button">
          ×
        </button>
      </div>
      <div className="event-list">
        {events.length === 0 ? (
          <div className="muted-line">기록 없음</div>
        ) : (
          events.map((event) => (
            <div className={`event-item ${event.tone}`} key={event.id}>
              <span>{event.at}</span>
              <b>{event.text}</b>
            </div>
          ))
        )}
      </div>
    </section>
  );
}

function ProgressBar({
  value,
  max,
  label,
  danger,
}: {
  value: number;
  max: number;
  label?: string;
  danger?: boolean;
}) {
  const width = `${Math.min(100, Math.round((value / Math.max(1, max)) * 100))}%`;
  return (
    <div className="progress-line">
      {label && <span>{label}</span>}
      <div className="progress-track">
        <div className={danger ? "progress-fill danger" : "progress-fill"} style={{ width }} />
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="stat">
      <span>{label}</span>
      <b>{value}</b>
    </div>
  );
}

function LoadingScreen({ text }: { text: string }) {
  return (
    <div className="activity-shell is-empty">
      <section className="empty-state">
        <div className="loading-ring" />
        <h1>{text}</h1>
      </section>
    </div>
  );
}

function ErrorScreen({ msg }: { msg: string }) {
  return (
    <div className="activity-shell is-empty">
      <section className="empty-state error">
        <div className="empty-mark">!</div>
        <h1>인증 실패</h1>
        <p>{msg}</p>
      </section>
    </div>
  );
}

function useRemainingSeconds(phaseEndsAt: number | null) {
  const [remaining, setRemaining] = useState<number | null>(null);

  useEffect(() => {
    if (!phaseEndsAt) {
      setRemaining(null);
      return;
    }
    const update = () => setRemaining(Math.max(0, Math.ceil((phaseEndsAt - Date.now()) / 1000)));
    update();
    const id = window.setInterval(update, 500);
    return () => window.clearInterval(id);
  }, [phaseEndsAt]);

  return remaining;
}

function actionHeadline(state: GameState) {
  if (state.contractor_can_act) return "청부 제출";
  if (state.phase === "Night" && state.can_act) return "밤 행동";
  if (state.phase === "Day") return "낮 진행";
  if (state.phase === "Vote") return "투표";
  if (state.phase === "FinalDefense") return "최후변론";
  if (state.phase === "ConfirmVote") return "처형 찬반";
  return "대기";
}

function filterLabel(filter: PlayerFilter) {
  const labels: Record<PlayerFilter, string> = {
    all: "전체",
    alive: "생존",
    dead: "사망",
    marked: "표시",
    voted: "득표",
  };
  return labels[filter];
}

function comparePlayers(
  a: PlayerDto,
  b: PlayerDto,
  sort: PlayerSort,
  marks: Record<string, PlayerMark>,
  votes: Record<string, number>,
) {
  const selfFirst = Number(b.is_you) - Number(a.is_you);
  if (selfFirst !== 0) return selfFirst;

  if (sort === "votes") {
    const byVotes = (votes[b.id] ?? 0) - (votes[a.id] ?? 0);
    if (byVotes !== 0) return byVotes;
  }

  if (sort === "name") {
    return a.name.localeCompare(b.name, "ko-KR");
  }

  if (sort === "mark") {
    const byMark = markRank(marks[b.id]) - markRank(marks[a.id]);
    if (byMark !== 0) return byMark;
  }

  return Number(b.alive) - Number(a.alive) || (votes[b.id] ?? 0) - (votes[a.id] ?? 0);
}

function markRank(mark?: PlayerMark) {
  if (mark === "suspect") return 3;
  if (mark === "watch") return 2;
  if (mark === "trust") return 1;
  return 0;
}

function voteLeader(state: GameState) {
  return Object.entries(state.vote_targets)
    .map(([id, votes]) => ({ player: state.players.find((item) => item.id === id), votes }))
    .filter((entry): entry is { player: PlayerDto; votes: number } => Boolean(entry.player) && entry.votes > 0)
    .sort((a, b) => b.votes - a.votes)[0];
}

function selectableTargetIds(state: GameState) {
  if (state.phase === "Night") {
    return state.night_target_ids;
  }
  if (state.phase === "Vote") {
    return state.players.filter((player) => player.alive).map((player) => player.id);
  }
  if (state.special_action) {
    return state.special_action_target_ids;
  }
  return [];
}

function snapshotGame(state: GameState): GameSnapshot {
  return {
    gameKey: state.game_key,
    phase: state.phase,
    dayNumber: state.day_number,
    nominee: state.nominee,
    confirmYes: state.confirm_yes,
    confirmNo: state.confirm_no,
    actionResult: state.my_action_result,
    votes: { ...state.vote_targets },
    skipVotes: state.vote_skip_count,
  };
}

function diffGameEvents(previous: GameSnapshot, next: GameSnapshot, state: GameState): ActivityEvent[] {
  const events: ActivityEvent[] = [];
  const phase = PHASE_META[next.phase];

  if (previous.phase !== next.phase || previous.dayNumber !== next.dayNumber) {
    events.push(makeEvent(`${next.dayNumber}일차 ${phase.label}`, "phase"));
  }

  if (next.actionResult && next.actionResult !== previous.actionResult) {
    events.push(makeEvent(`결과: ${next.actionResult}`, "action"));
  }

  for (const [id, votes] of Object.entries(next.votes)) {
    const previousVotes = previous.votes[id] ?? 0;
    if (votes > previousVotes) {
      const player = state.players.find((item) => item.id === id);
      events.push(makeEvent(`${player?.name ?? "대상"} ${votes}표`, "vote"));
    }
  }

  if (next.skipVotes > previous.skipVotes) {
    events.push(makeEvent(`스킵 ${next.skipVotes}표`, "vote"));
  }

  if (next.nominee && next.nominee !== previous.nominee) {
    const nominee = state.players.find((item) => item.id === next.nominee);
    events.push(makeEvent(`처형 후보 ${nominee?.name ?? "알 수 없음"}`, "vote"));
  }

  if (next.confirmYes !== previous.confirmYes || next.confirmNo !== previous.confirmNo) {
    events.push(makeEvent(`찬반 ${next.confirmYes}/${next.confirmNo}`, "vote"));
  }

  return events;
}

function makeEvent(text: string, tone: EventTone): ActivityEvent {
  return {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    at: new Date().toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" }),
    text,
    tone,
  };
}

function formatClock(seconds: number) {
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return value ? (JSON.parse(value) as T) : fallback;
  } catch {
    return fallback;
  }
}
