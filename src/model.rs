use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Mafia,
    Doctor,
    Nurse,
    Police,
    Agent,
    Vigilante,
    Inspector,
    Reporter,
    Hacker,
    Detective,
    Shaman,
    Priest,
    Soldier,
    Gangster,
    Prophet,
    Psychologist,
    Hypnotist,
    Mercenary,
    Spy,
    Contractor,
    Fraudster,
    Thief,
    Witch,
    Scientist,
    Madam,
    Graverobber,
    Godfather,
    Joker,
    Politician,
    Judge,
    Terrorist,
    Lover,
    CivilServant,
    Paparazzi,
    CultLeader,
    Fanatic,
    Frog,
    Villain,
    Citizen,
}

impl Role {
    pub const fn value(self) -> &'static str {
        match self {
            Self::Mafia => "마피아",
            Self::Doctor => "의사",
            Self::Nurse => "간호사",
            Self::Police => "경찰",
            Self::Agent => "요원",
            Self::Vigilante => "자경단원",
            Self::Inspector => "형사",
            Self::Reporter => "기자",
            Self::Hacker => "해커",
            Self::Detective => "사립탐정",
            Self::Shaman => "영매",
            Self::Priest => "성직자",
            Self::Soldier => "군인",
            Self::Gangster => "건달",
            Self::Prophet => "예언자",
            Self::Psychologist => "심리학자",
            Self::Hypnotist => "최면술사",
            Self::Mercenary => "용병",
            Self::Spy => "스파이",
            Self::Fraudster => "사기꾼",
            Self::Contractor => "청부업자",
            Self::Thief => "도둑",
            Self::Witch => "마녀",
            Self::Scientist => "과학자",
            Self::Madam => "마담",
            Self::Graverobber => "도굴꾼",
            Self::Godfather => "대부",
            Self::Joker => "조커",
            Self::Politician => "정치인",
            Self::Judge => "판사",
            Self::Terrorist => "테러리스트",
            Self::Lover => "연인",
            Self::CivilServant => "공무원",
            Self::Paparazzi => "파파라치",
            Self::CultLeader => "교주",
            Self::Fanatic => "광신도",
            Self::Frog => "개구리",
            Self::Villain => "악인",
            Self::Citizen => "시민",
        }
    }

    pub const fn is_mafia_team(self) -> bool {
        matches!(
            self,
            Self::Mafia
                | Self::Spy
                | Self::Contractor
                | Self::Fraudster
                | Self::Thief
                | Self::Witch
                | Self::Scientist
                | Self::Madam
                | Self::Godfather
                | Self::Villain
        )
    }

    pub const fn is_investigation_role(self) -> bool {
        matches!(
            self,
            Self::Police | Self::Agent | Self::Vigilante | Self::Inspector
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Night,
    Day,
    Vote,
    FinalDefense,
    ConfirmVote,
    Ended,
}

impl Phase {
    pub const fn value(self) -> &'static str {
        match self {
            Self::Night => "밤",
            Self::Day => "낮",
            Self::Vote => "투표",
            Self::FinalDefense => "최후변론",
            Self::ConfirmVote => "찬반투표",
            Self::Ended => "종료",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Winner {
    Mafia,
    Citizen,
    Joker,
    Cult,
}

impl Winner {
    pub const fn value(self) -> &'static str {
        match self {
            Self::Mafia => "마피아",
            Self::Citizen => "시민",
            Self::Joker => "조커",
            Self::Cult => "교주",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub user_id: u64,
    pub name: String,
    pub role: Role,
    pub alive: bool,
}

impl Player {
    pub fn new(user_id: u64, name: impl Into<String>, role: Role) -> Self {
        Self {
            user_id,
            name: name.into(),
            role,
            alive: true,
        }
    }
}

pub fn mafia_team_roles() -> HashSet<Role> {
    [
        Role::Mafia,
        Role::Spy,
        Role::Contractor,
        Role::Fraudster,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Godfather,
        Role::Villain,
    ]
    .into_iter()
    .collect()
}

pub fn investigation_roles() -> HashSet<Role> {
    [Role::Police, Role::Agent, Role::Vigilante, Role::Inspector]
        .into_iter()
        .collect()
}

pub const CITIZEN_SPECIAL_ROLES: &[Role] = &[
    Role::Inspector,
    Role::Detective,
    Role::Shaman,
    Role::Priest,
    Role::Graverobber,
    Role::Politician,
    Role::Judge,
    Role::Reporter,
    Role::Hacker,
    Role::Terrorist,
    Role::Lover,
    Role::CivilServant,
    Role::Paparazzi,
    Role::Soldier,
    Role::Nurse,
    Role::Gangster,
    Role::Prophet,
    Role::Psychologist,
    Role::Hypnotist,
    Role::Mercenary,
];

pub const MAFIA_SPECIAL_ROLES: &[Role] = &[
    Role::Spy,
    Role::Contractor,
    Role::Fraudster,
    Role::Thief,
    Role::Witch,
    Role::Scientist,
    Role::Madam,
    Role::Godfather,
];

pub const NEUTRAL_SPECIAL_ROLES: &[Role] = &[Role::Joker];

pub const PUBLIC_MAFIA_SPECIAL_ROLES: &[Role] = &[
    Role::Spy,
    Role::Contractor,
    Role::Fraudster,
    Role::Thief,
    Role::Witch,
    Role::Scientist,
    Role::Madam,
    Role::Godfather,
];

pub const PUBLIC_CITIZEN_SPECIAL_ROLES: &[Role] = &[
    Role::Inspector,
    Role::Detective,
    Role::Shaman,
    Role::Priest,
    Role::Graverobber,
    Role::Politician,
    Role::Judge,
    Role::Reporter,
    Role::Hacker,
    Role::Terrorist,
    Role::Lover,
    Role::CivilServant,
    Role::Paparazzi,
    Role::Soldier,
    Role::Nurse,
    Role::Gangster,
    Role::Prophet,
    Role::Psychologist,
    Role::Hypnotist,
    Role::Mercenary,
    Role::Fanatic,
];

pub const PUBLIC_NEUTRAL_SPECIAL_ROLES: &[Role] = &[Role::Joker];
pub const PUBLIC_CULT_SPECIAL_ROLES: &[Role] = &[Role::CultLeader];

pub const CONTRACTOR_GUESS_ROLES: &[Role] = &[
    Role::Mafia,
    Role::Doctor,
    Role::Witch,
    Role::Scientist,
    Role::Madam,
    Role::Thief,
    Role::Fraudster,
    Role::Detective,
    Role::Shaman,
    Role::Priest,
    Role::Graverobber,
    Role::Politician,
    Role::Judge,
    Role::Reporter,
    Role::Hacker,
    Role::Terrorist,
    Role::Lover,
    Role::CivilServant,
    Role::Paparazzi,
    Role::Soldier,
    Role::Nurse,
    Role::Gangster,
    Role::Prophet,
    Role::Psychologist,
    Role::Hypnotist,
    Role::Mercenary,
    Role::CultLeader,
    Role::Fanatic,
    Role::Joker,
    Role::Citizen,
];

/// 공무원 조회 대상 직업. "시민팀 직업 중 경찰 계열, 시민 직업을 제외한" 목록으로,
/// 이번 게임에 실제로 배정됐는지와 무관하게 항상 전부 고를 수 있다(없는 직업을
/// 고르면 조회가 헛돌고 그날 밤 능력이 소모되는 것이 규칙의 일부다).
/// Discord 셀렉트 상한(25개)을 넘지 않아야 한다.
pub const CIVIL_SERVANT_QUERY_ROLES: &[Role] = &[
    Role::Doctor,
    Role::Nurse,
    Role::Detective,
    Role::Shaman,
    Role::Priest,
    Role::Graverobber,
    Role::Politician,
    Role::Judge,
    Role::Reporter,
    Role::Hacker,
    Role::Terrorist,
    Role::Lover,
    Role::Paparazzi,
    Role::Soldier,
    Role::Gangster,
    Role::Prophet,
    Role::Psychologist,
    Role::Hypnotist,
    Role::Mercenary,
];

pub fn is_civil_servant_query_role(role: Role) -> bool {
    CIVIL_SERVANT_QUERY_ROLES.contains(&role)
}

/// 을/를 — 받침 유무로 목적격 조사를 고른다. 한글이 아니면 병기한다.
pub fn korean_object_particle(word: &str) -> &'static str {
    match word.chars().next_back() {
        Some(last) if ('가'..='힣').contains(&last) => {
            if (last as u32 - 0xAC00) % 28 != 0 {
                "을"
            } else {
                "를"
            }
        }
        _ => "을(를)",
    }
}

/// (으)로 — 받침이 없거나 ㄹ 받침이면 "로", 그 외 받침은 "으로".
pub fn korean_ro_particle(word: &str) -> &'static str {
    match word.chars().next_back() {
        Some(last) if ('가'..='힣').contains(&last) => {
            let jongseong = (last as u32 - 0xAC00) % 28;
            if jongseong == 0 || jongseong == 8 {
                "로"
            } else {
                "으로"
            }
        }
        _ => "(으)로",
    }
}

/// 역할과 별개로 게임마다 배정되는 개인 티어 능력. 2티어는 능력 없음이고,
/// 한 게임 안에서 같은 능력이 두 명에게 배정되지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TierAbility {
    /// 3티어: 패배 시 레이팅 손실 10% 완화
    RatingShield,
    /// 3티어: 게임 채널 슬로우모드 무시
    SlowmodeBypass,
    /// 4티어 마피아팀: 경찰을 공격하면 보호를 무시하고 처형
    Lawless,
    /// 4티어 마피아팀: 첫날 밤 공격이 자기 자신에게 쓴 치료를 무시
    NightRaid,
    /// 4티어 마피아팀: 마피아팀이 죽인 대상의 직업을 알아내고, 시민팀이면 '시민'으로 만든다
    Cleanup,
    /// 4티어 마피아팀: 투표 처형 시 도주, 다음날 투표 시작 때 사망
    Escape,
    /// 4티어 시민팀: 밤에 유언 작성, 밤에 죽으면 전체 공개
    LastWill,
    /// 4티어 공용: 밤에도 게임 채널에 채팅 가능
    Loudspeaker,
    /// 4티어 마피아 본대: 첫 낮에 접선하지 않은 마피아팀 명단 파악
    Wanted,
    /// 4티어 마피아팀·교주: 첫 낮에 지령 정보 (마피아·청부업자는 경찰 계열
    /// 한 명, 그 외 보조·교주는 미공개 시민팀 한 명의 직업)
    Directive,
    /// 4티어 마피아 본대: 첫 밤 동안 조사 판정이 의사로 나온다
    Hypocrisy,
    /// 4티어 마피아 본대: 처형 실패가 문구 없는 '조용한 밤'이 된다
    Concealment,
    /// 4티어 마피아 본대: 전날 밤 처형 실패 시 이번 밤 모든 보호 무시
    Snipe,
    /// 4티어 마피아 본대: 시민팀 처형 실패 시 중독 → 하루 뒤 사망
    Poison,
    /// 4티어 마피아 본대: 마지막 마피아가 되면 그 밤 처형은 무조건 성공
    AllIn,
    /// 4티어 마피아 본대: 마피아팀이 아닌 희생자를 처형 후 성불
    Exorcism,
    /// 4티어 마피아팀·교주: 절반 이하 + 2번째 밤 생존 시 소속 팀 즉시 승리
    TimeLimit,
    /// 4티어 마피아팀 보조: 두 번째 낮에 자동으로 마피아와 접선
    InsideMan,
    /// 4티어 스파이: 사망자가 생길 때마다 자동 조사
    Autopsy,
    /// 4티어 스파이: 마피아팀에 혼자 남으면 조사한 대상을 처형
    Assassin,
    /// 4티어 스파이·사기꾼: 시민팀 능력의 대상이 되면 그 사용자의 직업 파악
    Honeytrap,
    /// 4티어 마담: 유혹한 시민팀 대상의 직업 파악
    Allure,
    /// 4티어 마담: 첫날 시민팀을 유혹하면 그 대상의 투표권 한 표 박탈
    Debut,
    /// 4티어 도둑: 마피아 본대가 전멸하면 본인이 마피아가 된다
    Successor,
    /// 4티어 도둑: 밤에 사망자의 직업을 도벽할 수 있다
    Condolence,
}

impl TierAbility {
    pub const fn value(self) -> &'static str {
        match self {
            Self::RatingShield => "가호",
            Self::SlowmodeBypass => "달변",
            Self::Lawless => "무법",
            Self::NightRaid => "야습",
            Self::Cleanup => "수습",
            Self::Escape => "도주",
            Self::LastWill => "유언",
            Self::Loudspeaker => "확성",
            Self::Wanted => "수배",
            Self::Directive => "지령",
            Self::Hypocrisy => "위선",
            Self::Concealment => "은폐",
            Self::Snipe => "저격",
            Self::Poison => "독살",
            Self::AllIn => "승부수",
            Self::Exorcism => "퇴마",
            Self::TimeLimit => "시한부",
            Self::InsideMan => "밀정",
            Self::Autopsy => "부검",
            Self::Assassin => "자객",
            Self::Honeytrap => "미인계",
            Self::Allure => "현혹",
            Self::Debut => "데뷔",
            Self::Successor => "후계자",
            Self::Condolence => "조문",
        }
    }

    pub const fn tier(self) -> u8 {
        match self {
            Self::RatingShield | Self::SlowmodeBypass => 3,
            _ => 4,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::RatingShield => "패배해도 레이팅 손실이 10% 줄어듭니다.",
            Self::SlowmodeBypass => "게임 채널의 슬로우모드를 무시하고 채팅할 수 있습니다.",
            Self::Lawless => {
                "마피아팀의 밤 공격이 경찰 계열(경찰·요원·자경단원·형사)을 노리면 보호를 무시하고 무조건 처형합니다."
            }
            Self::NightRaid => {
                "첫날 밤 공격 대상이 자신을 치료한 의사라면 치료를 무시하고 처형하며, 그 의사의 정체가 모두에게 공개됩니다."
            }
            Self::Cleanup => {
                "마피아팀이 죽인 대상의 직업을 알아내고, 시민팀이면 그 직업을 '시민'으로 바꿔 숨깁니다."
            }
            Self::Escape => {
                "투표로 처형될 때 도주해 살아남지만, 다음날 투표가 시작될 때 사망합니다."
            }
            Self::LastWill => {
                "밤에 유언을 작성할 수 있고, 밤에 사망하면 작성한 유언이 모두에게 공개됩니다."
            }
            Self::Loudspeaker => {
                "밤에도 게임 채널에 메시지를 보낼 수 있습니다. 밤마다 단 한 번이며, 확성 보유자가 여러 명이면 그 밤에 먼저 보낸 한 명만 쓸 수 있습니다."
            }
            Self::Wanted => "첫 번째 낮이 될 때 아직 접선하지 않은 마피아팀 명단을 알 수 있습니다.",
            Self::Directive => {
                "첫 번째 낮이 될 때 지령을 받습니다. 마피아·청부업자는 경찰 계열 생존자 한 명이 누구인지, 그 외 보조 직업과 교주는 정체가 밝혀지지 않은 시민팀 한 명의 직업을 알아냅니다."
            }
            Self::Hypocrisy => "첫 번째 밤 동안 시민팀의 조사에 의사 직업으로 판정됩니다.",
            Self::Concealment => {
                "마피아팀의 처형이 실패하면 치료·방탄 문구가 나오지 않는 '조용한 밤'으로 진행됩니다."
            }
            Self::Snipe => {
                "전날 밤 마피아팀 처형이 실패했다면, 이번 밤 처형 대상의 치료·방탄 등 모든 보호를 무시합니다."
            }
            Self::Poison => {
                "밤에 시민팀 처형이 실패하면 대상을 중독시켜 하루 뒤 사망하게 합니다. 포교된 시민팀에게도 통하지만 교주·광신도·마피아팀 보조에게는 통하지 않습니다."
            }
            Self::AllIn => {
                "마피아가 모두 죽고 자신만 남았다면, 그 밤 처형 대상을 치료·방탄을 무시하고 무조건 처형합니다. 해커의 프록시로 대상이 바뀌면 바뀍 대상이 처형됩니다."
            }
            Self::Exorcism => {
                "마피아팀이 아닌 플레이어를 처형하면 그 희생자를 성불시켜 영매가 접촉할 수 없게 만듭니다."
            }
            Self::TimeLimit => {
                "생존자가 절반 이하로 줄어든 상태에서 2번째 밤 이후까지 살아남으면 자신이 속한 팀이 즉시 승리합니다. 포교당한 보유자는 교주팀 승리가 되고, 도주로 살아남은 상태에서는 발동하지 않습니다."
            }
            Self::InsideMan => "두 번째 낮이 될 때 자동으로 마피아와 접선합니다.",
            Self::Autopsy => "사망한 플레이어가 생길 때마다 자동으로 조사해 그 직업을 알아냅니다.",
            Self::Assassin => {
                "마피아팀에 혼자 남았을 경우, 그 밤에 첩보로 조사한 대상을 처형합니다."
            }
            Self::Honeytrap => {
                "시민팀 플레이어의 능력 대상이 되면 그 사용자의 직업을 알아냅니다. (요원 제외)"
            }
            Self::Allure => "유혹한 대상이 시민팀이면 그 대상의 직업을 알아냅니다.",
            Self::Debut => "첫날 시민팀을 유혹하면 그 대상의 투표권을 한 표 박탈합니다.",
            Self::Successor => {
                "마피아가 모두 사망하면 마피아의 능력을 이어받아 본인이 마피아가 됩니다."
            }
            Self::Condolence => {
                "밤에 성불하지 않은 사망자의 직업을 도벽할 수 있습니다. 훔친 능력은 다음 밤까지 사용할 수 있습니다."
            }
        }
    }
}

pub const TIER3_ABILITIES: &[TierAbility] =
    &[TierAbility::RatingShield, TierAbility::SlowmodeBypass];
pub const TIER4_MAFIA_ABILITIES: &[TierAbility] = &[
    TierAbility::Lawless,
    TierAbility::NightRaid,
    TierAbility::Cleanup,
    TierAbility::Escape,
    TierAbility::Loudspeaker,
    TierAbility::Wanted,
    TierAbility::Directive,
    TierAbility::Hypocrisy,
    TierAbility::Concealment,
    TierAbility::Snipe,
    TierAbility::Poison,
    TierAbility::AllIn,
    TierAbility::Exorcism,
    TierAbility::TimeLimit,
];
/// 보조 마피아(마피아 본대가 아닌 마피아팀)의 4티어 풀.
pub const TIER4_MAFIA_SUPPORT_ABILITIES: &[TierAbility] = &[
    TierAbility::Loudspeaker,
    TierAbility::LastWill,
    TierAbility::Escape,
    TierAbility::Directive,
    TierAbility::TimeLimit,
    TierAbility::InsideMan,
];
pub const TIER4_CITIZEN_ABILITIES: &[TierAbility] =
    &[TierAbility::LastWill, TierAbility::Loudspeaker];

/// 4티어 풀은 시작 시점 역할로 정해진다: 마피아 본대 / 마피아팀 보조(역할별
/// 고유 능력 포함) / 그 외. 역할별 고유 능력은 여기서 공통 풀에 덧붙인다.
pub fn tier4_pool(role: Role) -> Vec<TierAbility> {
    if role == Role::CultLeader {
        let mut pool = TIER4_CITIZEN_ABILITIES.to_vec();
        pool.push(TierAbility::Directive);
        pool.push(TierAbility::TimeLimit);
        return pool;
    }
    if !role.is_mafia_team() {
        return TIER4_CITIZEN_ABILITIES.to_vec();
    }
    if role == Role::Mafia {
        return TIER4_MAFIA_ABILITIES.to_vec();
    }
    let mut pool = TIER4_MAFIA_SUPPORT_ABILITIES.to_vec();
    match role {
        Role::Spy => pool.extend([
            TierAbility::Autopsy,
            TierAbility::Assassin,
            TierAbility::Honeytrap,
        ]),
        Role::Fraudster => pool.push(TierAbility::Honeytrap),
        Role::Madam => pool.extend([TierAbility::Allure, TierAbility::Debut]),
        Role::Thief => pool.extend([TierAbility::Successor, TierAbility::Condolence]),
        _ => {}
    }
    pool
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractorGuessRoleGroup {
    Citizen,
    MafiaCultNeutral,
}

impl Default for ContractorGuessRoleGroup {
    fn default() -> Self {
        Self::Citizen
    }
}

impl ContractorGuessRoleGroup {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Citizen => "시민팀",
            Self::MafiaCultNeutral => "마피아·교주·중립",
        }
    }

    pub const fn component_value(self) -> &'static str {
        match self {
            Self::Citizen => "citizen",
            Self::MafiaCultNeutral => "other",
        }
    }

    pub fn from_component_value(value: &str) -> Option<Self> {
        match value {
            "citizen" => Some(Self::Citizen),
            "other" => Some(Self::MafiaCultNeutral),
            _ => None,
        }
    }
}

pub fn is_contractor_guess_role(role: Role) -> bool {
    CONTRACTOR_GUESS_ROLES.contains(&role) && !role.is_investigation_role()
}

pub const fn contractor_guess_role_group(role: Role) -> ContractorGuessRoleGroup {
    match role {
        Role::Mafia
        | Role::Witch
        | Role::Scientist
        | Role::Madam
        | Role::Thief
        | Role::Fraudster
        | Role::CultLeader
        | Role::Fanatic
        | Role::Joker => ContractorGuessRoleGroup::MafiaCultNeutral,
        _ => ContractorGuessRoleGroup::Citizen,
    }
}

pub fn contractor_guessable_roles() -> impl Iterator<Item = Role> {
    CONTRACTOR_GUESS_ROLES
        .iter()
        .copied()
        .filter(|role| is_contractor_guess_role(*role))
}

pub fn contractor_guessable_roles_for_group(
    group: ContractorGuessRoleGroup,
) -> impl Iterator<Item = Role> {
    contractor_guessable_roles().filter(move |role| contractor_guess_role_group(*role) == group)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NightResult {
    pub killed: Option<Player>,
    pub protected: Option<Player>,
    pub mafia_target: Option<Player>,
    pub police_target: Option<Player>,
    pub police_target_is_mafia: Option<bool>,
    pub thief_police_results: std::collections::HashMap<u64, String>,
    pub killed_players: Vec<Player>,
    pub detective_results: std::collections::HashMap<u64, String>,
    pub inspector_results: std::collections::HashMap<u64, String>,
    pub inspector_target_notices: std::collections::HashMap<u64, String>,
    pub civil_servant_results: std::collections::HashMap<u64, String>,
    pub paparazzi_results: std::collections::HashMap<u64, String>,
    pub fraudster_results: std::collections::HashMap<u64, String>,
    pub fraudster_contacts: Vec<u64>,
    /// [불침번] 군인이 막아낸 능력 알림.
    pub soldier_watch_results: std::collections::HashMap<u64, String>,
    /// [은폐] 마피아팀 처형 실패가 조용한 밤으로 가려졌는지. 치료·방탄
    /// 공개 문구를 숨긴다.
    #[serde(default)]
    pub quiet_night: bool,
    /// [야습] 관통된 자가 치료 의사 — 아침에 정체가 전체 공개된다.
    #[serde(default)]
    pub night_raid_reveals: Vec<Player>,
    /// [밀정] 이번 밤 결산으로 마피아와 접선한 보유자 (채널 접근 부여용).
    #[serde(default)]
    pub tier_ability_contacts: Vec<u64>,
    /// 티어 능력(무법·야습·수습) 활약 알림.
    pub tier_ability_results: std::collections::HashMap<u64, String>,
    /// 밤에 사망한 유언 보유자의 (이름, 유언) — 아침에 전체 공개.
    pub published_wills: Vec<(String, String)>,
    pub spy_results: std::collections::HashMap<u64, String>,
    pub spy_contacts: Vec<u64>,
    pub contractor_results: std::collections::HashMap<u64, String>,
    pub contractor_contacts: Vec<u64>,
    pub contractor_kills: Vec<Player>,
    pub witch_results: std::collections::HashMap<u64, String>,
    pub witch_contacts: Vec<u64>,
    pub godfather_results: std::collections::HashMap<u64, String>,
    pub godfather_contacts: Vec<u64>,
    pub graverobber_results: std::collections::HashMap<u64, Role>,
    pub terrorist_retaliations: Vec<(Player, Player)>,
    pub soldier_blocks: Vec<Player>,
    pub lover_sacrifices: Vec<(Player, Player)>,
    pub shaman_results: std::collections::HashMap<u64, String>,
    pub shaman_purifications: Vec<u64>,
    pub priest_results: std::collections::HashMap<u64, String>,
    pub priest_revives: Vec<Player>,
    pub agent_results: std::collections::HashMap<u64, String>,
    pub reporter_results: std::collections::HashMap<u64, String>,
    pub hacker_results: std::collections::HashMap<u64, String>,
    pub vigilante_results: std::collections::HashMap<u64, String>,
    pub vigilante_kills: Vec<Player>,
    pub mercenary_results: std::collections::HashMap<u64, String>,
    pub mercenary_kills: Vec<Player>,
    pub nurse_results: std::collections::HashMap<u64, String>,
    pub nurse_contacts: Vec<u64>,
    pub cult_results: std::collections::HashMap<u64, String>,
    pub fanatic_results: std::collections::HashMap<u64, String>,
    pub fanatic_inherits: Vec<u64>,
    pub gangster_results: std::collections::HashMap<u64, String>,
    pub cult_bells: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoteResult {
    pub executed: Option<Player>,
    pub tied: bool,
    pub skipped: bool,
    #[serde(default)]
    pub weighted_vote_counts: std::collections::HashMap<Option<u64>, i32>,
    pub vote_counts: std::collections::HashMap<Option<u64>, i32>,
    pub madam_seduced: Vec<Player>,
    pub madam_newly_contacted: Vec<Player>,
    pub blocked_voters: Vec<Player>,
    /// 도둑별 도벽 결과. 투표가 끝난 뒤에야 어떤 능력을 훔쳤는지 알려준다.
    #[serde(default)]
    pub thief_steal_results: std::collections::HashMap<u64, String>,
    #[serde(default)]
    pub thief_newly_contacted: Vec<Player>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfirmVoteResult {
    pub executed: Option<Player>,
    /// [도주] 처형 대신 도주한 플레이어. 다음날 투표 시작 때 사망한다.
    #[serde(default)]
    pub escaped: Option<Player>,
    pub approved: bool,
    pub tied: bool,
    pub blocked_by_politician: bool,
    pub extra_killed: Vec<Player>,
    #[serde(default)]
    pub weighted_vote_counts: std::collections::HashMap<bool, i32>,
    pub vote_counts: std::collections::HashMap<bool, i32>,
    pub judge: Option<Player>,
    pub judge_choice: Option<bool>,
    pub decided_by_judge: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn korean_particles_follow_the_final_consonant() {
        assert_eq!(korean_object_particle("의사"), "를");
        assert_eq!(korean_object_particle("공무원"), "을");
        assert_eq!(korean_ro_particle("의사"), "로");
        assert_eq!(korean_ro_particle("연인"), "으로");
        // ㄹ 받침은 "로"를 쓴다.
        assert_eq!(korean_ro_particle("서울"), "로");
        assert_eq!(korean_ro_particle("abc"), "(으)로");
    }

    /// 공무원 조회 목록: 경찰 계열·시민·공무원 자신 제외, 시민팀만, 셀렉트 상한 이하.
    #[test]
    fn civil_servant_query_roles_exclude_police_lineage_and_citizen() {
        assert!(CIVIL_SERVANT_QUERY_ROLES.len() <= 25);
        for role in CIVIL_SERVANT_QUERY_ROLES {
            assert!(!role.is_investigation_role(), "{role:?}");
            assert!(!role.is_mafia_team(), "{role:?}");
            assert_ne!(*role, Role::Citizen);
            assert_ne!(*role, Role::CivilServant);
            assert_ne!(*role, Role::CultLeader);
            assert_ne!(*role, Role::Fanatic);
            assert_ne!(*role, Role::Joker);
        }
        assert!(is_civil_servant_query_role(Role::Doctor));
        assert!(is_civil_servant_query_role(Role::Paparazzi));
        assert!(!is_civil_servant_query_role(Role::Police));
    }

    #[test]
    fn contractor_guess_roles_are_partitioned_and_exclude_investigation_roles() {
        let all = contractor_guessable_roles().collect::<HashSet<_>>();
        let citizen = contractor_guessable_roles_for_group(ContractorGuessRoleGroup::Citizen)
            .collect::<HashSet<_>>();
        let other =
            contractor_guessable_roles_for_group(ContractorGuessRoleGroup::MafiaCultNeutral)
                .collect::<HashSet<_>>();
        let grouped = citizen.union(&other).copied().collect::<HashSet<_>>();

        assert!(citizen.is_disjoint(&other));
        assert_eq!(grouped, all);
        assert!(citizen.len() <= 25);
        assert!(other.len() <= 25);
        for role in [Role::Police, Role::Agent, Role::Vigilante, Role::Inspector] {
            assert!(!is_contractor_guess_role(role));
        }
        assert!(is_contractor_guess_role(Role::Detective));
    }
}
