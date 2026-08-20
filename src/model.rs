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
