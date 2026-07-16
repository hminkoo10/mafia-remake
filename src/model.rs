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
