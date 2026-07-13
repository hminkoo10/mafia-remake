use crate::{Recruitment, RunningGame};
use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use dashmap::DashMap;
use mafia_remake::config::{self, BotConfig};
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES, Phase, Role,
};
use mafia_remake::stats::{self, StatsFile};
use mafia_remake::system_random;
use poise::serenity_prelude as serenity;
use rustls::ServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

const WEB_SETTINGS_PATH: &str = "/web-settings";
const WEB_SETTINGS_SESSION_TTL_SECONDS: u64 = 600;
const MAX_GAME_PLAYERS: usize = 24;
const WEB_LEADERBOARD_METRICS: &[&str] = &[
    "rating", "wins", "streak", "winrate", "games", "mafia", "playtime",
];

struct WebRoleGuide {
    role: Role,
    team: &'static str,
    kind: &'static str,
    summary: &'static str,
    tips: &'static [&'static str],
    caution: &'static str,
}

const WEB_ROLE_GUIDES: &[WebRoleGuide] = &[
    WebRoleGuide {
        role: Role::Citizen,
        team: "시민팀",
        kind: "기본",
        summary: "특수 능력은 없지만 공개 정보, 발언, 투표 흐름을 모아 마피아 후보를 좁히는 기본 역할입니다. 시민은 죽지 않고 올바른 표를 모으는 것만으로도 게임을 크게 움직입니다.",
        tips: &[
            "확정 정보와 추측을 분리해서 메모하세요.",
            "직업 주장자가 여러 명이면 결과보다 시간순 모순을 먼저 보세요.",
            "스킵과 지목 중 어느 쪽이 시민팀 수 계산에 이득인지 확인하세요.",
            "사망자 역할 공개 여부에 따라 추론 강도를 조절하세요.",
        ],
        caution: "능력이 없다는 이유로 침묵하면 후반 표 계산에서 밀립니다.",
    },
    WebRoleGuide {
        role: Role::Police,
        team: "시민팀",
        kind: "수사",
        summary: "밤마다 한 명을 조사해 마피아 판정 여부를 확인합니다. 결과는 강력하지만 대부의 조사 회피, 보조직의 접선 상태 같은 예외가 있어 결과 해석이 중요합니다.",
        tips: &[
            "조사 대상, 결과, 일차를 함께 기록하세요.",
            "맞경이 있으면 서로의 대상 선정 이유와 공개 타이밍을 비교하세요.",
            "마녀 같은 일부 보조직은 접선 전후 판정 차이가 있을 수 있습니다.",
            "결과 공개 전 의사 생존 가능성과 본인 생존 위험을 계산하세요.",
        ],
        caution: "결과만 공개하고 이유를 설명하지 않으면 오히려 의심받기 쉽습니다.",
    },
    WebRoleGuide {
        role: Role::Doctor,
        team: "시민팀",
        kind: "방어",
        summary: "밤마다 한 명을 보호해 마피아 처치를 막을 수 있습니다. 공개 확직 보호와 마피아의 예측을 역이용하는 보호 사이에서 판단해야 합니다.",
        tips: &[
            "공개된 수사직, 핵심 발언자, 처형 구도상 중요한 사람을 우선 비교하세요.",
            "마피아가 뻔한 대상을 피할 가능성도 함께 고려하세요.",
            "치료 성공이 나오면 공격 대상과 마피아 의도를 같이 추론하세요.",
            "간호사 접선 여부가 있으면 치료 흐름을 더 안정적으로 잡을 수 있습니다.",
        ],
        caution: "매일 같은 대상만 보호하면 마피아가 우회하기 쉬워집니다.",
    },
    WebRoleGuide {
        role: Role::Agent,
        team: "시민팀",
        kind: "수사",
        summary: "경찰 계열 수사직으로 밤 결과를 통해 마피아 후보를 좁힙니다. 결과 공개 타이밍과 다른 수사직 주장과의 정합성이 핵심입니다.",
        tips: &[
            "조사 결과를 낮 토론 흐름과 연결해 설명하세요.",
            "결과가 확정 정보인지 보조 정보인지 구분하세요.",
            "다른 수사직과 결과가 충돌하면 대상 선정 이유를 비교하세요.",
            "살아남는 것이 정보 누적에 중요하므로 공개 타이밍을 조절하세요.",
        ],
        caution: "너무 늦은 공개는 시민팀 판단을 늦추고 신뢰를 떨어뜨립니다.",
    },
    WebRoleGuide {
        role: Role::Vigilante,
        team: "시민팀",
        kind: "수사/처형",
        summary: "낮 조사와 밤 숙청으로 마피아팀을 직접 압박합니다. 처형 능력은 강하지만 오판하면 시민 수가 줄어들어 패배 조건에 가까워집니다.",
        tips: &[
            "조사와 처형은 별개 판단으로 다루세요.",
            "처형 전 생존자 수와 마피아 수 우위 조건을 계산하세요.",
            "수사직 결과, 투표 라인, 발언 모순을 모두 확인한 뒤 처형하세요.",
            "후반에는 마피아 수 우위 승리를 막는 용도로 가치가 큽니다.",
        ],
        caution: "확신 없는 숙청은 마피아 처치보다 시민 손실 위험이 큽니다.",
    },
    WebRoleGuide {
        role: Role::Inspector,
        team: "시민팀",
        kind: "경찰계열",
        summary: "밤에 한 명을 수사합니다. 대상이 형사와 같은 팀이면 밤이 끝날 때 형사는 대상의 직업을 알게 되고, 수사를 받은 대상에게는 형사의 정체가 전달됩니다. 다른 팀을 수사하면 직업을 알 수 없고 대상에게도 수사 사실이 전달되지 않습니다.",
        tips: &[
            "같은 팀 확인이 뜨면 직업 정보와 함께 신뢰 가능한 연결고리가 생깁니다.",
            "수사 대상에게 형사 정체가 알려지므로 공개 타이밍과 생존 위험을 함께 계산하세요.",
            "경찰, 요원, 자경단원과 같은 경찰계열이므로 한 판에 함께 배정되지 않습니다.",
            "접선 전 특수 마피아는 기존 조사 규칙처럼 마피아팀으로 확정 판정되지 않습니다.",
        ],
        caution: "같은 팀을 수사하면 대상에게 형사의 정체가 전달되므로 공개 타이밍을 계산해야 합니다.",
    },
    WebRoleGuide {
        role: Role::Detective,
        team: "시민팀",
        kind: "추적",
        summary: "밤에 대상을 추적해 행동 경로 단서를 얻습니다. 직접적인 마피아 판정은 아니지만 직업 주장과 실제 행동이 맞는지 검증하는 데 강합니다.",
        tips: &[
            "누가 누구에게 행동했는지 날짜별로 누적하세요.",
            "밤 행동이 있는 직업 주장자 위주로 추적 가치가 높습니다.",
            "한 번의 결과보다 여러 밤의 이동 패턴을 비교하세요.",
            "경찰 결과와 결합하면 거짓 직업 주장을 좁히기 쉽습니다.",
        ],
        caution: "행동 경로는 팀 판정이 아니므로 단독 처형 근거로 과신하지 마세요.",
    },
    WebRoleGuide {
        role: Role::Reporter,
        team: "시민팀",
        kind: "공개 정보",
        summary: "대상을 취재해 공개 정보로 만들 수 있습니다. 취재는 판 전체가 보는 정보라 대상과 타이밍 선택이 매우 중요합니다.",
        tips: &[
            "이미 확정된 대상보다 판을 가르는 애매한 대상이 보통 더 좋습니다.",
            "취재 공개 후 투표 흐름이 어떻게 바뀔지 예상하세요.",
            "마피아가 취재 전 제거할 수 있는 대상이면 빠르게 사용하세요.",
            "취재 결과는 다른 수사 결과와 함께 정리하세요.",
        ],
        caution: "낮은 가치 대상 취재는 강한 능력을 단순 확인에 낭비합니다.",
    },
    WebRoleGuide {
        role: Role::Hacker,
        team: "시민팀",
        kind: "정보",
        summary: "상대 행동 정보를 얻어 다음 낮 토론의 근거를 만듭니다. 누가 어떤 행동을 했는지와 발언이 맞는지 비교할 때 강합니다.",
        tips: &[
            "행동 정보와 발언 모순을 같이 기록하세요.",
            "수사직 주장자 검증에 활용하세요.",
            "결과 하나로 확정하지 말고 투표 흐름과 결합하세요.",
            "다음 낮 지목 근거로 짧게 정리해두세요.",
        ],
        caution: "행동 정보는 맥락 없이 공개하면 오해를 만들 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Shaman,
        team: "시민팀",
        kind: "사망자 정보",
        summary: "사망자와 관련된 정보를 활용해 산 사람의 주장과 죽은 사람의 발언을 연결합니다. 사망자가 늘수록 정보량이 커집니다.",
        tips: &[
            "죽은 사람의 생전 투표와 발언을 복원하세요.",
            "사망자 채팅 정보와 공개 정보를 구분하세요.",
            "죽은 수사직의 결과 가능성을 우선 확인하세요.",
            "후반에는 사망자 정보가 생존자 표 계산에 직접 영향을 줍니다.",
        ],
        caution: "사망자 정보만 믿고 현재 발언 모순을 놓치면 안 됩니다.",
    },
    WebRoleGuide {
        role: Role::Priest,
        team: "시민팀",
        kind: "부활/정화",
        summary: "죽은 대상을 되살리거나 위험한 상태를 정리하는 보조 역할입니다. 한 번의 선택으로 판세를 크게 바꿀 수 있습니다.",
        tips: &[
            "부활 대상의 직업 가치와 공개 정보량을 같이 보세요.",
            "죽은 수사직이나 확정 시민은 높은 우선순위를 가집니다.",
            "부활 후 즉시 공개될 정보가 무엇인지 예상하세요.",
            "교주팀 관련 위협이 있으면 정화 가치도 고려하세요.",
        ],
        caution: "정보가 적은 대상 부활은 오히려 혼선을 만들 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Soldier,
        team: "시민팀",
        kind: "방어",
        summary: "마피아 공격을 한 번 버틸 수 있는 시민팀 방어 역할입니다. 방탄 발동은 강한 생존 정보가 되며 마피아의 공격 의도도 추론할 수 있습니다.",
        tips: &[
            "방탄 발동 사실을 언제 공개할지 판단하세요.",
            "왜 본인이 공격받았는지 마피아 시각으로 생각하세요.",
            "거짓 군인 주장과 충돌하면 발동 타이밍을 근거로 비교하세요.",
            "후반에는 살아남는 것 자체가 시민 수 방어입니다.",
        ],
        caution: "너무 빨리 정체를 공개하면 이후 방어 가치가 줄어듭니다.",
    },
    WebRoleGuide {
        role: Role::Gangster,
        team: "시민팀",
        kind: "투표 견제",
        summary: "밤에 한 명을 공갈해 다음 낮 투표권을 막습니다. 투표권 하나가 승패를 바꾸는 후반에 특히 강합니다.",
        tips: &[
            "막을 표가 실제 결과를 바꾸는지 계산하세요.",
            "정치인, 확정 마피아 후보, 라인 핵심 인물을 우선 보세요.",
            "공갈 후 투표 결과가 어떻게 달라졌는지 기록하세요.",
            "마피아 수 우위 조건 직전에는 방어적 사용도 중요합니다.",
        ],
        caution: "시민팀 핵심 표를 막으면 오히려 처형 실패를 만들 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Prophet,
        team: "시민팀",
        kind: "예측",
        summary: "예언 정보를 통해 장기적인 판세 판단에 도움을 주는 시민팀 역할입니다. 즉시 판정형보다 누적 추론과 공개 타이밍이 중요합니다.",
        tips: &[
            "예언 정보가 실제 투표에 어떤 영향을 주는지 정리하세요.",
            "확정 정보와 가능성 정보를 구분해서 말하세요.",
            "후반 생존자 수 계산과 함께 쓰면 가치가 커집니다.",
            "마피아가 정보 공개 전에 제거할 가능성을 고려하세요.",
        ],
        caution: "예언을 절대 판정처럼 말하면 시민팀 판단이 굳어질 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Psychologist,
        team: "시민팀",
        kind: "관찰",
        summary: "낮에 두 명의 관계나 태도를 관찰해 라인 단서를 얻습니다. 직접 판정은 아니지만 반복 관찰로 발언 변화와 투표 라인을 잡아낼 수 있습니다.",
        tips: &[
            "서로를 감싸거나 몰아가는 관계를 우선 관찰하세요.",
            "투표 전후 태도 변화를 기록하세요.",
            "같은 대상군을 반복 비교하면 모순이 잘 보입니다.",
            "결과를 다른 수사 결과와 연결해 해석하세요.",
        ],
        caution: "관찰 결과를 확정 마피아 판정처럼 쓰면 위험합니다.",
    },
    WebRoleGuide {
        role: Role::Hypnotist,
        team: "시민팀",
        kind: "누적 정보",
        summary: "밤에 최면 대상을 누적하고 낮에 한 번에 깨워 팀 또는 직업 정보를 확인합니다. 깨우면 다음 밤에는 최면을 쓸 수 없어 정보 공개 타이밍이 핵심입니다.",
        tips: &[
            "최면 대상과 날짜를 반드시 기록하세요.",
            "여러 명을 모아 한 번에 깨우면 팀 구도 재계산이 쉽습니다.",
            "낮에 깨운 다음 밤은 행동 불가라는 점을 고려하세요.",
            "마피아팀과 교주팀 정보는 즉시 투표 흐름에 연결하세요.",
        ],
        caution: "너무 일찍 깨우면 정보량이 적고, 너무 늦으면 죽을 위험이 있습니다.",
    },
    WebRoleGuide {
        role: Role::Mercenary,
        team: "시민팀",
        kind: "의뢰/처형",
        summary: "게임 시작 후 정체를 알 수 없는 시민팀 플레이어 한 명에게 의뢰를 받습니다. 의뢰인은 용병의 정체를 알지만, 용병은 의뢰인이 누구인지 알 수 없습니다. 의뢰인이 밤에 살해되면 별도 처형 능력을 얻습니다. 용병 처형은 마피아 처치나 자경단 처형과 다른 독립 능력입니다.",
        tips: &[
            "의뢰 수신 메시지에는 의뢰인의 이름이 표시되지 않습니다.",
            "의뢰인이 밤에 사망하면 오는 능력 해금 메시지로 무장 상태를 확인하세요.",
            "무장 후 처형은 마피아 수 우위 승리 조건을 막을 수 있습니다.",
            "처형 대상은 수사 결과와 투표 라인을 같이 보고 고르세요.",
        ],
        caution: "용병은 의뢰인을 특정하거나 직접 보호할 수 없습니다. 능력 해금 전에는 별도 처형 능력이 없습니다.",
    },
    WebRoleGuide {
        role: Role::Lover,
        team: "시민팀",
        kind: "특수 관계",
        summary: "서로를 알고 밤 대화로 정보를 맞출 수 있는 관계형 역할입니다. 둘 중 한 명의 신뢰가 다른 한 명에게 영향을 주므로 함께 움직이는 운영이 중요합니다.",
        tips: &[
            "밤 대화로 서로의 정보와 의심 대상을 맞추세요.",
            "한쪽 공개가 다른 한쪽 신뢰에 주는 영향을 계산하세요.",
            "둘 다 살아있을 때 정보 가치가 가장 큽니다.",
            "동시에 의심받지 않게 발언 일관성을 유지하세요.",
        ],
        caution: "한 명이 무너지면 둘 다 라인으로 묶여 의심받을 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Mafia,
        team: "마피아팀",
        kind: "처치",
        summary: "밤마다 처치 대상을 선택하는 마피아팀 중심 역할입니다. 낮에는 시민팀처럼 보이며 의심을 분산하고, 밤에는 팀 선택 현황을 맞춰 핵심 시민을 제거해야 합니다.",
        tips: &[
            "마피아 비밀방의 처치 선택 현황을 계속 확인하세요.",
            "수사직, 의사, 확정 시민 순서로 위협도를 계산하세요.",
            "낮 발언은 시민 관점으로 일관되게 유지하세요.",
            "팀원이 몰릴 때 표 분산과 라인 절단을 준비하세요.",
        ],
        caution: "밤 선택이 갈리면 처치가 약해지고 팀원 동선도 노출됩니다.",
    },
    WebRoleGuide {
        role: Role::Spy,
        team: "마피아팀",
        kind: "첩보/접선",
        summary: "첩보를 사용해 정보를 얻고 마피아와 접선하는 보조 역할입니다. 접선 전에는 시민처럼 움직이며 정보 손실을 줄이고, 접선 후에는 마피아팀 정보망에 합류합니다.",
        tips: &[
            "마피아 접선 전까지는 의심을 낮게 유지하세요.",
            "첩보 대상은 수사직 후보나 핵심 발언자가 좋습니다.",
            "접선 후 추가 첩보가 가능하면 즉시 가치 높은 대상을 고르세요.",
            "얻은 정보는 마피아 처치 우선순위와 연결하세요.",
        ],
        caution: "접선 전 무리한 발언은 마피아팀 보조직으로 찍히기 쉽습니다.",
    },
    WebRoleGuide {
        role: Role::Contractor,
        team: "마피아팀",
        kind: "추측/암살",
        summary: "두 대상과 각각의 직업을 추측해 청부를 시도합니다. 정확히 맞히면 큰 이득을 얻지만 실패하면 행동 가치를 잃습니다.",
        tips: &[
            "공개 정보가 충분한 대상끼리 묶어 제출하세요.",
            "직업 주장과 실제 행동 가능성을 대조하세요.",
            "수사직과 공개 직업 대상 제한을 확인하세요.",
            "성공 시 접선과 암살 가치까지 함께 계산하세요.",
        ],
        caution: "확률 낮은 청부는 마피아팀의 밤 템포를 낭비합니다.",
    },
    WebRoleGuide {
        role: Role::Thief,
        team: "마피아팀",
        kind: "도벽",
        summary: "지목 투표에서 자신이 투표한 대상의 직업을 훔쳐 다음 밤까지 그 능력을 사용할 수 있습니다. 별도 도벽 선택은 없으며, 수사직을 훔치면 기존 수사직과 독립된 결과를 얻습니다.",
        tips: &[
            "도벽 대상은 지목 투표 대상과 항상 같습니다.",
            "경찰 계열을 훔치면 기존 경찰과 별도 조사로 관리하세요.",
            "마피아 직업을 훔치면 접선 흐름을 확인하세요.",
            "대상 직업 가치와 본인 생존 가능성을 같이 계산하세요.",
        ],
        caution: "능력은 강하지만 선택을 잘못하면 마피아팀 보조 역할만 노출됩니다.",
    },
    WebRoleGuide {
        role: Role::Witch,
        team: "마피아팀",
        kind: "저주",
        summary: "밤에 대상을 개구리로 저주해 능력 사용과 낮 발언 방식을 제한합니다. 중요한 수사직이나 투표 영향력이 큰 사람을 흔드는 데 좋습니다.",
        tips: &[
            "저주 대상의 능력 가치와 다음 낮 영향력을 보세요.",
            "개구리 채팅 제한이 토론에 줄 혼선을 계산하세요.",
            "마피아 접선 여부에 따라 경찰 판정 해석이 달라질 수 있습니다.",
            "수사직 저주로 정보 공개 흐름을 끊을 수 있습니다.",
        ],
        caution: "무작정 저주하면 마피아 처치 우선순위와 충돌할 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Scientist,
        team: "마피아팀",
        kind: "소생",
        summary: "사망 이후 소생 가능성을 가진 마피아팀 역할입니다. 죽음이 곧 끝이 아니므로 사망 전 발언과 소생 후 행동을 모두 전략으로 써야 합니다.",
        tips: &[
            "사망 전 발언이 소생 후 의심에 미칠 영향을 생각하세요.",
            "소생 타이밍 뒤 마피아 수 계산을 다시 하세요.",
            "죽은 상태에서도 공개 정보가 어떻게 쌓이는지 보세요.",
            "소생 후 바로 표적이 될 수 있어 후속 발언을 준비하세요.",
        ],
        caution: "소생만 믿고 초반에 쉽게 노출되면 팀 전체가 흔들립니다.",
    },
    WebRoleGuide {
        role: Role::Madam,
        team: "마피아팀",
        kind: "유혹/투표",
        summary: "별도 유혹 행동 없이 지목 투표에서 마담이 선택한 대상이 유혹됩니다. 유혹된 대상은 능력과 발언이 제한되며, 핵심 시민 직업이나 중요한 투표권을 묶어 낮 구도를 흔들 수 있습니다.",
        tips: &[
            "마담의 일반 지목 투표 대상이 곧 유혹 대상입니다.",
            "수사직, 의사, 정치인처럼 낮 영향력이 큰 대상을 보세요.",
            "유혹 지속 기간과 다음 투표 구도를 같이 계산하세요.",
            "접선 후 마피아 비밀방 정보를 적극 공유하세요.",
        ],
        caution: "유혹만 따로 고를 수 없으므로 처형 지목표와 유혹 대상을 항상 같이 계산해야 합니다.",
    },
    WebRoleGuide {
        role: Role::Graverobber,
        team: "마피아팀",
        kind: "도굴",
        summary: "사망자의 직업을 이어받아 판세를 바꿀 수 있는 역할입니다. 어떤 직업을 도굴했는지에 따라 팀 기여 방식이 크게 달라집니다.",
        tips: &[
            "첫 사망자의 직업 가치와 팀을 확인하세요.",
            "도굴 후 자신의 승리 조건과 팀 판정을 다시 계산하세요.",
            "얻은 직업의 행동 가능 시점을 확인하세요.",
            "도굴 사실이 공개될 때 의심 흐름을 대비하세요.",
        ],
        caution: "마피아팀 직업 도굴 가능성이 있어 시민팀 판정만 믿으면 안 됩니다.",
    },
    WebRoleGuide {
        role: Role::Godfather,
        team: "마피아팀",
        kind: "조사 회피",
        summary: "조사 회피와 접선 흐름을 활용하는 마피아팀 특수 역할입니다. 경찰에게 바로 잡히지 않는 장점을 이용해 과감한 라인을 만들 수 있습니다.",
        tips: &[
            "조사 회피를 믿되 행동 모순은 숨길 수 없다는 점을 기억하세요.",
            "자동 접선 시점 이후 마피아팀과 적극적으로 맞추세요.",
            "수사직이 자신을 의심할 때 결과 외 근거를 차단하세요.",
            "후반 마피아 수 우위 조건을 계속 계산하세요.",
        ],
        caution: "조사 회피가 모든 정보 역할을 막는 것은 아닙니다.",
    },
    WebRoleGuide {
        role: Role::Villain,
        team: "마피아팀",
        kind: "보조",
        summary: "마피아팀 승리를 목표로 움직이는 보조 성향 역할입니다. 접선 전에는 시민처럼 정보를 정리하며 마피아팀과 연결될 기회를 봅니다.",
        tips: &[
            "마피아팀 승리 조건 기준으로 표를 움직이세요.",
            "접선 전에는 과한 마피아 편 발언을 피하세요.",
            "정체 노출 타이밍을 조절하세요.",
            "마피아와 연결될 밤 행동 기회를 확인하세요.",
        ],
        caution: "초반 노출은 시민팀 집중 견제를 부릅니다.",
    },
    WebRoleGuide {
        role: Role::CultLeader,
        team: "교주팀",
        kind: "포교",
        summary: "밤마다 포교로 세력을 늘리고 독자 승리 조건을 노리는 역할입니다. 시민팀과 마피아팀 싸움 사이에서 생존하며 숫자 우위를 만들어야 합니다.",
        tips: &[
            "포교 성공 후 교주팀 수와 비교주팀 수를 매일 계산하세요.",
            "마피아와 시민이 서로 싸우게 두는 흐름이 좋습니다.",
            "포교 대상은 생존력과 발언 영향력을 함께 보세요.",
            "승리 조건이 가까워지면 투표를 과감하게 조정하세요.",
        ],
        caution: "교주가 죽으면 교주팀 전체 계획이 크게 약해집니다.",
    },
    WebRoleGuide {
        role: Role::Fanatic,
        team: "교주팀",
        kind: "보조",
        summary: "교주팀 보조 역할로 교주 생존과 포교 정보 보존이 중요합니다. 교주팀 숫자 계산을 도와 승리 타이밍을 잡습니다.",
        tips: &[
            "교주 생존 여부를 최우선으로 보세요.",
            "포교 정보가 새어나가지 않게 관리하세요.",
            "교주팀 숫자 우위 가능성을 계산하세요.",
            "교주 노출 시 대체 표 흐름을 준비하세요.",
        ],
        caution: "교주팀은 숫자 조건을 놓치면 이길 타이밍을 잃습니다.",
    },
    WebRoleGuide {
        role: Role::Joker,
        team: "중립",
        kind: "단독 승리",
        summary: "낮 투표로 처형되면 단독 승리를 노립니다. 너무 노골적이면 견제당하고 너무 조용하면 처형 후보가 되기 어렵습니다.",
        tips: &[
            "의심받되 확정 마피아처럼 보이지 않게 조절하세요.",
            "후반 과반 계산과 투표 피로도를 이용하세요.",
            "찬반투표에서 처형 가능성이 높은 흐름을 유도하세요.",
            "마피아와 시민 어느 쪽에도 완전히 붙지 않는 태도가 좋습니다.",
        ],
        caution: "정체가 들키면 모두가 처형을 피하려 하므로 승리가 어려워집니다.",
    },
    WebRoleGuide {
        role: Role::Politician,
        team: "시민팀",
        kind: "투표 강화",
        summary: "투표에서 2표 영향력을 가지는 시민팀 역할입니다. 최종 투표 구도에서 한 명 이상의 힘을 내므로 표 계산의 중심이 됩니다.",
        tips: &[
            "자신의 2표가 결과를 바꾸는지 매번 계산하세요.",
            "스킵, 지목, 찬반 동률 가능성을 확인하세요.",
            "막판 표 이동을 주도할 수 있습니다.",
            "공갈 대상이 되면 영향력이 사라지므로 건달 가능성을 보세요.",
        ],
        caution: "잘못된 2표는 일반 시민의 오표보다 훨씬 크게 작용합니다.",
    },
    WebRoleGuide {
        role: Role::Judge,
        team: "시민팀",
        kind: "찬반 개입",
        summary: "찬반투표 동률이나 중요한 처형 판단에서 판세를 뒤집을 수 있습니다. 공개 전에는 일반 시민처럼 보이지만 결정 순간 영향력이 큽니다.",
        tips: &[
            "찬반 수와 처형 기준을 계속 확인하세요.",
            "공개 전후 영향력 차이를 계산하세요.",
            "처형 대상의 팀 가치를 따져 선택하세요.",
            "막판 뒤집기 가능성을 숨겨두는 것도 전략입니다.",
        ],
        caution: "감정적인 뒤집기는 시민팀 전체 신뢰를 무너뜨립니다.",
    },
    WebRoleGuide {
        role: Role::Terrorist,
        team: "시민팀",
        kind: "교환",
        summary: "밤에는 한 명을 지목하며, 그날 밤 테러리스트가 사망하면 지목한 다른 팀 대상도 함께 사망합니다. 낮 지목 투표에서 최후의 반론 대상이 되면 비밀 메시지로 습격 대상을 새로 선택합니다. 이후 찬반투표로 처형될 때 선택한 대상이 마피아 또는 접선을 완료한 마피아 보조직업이면 함께 사망합니다. 밤 지목과 투표 처형용 습격 선택은 서로 별개입니다.",
        tips: &[
            "최후의 반론 시간에 도착한 비밀 메시지에서 습격 대상을 반드시 선택하세요.",
            "투표 처형 습격은 확정 마피아나 접선 사실이 드러난 보조 마피아를 우선 지목하세요.",
            "밤 지목은 마피아팀뿐 아니라 현재 테러리스트와 다른 팀인 대상에게도 발동할 수 있습니다.",
            "습격 성공 후 바뀌는 생존자 수와 각 진영의 수적 우위 조건까지 계산하세요.",
        ],
        caution: "투표로 처형될 때 시민팀, 교주팀, 미접선 보조 마피아를 골랐다면 습격은 실패합니다. 최후의 반론에서 선택하지 않아도 아무도 함께 죽지 않습니다.",
    },
    WebRoleGuide {
        role: Role::Nurse,
        team: "시민팀",
        kind: "의사 보조",
        summary: "의사를 보조하고 의사와의 접선 정보를 활용합니다. 의사 위치를 파악하면 치료 흐름을 안정시키는 데 도움이 됩니다.",
        tips: &[
            "의사 접선 여부를 확인하세요.",
            "의사 주장자가 여러 명이면 접선과 치료 결과를 비교하세요.",
            "의사 생존 추정에 도움 되는 정보를 정리하세요.",
            "치료 관련 공개 정보와 모순을 점검하세요.",
        ],
        caution: "의사 위치를 성급하게 공개하면 마피아의 처치 목표가 됩니다.",
    },
    WebRoleGuide {
        role: Role::Frog,
        team: "상태",
        kind: "저주 상태",
        summary: "마녀 저주로 밤 능력을 사용할 수 없고 낮 발언 방식이 제한된 상태입니다. 짧은 표현으로 핵심 정보를 전달해야 합니다.",
        tips: &[
            "핵심 의심 대상과 이유를 최대한 짧게 남기세요.",
            "능력 사용 불가 상태임을 고려해 결과 부재를 설명하세요.",
            "누가 저주했을지 마녀 후보를 추론하세요.",
            "해제 후 이전 발언을 보강하세요.",
        ],
        caution: "제한된 말 때문에 오해받기 쉬우니 핵심만 반복하세요.",
    },
];

#[derive(Debug, Clone)]
pub struct WebSettingsSession {
    pub guild_id: u64,
    pub user_id: u64,
    pub user_label: String,
    pub expires_at: Instant,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeyStore {
    #[serde(default)]
    keys: Vec<ApiKeyRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiKeyRecord {
    id: String,
    label: String,
    guild_id: u64,
    created_by_user_id: u64,
    created_at: String,
    key_hash: String,
    #[serde(default)]
    revoked: bool,
}

#[derive(Clone)]
pub struct WebSettingsState {
    pub config: Arc<RwLock<BotConfig>>,
    pub config_path: Arc<PathBuf>,
    pub api_keys: Arc<RwLock<ApiKeyStore>>,
    pub api_keys_path: Arc<PathBuf>,
    pub stats: Arc<RwLock<StatsFile>>,
    pub games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
    pub completed_replays: Arc<RwLock<VecDeque<Value>>>,
    pub recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>>,
    pub sessions: Arc<DashMap<String, WebSettingsSession>>,
    pub started_at: Instant,
    pub bot_name: String,
    pub guild_count: usize,
    pub base_url: String,
}

pub fn load_api_key_store(path: impl AsRef<Path>) -> Result<ApiKeyStore> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(ApiKeyStore::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("API 키 파일을 읽지 못했습니다: {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("API 키 JSON을 파싱하지 못했습니다: {}", path.display()))
}

fn save_api_key_store(path: impl AsRef<Path>, store: &ApiKeyStore) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("API 키 디렉터리를 만들지 못했습니다: {}", parent.display())
        })?;
    }
    let text = serde_json::to_string_pretty(store).context("API 키 JSON 직렬화 실패")?;
    let temp_path = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("api_keys.json")
    ));
    fs::write(&temp_path, format!("{text}\n")).with_context(|| {
        format!(
            "API 키 임시 파일을 쓰지 못했습니다: {}",
            temp_path.display()
        )
    })?;
    if path.exists() {
        fs::remove_file(path).with_context(|| {
            format!("기존 API 키 파일을 교체하지 못했습니다: {}", path.display())
        })?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("API 키 파일을 교체하지 못했습니다: {}", path.display()))?;
    Ok(())
}

pub fn load_completed_replays(path: impl AsRef<Path>) -> Result<VecDeque<Value>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(VecDeque::new());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("replays JSON file read failed: {}", path.display()))?;
    let values = serde_json::from_str::<Vec<Value>>(&text)
        .with_context(|| format!("replays JSON parse failed: {}", path.display()))?;
    Ok(values.into())
}

pub fn save_completed_replays(path: impl AsRef<Path>, replays: &VecDeque<Value>) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("replays directory create failed: {}", parent.display()))?;
    }
    let values = replays.iter().cloned().collect::<Vec<_>>();
    let text = serde_json::to_string_pretty(&values).context("replays JSON serialize failed")?;
    let temp_path = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("replays.json")
    ));
    fs::write(&temp_path, format!("{text}\n"))
        .with_context(|| format!("replays temp write failed: {}", temp_path.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("replays old file replace failed: {}", path.display()))?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("replays file replace failed: {}", path.display()))?;
    Ok(())
}

fn api_key_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn issue_api_key(store: &mut ApiKeyStore, guild_id: u64, user_id: u64, label: String) -> String {
    let key = format!("mfr_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    store.keys.push(ApiKeyRecord {
        id: Uuid::new_v4().simple().to_string(),
        label,
        guild_id,
        created_by_user_id: user_id,
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        key_hash: api_key_hash(&key),
        revoked: false,
    });
    key
}

#[derive(Debug, Clone, Copy)]
enum WebFieldKind {
    Bool,
    Int,
    Text,
    IntList,
}

#[derive(Debug, Clone, Copy)]
struct WebConfigField {
    name: &'static str,
    label: &'static str,
    kind: WebFieldKind,
    min_value: Option<u64>,
}

const WEB_CONFIG_FIELDS: &[WebConfigField] = &[
    field(
        "participant_role",
        "참가자 역할 이름",
        WebFieldKind::Text,
        None,
    ),
    field("manager_role", "관리자 역할 이름", WebFieldKind::Text, None),
    field("game_enabled", "게임 시작 활성화", WebFieldKind::Bool, None),
    field(
        "max_player_count",
        "모집 최대 인원 (0 = 제한 없음)",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "night_seconds",
        "밤 진행 시간(초)",
        WebFieldKind::Int,
        Some(1),
    ),
    field(
        "discussion_seconds",
        "낮 토론 시간(초)",
        WebFieldKind::Int,
        Some(1),
    ),
    field("vote_seconds", "투표 시간(초)", WebFieldKind::Int, Some(1)),
    field(
        "chat_slowmode_seconds",
        "낮 채팅 슬로우모드(초)",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "default_mafia_count",
        "기본 마피아 수",
        WebFieldKind::Int,
        Some(1),
    ),
    field(
        "default_doctor_count",
        "기본 의사 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "default_police_count",
        "기본 경찰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "default_joker_count",
        "기본 조커 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "citizen_special_count",
        "시민 특수룰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "mafia_special_count",
        "마피아 특수룰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "neutral_special_count",
        "중립 특수룰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "reveal_death_roles",
        "사망 시 직업 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "reveal_public_police_status",
        "경찰 조사 결과 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "reveal_morning_mafia_count",
        "아침마다 생존 마피아 수 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "show_confirmation_vote_counts",
        "찬반투표 집계 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "anonymous_mode",
        "익명 채팅 모드 사용",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "anonymous_name_mode",
        "익명 이름 모드 (animal / number)",
        WebFieldKind::Text,
        None,
    ),
    field("use_agent", "요원 사용", WebFieldKind::Bool, None),
    field("use_vigilante", "자경단원 사용", WebFieldKind::Bool, None),
    field(
        "enable_detective",
        "사립탐정 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_inspector", "형사 활성화", WebFieldKind::Bool, None),
    field(
        "enable_graverobber",
        "도굴꾼 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_spy", "스파이 활성화", WebFieldKind::Bool, None),
    field(
        "enable_contractor",
        "청부업자 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_witch", "마녀 활성화", WebFieldKind::Bool, None),
    field(
        "enable_scientist",
        "과학자 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_madam", "마담 활성화", WebFieldKind::Bool, None),
    field("enable_godfather", "대부 활성화", WebFieldKind::Bool, None),
    field("enable_joker", "조커 활성화", WebFieldKind::Bool, None),
    field(
        "enable_politician",
        "정치인 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_judge", "판사 활성화", WebFieldKind::Bool, None),
    field("enable_reporter", "기자 활성화", WebFieldKind::Bool, None),
    field("enable_hacker", "해커 활성화", WebFieldKind::Bool, None),
    field(
        "enable_terrorist",
        "테러리스트 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_lover", "연인 활성화", WebFieldKind::Bool, None),
    field("enable_shaman", "영매 활성화", WebFieldKind::Bool, None),
    field("enable_priest", "성직자 활성화", WebFieldKind::Bool, None),
    field("enable_soldier", "군인 활성화", WebFieldKind::Bool, None),
    field("enable_nurse", "간호사 활성화", WebFieldKind::Bool, None),
    field("enable_gangster", "건달 활성화", WebFieldKind::Bool, None),
    field("enable_prophet", "예언자 활성화", WebFieldKind::Bool, None),
    field(
        "enable_psychologist",
        "심리학자 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "enable_hypnotist",
        "최면술사 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_mercenary", "용병 활성화", WebFieldKind::Bool, None),
    field("enable_thief", "도둑 활성화", WebFieldKind::Bool, None),
    field(
        "enable_cult_team",
        "교주/광신도 팀 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "blacklist_user_ids",
        "블랙리스트 유저 ID 목록",
        WebFieldKind::IntList,
        None,
    ),
];

const fn field(
    name: &'static str,
    label: &'static str,
    kind: WebFieldKind,
    min_value: Option<u64>,
) -> WebConfigField {
    WebConfigField {
        name,
        label,
        kind,
        min_value,
    }
}

const WEB_PAGE_STYLE: &str = r#"
<style>
  :root { color-scheme: light; --bg: #f4f6f8; --surface: #ffffff; --surface-strong: #f8fafc; --line: #dbe2e8; --text: #1f2933; --muted: #667085; --accent: #2563eb; --accent-strong: #1d4ed8; --warm: #a16207; --danger: #c2413b; }
  * { box-sizing: border-box; }
  html { min-width: 320px; background: var(--bg); }
  body { min-width: 320px; margin: 0; padding: 28px 20px 48px; background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Apple SD Gothic Neo", sans-serif; font-size: 15px; line-height: 1.55; }
  .site-shell { width: min(1120px, 100%); margin: 0 auto; }
  .site-header { display: flex; align-items: center; gap: 12px; padding: 0 0 18px; border-bottom: 1px solid var(--line); }
  .site-mark { display: grid; place-items: center; width: 34px; height: 34px; flex: 0 0 34px; border: 1px solid #bfdbfe; border-radius: 6px; background: #eff6ff; color: var(--accent-strong); text-decoration: none; font-weight: 800; letter-spacing: 0; }
  .eyebrow { margin: 0 0 2px; color: var(--muted); font-size: 0.72rem; font-weight: 700; letter-spacing: 0.06em; }
  h1, h2, h3 { color: var(--text); letter-spacing: 0; }
  h1 { margin: 0; font-size: 1.5rem; line-height: 1.2; }
  h2 { margin: 0 0 12px; font-size: 1.05rem; line-height: 1.3; }
  h3 { margin: 0 0 8px; font-size: 0.95rem; }
  a { color: var(--accent-strong); text-underline-offset: 3px; }
  a:hover { color: #1e40af; }
  main { min-width: 0; }
  .meta { margin: 0 0 20px; color: var(--muted); font-size: 0.92rem; }
  .nav { display: flex; flex-wrap: wrap; gap: 4px; margin: 14px 0 20px; padding: 5px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); box-shadow: 0 1px 2px rgb(31 41 51 / 0.04); }
  .nav a { padding: 7px 10px; border: 1px solid transparent; color: var(--muted); text-decoration: none; }
  .nav a:hover { border-color: #dbeafe; background: #eff6ff; color: var(--accent-strong); }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 190px), 1fr)); gap: 10px; margin: 16px 0; }
  .split { display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(0, 0.9fr); gap: 14px; }
  .card, .podium-card { min-width: 0; border: 1px solid var(--line); border-radius: 6px; padding: 14px; background: var(--surface); box-shadow: 0 1px 2px rgb(31 41 51 / 0.04); }
  .card span, .podium-card .rank { color: var(--muted); font-size: 0.82rem; }
  .card strong { display: block; margin-top: 5px; color: var(--text); font-size: 1.45rem; line-height: 1.1; overflow-wrap: anywhere; }
  .panel { min-width: 0; overflow-x: auto; border: 1px solid var(--line); border-radius: 6px; padding: 16px; margin: 14px 0; background: var(--surface); box-shadow: 0 1px 2px rgb(31 41 51 / 0.04); }
  .panel > :last-child { margin-bottom: 0; }
  .pill { display: inline-block; padding: 2px 8px; border: 1px solid var(--line); border-radius: 999px; color: var(--muted); font-size: 0.82rem; }
  .metric-tabs { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0 18px; }
  .metric-tabs a { padding: 6px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--surface); color: var(--muted); text-decoration: none; }
  .metric-tabs a:hover, .metric-tabs a.active { border-color: #bfdbfe; background: #eff6ff; color: var(--accent-strong); }
  .podium { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 190px), 1fr)); gap: 10px; margin-bottom: 16px; }
  .podium-card .name { margin: 7px 0; font-size: 1.05rem; font-weight: 800; overflow-wrap: anywhere; }
  .podium-card .rating { color: #854d0e; font-size: 1.35rem; font-weight: 800; }
  .endpoint { display: grid; grid-template-columns: minmax(0, 0.85fr) minmax(0, 1.15fr); gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--line); }
  .endpoint:last-child { border-bottom: 0; padding-bottom: 0; }
  .role-section h2 { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .role-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 270px), 1fr)); gap: 12px; }
  .role-card { min-width: 0; border: 1px solid var(--line); border-radius: 6px; padding: 14px; background: var(--surface-strong); }
  .role-card h3 { margin: 0; font-size: 1.06rem; }
  .role-card h4 { margin: 12px 0 6px; font-size: 0.85rem; color: var(--muted); }
  .role-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; margin-bottom: 9px; }
  .role-title { position: relative; display: flex; align-items: center; min-width: 0; gap: 6px; }
  .role-help { position: relative; display: inline-flex; flex: 0 0 auto; align-items: center; justify-content: center; width: 22px; height: 22px; border: 1px solid #bfdbfe; border-radius: 999px; background: #eff6ff; color: var(--accent-strong); font-size: 0.78rem; font-weight: 800; line-height: 1; cursor: help; }
  .role-help::after { content: attr(data-tip); position: absolute; z-index: 20; top: calc(100% + 8px); left: 0; width: min(340px, calc(100vw - 32px)); padding: 10px 11px; border: 1px solid #cbd5e1; border-radius: 6px; background: #fff; color: var(--text); box-shadow: 0 14px 32px rgb(15 23 42 / 0.16); font-size: 0.84rem; font-weight: 500; line-height: 1.55; text-align: left; white-space: normal; opacity: 0; pointer-events: none; transform: translateY(-4px); transition: opacity 140ms ease, transform 140ms ease; }
  .role-help:hover::after, .role-help:focus-visible::after { opacity: 1; transform: translateY(0); }
  .role-tags { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 5px; }
  .role-summary { margin: 0 0 10px; color: #344054; }
  .role-rating { margin: 0 0 10px; padding: 8px 10px; border: 1px solid #dbeafe; border-radius: 4px; background: #f8fbff; color: #1e3a8a; font-size: 0.88rem; line-height: 1.45; }
  .role-rating strong { color: #1d4ed8; }
  .role-card ul { margin: 0; padding-left: 18px; }
  .role-card li { margin: 4px 0; }
  .role-note { margin: 11px 0 0; padding: 9px 10px; border-left: 3px solid #f59e0b; border-radius: 4px; background: #fffbeb; color: #713f12; }
  code { display: inline; max-width: 100%; padding: 2px 5px; border: 1px solid #d9e2ec; border-radius: 4px; background: #f6f8fa; color: #334e68; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.88em; overflow-wrap: anywhere; word-break: break-word; }
  pre { max-width: 100%; margin: 10px 0 0; padding: 12px; overflow-x: auto; border: 1px solid #d9e2ec; border-radius: 4px; background: #f8fafc; color: #334155; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.82rem; line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word; }
  table { width: 100%; min-width: 560px; border-collapse: collapse; }
  th, td { padding: 9px 8px; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; overflow-wrap: anywhere; }
  th { color: var(--muted); font-size: 0.78rem; font-weight: 700; letter-spacing: 0.04em; }
  td.num, th.num { text-align: right; }
  fieldset { min-width: 0; margin: 0 0 16px; padding: 4px 16px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); }
  legend { padding: 0 6px; color: var(--text); font-weight: 700; }
  .row { display: flex; align-items: center; justify-content: space-between; min-width: 0; gap: 16px; padding: 10px 0; border-bottom: 1px solid #edf0f2; }
  .row:last-child { border-bottom: none; }
  .row span { min-width: 0; flex: 1 1 auto; overflow-wrap: anywhere; }
  input[type="text"], input[type="number"], textarea { width: min(400px, 100%); min-width: 0; padding: 8px 10px; border: 1px solid #cbd5df; border-radius: 4px; background: #fff; color: var(--text); font: inherit; font-size: 0.92rem; }
  input[type="text"]:focus, input[type="number"]:focus, textarea:focus { outline: 2px solid #bfdbfe; outline-offset: 1px; border-color: var(--accent); }
  textarea { min-height: 88px; resize: vertical; }
  input[type="checkbox"] { width: 18px; height: 18px; accent-color: var(--accent); }
  button { margin-top: 14px; padding: 9px 14px; border: 1px solid var(--accent-strong); border-radius: 4px; background: var(--accent-strong); color: #fff; font: inherit; font-weight: 700; cursor: pointer; transition: background 140ms ease, border-color 140ms ease; }
  button:hover { border-color: #1e40af; background: #1e40af; }
  button:focus-visible, a:focus-visible { outline: 2px solid #93c5fd; outline-offset: 2px; }
  .message { margin: 0 0 16px; padding: 11px 12px; border: 1px solid #fde68a; border-left: 3px solid var(--warm); border-radius: 4px; background: #fffbeb; color: #713f12; }
  .message.error { border-color: #fecaca; border-left-color: var(--danger); background: #fef2f2; color: #991b1b; }
  small { color: var(--muted); }
  @media (max-width: 760px) {
    body { padding: 18px 12px 32px; }
    .site-header { align-items: flex-start; }
    .nav { margin-bottom: 14px; }
    .split, .endpoint { grid-template-columns: minmax(0, 1fr); }
    .row { align-items: stretch; flex-direction: column; gap: 8px; }
    input[type="text"], input[type="number"], textarea { width: 100%; }
    table { font-size: 0.88rem; }
  }
</style>
"#;

pub fn settings_path() -> &'static str {
    WEB_SETTINGS_PATH
}

pub fn session_ttl_minutes() -> u64 {
    (WEB_SETTINGS_SESSION_TTL_SECONDS / 60).max(1)
}

pub fn base_url(host: &str, port: u16, use_https: bool) -> String {
    if let Ok(base_url) = std::env::var("WEB_SETTINGS_BASE_URL")
        && !base_url.trim().is_empty()
    {
        return base_url.trim_end_matches('/').to_string();
    }
    let display_host = if matches!(host, "0.0.0.0" | "::") {
        "localhost"
    } else {
        host
    };
    let scheme = if use_https { "https" } else { "http" };
    format!("{scheme}://{display_host}:{port}")
}

pub fn issue_session(
    sessions: &DashMap<String, WebSettingsSession>,
    guild_id: u64,
    user_id: u64,
    user_label: String,
) -> String {
    purge_expired_sessions(sessions);
    let mut bytes = [0u8; 32];
    system_random::fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    sessions.insert(
        token.clone(),
        WebSettingsSession {
            guild_id,
            user_id,
            user_label,
            expires_at: Instant::now() + Duration::from_secs(WEB_SETTINGS_SESSION_TTL_SECONDS),
        },
    );
    token
}

pub async fn run_server(
    state: WebSettingsState,
    host: String,
    port: u16,
    tls_cert: Option<String>,
    tls_key: Option<String>,
) -> Result<()> {
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        let tls_config = Arc::new(load_tls_config(&cert, &key)?);
        let acceptor = TlsAcceptor::from(tls_config);
        println!("Rust web settings server ready (HTTPS): https://{host}:{port}");
        loop {
            let (stream, _addr) = listener.accept().await?;
            let state = state.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(stream) => {
                        if let Err(error) = handle_connection(stream, state).await {
                            eprintln!("web settings error: {error:?}");
                        }
                    }
                    Err(error) => eprintln!("web settings tls error: {error:?}"),
                }
            });
        }
    }

    println!("Rust web settings server ready (HTTP): http://{host}:{port}");
    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, state).await {
                eprintln!("web settings error: {error:?}");
            }
        });
    }
}

fn load_tls_config(cert_path: &str, key_path: &str) -> Result<ServerConfig> {
    let mut cert_reader = BufReader::new(
        File::open(cert_path).with_context(|| format!("failed to open TLS cert: {cert_path}"))?,
    );
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read TLS cert: {cert_path}"))?;
    if certs.is_empty() {
        bail!("TLS cert file has no certificates: {cert_path}");
    }

    let mut key_reader = BufReader::new(
        File::open(key_path).with_context(|| format!("failed to open TLS key: {key_path}"))?,
    );
    let key = rustls_pemfile::private_key(&mut key_reader)
        .with_context(|| format!("failed to read TLS key: {key_path}"))?
        .with_context(|| format!("TLS key file has no private key: {key_path}"))?;

    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("failed to build web settings TLS config")
}

async fn handle_connection<S>(mut stream: S, state: WebSettingsState) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let response = match read_http_request(&mut stream).await {
        Ok(request) => route_request(&state, request).await,
        Err(error) => http_response(
            "400 Bad Request",
            &render_message_page("잘못된 요청", &error.to_string()),
        ),
    };
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

async fn route_request(state: &WebSettingsState, request: HttpRequest) -> String {
    let (path, query) = request.path.split_once('?').unwrap_or((&request.path, ""));
    if request.method == "OPTIONS" && path.starts_with("/api/") {
        return api_options_response();
    }
    if let Some(response) = route_protected_api_request(state, &request, path, query).await {
        return response;
    }
    if request.method == "GET"
        && let Some(response) = route_public_request(state, path, query).await
    {
        return response;
    }
    let Some(session_path) = path.strip_prefix(&format!("{WEB_SETTINGS_PATH}/")) else {
        return http_response(
            "404 Not Found",
            &render_message_page("404", "요청한 페이지를 찾을 수 없습니다."),
        );
    };
    let (token, subpath) = session_path.split_once('/').unwrap_or((session_path, ""));
    purge_expired_sessions(&state.sessions);
    let Some(session) = state.sessions.get(token).map(|entry| entry.clone()) else {
        return http_response("410 Gone", &expired_page());
    };
    let _session_scope = (session.guild_id, session.user_id);

    if subpath == "api-keys" {
        return route_api_key_management(state, &session, token, &request).await;
    }
    if !subpath.is_empty() {
        return http_response(
            "404 Not Found",
            &render_message_page("404", "요청한 페이지를 찾을 수 없습니다."),
        );
    }

    match request.method.as_str() {
        "GET" => {
            let config = state.config.read().await.clone();
            http_response(
                "200 OK",
                &render_settings_page(
                    &session,
                    &format!("{WEB_SETTINGS_PATH}/{token}"),
                    &config,
                    Some(&web_status_values(state).await),
                    None,
                ),
            )
        }
        "POST" => {
            let updates = match parse_form_updates(&request.body) {
                Ok(updates) => updates,
                Err(error) => {
                    let config = state.config.read().await.clone();
                    return http_response(
                        "400 Bad Request",
                        &render_settings_page(
                            &session,
                            &format!("{WEB_SETTINGS_PATH}/{token}"),
                            &config,
                            Some(&web_status_values(state).await),
                            Some(&error),
                        ),
                    );
                }
            };
            let mut config = state.config.write().await;
            if let Err(error) = apply_updates(&mut config, &updates) {
                let page_config = config.clone();
                drop(config);
                let status = web_status_values(state).await;
                return http_response(
                    "400 Bad Request",
                    &render_settings_page(
                        &session,
                        &format!("{WEB_SETTINGS_PATH}/{token}"),
                        &page_config,
                        Some(&status),
                        Some(&error),
                    ),
                );
            }
            if let Err(error) = config::save_config(&*state.config_path, &config) {
                let page_config = config.clone();
                let error = error.to_string();
                drop(config);
                let status = web_status_values(state).await;
                return http_response(
                    "500 Internal Server Error",
                    &render_settings_page(
                        &session,
                        &format!("{WEB_SETTINGS_PATH}/{token}"),
                        &page_config,
                        Some(&status),
                        Some(&error),
                    ),
                );
            }
            drop(config);
            state.sessions.remove(token);
            http_response("200 OK", &saved_page())
        }
        _ => http_response(
            "405 Method Not Allowed",
            &render_message_page(
                "지원하지 않는 요청",
                "GET 또는 POST 요청만 사용할 수 있습니다.",
            ),
        ),
    }
}

fn purge_expired_sessions(sessions: &DashMap<String, WebSettingsSession>) {
    let now = Instant::now();
    sessions.retain(|_token, session| session.expires_at > now);
}

#[derive(Debug)]
enum ApiAuthError {
    Missing,
    Invalid,
    Forbidden,
}

impl ApiAuthError {
    fn response(&self) -> String {
        match self {
            Self::Missing => json_error("401 Unauthorized", "missing API key"),
            Self::Invalid => json_error("401 Unauthorized", "invalid API key"),
            Self::Forbidden => {
                json_error("403 Forbidden", "API key is not authorized for this guild")
            }
        }
    }
}

fn request_api_key(request: &HttpRequest) -> Option<&str> {
    request
        .headers
        .get("x-api-key")
        .map(String::as_str)
        .or_else(|| {
            request
                .headers
                .get("authorization")
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn authenticate_api_key(
    state: &WebSettingsState,
    request: &HttpRequest,
) -> std::result::Result<ApiKeyRecord, ApiAuthError> {
    let key = request_api_key(request).ok_or(ApiAuthError::Missing)?;
    let key_hash = api_key_hash(key);
    state
        .api_keys
        .read()
        .await
        .keys
        .iter()
        .find(|record| !record.revoked && record.key_hash == key_hash)
        .cloned()
        .ok_or(ApiAuthError::Invalid)
}

fn require_key_guild(
    record: &ApiKeyRecord,
    guild_id: u64,
) -> std::result::Result<(), ApiAuthError> {
    if record.guild_id == guild_id {
        Ok(())
    } else {
        Err(ApiAuthError::Forbidden)
    }
}

fn api_key_value(record: &ApiKeyRecord) -> Value {
    json!({
        "id": record.id,
        "label": record.label,
        "guild_id": record.guild_id,
        "created_at": record.created_at,
        "revoked": record.revoked,
    })
}

fn parse_api_guild_path<'a>(path: &'a str, prefix: &str) -> Option<(u64, Option<&'a str>)> {
    let rest = path.strip_prefix(prefix)?;
    let (guild_id, suffix) = rest
        .split_once('/')
        .map_or((rest, None), |(id, suffix)| (id, Some(suffix)));
    Some((guild_id.parse().ok()?, suffix))
}

async fn api_game_value(state: &WebSettingsState, guild_id: u64) -> Option<Value> {
    let running = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())?;
    let running = running.read().await;
    let mut players = running
        .game
        .players
        .iter()
        .map(|player| {
            json!({
                "user_id": player.user_id,
                "name": player.name,
                "alive": player.alive,
                "role": player.role.value(),
            })
        })
        .collect::<Vec<_>>();
    players.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    Some(json!({
        "guild_id": guild_id,
        "game_key": running.activity_game_key.clone(),
        "channel_id": running.channel_id.get(),
        "phase": running.game.phase.value(),
        "day_number": running.game.day_number,
        "participant_count": running.game.players.len(),
        "alive_count": running.game.alive_players().len(),
        "dead_count": running.game.dead_players().len(),
        "spectator_count": running.spectator_user_ids.len(),
        "anonymous_enabled": running.anonymous_enabled,
        "phase_remaining_seconds": running.phase_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()).as_secs()),
        "day_skip_votes": running.day_skip_voter_ids.len(),
        "day_skip_confirmed": running.day_skip_confirmed,
        "replay_event_count": running.replay_events.len(),
        "players": players,
    }))
}

async fn api_game_replay_value(state: &WebSettingsState, guild_id: u64) -> Option<Value> {
    if let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    {
        let running = running.read().await;
        let winner = running.game.winner();
        let status = if running.game.phase == Phase::Ended {
            "completed"
        } else {
            "active"
        };
        return Some(running.replay_snapshot(status, winner, &[]));
    }
    latest_completed_replay_for_guild(state, guild_id).await
}

async fn latest_completed_replay_for_guild(
    state: &WebSettingsState,
    guild_id: u64,
) -> Option<Value> {
    let completed_replays = state.completed_replays.read().await;
    completed_replays
        .iter()
        .find(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
}

async fn api_replay_summaries(state: &WebSettingsState, guild_id: u64) -> Value {
    let mut replays = Vec::new();
    if let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    {
        let running = running.read().await;
        replays.push(running.replay_summary("active", running.game.winner()));
    }
    {
        let completed_replays = state.completed_replays.read().await;
        replays.extend(
            completed_replays
                .iter()
                .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
                .map(|replay| {
                    let event_count = replay["events"]
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or_default();
                    let participant_count = replay["participants"]
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or_default();
                    json!({
                        "game_key": replay["game_key"].clone(),
                        "guild_id": replay["guild_id"].clone(),
                        "channel_id": replay["channel_id"].clone(),
                        "status": replay["status"].clone(),
                        "phase": replay["phase"].clone(),
                        "phase_key": replay["phase_key"].clone(),
                        "day_number": replay["day_number"].clone(),
                        "elapsed_seconds": replay["elapsed_seconds"].clone(),
                        "winner": replay["winner"].clone(),
                        "winner_key": replay["winner_key"].clone(),
                        "participant_count": participant_count,
                        "event_count": event_count,
                    })
                }),
        );
    }
    json!({"replays": replays})
}

async fn api_replay_by_key(
    state: &WebSettingsState,
    guild_id: u64,
    game_key: &str,
) -> Option<Value> {
    if let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    {
        let running = running.read().await;
        if running.activity_game_key == game_key {
            let winner = running.game.winner();
            let status = if running.game.phase == Phase::Ended {
                "completed"
            } else {
                "active"
            };
            return Some(running.replay_snapshot(status, winner, &[]));
        }
    }
    let completed_replays = state.completed_replays.read().await;
    completed_replays
        .iter()
        .find(|replay| {
            replay["guild_id"].as_u64() == Some(guild_id)
                && replay["game_key"].as_str() == Some(game_key)
        })
        .cloned()
}

fn json_page_params(query: &HashMap<String, String>, default_limit: usize) -> (usize, usize) {
    let page = query
        .get("page")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let per_page = query
        .get("per_page")
        .or_else(|| query.get("limit"))
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_limit)
        .min(100);
    (page, per_page)
}

fn slug_key(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

fn winner_slug(replay: &Value) -> Option<String> {
    replay["winner_key"].as_str().map(slug_key)
}

fn player_id_string(value: &Value) -> Option<String> {
    value
        .as_u64()
        .map(|id| id.to_string())
        .or_else(|| value.as_str().map(str::to_string))
}

fn participant_id(participant: &Value) -> Option<String> {
    player_id_string(&participant["user_id"])
}

fn participant_for_user<'a>(replay: &'a Value, user_id: &str) -> Option<&'a Value> {
    replay["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|participant| participant_id(participant).as_deref() == Some(user_id))
}

fn participant_nickname(participant: &Value) -> String {
    participant["name"]
        .as_str()
        .or_else(|| participant["nickname"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn participant_role_slug(participant: &Value) -> String {
    participant["final_role_key"]
        .as_str()
        .or_else(|| participant["role_key"].as_str())
        .map(slug_key)
        .unwrap_or_else(|| "unknown".to_string())
}

fn participant_survived(participant: &Value) -> bool {
    participant["alive"].as_bool().unwrap_or(false)
}

fn participant_revealed_role(replay: &Value, user_id: &str) -> Option<String> {
    participant_for_user(replay, user_id).map(participant_role_slug)
}

fn participant_won(participant: &Value, replay: &Value) -> bool {
    let Some(winner) = winner_slug(replay) else {
        return false;
    };
    participant["final_team"].as_str() == Some(winner.as_str())
}

fn death_info_for(replay: &Value, user_id: &str) -> (Option<u64>, Option<String>) {
    let Some(events) = replay["events"].as_array() else {
        return (None, None);
    };
    for event in events {
        let round = event["day_number"].as_u64();
        let details = &event["details"];
        match event["kind"].as_str().unwrap_or_default() {
            "confirmation_vote_resolved" => {
                if player_id_string(&details["executed_user_id"]).as_deref() == Some(user_id) {
                    return (round, Some("execution".to_string()));
                }
                if details["extra_killed_user_ids"]
                    .as_array()
                    .is_some_and(|ids| {
                        ids.iter()
                            .any(|id| player_id_string(id).as_deref() == Some(user_id))
                    })
                {
                    return (round, Some("other".to_string()));
                }
            }
            "night_resolved" => {
                let killed = details["killed_user_ids"].as_array().is_some_and(|ids| {
                    ids.iter()
                        .any(|id| player_id_string(id).as_deref() == Some(user_id))
                });
                if !killed {
                    continue;
                }
                let list_has = |name: &str| {
                    details[name].as_array().is_some_and(|ids| {
                        ids.iter()
                            .any(|id| player_id_string(id).as_deref() == Some(user_id))
                    })
                };
                let cause = if player_id_string(&details["mafia_target_user_id"]).as_deref()
                    == Some(user_id)
                {
                    "mafia_kill"
                } else if list_has("contractor_kill_user_ids") {
                    "contractor_kill"
                } else if list_has("vigilante_kill_user_ids") {
                    "vigilante_kill"
                } else if list_has("mercenary_kill_user_ids") {
                    "mercenary_kill"
                } else {
                    "other"
                };
                return (round, Some(cause.to_string()));
            }
            _ => {}
        }
    }
    (None, None)
}

fn compatible_game_summary(replay: &Value) -> Value {
    let participants = replay["participants"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    json!({
        "game_id": replay["game_key"].clone(),
        "started_at": replay["started_at"].clone(),
        "ended_at": replay["ended_at"].clone(),
        "player_count": participants.len(),
        "winner": winner_slug(replay),
        "rounds": replay["day_number"].clone(),
    })
}

fn compatible_game_detail(replay: &Value) -> Value {
    let players = replay["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|participant| {
            json!({
                "user_id": participant_id(participant),
                "nickname": participant_nickname(participant),
            })
        })
        .collect::<Vec<_>>();
    let mut value = compatible_game_summary(replay);
    if let Some(object) = value.as_object_mut() {
        object.insert("players".to_string(), Value::Array(players));
    }
    value
}

fn compatible_game_result(replay: &Value) -> Value {
    let players = replay["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|participant| {
            let user_id = participant_id(participant).unwrap_or_default();
            let (died_at_round, cause_of_death) = death_info_for(replay, &user_id);
            json!({
                "user_id": user_id,
                "nickname": participant_nickname(participant),
                "role": participant_role_slug(participant),
                "role_name": participant["final_role"].clone(),
                "survived": participant_survived(participant),
                "died_at_round": died_at_round,
                "cause_of_death": cause_of_death,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "game_id": replay["game_key"].clone(),
        "winner": winner_slug(replay),
        "ended_at": replay["ended_at"].clone(),
        "total_rounds": replay["day_number"].clone(),
        "players": players,
    })
}

fn compatible_event_type(kind: &str) -> String {
    match kind {
        "game_started" => "game_start".to_string(),
        "phase_started" => "phase_change".to_string(),
        "day_vote" | "confirmation_vote" | "day_skip_vote" | "day_extension_vote" => {
            "vote".to_string()
        }
        "night_action"
        | "contractor_contract"
        | "hacker_action"
        | "vigilante_investigation"
        | "psychologist_observation"
        | "hypnotist_wake" => "role_action".to_string(),
        "game_ended" => "game_end".to_string(),
        _ => kind.to_string(),
    }
}

fn compatible_events(replay: &Value) -> Value {
    let mut events = Vec::new();
    for event in replay["events"].as_array().into_iter().flatten() {
        let kind = event["kind"].as_str().unwrap_or_default();
        let event_id = event["id"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("e_{:06}", event["seq"].as_u64().unwrap_or_default()));
        let actor_id = event["actor"]["user_id"].as_u64().map(|id| id.to_string());
        let target_id = event["target_user_ids"]
            .as_array()
            .and_then(|ids| ids.first())
            .and_then(player_id_string);
        events.push(json!({
            "id": event_id,
            "timestamp": event["timestamp"].clone(),
            "round": event["day_number"].clone(),
            "type": compatible_event_type(kind),
            "actor_id": actor_id,
            "target_id": target_id,
            "payload": event["details"].clone(),
        }));

        if kind == "night_resolved" {
            for target in event["details"]["killed_user_ids"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(target_id) = player_id_string(target) else {
                    continue;
                };
                let (_, cause) = death_info_for(replay, &target_id);
                let role_revealed = participant_revealed_role(replay, &target_id);
                events.push(json!({
                    "id": format!("{event_id}_death_{target_id}"),
                    "timestamp": event["timestamp"].clone(),
                    "round": event["day_number"].clone(),
                    "type": "death",
                    "actor_id": Value::Null,
                    "target_id": target_id,
                    "payload": {
                        "cause": cause.unwrap_or_else(|| "other".to_string()),
                        "role_revealed": role_revealed,
                    },
                }));
            }
        } else if kind == "confirmation_vote_resolved" {
            if let Some(target_id) = player_id_string(&event["details"]["executed_user_id"]) {
                let role_revealed = participant_revealed_role(replay, &target_id);
                let vote_count = event["details"]["vote_counts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|count| count["approve"].as_bool() == Some(true))
                    .and_then(|count| count["count"].as_i64());
                events.push(json!({
                    "id": format!("{event_id}_death_{target_id}"),
                    "timestamp": event["timestamp"].clone(),
                    "round": event["day_number"].clone(),
                    "type": "death",
                    "actor_id": Value::Null,
                    "target_id": target_id,
                    "payload": {
                        "cause": "execution",
                        "role_revealed": role_revealed,
                        "vote_count": vote_count,
                    },
                }));
            }
        }
    }
    json!({
        "game_id": replay["game_key"].clone(),
        "events": events,
    })
}

async fn api_recent_games_value(
    state: &WebSettingsState,
    guild_id: u64,
    query: &HashMap<String, String>,
) -> Value {
    let (page, per_page) = json_page_params(query, 10);
    let completed_replays = state.completed_replays.read().await;
    let all = completed_replays
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .map(compatible_game_summary)
        .collect::<Vec<_>>();
    let total = all.len();
    let start = per_page.saturating_mul(page.saturating_sub(1));
    let data = all
        .into_iter()
        .skip(start)
        .take(per_page)
        .collect::<Vec<_>>();
    json!({
        "data": data,
        "total": total,
        "current_page": page,
        "per_page": per_page,
    })
}

async fn replay_for_compatible_game(
    state: &WebSettingsState,
    guild_id: u64,
    game_key: &str,
) -> Option<Value> {
    api_replay_by_key(state, guild_id, game_key).await
}

async fn api_recruitment_value(state: &WebSettingsState, guild_id: u64) -> Option<Value> {
    let recruitment = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())?;
    let recruitment = recruitment.read().await;
    let mut participants = recruitment
        .joined_ids
        .iter()
        .map(|user_id| {
            json!({
                "user_id": user_id,
                "name": recruitment.joined_names.get(user_id).cloned().unwrap_or_else(|| user_id.to_string()),
            })
        })
        .collect::<Vec<_>>();
    participants.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    let mut spectators = recruitment
        .spectator_ids
        .iter()
        .map(|user_id| {
            json!({
                "user_id": user_id,
                "name": recruitment.spectator_names.get(user_id).cloned().unwrap_or_else(|| user_id.to_string()),
            })
        })
        .collect::<Vec<_>>();
    spectators.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    let mut role_counts = recruitment
        .role_counts
        .iter()
        .map(|(role, count)| json!({"role": role.value(), "count": count}))
        .collect::<Vec<_>>();
    role_counts.sort_by_key(|item| item["role"].as_str().unwrap_or_default().to_string());
    Some(json!({
        "guild_id": guild_id,
        "host_user_id": recruitment.host_user_id.get(),
        "accepting": recruitment.accepting,
        "cancelled": recruitment.cancelled,
        "minimum_players": recruitment.minimum_players,
        "max_players": recruitment.max_players,
        "participant_count": participants.len(),
        "spectator_count": spectators.len(),
        "participants": participants,
        "spectators": spectators,
        "role_counts": role_counts,
        "special_roles": recruitment.special_roles.iter().map(|role| role.value()).collect::<Vec<_>>(),
    }))
}

async fn control_game(
    state: &WebSettingsState,
    guild_id: u64,
    action: &str,
) -> std::result::Result<Value, String> {
    let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("game not found".to_string());
    };
    let notifications = {
        let mut running = running.write().await;
        match action {
            "stop" => {
                if running.game.phase == Phase::Ended {
                    return Err("game is already ending".to_string());
                }
                running.game.phase = Phase::Ended;
                running.phase_deadline = None;
                vec![
                    running.night_notify.clone(),
                    running.vote_notify.clone(),
                    running.confirm_notify.clone(),
                    running.day_notify.clone(),
                ]
            }
            "skip_day" => {
                if running.game.phase != Phase::Day {
                    return Err("skip_day is only available during day discussion".to_string());
                }
                running.day_skip_confirmed = true;
                running.day_extension_active = false;
                vec![running.day_notify.clone()]
            }
            "extend_day" => {
                if running.game.phase != Phase::Day || !running.day_extension_active {
                    return Err(
                        "extend_day is only available during the day extension vote".to_string()
                    );
                }
                running.day_extension_confirmed = true;
                vec![running.day_notify.clone()]
            }
            _ => return Err("unsupported game action".to_string()),
        }
    };
    for notify in notifications {
        notify.notify_waiters();
    }
    Ok(json!({"ok": true, "guild_id": guild_id, "action": action}))
}

async fn cancel_recruitment(
    state: &WebSettingsState,
    guild_id: u64,
) -> std::result::Result<Value, String> {
    let Some(recruitment) = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("recruitment not found".to_string());
    };
    let notify = {
        let mut recruitment = recruitment.write().await;
        if !recruitment.accepting {
            return Err("recruitment is no longer accepting players".to_string());
        }
        recruitment.cancelled = true;
        recruitment.accepting = false;
        recruitment.done.clone()
    };
    notify.notify_waiters();
    Ok(json!({"ok": true, "guild_id": guild_id, "action": "cancel"}))
}

async fn start_recruitment(
    state: &WebSettingsState,
    guild_id: u64,
) -> std::result::Result<Value, String> {
    let Some(recruitment) = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("recruitment not found".to_string());
    };
    let notify = {
        let mut recruitment = recruitment.write().await;
        if !recruitment.accepting {
            return Err("recruitment is no longer accepting players".to_string());
        }
        if recruitment.joined_ids.len() < recruitment.minimum_players {
            return Err("not enough players to start".to_string());
        }
        recruitment.accepting = false;
        recruitment.done.clone()
    };
    notify.notify_waiters();
    Ok(json!({"ok": true, "guild_id": guild_id, "action": "start"}))
}

async fn route_protected_api_request(
    state: &WebSettingsState,
    request: &HttpRequest,
    path: &str,
    query: &str,
) -> Option<String> {
    let compatible_api_path = path == "/games/recent"
        || path == "/stats/leaderboard"
        || path.starts_with("/game/")
        || path.starts_with("/stats/user/");
    if !path.starts_with("/api/v1/") && !compatible_api_path {
        return None;
    }
    let key = match authenticate_api_key(state, request).await {
        Ok(key) => key,
        Err(error) => return Some(error.response()),
    };
    let query = parse_urlencoded(query);
    let response = match (request.method.as_str(), path) {
        ("GET", "/api/v1/me") => json_response(json!({"key": api_key_value(&key)})),
        ("GET", "/api/v1/config") => {
            let status = web_status_values(state).await;
            json_response(json!({"settings": status["settings"].clone()}))
        }
        ("GET", "/api/v1/stats") => json_response(web_stats_summary(state).await),
        ("GET", "/api/v1/stats/leaderboard") | ("GET", "/stats/leaderboard") => {
            json_response(compatible_leaderboard_values(state, key.guild_id, &query).await)
        }
        ("GET", "/api/v1/games") => {
            let games = api_game_value(state, key.guild_id)
                .await
                .into_iter()
                .collect::<Vec<_>>();
            json_response(json!({"games": games}))
        }
        ("GET", "/api/v1/games/recent") | ("GET", "/games/recent") => {
            json_response(api_recent_games_value(state, key.guild_id, &query).await)
        }
        ("GET", "/api/v1/replays") => {
            json_response(api_replay_summaries(state, key.guild_id).await)
        }
        ("GET", "/api/v1/leaderboard") => {
            let limit = query
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10);
            json_response(web_leaderboard_values(state, "rating", limit).await)
        }
        _ => {
            if let Some(user_path) = path
                .strip_prefix("/api/v1/stats/user/")
                .or_else(|| path.strip_prefix("/stats/user/"))
            {
                if let Some(user_id) = user_path.strip_suffix("/games") {
                    if request.method == "GET" {
                        json_response(
                            compatible_user_games_value(state, key.guild_id, user_id, &query).await,
                        )
                    } else {
                        json_error("404 Not Found", "API endpoint not found")
                    }
                } else if request.method == "GET" {
                    compatible_user_stats_value(state, key.guild_id, user_path)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "user not found"))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some(game_path) = path
                .strip_prefix("/api/v1/game/")
                .or_else(|| path.strip_prefix("/game/"))
            {
                let (game_key, suffix) = game_path
                    .split_once('/')
                    .map_or((game_path, None), |(game_key, suffix)| {
                        (game_key, Some(suffix))
                    });
                if request.method != "GET" {
                    json_error("404 Not Found", "API endpoint not found")
                } else if let Some(replay) =
                    replay_for_compatible_game(state, key.guild_id, game_key).await
                {
                    match suffix {
                        None => json_response(compatible_game_detail(&replay)),
                        Some("result") => json_response(compatible_game_result(&replay)),
                        Some("events") => json_response(compatible_events(&replay)),
                        _ => json_error("404 Not Found", "API endpoint not found"),
                    }
                } else {
                    json_error("404 Not Found", "game not found")
                }
            } else if let Some(metric) = path.strip_prefix("/api/v1/leaderboard/") {
                if !WEB_LEADERBOARD_METRICS.contains(&metric) {
                    json_error("400 Bad Request", "unsupported leaderboard metric")
                } else {
                    let limit = query
                        .get("limit")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(10);
                    json_response(web_leaderboard_values(state, metric, limit).await)
                }
            } else if let Some(game_key) = path.strip_prefix("/api/v1/replays/") {
                if request.method == "GET" {
                    api_replay_by_key(state, key.guild_id, game_key)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "replay not found"))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some((guild_id, suffix)) = parse_api_guild_path(path, "/api/v1/games/") {
                if let Err(error) = require_key_guild(&key, guild_id) {
                    error.response()
                } else if suffix.is_none() && request.method == "GET" {
                    api_game_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "game not found"))
                } else if suffix == Some("replay") && request.method == "GET" {
                    api_game_replay_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "replay not found"))
                } else if suffix == Some("actions") && request.method == "POST" {
                    let action =
                        serde_json::from_str::<Value>(&request.body)
                            .ok()
                            .and_then(|body| {
                                body.get("action")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            });
                    let Some(action) = action else {
                        return Some(json_error("400 Bad Request", "JSON body requires action"));
                    };
                    control_game(state, guild_id, &action)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|message| json_error("409 Conflict", &message))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some((guild_id, suffix)) =
                parse_api_guild_path(path, "/api/v1/recruitments/")
            {
                if let Err(error) = require_key_guild(&key, guild_id) {
                    error.response()
                } else if suffix.is_none() && request.method == "GET" {
                    api_recruitment_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "recruitment not found"))
                } else if suffix == Some("actions") && request.method == "POST" {
                    let action =
                        serde_json::from_str::<Value>(&request.body)
                            .ok()
                            .and_then(|body| {
                                body.get("action")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            });
                    match action.as_deref() {
                        Some("cancel") => cancel_recruitment(state, guild_id)
                            .await
                            .map(json_response)
                            .unwrap_or_else(|message| json_error("409 Conflict", &message)),
                        Some("start") => start_recruitment(state, guild_id)
                            .await
                            .map(json_response)
                            .unwrap_or_else(|message| json_error("409 Conflict", &message)),
                        _ => json_error(
                            "400 Bad Request",
                            "supported recruitment actions: start, cancel",
                        ),
                    }
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else {
                json_error("404 Not Found", "API endpoint not found")
            }
        }
    };
    Some(response)
}

async fn route_public_request(state: &WebSettingsState, path: &str, query: &str) -> Option<String> {
    let query = parse_urlencoded(query);
    match path {
        "/" => {
            let status = web_status_values(state).await;
            let leaderboard = web_leaderboard_values(state, "rating", 3).await;
            let stats = web_stats_summary(state).await;
            Some(http_response(
                "200 OK",
                &render_home_page(&status, &leaderboard, &stats),
            ))
        }
        "/status" => {
            let status = web_status_values(state).await;
            Some(http_response("200 OK", &render_status_page(&status)))
        }
        "/leaderboard" => {
            let metric = query.get("metric").map(String::as_str).unwrap_or("rating");
            let leaderboard = web_leaderboard_values(state, metric, 20).await;
            let stats = web_stats_summary(state).await;
            Some(http_response(
                "200 OK",
                &render_leaderboard_page(&leaderboard, &stats),
            ))
        }
        "/rating" => Some(http_response("200 OK", &render_rating_page())),
        "/roles" => Some(http_response("200 OK", &render_roles_page())),
        "/api" | "/api/docs" => Some(http_response(
            "200 OK",
            &render_api_docs_page(&state.base_url),
        )),
        "/health" => Some(json_response(
            json!({"ok": true, "service": "mafia-discord-bot"}),
        )),
        "/api/status" => Some(json_response(web_status_values(state).await)),
        "/api/games" => {
            let status = web_status_values(state).await;
            Some(json_response(json!({"games": status["games"].clone()})))
        }
        "/api/settings" => {
            let status = web_status_values(state).await;
            Some(json_response(
                json!({"settings": status["settings"].clone()}),
            ))
        }
        "/api/stats" => Some(json_response(web_stats_summary(state).await)),
        "/api/leaderboard" => {
            let limit = query
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10);
            Some(json_response(
                web_leaderboard_values(state, "rating", limit).await,
            ))
        }
        _ => {
            if let Some(metric) = path.strip_prefix("/api/leaderboard/") {
                let limit = query
                    .get("limit")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(10);
                Some(json_response(
                    web_leaderboard_values(state, metric, limit).await,
                ))
            } else {
                None
            }
        }
    }
}

fn valid_api_key_label(value: &str) -> std::result::Result<String, String> {
    let label = value.trim();
    if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
        return Err("API 키 이름은 제어 문자 없이 1~64자여야 합니다.".to_string());
    }
    Ok(label.to_string())
}

fn api_key_records_for_guild(store: &ApiKeyStore, guild_id: u64) -> Vec<ApiKeyRecord> {
    let mut records = store
        .keys
        .iter()
        .filter(|record| record.guild_id == guild_id)
        .cloned()
        .collect::<Vec<_>>();
    records.sort_by_key(|record| std::cmp::Reverse(record.created_at.clone()));
    records
}

async fn route_api_key_management(
    state: &WebSettingsState,
    session: &WebSettingsSession,
    token: &str,
    request: &HttpRequest,
) -> String {
    let action = format!("{WEB_SETTINGS_PATH}/{token}/api-keys");
    match request.method.as_str() {
        "GET" => {
            let store = state.api_keys.read().await;
            let records = api_key_records_for_guild(&store, session.guild_id);
            http_response(
                "200 OK",
                &render_api_key_page(session, &action, &records, None, None),
            )
        }
        "POST" => {
            let form = parse_urlencoded(&request.body);
            let result = match form.get("action").map(String::as_str) {
                Some("create") => {
                    let label = form
                        .get("label")
                        .ok_or_else(|| "API 키 이름을 입력하세요.".to_string())
                        .and_then(|value| valid_api_key_label(value));
                    let label = match label {
                        Ok(label) => label,
                        Err(error) => {
                            return api_key_management_error(state, session, &action, error).await;
                        }
                    };
                    let mut store = state.api_keys.write().await;
                    let previous = store.clone();
                    let key = issue_api_key(&mut store, session.guild_id, session.user_id, label);
                    if let Err(error) = save_api_key_store(&*state.api_keys_path, &store) {
                        *store = previous;
                        let error = error.to_string();
                        drop(store);
                        return api_key_management_error(state, session, &action, error).await;
                    }
                    Ok(Some(key))
                }
                Some("revoke") => {
                    let Some(key_id) = form.get("key_id") else {
                        return api_key_management_error(
                            state,
                            session,
                            &action,
                            "폐기할 API 키를 선택하세요.".to_string(),
                        )
                        .await;
                    };
                    let mut store = state.api_keys.write().await;
                    let previous = store.clone();
                    let Some(record) = store
                        .keys
                        .iter_mut()
                        .find(|record| record.id == *key_id && record.guild_id == session.guild_id)
                    else {
                        drop(store);
                        return api_key_management_error(
                            state,
                            session,
                            &action,
                            "API 키를 찾을 수 없습니다.".to_string(),
                        )
                        .await;
                    };
                    record.revoked = true;
                    if let Err(error) = save_api_key_store(&*state.api_keys_path, &store) {
                        *store = previous;
                        let error = error.to_string();
                        drop(store);
                        return api_key_management_error(state, session, &action, error).await;
                    }
                    Ok(None)
                }
                _ => Err("지원하지 않는 API 키 작업입니다.".to_string()),
            };
            match result {
                Ok(issued_key) => {
                    let store = state.api_keys.read().await;
                    let records = api_key_records_for_guild(&store, session.guild_id);
                    http_response(
                        "200 OK",
                        &render_api_key_page(
                            session,
                            &action,
                            &records,
                            issued_key.as_deref(),
                            None,
                        ),
                    )
                }
                Err(error) => api_key_management_error(state, session, &action, error).await,
            }
        }
        _ => json_error("405 Method Not Allowed", "GET or POST is required"),
    }
}

async fn api_key_management_error(
    state: &WebSettingsState,
    session: &WebSettingsSession,
    action: &str,
    error: String,
) -> String {
    let store = state.api_keys.read().await;
    let records = api_key_records_for_guild(&store, session.guild_id);
    http_response(
        "400 Bad Request",
        &render_api_key_page(session, action, &records, None, Some(&error)),
    )
}

async fn web_status_values(state: &WebSettingsState) -> Value {
    let now = Instant::now();
    let config = state.config.read().await.clone();
    let mut games = Vec::new();
    for entry in state.games.iter() {
        let guild_id = entry.key().get();
        let running = entry.value().read().await;
        let alive_count = running.game.alive_players().len();
        let dead_count = running.game.dead_players().len();
        games.push(json!({
            "guild_id": guild_id,
            "guild_name": guild_id.to_string(),
            "channel_id": running.channel_id.get(),
            "channel_name": format!("#{}", running.channel_id.get()),
            "phase": running.game.phase.value(),
            "day": format!("{}일차", running.game.day_number),
            "participant_count": running.game.players.len(),
            "alive_count": alive_count,
            "dead_count": dead_count,
            "spectator_count": running.spectator_user_ids.len(),
            "anonymous_enabled": running.anonymous_enabled,
            "elapsed": stats::play_duration_text(running.started_at.elapsed().as_secs() as i64),
        }));
    }
    games.sort_by_key(|item| {
        item.get("guild_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });
    json!({
        "bot": {
            "ready": true,
            "name": state.bot_name,
            "latency_ms": 0,
            "guild_count": state.guild_count,
            "user_count": 0,
            "uptime": stats::play_duration_text(now.duration_since(state.started_at).as_secs() as i64),
        },
        "api": {
            "base_url": format!("{}/api", state.base_url.trim_end_matches('/')),
        },
        "games": games,
        "recruiting_guild_count": state.recruitments.len(),
        "settings": {
            "game_enabled": config.game_enabled,
            "max_player_count_text": if config.max_player_count == 0 {
                "제한 없음".to_string()
            } else {
                format!("{}명", config.max_player_count)
            },
            "role_summary": format!(
                "마피아 {}명, 의사 {}명, 수사직 {}명",
                config.default_mafia_count, config.default_doctor_count, config.default_police_count
            ),
            "special_summary": format!(
                "시민 {}개, 마피아 {}개, 중립 {}개",
                config.citizen_special_count, config.mafia_special_count, config.neutral_special_count
            ),
            "anonymous_mode_text": if config.anonymous_mode {
                format!("켜짐 ({})", match config.anonymous_name_mode.as_str() {
                    "number" => "숫자",
                    _ => "동물",
                })
            } else {
                "꺼짐".to_string()
            },
            "slowmode_text": format!("{}초", config.chat_slowmode_seconds),
            "cult_team_text": if config.enable_cult_team { "켜짐" } else { "꺼짐" },
        }
    })
}

async fn web_stats_summary(state: &WebSettingsState) -> Value {
    let entries = {
        let stats_read = state.stats.read().await;
        stats_read.users.values().cloned().collect::<Vec<_>>()
    };
    let played_entries = entries
        .iter()
        .filter(|entry| entry.games > 0)
        .collect::<Vec<_>>();
    let total_player_games = played_entries.iter().map(|entry| entry.games).sum::<i64>();
    let total_wins = played_entries.iter().map(|entry| entry.wins).sum::<i64>();
    let total_play_seconds = played_entries
        .iter()
        .map(|entry| entry.play_seconds)
        .sum::<i64>();
    let average_rating = if played_entries.is_empty() {
        stats::INITIAL_RATING
    } else {
        (played_entries.iter().map(|entry| entry.rating).sum::<i64>() as f64
            / played_entries.len() as f64)
            .round() as i64
    };
    json!({
        "registered_users": entries.len(),
        "recorded_players": played_entries.len(),
        "total_player_games": total_player_games,
        "total_wins": total_wins,
        "total_playtime": stats::play_duration_text(total_play_seconds),
        "total_play_seconds": total_play_seconds,
        "average_rating": average_rating,
    })
}

async fn web_leaderboard_values(state: &WebSettingsState, metric: &str, limit: usize) -> Value {
    let metric = if WEB_LEADERBOARD_METRICS.contains(&metric) {
        metric
    } else {
        "rating"
    };
    let safe_limit = limit.clamp(1, 50);
    let stats_read = {
        let stats_read = state.stats.read().await;
        stats_read.clone()
    };
    let entries = stats::leaderboard_entries(&stats_read, metric, safe_limit)
        .into_iter()
        .enumerate()
        .map(|(index, (user_id, entry))| {
            let winrate = if entry.games > 0 {
                ((entry.wins as f64 / entry.games as f64 * 1000.0).round()) / 10.0
            } else {
                0.0
            };
            json!({
                "rank": index + 1,
                "user_id": user_id,
                "name": if entry.name.is_empty() { "알 수 없음".to_string() } else { entry.name.clone() },
                "games": entry.games,
                "wins": entry.wins,
                "losses": entry.losses,
                "win_streak": entry.win_streak,
                "best_win_streak": entry.best_win_streak,
                "streak_text": format!("{}연승", entry.win_streak),
                "best_streak_text": format!("{}연승", entry.best_win_streak),
                "winrate": winrate,
                "winrate_text": stats::win_rate_text(entry.wins, entry.games),
                "mafia_team_games": entry.mafia_team_games,
                "play_seconds": entry.play_seconds,
                "playtime": stats::play_duration_text(entry.play_seconds),
                "rating": entry.rating,
                "rating_rank": stats::rating_rank(entry.rating),
                "rating_peak": entry.rating_peak,
                "rating_peak_rank": stats::rating_rank(entry.rating_peak),
                "rating_games": entry.rating_games,
                "value": stats::leaderboard_value(&entry, metric),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "metric": metric,
        "metric_name": stats::leaderboard_metric_name(metric),
        "metrics": WEB_LEADERBOARD_METRICS
            .iter()
            .map(|key| json!({"key": key, "name": stats::leaderboard_metric_name(key)}))
            .collect::<Vec<_>>(),
        "limit": safe_limit,
        "entries": entries,
    })
}

fn most_played_role(entry: &stats::PlayerStats) -> Option<String> {
    entry
        .roles
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(role, _)| role.clone())
}

fn compatible_user_game_rows(replays: &[Value], user_id: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    for replay in replays {
        let Some(participant) = replay["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|participant| participant_id(participant).as_deref() == Some(user_id))
        else {
            continue;
        };
        rows.push(json!({
            "game_id": replay["game_key"].clone(),
            "ended_at": replay["ended_at"].clone(),
            "role": participant_role_slug(participant),
            "role_name": participant["final_role"].clone(),
            "result": if participant_won(participant, replay) { "win" } else { "loss" },
            "survived": participant_survived(participant),
        }));
    }
    rows
}

fn compatible_user_recent_counts(
    replays: &[Value],
    user_id: &str,
) -> (
    i64,
    HashMap<String, i64>,
    HashMap<String, f64>,
    i64,
    i64,
    i64,
) {
    let mut survived = 0;
    let mut role_counts: HashMap<String, i64> = HashMap::new();
    let mut role_wins: HashMap<String, i64> = HashMap::new();
    let mut role_games: HashMap<String, i64> = HashMap::new();
    let mut executed = 0;
    let mut killed_by_mafia = 0;
    for replay in replays {
        let Some(participant) = replay["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|participant| participant_id(participant).as_deref() == Some(user_id))
        else {
            continue;
        };
        let role = participant_role_slug(participant);
        *role_counts.entry(role.clone()).or_default() += 1;
        *role_games.entry(role.clone()).or_default() += 1;
        if participant_won(participant, replay) {
            *role_wins.entry(role).or_default() += 1;
        }
        if participant_survived(participant) {
            survived += 1;
        } else {
            let (_, cause) = death_info_for(replay, user_id);
            match cause.as_deref() {
                Some("execution") => executed += 1,
                Some("mafia_kill") => killed_by_mafia += 1,
                _ => {}
            }
        }
    }
    let win_rate_by_role = role_games
        .iter()
        .map(|(role, games)| {
            let wins = role_wins.get(role).copied().unwrap_or(0);
            let rate = if *games > 0 {
                ((wins as f64 / *games as f64) * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            (role.clone(), rate)
        })
        .collect::<HashMap<_, _>>();
    (
        survived,
        role_counts,
        win_rate_by_role,
        executed,
        killed_by_mafia,
        0,
    )
}

async fn compatible_leaderboard_values(
    state: &WebSettingsState,
    guild_id: u64,
    query: &HashMap<String, String>,
) -> Value {
    let sort = query.get("sort").map(String::as_str).unwrap_or("rating");
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 100);
    let metric = match sort {
        "winrate" => "winrate",
        "games" => "games",
        "wins" => "wins",
        "kills" => "wins",
        "rating" => "rating",
        _ => "rating",
    };
    let stats_read = state.stats.read().await.clone();
    let replays = state
        .completed_replays
        .read()
        .await
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
        .collect::<Vec<_>>();
    let data = stats::leaderboard_entries(&stats_read, metric, limit)
        .into_iter()
        .map(|(user_id, entry)| {
            let (_, _, _, executed, killed_by_mafia, kills) =
                compatible_user_recent_counts(&replays, &user_id);
            let win_rate = if entry.games > 0 {
                ((entry.wins as f64 / entry.games as f64) * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            json!({
                "user_id": user_id,
                "nickname": entry.name,
                "games_played": entry.games,
                "wins": entry.wins,
                "losses": entry.losses,
                "win_rate": win_rate,
                "rating": entry.rating,
                "rating_rank": stats::rating_rank(entry.rating),
                "win_streak": entry.win_streak,
                "best_win_streak": entry.best_win_streak,
                "most_played_role": most_played_role(&entry),
                "times_executed": executed,
                "times_killed_by_mafia": killed_by_mafia,
                "kills": kills,
                "most_frequent_killer": Value::Null,
            })
        })
        .collect::<Vec<_>>();
    json!({"data": data})
}

async fn compatible_user_stats_value(
    state: &WebSettingsState,
    guild_id: u64,
    user_id: &str,
) -> Option<Value> {
    let stats_read = state.stats.read().await.clone();
    let entry = stats_read.users.get(user_id).cloned()?;
    let replays = state
        .completed_replays
        .read()
        .await
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
        .collect::<Vec<_>>();
    let (survived, recent_role_counts, win_rate_by_role, executed, killed_by_mafia, kills) =
        compatible_user_recent_counts(&replays, user_id);
    let role_play_count = if recent_role_counts.is_empty() {
        entry.roles.clone()
    } else {
        recent_role_counts
    };
    let win_rate = if entry.games > 0 {
        ((entry.wins as f64 / entry.games as f64) * 1000.0).round() / 1000.0
    } else {
        0.0
    };
    Some(json!({
        "user_id": user_id,
        "nickname": entry.name,
        "total_games": entry.games,
        "wins": entry.wins,
        "losses": entry.losses,
        "win_rate": win_rate,
        "rating": entry.rating,
        "rating_rank": stats::rating_rank(entry.rating),
        "win_streak": entry.win_streak,
        "best_win_streak": entry.best_win_streak,
        "win_rate_by_role": win_rate_by_role,
        "role_play_count": role_play_count,
        "most_killed_by": Value::Null,
        "most_killed": Value::Null,
        "kills": kills,
        "times_executed": executed,
        "times_killed_by_mafia": killed_by_mafia,
        "times_survived": survived,
    }))
}

async fn compatible_user_games_value(
    state: &WebSettingsState,
    guild_id: u64,
    user_id: &str,
    query: &HashMap<String, String>,
) -> Value {
    let (page, per_page) = json_page_params(query, 20);
    let replays = state
        .completed_replays
        .read()
        .await
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
        .collect::<Vec<_>>();
    let rows = compatible_user_game_rows(&replays, user_id);
    let total = rows.len();
    let start = per_page.saturating_mul(page.saturating_sub(1));
    let data = rows
        .into_iter()
        .skip(start)
        .take(per_page)
        .collect::<Vec<_>>();
    json!({
        "data": data,
        "total": total,
        "current_page": page,
        "per_page": per_page,
    })
}

fn render_settings_page(
    session: &WebSettingsSession,
    action: &str,
    config: &BotConfig,
    status: Option<&Value>,
    error: Option<&str>,
) -> String {
    let message_html = error.map_or_else(String::new, |message| {
        format!(
            r#"<p class="message error">⚠️ {}</p>"#,
            html_escape(message)
        )
    });
    let rows = WEB_CONFIG_FIELDS
        .iter()
        .map(|field| render_field(*field, config))
        .collect::<Vec<_>>()
        .join("\n");
    let status_html = status.map(render_status_summary).unwrap_or_default();
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 게임 설정</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p class="meta">{} 님 전용 1회용 링크입니다. 저장하면 이 링크는 더 이상 사용할 수 없습니다.</p>
{}
{message_html}
<form method="post" action="{}">
  <fieldset>
    <legend>설정 항목</legend>
    {rows}
  </fieldset>
  <button type="submit">저장하기</button>
</form>
<p><a href="{}/api-keys">API 키 관리</a></p>
</main>
</div>
</body>
</html>"#,
        render_page_header("🕵️ 마피아 게임 웹 설정", false),
        html_escape(&session.user_label),
        status_html,
        html_escape(action),
        html_escape(action)
    )
}

fn render_api_key_page(
    session: &WebSettingsSession,
    action: &str,
    records: &[ApiKeyRecord],
    issued_key: Option<&str>,
    error: Option<&str>,
) -> String {
    let message_html = error.map_or_else(String::new, |message| {
        format!(
            r#"<p class="message error">⚠️ {}</p>"#,
            html_escape(message)
        )
    });
    let issued_html = issued_key.map_or_else(String::new, |key| {
        format!(
            r#"<section class="panel"><h2>새 API 키</h2><p class="message error">이 키는 지금 한 번만 표시됩니다. 안전한 곳에 보관하세요.</p><pre>{}</pre></section>"#,
            html_escape(key)
        )
    });
    let rows = records
        .iter()
        .map(|record| {
            let state = if record.revoked { "폐기됨" } else { "활성" };
            let action = if record.revoked {
                String::new()
            } else {
                format!(
                    r#"<form method="post" action="{action}"><input type="hidden" name="action" value="revoke"><input type="hidden" name="key_id" value="{}"><button type="submit">폐기</button></form>"#,
                    html_escape(&record.id)
                )
            };
            format!(
                r#"<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{action}</td></tr>"#,
                html_escape(&record.label),
                html_escape(&record.id),
                html_escape(&record.created_at),
                state,
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let table = if rows.is_empty() {
        "<p class=\"meta\">발급된 API 키가 없습니다.</p>".to_string()
    } else {
        format!(
            r#"<table><thead><tr><th>이름</th><th>키 ID</th><th>발급 시각</th><th>상태</th><th></th></tr></thead><tbody>{rows}</tbody></table>"#
        )
    };
    let settings_path = action.trim_end_matches("/api-keys");
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 API 키 관리</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p class="meta">{} 서버 전용 키입니다. 발급된 키는 이 서버의 보호 API만 사용할 수 있습니다.</p>
{message_html}
{issued_html}
<section class="panel"><h2>키 발급</h2><form method="post" action="{action}"><input type="hidden" name="action" value="create"><label class="row" for="label"><span>키 이름</span><input type="text" id="label" name="label" maxlength="64" required></label><button type="submit">키 발급</button></form></section>
<section class="panel"><h2>발급된 키</h2>{table}</section>
<p><a href="{settings_path}">설정으로 돌아가기</a></p>
</main>
</div>
</body>
</html>"#,
        render_page_header("마피아 API 키 관리", false),
        html_escape(&session.user_label),
        action = html_escape(action),
        settings_path = html_escape(settings_path),
    )
}

fn safe_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => html_escape(text),
        Some(Value::Number(number)) => html_escape(&number.to_string()),
        Some(Value::Bool(value)) => html_escape(&value.to_string()),
        _ => "-".to_string(),
    }
}

fn render_nav() -> &'static str {
    r#"<nav class="nav"><a href="/">홈</a><a href="/status">상태판</a><a href="/leaderboard">리더보드</a><a href="/rating">레이팅 설명</a><a href="/roles">역할 설명</a><a href="/api/docs">API 문서</a></nav>"#
}

fn render_status_summary(status: &Value) -> String {
    let bot = status.get("bot").unwrap_or(&Value::Null);
    let settings = status.get("settings").unwrap_or(&Value::Null);
    let games_len = status
        .get("games")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let cards = [
        (
            "봇 상태",
            if bot["ready"].as_bool().unwrap_or(false) {
                "온라인".to_string()
            } else {
                "시작 중".to_string()
            },
        ),
        ("서버 수", safe_text(bot.get("guild_count"))),
        ("진행 중 게임", games_len.to_string()),
        (
            "모집 중 서버",
            safe_text(status.get("recruiting_guild_count")),
        ),
        (
            "게임 시작",
            if settings["game_enabled"].as_bool().unwrap_or(false) {
                "활성화".to_string()
            } else {
                "비활성화".to_string()
            },
        ),
        ("업타임", safe_text(bot.get("uptime"))),
    ];
    format!(
        r#"<section class="grid">{}</section>"#,
        cards
            .into_iter()
            .map(|(label, value)| format!(
                r#"<div class="card"><span>{}</span><strong>{}</strong></div>"#,
                html_escape(label),
                value
            ))
            .collect::<Vec<_>>()
            .join("")
    )
}

fn render_games_table(status: &Value) -> String {
    let Some(games) = status.get("games").and_then(Value::as_array) else {
        return r#"<section class="panel"><h2>진행 중 게임</h2><p class="meta">현재 진행 중인 게임이 없습니다.</p></section>"#.to_string();
    };
    if games.is_empty() {
        return r#"<section class="panel"><h2>진행 중 게임</h2><p class="meta">현재 진행 중인 게임이 없습니다.</p></section>"#.to_string();
    }
    let rows = games
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td></tr>",
                safe_text(item.get("guild_name")),
                safe_text(item.get("channel_name")),
                safe_text(item.get("phase")),
                safe_text(item.get("day")),
                safe_text(item.get("alive_count")),
                safe_text(item.get("participant_count")),
                safe_text(item.get("dead_count")),
                safe_text(item.get("elapsed")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<section class="panel"><h2>진행 중 게임</h2><table><thead><tr><th>서버</th><th>채널</th><th>단계</th><th>일차</th><th>생존/참가</th><th>사망</th><th>진행 시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

fn base_html(title: &str, body: &str, auto_refresh: bool) -> String {
    let refresh = if auto_refresh {
        r#"<meta http-equiv="refresh" content="20">"#
    } else {
        ""
    };
    format!(
        r#"<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex">{refresh}<title>{}</title>{WEB_PAGE_STYLE}</head><body><div class="site-shell">{}{body}</main></div></body></html>"#,
        html_escape(title),
        render_page_header(title, true),
    )
}

fn render_page_header(title: &str, with_nav: bool) -> String {
    let nav = if with_nav { render_nav() } else { "" };
    format!(
        r#"<header class="site-header"><a class="site-mark" href="/" aria-label="마피아 봇 홈">M</a><div><p class="eyebrow">MAFIA REMAKE</p><h1>{}</h1></div></header>{nav}<main>"#,
        html_escape(title),
    )
}

fn render_home_page(status: &Value, leaderboard: &Value, stats_summary: &Value) -> String {
    let body = format!(
        r#"<p class="meta">봇 상태와 전적을 한눈에 보는 홈입니다. 상태 정보는 20초마다 자동 새로고침됩니다.</p>{}{}{}"#,
        render_status_summary(status),
        render_games_table(status),
        render_stats_cards(stats_summary),
    );
    let body = format!(
        "{body}<section class=\"panel\"><h2>레이팅 TOP 3</h2>{}</section>",
        render_leaderboard_podium(leaderboard)
    );
    base_html("마피아 봇 홈", &body, true)
}

fn render_status_page(status: &Value) -> String {
    let settings = status.get("settings").unwrap_or(&Value::Null);
    let rows = [
        (
            "최대 인원",
            safe_text(settings.get("max_player_count_text")),
        ),
        ("기본 구성", safe_text(settings.get("role_summary"))),
        ("특수룰 수", safe_text(settings.get("special_summary"))),
        ("익명 채팅", safe_text(settings.get("anonymous_mode_text"))),
        ("채팅 슬로우모드", safe_text(settings.get("slowmode_text"))),
        ("교주팀", safe_text(settings.get("cult_team_text"))),
    ]
    .into_iter()
    .map(|(label, value)| format!("<tr><th>{}</th><td>{value}</td></tr>", html_escape(label)))
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<p class="meta">진행 중 게임, 서버 연결 상태, 주요 게임 설정만 보여줍니다. 20초마다 자동 새로고침됩니다.</p>{}<section class="panel"><h2>현재 주요 설정</h2><table><tbody>{rows}</tbody></table></section>{}"#,
        render_status_summary(status),
        render_games_table(status),
    );
    base_html("마피아 봇 상태판", &body, true)
}

fn render_stats_cards(stats_summary: &Value) -> String {
    let cards = [
        (
            "기록된 유저",
            safe_text(stats_summary.get("recorded_players")),
        ),
        (
            "누적 플레이",
            safe_text(stats_summary.get("total_player_games")),
        ),
        ("누적 시간", safe_text(stats_summary.get("total_playtime"))),
        (
            "평균 레이팅",
            safe_text(stats_summary.get("average_rating")),
        ),
    ];
    format!(
        r#"<section class="grid">{}</section>"#,
        cards
            .into_iter()
            .map(|(label, value)| format!(
                r#"<div class="card"><span>{}</span><strong>{value}</strong></div>"#,
                html_escape(label)
            ))
            .collect::<Vec<_>>()
            .join("")
    )
}

fn render_metric_tabs(leaderboard: &Value) -> String {
    let current = leaderboard
        .get("metric")
        .and_then(Value::as_str)
        .unwrap_or("rating");
    let Some(metrics) = leaderboard.get("metrics").and_then(Value::as_array) else {
        return String::new();
    };
    let links = metrics
        .iter()
        .filter_map(|metric| {
            let key = metric.get("key").and_then(Value::as_str)?;
            let name = metric.get("name").and_then(Value::as_str).unwrap_or(key);
            let class_attr = if key == current {
                r#" class="active""#
            } else {
                ""
            };
            Some(format!(
                r#"<a href="/leaderboard?metric={}"{}>{}</a>"#,
                html_escape(key),
                class_attr,
                html_escape(name)
            ))
        })
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<div class="metric-tabs">{links}</div>"#)
}

fn render_leaderboard_podium(leaderboard: &Value) -> String {
    let entries = leaderboard
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<p class="meta">아직 기록된 게임 전적이 없습니다.</p>"#.to_string();
    }
    let cards = entries
        .iter()
        .take(3)
        .map(|entry| {
            format!(
                r#"<div class="podium-card"><div class="rank">#{}</div><div class="name">{}</div><div class="rating">{}점 · {}랭크</div><div class="meta">{}승 {}패 · 승률 {} · 연승 {}</div></div>"#,
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("rating_rank")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
                safe_text(entry.get("streak_text")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<div class="podium">{cards}</div>"#)
}

fn render_leaderboard_page(leaderboard: &Value, stats_summary: &Value) -> String {
    let body = format!(
        r#"<p class="meta">현재 기준: <span class="pill">{}</span></p>{}{}{}{}"#,
        safe_text(leaderboard.get("metric_name")),
        render_metric_tabs(leaderboard),
        render_leaderboard_podium(leaderboard),
        render_leaderboard_table(leaderboard, false),
        render_stats_cards(stats_summary),
    );
    base_html("마피아 리더보드", &body, false)
}

fn render_leaderboard_table(leaderboard: &Value, compact: bool) -> String {
    let entries = leaderboard
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<p class="meta">아직 기록된 게임 전적이 없습니다.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<tr><td class="num">{}</td><td>{}</td><td class="num">{}점 · {}</td><td>{}승 {}패</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td>{}</td></tr>"#,
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("rating_rank")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
                safe_text(entry.get("streak_text")),
                safe_text(entry.get("games")),
                safe_text(entry.get("mafia_team_games")),
                safe_text(entry.get("playtime")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let title = if compact {
        ""
    } else {
        "<h2>전체 순위</h2>"
    };
    format!(
        r#"<section class="panel">{title}<table><thead><tr><th class="num">순위</th><th>이름</th><th class="num">레이팅/랭크</th><th>승패</th><th class="num">승률</th><th class="num">연승</th><th class="num">판수</th><th class="num">마피아팀</th><th>게임시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

fn render_rating_page() -> String {
    let rank_rows = [
        (
            "C",
            "950점 미만",
            "시작 전 적응 구간입니다. 이기면 크게 오르고, 져도 적게 떨어집니다.",
        ),
        (
            "B",
            "950~1099점",
            "기본 구간입니다. 초기 레이팅 1000점이 여기에 속합니다.",
        ),
        ("A", "1100~1299점", "안정적으로 승리를 쌓은 구간입니다."),
        (
            "S",
            "1300~1549점",
            "상위권 입구입니다. 승리 보상은 줄고 패배 부담은 커집니다.",
        ),
        (
            "SS",
            "1550~1849점",
            "강한 실력 구간입니다. 한 번의 패배 손실이 꽤 큽니다.",
        ),
        (
            "X",
            "1850점 이상",
            "최상위 구간입니다. 유지하려면 꾸준히 이겨야 합니다.",
        ),
    ]
    .into_iter()
    .map(|(rank, range, description)| {
        format!(
            "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>",
            html_escape(rank),
            html_escape(range),
            html_escape(description)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let gain_rows = [
        (
            "낮은 레이팅",
            "승리 보상 큼",
            "패배 손실 작음",
            "점수 복구가 쉽습니다.",
        ),
        (
            "중간 레이팅",
            "표준에 가까움",
            "표준에 가까움",
            "승패와 활약이 균형 있게 반영됩니다.",
        ),
        (
            "높은 레이팅",
            "승리 보상 작음",
            "패배 손실 큼",
            "상위권 유지가 어려워집니다.",
        ),
    ]
    .into_iter()
    .map(|(rating, win, loss, note)| {
        format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            html_escape(rating),
            html_escape(win),
            html_escape(loss),
            html_escape(note)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let role_rows = [
        ("의사", "마피아 공격 치료 성공", "+5"),
        ("경찰", "마피아팀 조사 성공", "+4"),
        ("자경단원", "마피아팀 숙청 처형 성공", "+6"),
        ("용병", "의뢰 처형 성공", "+6"),
        ("성직자", "소생 성공", "+6"),
        ("스파이/마녀/청부업자", "마피아팀 접선", "+4"),
        ("교주", "포교 성공", "+5"),
        ("군인", "방탄 발동", "+5"),
        ("테러리스트", "적팀 반격", "+6"),
        ("도둑", "도벽 실행 + 접선", "+3 / +2"),
        ("최면술사", "비시민 직업 확인", "대상당 +3, 최대 +9"),
        (
            "핵심 능력 미사용",
            "2일차 이후까지 생존했는데 능력 미사용",
            "-2",
        ),
    ]
    .into_iter()
    .map(|(role, action, points)| {
        format!(
            "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td></tr>",
            html_escape(role),
            html_escape(action),
            html_escape(points)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<p class="meta">레이팅은 단순 승률이 아니라 상대 난이도, 현재 점수 구간, 역할 기여를 같이 보는 점수입니다. 낮은 점수에서는 복구가 쉽고, 높은 점수에서는 유지가 어렵게 설계되어 있습니다.</p>
<section class="grid">
  <div class="card"><span>초기 레이팅</span><strong>1000점</strong></div>
  <div class="card"><span>한 판 최대 변동</span><strong>±80점</strong></div>
  <div class="card"><span>역할 보정</span><strong>±14점</strong></div>
  <div class="card"><span>패배팀 최대 상승</span><strong>+5점</strong></div>
  <div class="card"><span>연승 보너스</span><strong>최대 +16점</strong></div>
  <div class="card"><span>첫 사망 패배 완화</span><strong>손실 25% 완화</strong></div>
</section>
<section class="panel">
  <h2>점수가 오르는 기준</h2>
  <p class="meta">기본은 승리입니다. 상대 평균 레이팅이 높을수록, 내 현재 레이팅이 낮을수록 승리 보상이 커집니다. 여기에 역할 기여 점수와 연승 보너스가 더해집니다. 연승 보너스는 이번 승리 후 연승 수가 높을수록 커지고, 최대 +16점까지 반영됩니다.</p>
  <table><thead><tr><th>내 구간</th><th>이겼을 때</th><th>졌을 때</th><th>느낌</th></tr></thead><tbody>{gain_rows}</tbody></table>
</section>
<section class="panel">
  <h2>랭크표</h2>
  <p class="meta">랭크는 현재 레이팅을 보기 쉽게 나눈 표시입니다. 계산 자체에는 영향을 주지 않습니다.</p>
  <table><thead><tr><th>랭크</th><th>레이팅</th><th>설명</th></tr></thead><tbody>{rank_rows}</tbody></table>
</section>
<section class="panel">
  <h2>역할 기여 점수</h2>
  <p class="meta">승패 점수와 별개로 역할을 잘 수행하면 추가 점수를 받습니다. 한 판 역할 보정은 최종적으로 -14점부터 +14점까지만 반영됩니다.</p>
  <table><thead><tr><th>역할</th><th>대표 기여</th><th class="num">점수</th></tr></thead><tbody>{role_rows}</tbody></table>
</section>
<section class="panel">
  <h2>게임 끝나고 보이는 로그 읽는 법</h2>
  <pre>- 닉네임 (의사) 1000 -&gt; 1037 (+37) [팀 +32 / 직업 +5]
  사유: 소속 진영 승리, 마피아 공격 치료 성공 +5, 레이팅 구간 보정 x1.15</pre>
  <p class="meta">팀 점수는 승패와 상대 난이도에서 나온 값이고, 직업 점수는 해당 판 활약에서 나온 값입니다. 두 값을 합친 뒤 상한과 패배팀 제한을 적용하고, 첫 사망자가 패배한 경우 손실을 조금 완화해 최종 변화량이 됩니다.</p>
</section>
<section class="panel">
  <h2>자주 묻는 질문</h2>
  <table><tbody>
    <tr><th>졌는데 왜 점수가 올랐나요?</th><td>역할 활약 점수가 컸기 때문입니다. 다만 패배팀은 한 판에 최대 +5점까지만 오릅니다.</td></tr>
    <tr><th>제일 먼저 죽고 졌는데 왜 덜 깎였나요?</th><td>첫 사망자는 게임에 영향을 줄 기회가 가장 적으므로, 패배 시 최종 손실의 25%를 완화합니다.</td></tr>
    <tr><th>이겼는데 왜 조금 올랐나요?</th><td>이미 레이팅이 높거나, 상대 평균 레이팅이 낮으면 기대 승률이 높아서 보상이 줄어듭니다.</td></tr>
    <tr><th>역할 행동을 실패하면 무조건 감점인가요?</th><td>아닙니다. 능력을 제출했다면 핵심 능력 미사용 감점은 피합니다. 성공 이벤트가 없으면 추가 점수만 없는 구조입니다.</td></tr>
    <tr><th>랭크는 어디서 보나요?</th><td>내정보, 리더보드, 웹 리더보드, API 응답에서 볼 수 있습니다.</td></tr>
  </tbody></table>
</section>"#
    );
    base_html("마피아 레이팅 설명", &body, false)
}

fn render_roles_page() -> String {
    let sections = [
        (
            "시민팀",
            "시민팀은 공개 정보와 밤 행동 결과를 모아 마피아팀을 제거하는 진영입니다.",
        ),
        (
            "마피아팀",
            "마피아팀은 밤 행동과 낮 발언을 맞춰 시민팀의 추론을 흔드는 진영입니다.",
        ),
        (
            "교주팀",
            "교주팀은 포교로 독자 세력을 만들고 숫자 우위를 노리는 진영입니다.",
        ),
        ("중립", "중립 역할은 별도 승리 조건을 중심으로 움직입니다."),
        (
            "상태",
            "상태 항목은 특정 역할이 만드는 임시 상태 설명입니다.",
        ),
    ];
    let mut body = String::from(
        r#"<p class="meta">메인 웹 전용 역할 설명입니다. 디스코드 명령어 설명은 그대로 유지하고, 여기서는 판 운영에 필요한 세부 규칙과 판단 포인트를 길게 보여줍니다.</p>"#,
    );
    for (team, description) in sections {
        let cards = WEB_ROLE_GUIDES
            .iter()
            .filter(|guide| guide.team == team)
            .map(render_role_card)
            .collect::<Vec<_>>()
            .join("");
        if cards.is_empty() {
            continue;
        }
        let count = WEB_ROLE_GUIDES
            .iter()
            .filter(|guide| guide.team == team)
            .count();
        let _ = write!(
            body,
            r#"<section class="panel role-section"><h2>{}<span class="pill">{}개</span></h2><p class="meta">{}</p><div class="role-grid">{}</div></section>"#,
            html_escape(team),
            count,
            html_escape(description),
            cards
        );
    }
    base_html("마피아 역할 설명", &body, false)
}

fn render_role_card(guide: &WebRoleGuide) -> String {
    let tips = guide
        .tips
        .iter()
        .map(|tip| format!("<li>{}</li>", html_escape(tip)))
        .collect::<Vec<_>>()
        .join("");
    let rating_hint = role_rating_hint(guide.role);
    let hover_text = format!(
        "{}: {} 레이팅 요소: {} 주의: {}",
        guide.role.value(),
        guide.summary,
        rating_hint,
        guide.caution
    );
    format!(
        r#"<article class="role-card"><div class="role-head"><div class="role-title"><h3>{}</h3><span class="role-help" tabindex="0" aria-label="{} 상세 설명" data-tip="{}">?</span></div><div class="role-tags"><span class="pill">{}</span><span class="pill">{}</span></div></div><p class="role-summary">{}</p><p class="role-rating"><strong>레이팅:</strong> {}</p><h4>운영 포인트</h4><ul>{}</ul><p class="role-note"><strong>주의:</strong> {}</p></article>"#,
        html_escape(guide.role.value()),
        html_escape(guide.role.value()),
        html_escape(&hover_text),
        html_escape(guide.team),
        html_escape(guide.kind),
        html_escape(guide.summary),
        html_escape(rating_hint),
        tips,
        html_escape(guide.caution)
    )
}

fn role_rating_hint(role: Role) -> &'static str {
    match role {
        Role::Citizen => "생존 승리, 공개 정보 정리, 투표 기여를 중심으로 평가합니다.",
        Role::Police => "조사 결과 공개와 생존한 정보 유지 기여를 크게 봅니다.",
        Role::Doctor => "치료 성공, 핵심 직업 보호, 보호 동선 판단을 평가합니다.",
        Role::Nurse => "보호 보조와 핵심 직업 생존 지원을 평가합니다.",
        Role::Agent => "수사 결과로 의심 대상을 좁힌 기여를 평가합니다.",
        Role::Vigilante => "정확한 조사와 처형 압박, 오처형 회피를 평가합니다.",
        Role::Inspector => "같은 팀 수사 성공, 직업 정보 공유, 정체 공개 타이밍을 평가합니다.",
        Role::Reporter => "취재 공개 정보가 투표 판단에 준 기여를 평가합니다.",
        Role::Hacker => "행동 정보로 거짓 직업 주장이나 밤 동선을 잡은 기여를 평가합니다.",
        Role::Detective => "추적 결과를 누적해 행동 모순을 밝힌 기여를 평가합니다.",
        Role::Shaman => "사망자 정보와 공개 추론을 연결한 기여를 평가합니다.",
        Role::Priest => "부활 또는 정화 선택으로 판세를 바꾼 기여를 평가합니다.",
        Role::Soldier => "방탄 생존과 공격 유도 정보 제공을 평가합니다.",
        Role::Gangster => "투표 제어와 핵심 타이밍 방해 기여를 평가합니다.",
        Role::Prophet => "장기 생존과 예언 타이밍으로 만든 확정 정보를 평가합니다.",
        Role::Psychologist => "관계 분석으로 팀 구도를 좁힌 기여를 평가합니다.",
        Role::Hypnotist => "최면 누적과 해제 타이밍으로 얻은 판별 정보를 평가합니다.",
        Role::Mercenary => "의뢰인 보호, 의뢰 달성 뒤 처형 판단을 평가합니다.",
        Role::Lover => "연인 생존 연계와 공개 타이밍 조절을 평가합니다.",
        Role::Mafia => "밤 처형 성공, 낮 발언 교란, 팀 승리 기여를 평가합니다.",
        Role::Spy => "접선, 정보 전달, 시민팀 추론 방해를 평가합니다.",
        Role::Contractor => "청부 표적 압박과 마피아팀 승리 보조를 평가합니다.",
        Role::Thief => "탈취한 능력을 독립적으로 활용한 기여를 평가합니다.",
        Role::Witch => "저주로 시민팀 행동을 흔든 기여를 평가합니다.",
        Role::Scientist => "부활 타이밍과 마피아팀 생존 변수 창출을 평가합니다.",
        Role::Madam => "접대, 접선, 밤 대화 합류 후 정보 공유를 평가합니다.",
        Role::Graverobber => "도굴한 역할의 가치와 이후 행동 기여를 평가합니다.",
        Role::Godfather => "마피아팀 지휘, 은폐, 처형 우선순위 판단을 평가합니다.",
        Role::Villain => "마피아팀 보조와 낮 발언 교란 기여를 평가합니다.",
        Role::CultLeader => "포교 성공, 교주팀 생존, 숫자 우위 운영을 평가합니다.",
        Role::Fanatic => "교주팀 보조와 포교 이후 정보 교란 기여를 평가합니다.",
        Role::Joker => "단독 승리 조건 달성과 처형 유도 성공을 크게 평가합니다.",
        Role::Politician => "찬반투표와 공개 정치 운영으로 만든 판세 기여를 평가합니다.",
        Role::Judge => "처형 판정으로 확정 구도를 만든 기여를 평가합니다.",
        Role::Terrorist => "교환 압박과 희생 타이밍으로 만든 판세 기여를 평가합니다.",
        Role::Frog => "개구리 상태에서 생존하거나 정보 혼선을 관리한 기여를 평가합니다.",
    }
}

fn render_api_docs_page(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let api_url = format!("{base_url}/api");
    let protected_api_url = format!("{api_url}/v1");
    let public_endpoints = [
        ("GET /health", "봇 웹 서버가 살아 있는지 확인합니다."),
        (
            "GET /api/status",
            "봇 연결 상태, 진행 중 게임, 공개 설정 요약을 반환합니다.",
        ),
        ("GET /api/games", "진행 중 게임 목록만 반환합니다."),
        (
            "GET /api/settings",
            "공개 가능한 게임 설정 요약을 반환합니다.",
        ),
        ("GET /api/stats", "전적 요약 정보를 반환합니다."),
        (
            "GET /api/leaderboard",
            "레이팅 기준 리더보드를 반환합니다. 각 항목에 rating_rank가 포함됩니다.",
        ),
        (
            "GET /api/leaderboard/{metric}",
            "wins, streak, winrate, games, mafia, playtime, rating 기준 리더보드를 반환합니다. 각 항목에 rating_rank와 win_streak가 포함됩니다.",
        ),
    ];
    let protected_endpoints = [
        (
            "GET /api/v1/me",
            "API 키 정보와 서버 범위를 반환합니다. API 키 필요.",
        ),
        (
            "GET /api/v1/config",
            "게임 설정 요약을 반환합니다. API 키 필요.",
        ),
        ("GET /api/v1/stats", "전적 요약을 반환합니다. API 키 필요."),
        (
            "GET /api/v1/stats/leaderboard",
            "Laravel-friendly leaderboard JSON. Query: sort, limit. API key required.",
        ),
        (
            "GET /stats/leaderboard",
            "Alias for Laravel spec. Query: sort, limit. API key required.",
        ),
        (
            "GET /api/v1/stats/user/{user_id}",
            "Laravel-friendly user profile stats. API key required.",
        ),
        (
            "GET /api/v1/stats/user/{user_id}/games",
            "Laravel-friendly user game history. Query: page, per_page. API key required.",
        ),
        (
            "GET /stats/user/{user_id}",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /stats/user/{user_id}/games",
            "Alias for Laravel spec. Query: page, per_page. API key required.",
        ),
        (
            "GET /api/v1/leaderboard/{metric}",
            "보호 리더보드를 반환합니다. streak 정렬과 win_streak 필드가 포함됩니다. API 키 필요.",
        ),
        (
            "GET /api/v1/games",
            "키 발급 서버의 진행 중 게임을 반환합니다. API 키 필요.",
        ),
        (
            "GET /api/v1/games/recent",
            "Laravel-friendly recent completed games. Query: page, limit/per_page. API key required.",
        ),
        (
            "GET /games/recent",
            "Alias for Laravel spec. Query: page, limit/per_page. API key required.",
        ),
        (
            "GET /api/v1/game/{game_key}",
            "Laravel-friendly completed game summary by replay game_key. API key required.",
        ),
        (
            "GET /api/v1/game/{game_key}/result",
            "Laravel-friendly completed game result summary. API key required.",
        ),
        (
            "GET /api/v1/game/{game_key}/events",
            "Laravel-friendly replay timeline events. API key required.",
        ),
        (
            "GET /game/{game_key}",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /game/{game_key}/result",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /game/{game_key}/events",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /api/v1/games/{guild_id}",
            "참가자, 직업, 단계, 타이머를 포함한 게임 상세를 반환합니다. API 키 필요.",
        ),
        (
            "GET /api/v1/games/{guild_id}/replay",
            "Replay JSON with participants, votes, role actions, phase results, and rating log. API key required.",
        ),
        (
            "POST /api/v1/games/{guild_id}/actions",
            "JSON action: skip_day, extend_day 또는 stop. API 키 필요.",
        ),
        (
            "GET /api/v1/replays",
            "Recent replay summaries for the API key guild. Includes active game and completed games.",
        ),
        (
            "GET /api/v1/replays/{game_key}",
            "Full replay JSON by game_key. API key required.",
        ),
        (
            "GET /api/v1/recruitments/{guild_id}",
            "모집 인원과 역할 구성을 반환합니다. API 키 필요.",
        ),
        (
            "POST /api/v1/recruitments/{guild_id}/actions",
            "JSON action: start 또는 cancel. API 키 필요.",
        ),
    ];
    let render_rows = |endpoints: &[(&str, &str)]| {
        endpoints
            .iter()
            .map(|(path, desc)| {
                format!(
                    r#"<div class="endpoint"><code>{}</code><span>{}</span></div>"#,
                    html_escape(path),
                    html_escape(desc)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };
    let public_rows = render_rows(&public_endpoints);
    let protected_rows = render_rows(&protected_endpoints);
    let body = format!(
        r#"<p class="meta">기본 API 주소는 <code>{api_url}</code>입니다. 모든 응답은 JSON이며, <code>limit</code>은 1~50 범위입니다.</p>
<section class="panel"><h2>인증</h2><p>관리자는 <code>/마피아웹설정</code>에서 서버 전용 API 키를 발급합니다. 보호 API는 키 발급 서버의 데이터와 작업만 허용합니다.</p><pre>X-API-Key: mfr_...
Authorization: Bearer mfr_...</pre></section>
<section class="panel"><h2>공개 조회 API</h2>{public_rows}</section>
<section class="panel"><h2>보호 관리 API</h2>{protected_rows}</section>
<section class="panel"><h2>관리 작업 본문</h2><pre>POST {protected_api_url}/games/{{guild_id}}/actions
{{"action":"skip_day"}}   # 낮 토론 즉시 종료
{{"action":"extend_day"}} # 연장 투표 중 1분 연장 승인
{{"action":"stop"}}       # 게임 종료

POST {protected_api_url}/recruitments/{{guild_id}}/actions
{{"action":"start"}}      # 최소 인원 충족 시 즉시 시작
{{"action":"cancel"}}     # 모집 취소</pre></section>
<section class="panel"><h2>응답 코드</h2><pre>200 성공 · 400 잘못된 요청 · 401 키 없음/오류 · 403 다른 서버 키 · 404 대상 없음 · 409 현재 상태에서 작업 불가</pre></section>
<section class="panel"><h2>호출 예시</h2><pre>curl -H "X-API-Key: mfr_..." {protected_api_url}/games/123

curl -X POST -H "Authorization: Bearer mfr_..." -H "Content-Type: application/json" \
  -d '{{"action":"skip_day"}}' {protected_api_url}/games/123/actions</pre></section>"#,
        api_url = html_escape(&api_url),
        protected_api_url = html_escape(&protected_api_url),
    );
    base_html("마피아 봇 API 문서", &body, false)
}

fn render_field(field: WebConfigField, config: &BotConfig) -> String {
    let field_id = format!("field_{}", field.name);
    let label = html_escape(field.label);
    match field.kind {
        WebFieldKind::Bool => {
            let checked = if config_value(config, field.name) == "true" {
                " checked"
            } else {
                ""
            };
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}</span><input type="checkbox" id="{field_id}" name="{}"{checked}></label>"#,
                field.name
            )
        }
        WebFieldKind::Int => {
            let min_attr = field
                .min_value
                .map(|value| format!(r#" min="{value}""#))
                .unwrap_or_default();
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}</span><input type="number" id="{field_id}" name="{}" value="{}"{min_attr} required></label>"#,
                field.name,
                html_escape(&config_value(config, field.name))
            )
        }
        WebFieldKind::Text => format!(
            r#"<label class="row" for="{field_id}"><span>{label}</span><input type="text" id="{field_id}" name="{}" value="{}" required></label>"#,
            field.name,
            html_escape(&config_value(config, field.name))
        ),
        WebFieldKind::IntList => {
            let value = config
                .blacklist_user_ids
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}<br><small>한 줄에 하나씩, 또는 쉼표/공백으로 구분</small></span><textarea id="{field_id}" name="{}">{}</textarea></label>"#,
                field.name,
                html_escape(&value)
            )
        }
    }
}

fn render_message_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>{}</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p>{}</p>
</main>
</div>
</body>
</html>"#,
        html_escape(title),
        render_page_header(title, false),
        html_escape(message)
    )
}

fn expired_page() -> String {
    render_message_page(
        "🔒 링크가 만료되었습니다",
        "이 링크는 더 이상 유효하지 않습니다. 디스코드에서 /마피아웹설정 명령어를 다시 실행해 새 링크를 발급받으세요.",
    )
}

fn saved_page() -> String {
    render_message_page(
        "✅ 설정을 저장했습니다",
        "마피아 게임 설정이 반영되었습니다. 이 창은 닫으셔도 됩니다.",
    )
}

fn config_value(config: &BotConfig, name: &str) -> String {
    match name {
        "participant_role" => config.participant_role.clone(),
        "manager_role" => config.manager_role.clone(),
        "game_enabled" => config.game_enabled.to_string(),
        "max_player_count" => config.max_player_count.to_string(),
        "night_seconds" => config.night_seconds.to_string(),
        "discussion_seconds" => config.discussion_seconds.to_string(),
        "vote_seconds" => config.vote_seconds.to_string(),
        "chat_slowmode_seconds" => config.chat_slowmode_seconds.to_string(),
        "default_mafia_count" => config.default_mafia_count.to_string(),
        "default_doctor_count" => config.default_doctor_count.to_string(),
        "default_police_count" => config.default_police_count.to_string(),
        "default_joker_count" => config.default_joker_count.to_string(),
        "citizen_special_count" => config.citizen_special_count.to_string(),
        "mafia_special_count" => config.mafia_special_count.to_string(),
        "neutral_special_count" => config.neutral_special_count.to_string(),
        "reveal_death_roles" => config.reveal_death_roles.to_string(),
        "reveal_public_police_status" => config.reveal_public_police_status.to_string(),
        "reveal_morning_mafia_count" => config.reveal_morning_mafia_count.to_string(),
        "show_confirmation_vote_counts" => config.show_confirmation_vote_counts.to_string(),
        "anonymous_mode" => config.anonymous_mode.to_string(),
        "anonymous_name_mode" => config.anonymous_name_mode.clone(),
        "use_agent" => config.use_agent.to_string(),
        "use_vigilante" => config.use_vigilante.to_string(),
        "enable_detective" => config.enable_detective.to_string(),
        "enable_inspector" => config.enable_inspector.to_string(),
        "enable_graverobber" => config.enable_graverobber.to_string(),
        "enable_spy" => config.enable_spy.to_string(),
        "enable_contractor" => config.enable_contractor.to_string(),
        "enable_witch" => config.enable_witch.to_string(),
        "enable_scientist" => config.enable_scientist.to_string(),
        "enable_madam" => config.enable_madam.to_string(),
        "enable_godfather" => config.enable_godfather.to_string(),
        "enable_joker" => config.enable_joker.to_string(),
        "enable_politician" => config.enable_politician.to_string(),
        "enable_judge" => config.enable_judge.to_string(),
        "enable_reporter" => config.enable_reporter.to_string(),
        "enable_hacker" => config.enable_hacker.to_string(),
        "enable_terrorist" => config.enable_terrorist.to_string(),
        "enable_lover" => config.enable_lover.to_string(),
        "enable_shaman" => config.enable_shaman.to_string(),
        "enable_priest" => config.enable_priest.to_string(),
        "enable_soldier" => config.enable_soldier.to_string(),
        "enable_nurse" => config.enable_nurse.to_string(),
        "enable_gangster" => config.enable_gangster.to_string(),
        "enable_prophet" => config.enable_prophet.to_string(),
        "enable_psychologist" => config.enable_psychologist.to_string(),
        "enable_hypnotist" => config.enable_hypnotist.to_string(),
        "enable_mercenary" => config.enable_mercenary.to_string(),
        "enable_thief" => config.enable_thief.to_string(),
        "enable_cult_team" => config.enable_cult_team.to_string(),
        "blacklist_user_ids" => config
            .blacklist_user_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_form_updates(body: &str) -> std::result::Result<HashMap<String, String>, String> {
    let raw_form = parse_urlencoded(body);
    let mut updates = HashMap::new();
    for field in WEB_CONFIG_FIELDS {
        if matches!(field.kind, WebFieldKind::Bool) {
            updates.insert(
                field.name.to_string(),
                raw_form.contains_key(field.name).to_string(),
            );
            continue;
        }
        let raw_value = raw_form
            .get(field.name)
            .ok_or_else(|| format!("'{}' 값이 비어 있습니다.", field.label))?;
        let text_value = raw_value.trim();
        if matches!(field.kind, WebFieldKind::IntList) && text_value.is_empty() {
            updates.insert(field.name.to_string(), String::new());
            continue;
        }
        if text_value.is_empty() {
            return Err(format!("'{}' 값이 비어 있습니다.", field.label));
        }
        if matches!(field.kind, WebFieldKind::Int) {
            let parsed = text_value
                .parse::<u64>()
                .map_err(|_| format!("'{}' 값은 숫자여야 합니다.", field.label))?;
            if let Some(min_value) = field.min_value
                && parsed < min_value
            {
                return Err(format!(
                    "'{}' 값은 {min_value} 이상이어야 합니다.",
                    field.label
                ));
            }
        }
        updates.insert(field.name.to_string(), text_value.to_string());
    }
    Ok(updates)
}

fn apply_updates(
    config: &mut BotConfig,
    updates: &HashMap<String, String>,
) -> std::result::Result<(), String> {
    let previous = config.clone();
    for field in WEB_CONFIG_FIELDS {
        let value = updates
            .get(field.name)
            .ok_or_else(|| format!("'{}' 값이 비어 있습니다.", field.label))?;
        match field.kind {
            WebFieldKind::Bool => set_bool(config, field.name, value == "true")?,
            WebFieldKind::Text => set_text(config, field.name, value.clone())?,
            WebFieldKind::Int => set_int(config, field.name, value.parse::<u64>().unwrap_or(0))?,
            WebFieldKind::IntList => set_int_list(config, field.name, value)?,
        }
    }
    if let Err(error) = validate_config(config) {
        *config = previous;
        return Err(error);
    }
    Ok(())
}

fn set_bool(config: &mut BotConfig, name: &str, value: bool) -> std::result::Result<(), String> {
    match name {
        "game_enabled" => config.game_enabled = value,
        "reveal_death_roles" => config.reveal_death_roles = value,
        "reveal_public_police_status" => config.reveal_public_police_status = value,
        "reveal_morning_mafia_count" => config.reveal_morning_mafia_count = value,
        "show_confirmation_vote_counts" => config.show_confirmation_vote_counts = value,
        "anonymous_mode" => config.anonymous_mode = value,
        "use_agent" => config.use_agent = value,
        "use_vigilante" => config.use_vigilante = value,
        "enable_detective" => config.enable_detective = value,
        "enable_inspector" => config.enable_inspector = value,
        "enable_graverobber" => config.enable_graverobber = value,
        "enable_spy" => config.enable_spy = value,
        "enable_contractor" => config.enable_contractor = value,
        "enable_witch" => config.enable_witch = value,
        "enable_scientist" => config.enable_scientist = value,
        "enable_madam" => config.enable_madam = value,
        "enable_godfather" => config.enable_godfather = value,
        "enable_joker" => config.enable_joker = value,
        "enable_politician" => config.enable_politician = value,
        "enable_judge" => config.enable_judge = value,
        "enable_reporter" => config.enable_reporter = value,
        "enable_hacker" => config.enable_hacker = value,
        "enable_terrorist" => config.enable_terrorist = value,
        "enable_lover" => config.enable_lover = value,
        "enable_shaman" => config.enable_shaman = value,
        "enable_priest" => config.enable_priest = value,
        "enable_soldier" => config.enable_soldier = value,
        "enable_nurse" => config.enable_nurse = value,
        "enable_gangster" => config.enable_gangster = value,
        "enable_prophet" => config.enable_prophet = value,
        "enable_psychologist" => config.enable_psychologist = value,
        "enable_hypnotist" => config.enable_hypnotist = value,
        "enable_mercenary" => config.enable_mercenary = value,
        "enable_thief" => config.enable_thief = value,
        "enable_cult_team" => config.enable_cult_team = value,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn set_text(config: &mut BotConfig, name: &str, value: String) -> std::result::Result<(), String> {
    match name {
        "participant_role" => config.participant_role = value,
        "manager_role" => config.manager_role = value,
        "anonymous_name_mode" => config.anonymous_name_mode = value,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn set_int(config: &mut BotConfig, name: &str, value: u64) -> std::result::Result<(), String> {
    match name {
        "max_player_count" => config.max_player_count = value as u32,
        "night_seconds" => config.night_seconds = value,
        "discussion_seconds" => config.discussion_seconds = value,
        "vote_seconds" => config.vote_seconds = value,
        "chat_slowmode_seconds" => config.chat_slowmode_seconds = value,
        "default_mafia_count" => config.default_mafia_count = value as u32,
        "default_doctor_count" => config.default_doctor_count = value as u32,
        "default_police_count" => config.default_police_count = value as u32,
        "default_joker_count" => config.default_joker_count = value as u32,
        "citizen_special_count" => config.citizen_special_count = value as u32,
        "mafia_special_count" => config.mafia_special_count = value as u32,
        "neutral_special_count" => config.neutral_special_count = value as u32,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn set_int_list(
    config: &mut BotConfig,
    name: &str,
    value: &str,
) -> std::result::Result<(), String> {
    match name {
        "blacklist_user_ids" => {
            let normalized = value.replace(',', " ");
            let mut values = Vec::new();
            for chunk in normalized.split_whitespace() {
                values.push(chunk.parse::<u64>().map_err(|_| {
                    "블랙리스트 유저 ID 목록에는 숫자 ID만 입력할 수 있습니다.".to_string()
                })?);
            }
            values.sort_unstable();
            values.dedup();
            config.blacklist_user_ids = values;
        }
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn validate_config(config: &BotConfig) -> std::result::Result<(), String> {
    if config.default_mafia_count < 1 {
        return Err("마피아는 최소 1명이어야 합니다.".to_string());
    }
    if !can_fill_special_slots(
        config,
        CITIZEN_SPECIAL_ROLES,
        config.citizen_special_count as usize,
    ) {
        return Err(
            "활성화된 시민 특수 역할로 설정한 인원 수를 구성할 수 없습니다. 연인은 2명으로 계산됩니다."
                .to_string(),
        );
    }
    let mafia_enabled = enabled_special_count(config, MAFIA_SPECIAL_ROLES);
    if config.mafia_special_count as usize > mafia_enabled {
        return Err("마피아 특수룰 수가 활성화된 마피아 특수 역할보다 많습니다.".to_string());
    }
    let neutral_enabled = enabled_special_count(config, NEUTRAL_SPECIAL_ROLES);
    if config.neutral_special_count as usize > neutral_enabled {
        return Err("중립 특수룰 수가 활성화된 중립 특수 역할보다 많습니다.".to_string());
    }
    if config.mafia_special_count > config.default_mafia_count {
        return Err(format!(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. 현재 마피아 {}명, 마피아 특수 {}명입니다.",
            config.default_mafia_count, config.mafia_special_count
        ));
    }
    if config
        .default_mafia_count
        .saturating_sub(config.mafia_special_count)
        < 1
    {
        return Err("접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. 일반 마피아가 최소 1명 필요합니다.".to_string());
    }
    let minimum_players = minimum_player_count(config);
    let max_players = if config.max_player_count == 0 {
        MAX_GAME_PLAYERS
    } else {
        (config.max_player_count as usize).min(MAX_GAME_PLAYERS)
    };
    if max_players < minimum_players {
        return Err(format!(
            "현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."
        ));
    }
    Ok(())
}

fn enabled_special_count(config: &BotConfig, roles: &[Role]) -> usize {
    roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .count()
}

fn special_role_enabled(config: &BotConfig, role: Role) -> bool {
    match role {
        Role::Inspector => config.enable_inspector,
        Role::Detective => config.enable_detective,
        Role::Graverobber => config.enable_graverobber,
        Role::Spy => config.enable_spy,
        Role::Contractor => config.enable_contractor,
        Role::Witch => config.enable_witch,
        Role::Scientist => config.enable_scientist,
        Role::Madam => config.enable_madam,
        Role::Godfather => config.enable_godfather,
        Role::Joker => config.enable_joker,
        Role::Politician => config.enable_politician,
        Role::Judge => config.enable_judge,
        Role::Reporter => config.enable_reporter,
        Role::Hacker => config.enable_hacker,
        Role::Terrorist => config.enable_terrorist,
        Role::Lover => config.enable_lover,
        Role::Shaman => config.enable_shaman,
        Role::Priest => config.enable_priest,
        Role::Soldier => config.enable_soldier,
        Role::Nurse => config.enable_nurse,
        Role::Gangster => config.enable_gangster,
        Role::Prophet => config.enable_prophet,
        Role::Psychologist => config.enable_psychologist,
        Role::Hypnotist => config.enable_hypnotist,
        Role::Mercenary => config.enable_mercenary,
        Role::Thief => config.enable_thief,
        _ => true,
    }
}

fn special_role_player_count(role: Role) -> usize {
    if role == Role::Lover { 2 } else { 1 }
}

fn can_fill_special_slots(config: &BotConfig, roles: &[Role], target_slots: usize) -> bool {
    let mut possible = vec![false; target_slots + 1];
    possible[0] = true;
    for slots in roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .map(|role| special_role_player_count(*role))
    {
        if slots > target_slots {
            continue;
        }
        for total in (slots..=target_slots).rev() {
            possible[total] |= possible[total - slots];
        }
    }
    possible[target_slots]
}

fn selected_special_player_count(config: &BotConfig, roles: &[Role], count: u32) -> usize {
    let mut candidates = roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .map(|role| special_role_player_count(*role))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    candidates.into_iter().take(count as usize).sum()
}

fn minimum_player_count(config: &BotConfig) -> usize {
    let cult_count = if config.enable_cult_team { 2 } else { 0 };
    let selected_count = config
        .default_mafia_count
        .saturating_sub(config.mafia_special_count) as usize
        + config.default_doctor_count as usize
        + config.default_police_count as usize
        + config.citizen_special_count as usize
        + selected_special_player_count(config, MAFIA_SPECIAL_ROLES, config.mafia_special_count)
        + selected_special_player_count(
            config,
            NEUTRAL_SPECIAL_ROLES,
            config.neutral_special_count,
        )
        + cult_count;
    3.max(selected_count)
        .max(config.default_mafia_count as usize * 2 + 1)
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

async fn read_http_request<S>(stream: &mut S) -> Result<HttpRequest>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = Vec::with_capacity(8192);
    let mut temp = [0u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;
    loop {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&temp[..read]);
        if header_end.is_none()
            && let Some(index) = find_header_end(&buffer)
        {
            header_end = Some(index);
            let headers = String::from_utf8_lossy(&buffer[..index]);
            content_length = parse_content_length(&headers).unwrap_or(0);
        }
        if let Some(index) = header_end
            && buffer.len() >= index + 4 + content_length
        {
            break;
        }
        if buffer.len() > 128 * 1024 {
            bail!("요청이 너무 큽니다.");
        }
    }
    let Some(index) = header_end else {
        bail!("HTTP 헤더를 찾지 못했습니다.");
    };
    let raw_headers = String::from_utf8_lossy(&buffer[..index]).to_string();
    let mut first_line = raw_headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = first_line.next().unwrap_or_default().to_string();
    let path = first_line.next().unwrap_or_default().to_string();
    let body_start = index + 4;
    let body_end = (body_start + content_length).min(buffer.len());
    let body = String::from_utf8_lossy(&buffer[body_start..body_end]).to_string();
    Ok(HttpRequest {
        method,
        path,
        headers: parse_http_headers(&raw_headers),
        body,
    })
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn json_response(value: Value) -> String {
    json_response_with_status("200 OK", value)
}

fn json_error(status: &str, message: &str) -> String {
    json_response_with_status(status, json!({"error": message}))
}

fn json_response_with_status(status: &str, value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-API-Key\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn api_options_response() -> String {
    "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-API-Key\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Max-Age: 600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

fn parse_http_headers(headers: &str) -> HashMap<String, String> {
    headers
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

fn parse_urlencoded(body: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for pair in body.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    values
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    output.push(hex);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BotConfig {
        BotConfig {
            game_enabled: true,
            participant_role: "participant".to_string(),
            manager_role: "manager".to_string(),
            default_mafia_count: 2,
            default_doctor_count: 1,
            default_police_count: 1,
            default_joker_count: 0,
            max_player_count: 0,
            night_seconds: 60,
            discussion_seconds: 60,
            vote_seconds: 30,
            chat_slowmode_seconds: 3,
            reveal_death_roles: true,
            reveal_public_police_status: true,
            reveal_morning_mafia_count: true,
            show_confirmation_vote_counts: true,
            citizen_special_count: 0,
            mafia_special_count: 0,
            neutral_special_count: 0,
            enable_detective: true,
            enable_inspector: true,
            enable_graverobber: true,
            enable_spy: true,
            enable_contractor: true,
            enable_witch: true,
            enable_scientist: true,
            enable_madam: true,
            enable_godfather: true,
            enable_joker: true,
            enable_politician: true,
            enable_judge: true,
            enable_reporter: true,
            enable_hacker: true,
            enable_terrorist: true,
            enable_lover: true,
            enable_shaman: true,
            enable_priest: true,
            enable_soldier: true,
            enable_nurse: true,
            enable_gangster: true,
            enable_prophet: true,
            enable_psychologist: true,
            enable_hypnotist: true,
            enable_mercenary: true,
            enable_thief: true,
            enable_cult_team: false,
            use_agent: false,
            use_vigilante: false,
            anonymous_mode: false,
            anonymous_name_mode: "animal".to_string(),
            blacklist_user_ids: Vec::new(),
        }
    }

    fn test_state() -> WebSettingsState {
        WebSettingsState {
            config: Arc::new(RwLock::new(test_config())),
            config_path: Arc::new(PathBuf::from("unused-config.json")),
            api_keys: Arc::new(RwLock::new(ApiKeyStore::default())),
            api_keys_path: Arc::new(PathBuf::from("unused-api-keys.json")),
            stats: Arc::new(RwLock::new(StatsFile::default())),
            games: Arc::new(DashMap::new()),
            completed_replays: Arc::new(RwLock::new(VecDeque::new())),
            recruitments: Arc::new(DashMap::new()),
            sessions: Arc::new(DashMap::new()),
            started_at: Instant::now(),
            bot_name: "bot".to_string(),
            guild_count: 1,
            base_url: "https://mafia.example".to_string(),
        }
    }

    fn api_request(method: &str, path: &str, key: Option<(&str, &str)>) -> HttpRequest {
        let mut headers = HashMap::new();
        if let Some((name, value)) = key {
            headers.insert(name.to_ascii_lowercase(), value.to_string());
        }
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body: String::new(),
        }
    }

    fn updates_for(config: &BotConfig) -> HashMap<String, String> {
        WEB_CONFIG_FIELDS
            .iter()
            .map(|field| (field.name.to_string(), config_value(config, field.name)))
            .collect()
    }

    fn form_body_for(config: &BotConfig) -> String {
        WEB_CONFIG_FIELDS
            .iter()
            .filter_map(|field| {
                let value = config_value(config, field.name);
                if matches!(field.kind, WebFieldKind::Bool) && value != "true" {
                    None
                } else {
                    Some(format!("{}={}", field.name, value.replace('\n', "%0A")))
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    #[test]
    fn rejects_all_special_mafia_and_rolls_back() {
        let mut config = test_config();
        let mut updates = updates_for(&config);
        updates.insert("default_mafia_count".to_string(), "1".to_string());
        updates.insert("mafia_special_count".to_string(), "1".to_string());

        assert!(apply_updates(&mut config, &updates).is_err());
        assert_eq!(config.default_mafia_count, 2);
        assert_eq!(config.mafia_special_count, 0);
    }

    #[test]
    fn lover_does_not_inflate_web_minimum() {
        let mut config = test_config();
        config.default_mafia_count = 1;
        config.citizen_special_count = 1;
        config.max_player_count = 4;

        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn lover_uses_two_citizen_special_slots() {
        let mut config = test_config();
        config.citizen_special_count = 2;
        config.enable_detective = false;
        config.enable_graverobber = false;
        config.enable_politician = false;
        config.enable_judge = false;
        config.enable_reporter = false;
        config.enable_hacker = false;
        config.enable_terrorist = false;
        config.enable_shaman = false;
        config.enable_priest = false;
        config.enable_soldier = false;
        config.enable_nurse = false;
        config.enable_gangster = false;
        config.enable_prophet = false;
        config.enable_psychologist = false;
        config.enable_hypnotist = false;
        config.enable_mercenary = false;

        let roles = crate::channel::choose_special_roles(&config).unwrap();
        let role_counts = crate::channel::selected_role_counts(&config, &roles).unwrap();

        assert_eq!(roles, vec![Role::Lover]);
        assert_eq!(role_counts.get(&Role::Lover), Some(&2));
        assert_eq!(crate::channel::minimum_player_count(&role_counts), 6);
    }

    #[tokio::test]
    async fn invalid_post_returns_error_without_lock_deadlock() {
        let config = test_config();
        let state = test_state();
        let token = "test-token".to_string();
        state.sessions.insert(
            token.clone(),
            WebSettingsSession {
                guild_id: 1,
                user_id: 2,
                user_label: "tester".to_string(),
                expires_at: Instant::now() + Duration::from_secs(60),
            },
        );
        let body = form_body_for(&config)
            .replace("default_mafia_count=2", "default_mafia_count=1")
            .replace("mafia_special_count=0", "mafia_special_count=1");

        let response = tokio::time::timeout(
            Duration::from_secs(1),
            route_request(
                &state,
                HttpRequest {
                    method: "POST".to_string(),
                    path: format!("{WEB_SETTINGS_PATH}/{token}"),
                    headers: HashMap::new(),
                    body,
                },
            ),
        )
        .await
        .expect("invalid settings POST should not deadlock");

        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    }

    #[tokio::test]
    async fn public_status_api_returns_json() {
        let state = test_state();
        let response = route_request(&state, api_request("GET", "/api/status", None)).await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Type: application/json"));
        assert!(response.contains(r#""base_url":"https://mafia.example/api""#));
    }

    #[tokio::test]
    async fn protected_api_requires_key() {
        let state = test_state();
        let response = route_request(&state, api_request("GET", "/api/v1/me", None)).await;

        assert!(response.starts_with("HTTP/1.1 401 Unauthorized"));
        assert!(response.contains("missing API key"));
    }

    #[tokio::test]
    async fn protected_api_accepts_bearer_key() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "integration".to_string())
        };
        let response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/me",
                Some(("Authorization", &format!("Bearer {raw_key}"))),
            ),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("integration"));
    }

    #[tokio::test]
    async fn protected_api_blocks_other_guild() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "guild-one".to_string())
        };
        let response = route_request(
            &state,
            api_request("GET", "/api/v1/games/2", Some(("X-API-Key", &raw_key))),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
    }

    #[tokio::test]
    async fn protected_replay_api_returns_completed_replay() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "replay".to_string())
        };
        state.completed_replays.write().await.push_front(json!({
            "game_key": "game-1",
            "guild_id": 1,
            "channel_id": 10,
            "status": "completed",
            "phase": "종료",
            "phase_key": "Ended",
            "day_number": 3,
            "elapsed_seconds": 123,
            "winner": "시민",
            "winner_key": "Citizen",
            "participants": [],
            "events": [{"kind": "day_vote"}],
            "rating_log": [],
        }));

        let list_response = route_request(
            &state,
            api_request("GET", "/api/v1/replays", Some(("X-API-Key", &raw_key))),
        )
        .await;
        assert!(list_response.starts_with("HTTP/1.1 200 OK"));
        assert!(list_response.contains("game-1"));
        assert!(list_response.contains(r#""event_count":1"#));

        let replay_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/replays/game-1",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(replay_response.starts_with("HTTP/1.1 200 OK"));
        assert!(replay_response.contains("day_vote"));

        let guild_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/games/1/replay",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(guild_response.starts_with("HTTP/1.1 200 OK"));
        assert!(guild_response.contains("game-1"));
    }

    #[tokio::test]
    async fn protected_compatible_stats_and_replay_api_match_laravel_contract() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "laravel".to_string())
        };
        let mut roles = HashMap::new();
        roles.insert("mafia".to_string(), 2);
        state.stats.write().await.users.insert(
            "11".to_string(),
            stats::PlayerStats {
                name: "Alice".to_string(),
                games: 3,
                wins: 2,
                losses: 1,
                rating: 1120,
                roles,
                ..Default::default()
            },
        );
        state.completed_replays.write().await.push_front(json!({
            "game_key": "game-1",
            "game_id": "game-1",
            "guild_id": 1,
            "channel_id": 10,
            "status": "completed",
            "started_at": "2026-07-08T21:00:00Z",
            "ended_at": "2026-07-08T21:45:00Z",
            "phase": "Ended",
            "phase_key": "Ended",
            "day_number": 3,
            "elapsed_seconds": 123,
            "winner": "Citizen",
            "winner_key": "Citizen",
            "participants": [
                {
                    "user_id": 11,
                    "name": "Alice",
                    "initial_role": "Mafia",
                    "initial_role_key": "Mafia",
                    "initial_team": "mafia",
                    "final_role": "Mafia",
                    "final_role_key": "Mafia",
                    "final_team": "mafia",
                    "alive": true,
                    "death_order": null
                },
                {
                    "user_id": 12,
                    "name": "Bob",
                    "initial_role": "Citizen",
                    "initial_role_key": "Citizen",
                    "initial_team": "citizen",
                    "final_role": "Citizen",
                    "final_role_key": "Citizen",
                    "final_team": "citizen",
                    "alive": false,
                    "death_order": 1
                }
            ],
            "events": [
                {
                    "seq": 0,
                    "id": "e_000000",
                    "timestamp": "2026-07-08T21:00:00Z",
                    "day_number": 0,
                    "phase": "Recruiting",
                    "phase_key": "Recruiting",
                    "kind": "game_started",
                    "actor": null,
                    "target_user_ids": [],
                    "details": {"player_count": 2}
                },
                {
                    "seq": 1,
                    "id": "e_000001",
                    "timestamp": "2026-07-08T21:10:00Z",
                    "day_number": 1,
                    "phase": "Day",
                    "phase_key": "Day",
                    "kind": "day_vote",
                    "actor": {"user_id": 11, "name": "Alice"},
                    "target_user_ids": [12],
                    "details": {"choice": "player"}
                },
                {
                    "seq": 2,
                    "id": "e_000002",
                    "timestamp": "2026-07-08T21:12:00Z",
                    "day_number": 1,
                    "phase": "ConfirmationVote",
                    "phase_key": "ConfirmationVote",
                    "kind": "confirmation_vote_resolved",
                    "actor": null,
                    "target_user_ids": [12],
                    "details": {
                        "executed_user_id": 12,
                        "approved": true,
                        "vote_counts": [{"approve": true, "count": 2}],
                        "weighted_vote_counts": [{"approve": true, "count": 2}]
                    }
                }
            ],
            "rating_log": [],
        }));

        let recent_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/games/recent?limit=5",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(recent_response.starts_with("HTTP/1.1 200 OK"));
        assert!(recent_response.contains(r#""player_count":2"#));

        let recent_alias_response = route_request(
            &state,
            api_request(
                "GET",
                "/games/recent?limit=5",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(recent_alias_response.starts_with("HTTP/1.1 200 OK"));
        assert!(recent_alias_response.contains(r#""player_count":2"#));

        let game_response = route_request(
            &state,
            api_request("GET", "/api/v1/game/game-1", Some(("X-API-Key", &raw_key))),
        )
        .await;
        assert!(game_response.starts_with("HTTP/1.1 200 OK"));
        assert!(game_response.contains(r#""nickname":"Alice""#));

        let result_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/game/game-1/result",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(result_response.starts_with("HTTP/1.1 200 OK"));
        assert!(result_response.contains(r#""cause_of_death":"execution""#));

        let events_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/game/game-1/events",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(events_response.starts_with("HTTP/1.1 200 OK"));
        assert!(events_response.contains(r#""type":"vote""#));
        assert!(events_response.contains(r#""type":"death""#));
        assert!(events_response.contains(r#""role_revealed":"citizen""#));
        assert!(events_response.contains(r#""vote_count":2"#));

        let events_alias_response = route_request(
            &state,
            api_request("GET", "/game/game-1/events", Some(("X-API-Key", &raw_key))),
        )
        .await;
        assert!(events_alias_response.starts_with("HTTP/1.1 200 OK"));
        assert!(events_alias_response.contains(r#""type":"death""#));

        let leaderboard_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/stats/leaderboard?sort=games&limit=5",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(leaderboard_response.starts_with("HTTP/1.1 200 OK"));
        assert!(leaderboard_response.contains(r#""nickname":"Alice""#));

        let user_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/stats/user/11",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(user_response.starts_with("HTTP/1.1 200 OK"));
        assert!(user_response.contains(r#""total_games":3"#));

        let user_games_response = route_request(
            &state,
            api_request(
                "GET",
                "/api/v1/stats/user/11/games?per_page=5",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(user_games_response.starts_with("HTTP/1.1 200 OK"));
        assert!(user_games_response.contains(r#""result":"loss""#));

        let user_games_alias_response = route_request(
            &state,
            api_request(
                "GET",
                "/stats/user/11/games?per_page=5",
                Some(("X-API-Key", &raw_key)),
            ),
        )
        .await;
        assert!(user_games_alias_response.starts_with("HTTP/1.1 200 OK"));
        assert!(user_games_alias_response.contains(r#""result":"loss""#));
    }

    #[tokio::test]
    async fn api_key_management_issues_and_revokes_key() {
        let mut state = test_state();
        let key_path = std::env::temp_dir().join(format!("mafia-api-keys-{}.json", Uuid::new_v4()));
        state.api_keys_path = Arc::new(key_path.clone());
        let token = "api-key-test";
        state.sessions.insert(
            token.to_string(),
            WebSettingsSession {
                guild_id: 1,
                user_id: 2,
                user_label: "tester".to_string(),
                expires_at: Instant::now() + Duration::from_secs(60),
            },
        );
        let create_response = route_request(
            &state,
            HttpRequest {
                method: "POST".to_string(),
                path: format!("{WEB_SETTINGS_PATH}/{token}/api-keys"),
                headers: HashMap::new(),
                body: "action=create&label=integration".to_string(),
            },
        )
        .await;
        assert!(create_response.starts_with("HTTP/1.1 200 OK"));
        assert!(create_response.contains("mfr_"));
        let key_id = state.api_keys.read().await.keys[0].id.clone();

        let revoke_response = route_request(
            &state,
            HttpRequest {
                method: "POST".to_string(),
                path: format!("{WEB_SETTINGS_PATH}/{token}/api-keys"),
                headers: HashMap::new(),
                body: format!("action=revoke&key_id={key_id}"),
            },
        )
        .await;
        assert!(revoke_response.starts_with("HTTP/1.1 200 OK"));
        assert!(state.api_keys.read().await.keys[0].revoked);
        let _ = std::fs::remove_file(key_path);
    }

    #[tokio::test]
    async fn protected_api_starts_ready_recruitment() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "host".to_string())
        };
        let recruitment = Arc::new(RwLock::new(Recruitment {
            host_user_id: serenity::UserId::new(2),
            participant_role_id: serenity::RoleId::new(3),
            role_counts: HashMap::new(),
            special_roles: Vec::new(),
            max_players: 8,
            minimum_players: 2,
            joined_ids: std::collections::HashSet::from([2, 3]),
            joined_names: HashMap::new(),
            spectator_ids: std::collections::HashSet::new(),
            spectator_names: HashMap::new(),
            accepting: true,
            cancelled: false,
            done: Arc::new(tokio::sync::Notify::new()),
        }));
        state
            .recruitments
            .insert(serenity::GuildId::new(1), recruitment.clone());
        let response = route_request(
            &state,
            HttpRequest {
                method: "POST".to_string(),
                path: "/api/v1/recruitments/1/actions".to_string(),
                headers: HashMap::from([("x-api-key".to_string(), raw_key)]),
                body: r#"{"action":"start"}"#.to_string(),
            },
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(!recruitment.read().await.accepting);
    }

    #[test]
    fn api_key_store_never_serializes_raw_key() {
        let mut store = ApiKeyStore::default();
        let raw_key = issue_api_key(&mut store, 1, 2, "test".to_string());
        let serialized = serde_json::to_string(&store).unwrap();

        assert!(!serialized.contains(&raw_key));
        assert!(serialized.contains("key_hash"));
    }

    #[test]
    fn parses_api_key_headers_case_insensitively() {
        let headers = parse_http_headers("GET / HTTP/1.1\r\nX-API-Key: key-value\r\n");

        assert_eq!(
            headers.get("x-api-key").map(String::as_str),
            Some("key-value")
        );
    }

    #[test]
    fn api_docs_separate_public_and_protected_endpoints() {
        let html = render_api_docs_page("https://mafia.example/");

        assert!(html.contains("공개 조회 API"));
        assert!(html.contains("보호 관리 API"));
        assert!(html.contains("/api/v1/games/recent"));
        assert!(html.contains("/api/v1/game/{game_key}/events"));
        assert!(html.contains("/api/v1/stats/user/{user_id}/games"));
        assert!(html.contains("GET /games/recent"));
        assert!(html.contains("GET /game/{game_key}/events"));
        assert!(html.contains("GET /stats/user/{user_id}/games"));
        assert!(html.contains("/api/v1/games/{guild_id}/actions"));
        assert!(html.contains("/api/v1/games/{guild_id}/replay"));
        assert!(html.contains("/api/v1/replays/{game_key}"));
        assert!(html.contains("https://mafia.example/api/v1/games/123"));
        assert!(!html.contains("example.com"));
        assert!(html.contains("overflow-wrap: anywhere"));
        assert!(html.contains("word-break: break-word"));
        assert!(html.contains("site-shell"));
        assert!(html.contains("응답 코드"));
    }

    #[test]
    fn roles_page_renders_detailed_guides() {
        let html = render_roles_page();

        assert!(html.contains("역할 설명"));
        assert!(html.contains(r#"<a href="/roles">역할 설명</a>"#));
        assert!(html.contains("마피아 비밀방의 처치 선택 현황"));
        assert!(html.contains("최면술사"));
        assert!(html.contains("운영 포인트"));
        assert!(html.contains("주의:"));
        assert!(html.contains("role-help"));
        assert!(html.contains("role-rating"));
        assert!(html.contains("레이팅 요소"));
        assert!(html.contains("role-grid"));
    }

    #[test]
    fn rating_page_explains_rating_for_players() {
        let html = render_rating_page();

        assert!(html.contains("레이팅 설명"));
        assert!(html.contains(r#"<a href="/rating">레이팅 설명</a>"#));
        assert!(html.contains("초기 레이팅"));
        assert!(html.contains("패배팀 최대 상승"));
        assert!(html.contains("첫 사망 패배 완화"));
        assert!(html.contains("랭크표"));
        assert!(html.contains("자주 묻는 질문"));
        assert!(html.contains("졌는데 왜 점수가 올랐나요?"));
    }
}
