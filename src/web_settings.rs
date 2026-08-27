use crate::{Recruitment, RunningGame};
use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use dashmap::DashMap;
use mafia_remake::config::{self, BotConfig};
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES, Phase, Role,
    TIER3_ABILITIES, TIER4_CITIZEN_ABILITIES, TIER4_MAFIA_ABILITIES, TIER4_MAFIA_SUPPORT_ABILITIES,
    TierAbility, tier4_pool,
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

mod api;
mod pages;
pub(crate) use self::api::*;
pub(crate) use self::pages::*;

const WEB_SETTINGS_PATH: &str = "/web-settings";
const WEB_SETTINGS_SESSION_TTL_SECONDS: u64 = 600;
const MAX_GAME_PLAYERS: usize = 24;
const WEB_LEADERBOARD_METRICS: &[&str] = &[
    "rating", "wins", "streak", "winrate", "games", "mafia", "playtime",
];

pub(crate) struct WebRoleGuide {
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
        summary: "밤마다 한 명을 조사해 마피아 판정 여부를 확인합니다. 대상을 제출하는 즉시 결과가 나오고, 그 밤에는 대상을 다시 바꿀 수 없습니다. 결과는 강력하지만 대부의 조사 회피, 보조직의 접선 상태 같은 예외가 있어 결과 해석이 중요합니다.",
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
        summary: "낮 조사와 밤 숙청으로 마피아팀을 직접 압박합니다. 조사(게임 중 1회)는 제출 즉시 마피아팀 여부가 나옵니다. 처형 능력은 강하지만 오판하면 시민 수가 줄어들어 패배 조건에 가까워집니다.",
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
        summary: "게임 중 한 번만 밤에 한 명을 수사합니다. 결과는 대상을 제출하는 즉시 나오고, 대상은 다시 바꿀 수 없습니다. 대상이 형사와 같은 팀이면 직업을 알게 되고, 수사를 받은 대상에게는 밤이 끝날 때 형사의 정체가 전달됩니다. 마피아팀·교주팀 등 다른 팀을 수사하면 \"시민팀이 아닙니다\"만 표시되고 대상에게는 아무 알림도 가지 않으며, 1회용 수사는 그대로 소모됩니다.",
        tips: &[
            "수사는 1회용이므로 같은 팀일 가능성이 높은 대상을 고르세요.",
            "같은 팀 확인이 뜨면 직업 정보와 함께 신뢰 가능한 연결고리가 생깁니다.",
            "같은 팀 수사 대상에게만 형사 정체가 알려지므로 공개 타이밍과 생존 위험을 함께 계산하세요.",
            "경찰, 요원, 자경단원과 같은 경찰계열이므로 한 판에 함께 배정되지 않습니다.",
            "형사는 접선 여부와 무관하게 실제 소속으로 판정하므로, 접선 전 마피아 보조도 \"시민팀이 아닙니다\"로 나옵니다. 단, 변장 사기꾼은 시민으로 판정되어 속습니다.",
        ],
        caution: "수사는 게임 중 1회뿐이고 다른 팀을 수사해도 소모됩니다. 같은 팀을 수사하면 대상에게 형사의 정체가 전달되므로 공개 타이밍도 계산해야 합니다. 다른 팀 수사는 \"시민팀이 아닙니다\"만 나오고 대상은 눈치채지 못합니다.",
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
        summary: "마피아 공격을 한 번 버틸 수 있는 시민팀 방어 역할입니다. [불침번] 자신을 노린 스파이의 첩보, 도둑의 도벽, 사기꾼의 사기, 청부업자의 청부를 무효화하고 그 사용자의 정체를 개인 DM으로 알아냅니다. 방탄 발동은 강한 생존 정보가 되며 마피아의 공격 의도도 추론할 수 있습니다.",
        tips: &[
            "불침번 알림이 오면 마피아 보조직 하나의 정체를 확보한 것입니다. 공개 타이밍을 계산하세요.",
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
        role: Role::CivilServant,
        team: "시민팀",
        kind: "정보 수집",
        summary: "밤마다 직업 하나를 지목해 조회합니다. 경찰 계열과 시민 직업은 조회할 수 없습니다. 밤이 끝날 때 그 직업을 가진 플레이어가 누구인지 정확히 알게 되며, 사망자도 조회에 걸립니다. 그 직업이 이번 게임에 없으면 없다는 결과만 받습니다. 없는 직업을 골라도 그날 밤 조회는 소모되며, 같은 밤에 다시 시도할 수 없습니다.",
        tips: &[
            "공개된 정보로 이번 게임에 있을 법한 직업부터 조회하세요.",
            "없는 직업을 고르면 그날 밤 조회가 헛돌기 때문에 직업 구성 추리가 중요합니다.",
            "조회 성공은 파파라치의 이슈로 공유될 수 있습니다.",
            "확보한 직업 정보는 공개 타이밍을 계산해 사용하세요.",
        ],
        caution: "조회 결과를 성급히 공개하면 마피아에게 다음 밤 표적을 알려주는 셈이 됩니다.",
    },
    WebRoleGuide {
        role: Role::Paparazzi,
        team: "시민팀",
        kind: "정보 수집",
        summary: "하루에 한 번, 시민팀이 다른 사람의 직업을 명확하게 알아내면 그 정보를 함께 공유받습니다. 하루 중 가장 먼저 알아낸 정보만 공유되고, 팀만 알아내는 능력(경찰 조사 등)이나 자기 자신에 대한 정보는 공유 대상이 아닙니다.",
        tips: &[
            "형사·공무원·요원·영매·해커의 성공과 기자의 특종이 공유 대상입니다. 기자가 자신을 특종하면 공유되지 않습니다.",
            "경찰 조사는 마피아 여부(팀)만 알아내므로 공유되지 않습니다.",
            "공유받은 정보의 출처를 추리하면 시민팀 구성을 역산할 수 있습니다.",
            "정보를 아는 티를 내지 않고 투표를 유도하는 것이 안전합니다.",
        ],
        caution: "아는 정보를 서둘러 공개하면 파파라치임이 드러나 마피아의 표적이 됩니다.",
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
        summary: "밤마다 플레이어 한 명을 선택해 직업을 알아냅니다. 첩보로 마피아를 찾아내면 그 밤 첩보를 한 번 더 사용할 수 있고, 처음 마피아를 찾은 시점에 마피아팀과 접선합니다.",
        tips: &[
            "마피아를 먼저 찾아낼수록 그 밤의 추가 첩보로 정보가 두 배가 됩니다.",
            "마피아 접선 전까지는 의심을 낮게 유지하세요.",
            "첩보 대상은 수사직 후보나 핵심 발언자가 좋습니다.",
            "얻은 정보는 마피아 처치 우선순위와 연결하세요.",
        ],
        caution: "접선 전 무리한 발언은 마피아팀 보조직으로 찍히기 쉽습니다.",
    },
    WebRoleGuide {
        role: Role::Fraudster,
        team: "마피아팀",
        kind: "변장/교섭",
        summary: "게임 시작 시 시민 한 명의 정체를 알아내고 그 직업으로 변장합니다. 조사 판정이 변장한 직업으로 표시되어 경찰·형사·공무원 등 각종 조사를 속이며, 속일 때마다 알림을 받습니다. 사기꾼 본인 또는 사기 대상이 마피아팀의 처형 대상이 되면 처형 성공 여부와 관계없이 마피아팀과 접선하고, 사기꾼 본인은 마피아팀에게 처형되지 않습니다.",
        tips: &[
            "변장한 직업의 행세를 자연스럽게 하려면 그 직업의 규칙을 파악해 두세요.",
            "사기 대상이 살아있는 동안 같은 직업 주장이 겹치면 의심을 삽니다.",
            "속임 알림이 오면 누가 조사직인지 역추적할 수 있습니다.",
            "접선 후에는 표준 규칙대로 경찰 계열 판정에 마피아팀으로 잡히니 주의하세요.",
        ],
        caution: "사기 대상이 공개적으로 직업을 증명하면 같은 직업을 주장하던 변장이 무너집니다.",
    },
    WebRoleGuide {
        role: Role::Contractor,
        team: "마피아팀",
        kind: "추측/암살",
        summary: "두 대상과 각각의 직업을 추측해 청부를 시도합니다. 정확히 맞히면 큰 이득을 얻지만 실패하면 행동 가치를 잃습니다.",
        tips: &[
            "공개 정보가 충분한 대상끼리 묶어 제출하세요.",
            "직업 주장과 실제 행동 가능성을 대조하세요.",
            "경찰 계열도 대상이지만 경찰 계열 직업으로는 추측할 수 없습니다.",
            "마피아 보조는 한 판에 청부업자 본인뿐이라 추측 목록에 다른 보조 직업은 없습니다.",
            "성공 시 접선과 암살 가치까지 함께 계산하세요.",
        ],
        caution: "확률 낮은 청부는 마피아팀의 밤 템포를 낭비합니다.",
    },
    WebRoleGuide {
        role: Role::Thief,
        team: "마피아팀",
        kind: "도벽",
        summary: "지목 투표에서 마지막으로 투표한 대상의 직업을 훔쳐 다음 밤까지 그 능력을 사용할 수 있습니다. 별도 도벽 선택은 없고, 어떤 능력을 훔쳤는지는 투표가 끝난 뒤에 전달됩니다. 수사직을 훔치면 기존 수사직과 독립된 결과를 얻습니다.",
        tips: &[
            "도벽 대상은 마지막 지목 투표 대상과 같고, 결과는 투표 종료 후에 옵니다.",
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
        summary: "밤에 대상을 개구리로 저주해 밤 능력과 모든 게임 채팅 발언을 막습니다. 중요한 수사직이나 투표 영향력이 큰 사람을 흔드는 데 좋습니다.",
        tips: &[
            "저주 대상의 능력 가치와 다음 낮 영향력을 보세요.",
            "완전한 발언 차단이 토론에 줄 혼선을 계산하세요.",
            "마피아 접선 여부에 따라 경찰 판정 해석이 달라질 수 있습니다.",
            "수사직 저주로 정보 공개 흐름을 끊을 수 있습니다.",
        ],
        caution: "무작정 저주하면 마피아 처치 우선순위와 충돌할 수 있습니다.",
    },
    WebRoleGuide {
        role: Role::Scientist,
        team: "마피아팀",
        kind: "소생",
        summary: "소속과 승패는 처음부터 마피아팀입니다. 첫 사망 전에는 미접선 마피아 보조처럼 조사와 관계 판정에서 시민으로 위장되며 마피아 비밀방과 생존 마피아 수에는 포함되지 않습니다. 처음 사망하면 접선 상태가 되고 다음 밤에 부활하며, 이후에는 마피아 비밀방과 마피아 판정에 포함됩니다.",
        tips: &[
            "접선 전 시민 판정을 이용하되 실제 승리 목표는 마피아팀이라는 점을 잊지 마세요.",
            "소생 타이밍 뒤 마피아 수 계산을 다시 하세요.",
            "죽은 상태에서도 공개 정보가 어떻게 쌓이는지 보세요.",
            "소생 후 바로 표적이 될 수 있어 후속 발언을 준비하세요.",
        ],
        caution: "접선 전 시민 판정은 위장 판정일 뿐입니다. 역할 소속, 승패, 레이팅은 처음부터 마피아팀 기준입니다.",
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
        summary: "마녀 저주로 밤 능력을 사용할 수 없고 저주가 풀릴 때까지 모든 게임 채팅에서 발언할 수 없는 상태입니다.",
        tips: &[
            "저주 전에 확보한 정보와 투표 흐름을 활용하세요.",
            "능력과 발언이 모두 차단된 상태임을 기억하세요.",
            "누가 저주했을지 마녀 후보를 추론하세요.",
            "해제된 뒤 누락된 정보와 판단 근거를 설명하세요.",
        ],
        caution: "저주 중에는 게임 채팅으로 어떤 메시지도 전달할 수 없습니다.",
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
pub(crate) struct ApiKeyRecord {
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
pub(crate) struct WebConfigField {
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
        "recruitment_seconds",
        "참가자 모집 시간(초)",
        WebFieldKind::Int,
        Some(config::MIN_RECRUITMENT_SECONDS),
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
        "의사 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "default_police_count",
        "경찰 활성화",
        WebFieldKind::Bool,
        None,
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
    field(
        "enable_fraudster",
        "사기꾼 활성화",
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
    field(
        "enable_civil_servant",
        "공무원 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "enable_paparazzi",
        "파파라치 활성화",
        WebFieldKind::Bool,
        None,
    ),
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
pub(crate) enum ApiAuthError {
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

#[cfg(test)]
mod tests;
