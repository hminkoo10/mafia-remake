// game/mod.rs
// 역할: MafiaGame 구조체 정의, 생성자, 기본 플레이어 조회, 팀 판별, 승리 조건,
//        공유 유틸리티 메서드 (majority_target, mark_dead, ensure_fanatic_reincarnation 등)

#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

pub mod actions;
pub mod actors;
pub mod resolve;
pub mod vote;

use crate::model::{Phase, Player, Role, TierAbility, Winner};
use crate::system_random;
use anyhow::{Result, bail};
use rand::{RngCore, seq::SliceRandom};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct MafiaGame {
    pub players: Vec<Player>,
    players_by_id: HashMap<u64, usize>,
    pub phase: Phase,
    pub day_number: u32,
    pub mafia_targets: HashMap<u64, u64>,
    pub mafia_display_targets: HashMap<u64, u64>,
    pub doctor_targets: HashMap<u64, u64>,
    pub nurse_targets: HashMap<u64, u64>,
    pub nurse_prescription_targets: HashMap<u64, u64>,
    pub nurse_contacted: HashSet<u64>,
    pub nurse_contacts_this_night: Vec<u64>,
    pub gangster_targets: HashMap<u64, u64>,
    pub gangster_used_ids: HashSet<u64>,
    pub gangster_blocked_vote_days: HashMap<u64, u32>,
    pub police_targets: HashMap<u64, u64>,
    pub thief_police_targets: HashMap<u64, u64>,
    pub inspector_targets: HashMap<u64, u64>,
    pub inspector_used_ids: HashSet<u64>,
    /// 공무원 조회: actor → 이번 밤 조회할 직업
    pub civil_servant_targets: HashMap<u64, Role>,
    /// 파파라치 이슈: 이슈가 이미 발동한 day_number들. 하루의 첫 직업 정보만 공유된다.
    pub paparazzi_shared_days: HashSet<u32>,
    pub vigilante_targets: HashMap<u64, u64>,
    /// 낮 조사가 변장 사기꾼을 평가해 생긴 "속임" 알림 (사기꾼 id, 조사자 이름).
    /// 밤 시작에 전달된다.
    pub pending_deception_notices: Vec<(u64, String)>,
    /// [불침번] 군인이 이번 밤 막아낸 능력 알림 (군인 id, 메시지). 밤 결산 때 전달.
    pub pending_soldier_watch_notices: Vec<(u64, String)>,
    /// [불침번]에 막혀 변장에 실패한 사기꾼 → 막아낸 군인.
    pub fraudster_blocked_by_soldier: HashMap<u64, u64>,
    /// 개인 티어 (2~6). 능력은 tier_abilities에 별도 저장.
    pub player_tiers: HashMap<u64, u8>,
    /// 티어 능력 보유 목록. 5티어는 2개, 6티어는 3개까지 들어간다.
    pub tier_abilities: HashMap<u64, Vec<TierAbility>>,
    /// [유언] 작성된 유언.
    pub last_wills: HashMap<u64, String>,
    /// [도주] 도주한 플레이어 → 도주한 날. 다음날 투표 시작 때 사망 처리한다.
    pub escaped_on_day: HashMap<u64, u32>,
    /// 티어 능력(무법·야습) 발동 알림 대기열. 밤 결산 때 전달한다.
    pub pending_tier_ability_notices: Vec<(u64, String)>,
    /// [수습]으로 직업이 '시민'으로 가려진 사망자. 실제 role은 유지하고 판정만
    /// 가린다 — 역할 기반 내부 로직(요원 지령 등)이 깨지지 않게 하기 위함.
    pub cleanup_masked_ids: HashSet<u64>,
    /// [확성]이 사용된 밤(day_number). 밤마다 보유자 전체가 1회만 쓸 수 있다.
    pub loudspeaker_used_days: HashSet<u32>,
    /// [확성] 이미 사용해 소진된 보유자 (인당 게임 중 1회).
    pub loudspeaker_spent_ids: HashSet<u64>,
    /// [은폐] 이번 밤 마피아팀 처형 실패가 조용한 밤으로 가려졌는지.
    pub concealed_kill_failure: bool,
    /// [저격] 전날 밤 마피아팀 처형이 실패해 이번 밤 관통이 장전된 상태인지.
    pub snipe_armed: bool,
    /// [야습] 이번 밤 관통된 자가 치료 의사(아침에 전체 공개).
    pub pending_night_raid_reveals: Vec<Player>,
    /// [독살] 중독된 플레이어 → 사망하는 밤의 day_number.
    pub poisoned_death_days: HashMap<u64, u32>,
    /// [미인계] 이번 밤 이미 알림을 보낸 (사용자, 보유자) 쌍.
    pub honeytrap_noticed: HashSet<(u64, u64)>,
    /// 사립탐정 실시간 추적: (사탐, 추적 대상) → 마지막으로 알린 손 위치.
    pub detective_live_last: HashMap<(u64, u64), u64>,
    /// 사립탐정에게 즉시 보낼 실시간 추적 알림 대기열.
    pub pending_detective_live_notices: Vec<(u64, String)>,
    /// [데뷔] 투표권이 한 표 깎인 플레이어.
    pub debut_vote_penalty_ids: HashSet<u64>,
    /// [후계자] 등 밤 결산에 채널 접근을 부여해야 하는 접선자 대기열.
    pub pending_tier_ability_contacts: Vec<u64>,
    /// [조문] 이번 밤에 도벽한 도둑 — 이번 밤 정리에서 훔친 직업을 지우지 않는다.
    pub condolence_stolen_this_night: HashSet<u64>,
    /// [망각술] 저주 상태로 죽어 사망 능력이 봉인된 플레이어.
    pub amnesia_suppressed_ids: HashSet<u64>,
    /// [분석] 부활 시 전달할 공격자 정보.
    pub pending_analysis_notices: HashMap<u64, String>,
    /// [직감] 역할 DM에 함께 전달할 시민팀 직업 힌트.
    pub intuition_hints: HashMap<u64, String>,
    pub vigilante_known_enemy_ids: HashMap<u64, HashSet<u64>>,
    pub vigilante_investigation_used_ids: HashSet<u64>,
    pub vigilante_execution_used_ids: HashSet<u64>,
    pub reporter_targets: HashMap<u64, u64>,
    pub reporter_skip_submitted: HashSet<u64>,
    pub reporter_used_ids: HashSet<u64>,
    pub hacker_targets: HashMap<u64, u64>,
    /// 해커 → (해킹 대상, 해킹한 날). 결과는 다음 밤 시작에 전달되지만 파파라치
    /// 이슈의 "하루 한 번"은 해킹이 일어난 날 기준으로 계산해야 한다.
    pub hacker_pending_results: HashMap<u64, (u64, u32)>,
    pub hacker_used_ids: HashSet<u64>,
    pub hacker_proxy_targets: HashMap<u64, u64>,
    pub psychologist_used_days: HashMap<u64, u32>,
    pub hypnotist_targets: HashMap<u64, u64>,
    pub hypnotized_targets: HashMap<u64, HashSet<u64>>,
    pub hypnotist_skip_night_days: HashMap<u64, u32>,
    pub mercenary_client_ids: HashMap<u64, u64>,
    pub mercenary_contract_received_ids: HashSet<u64>,
    pub mercenary_armed_ids: HashSet<u64>,
    pub mercenary_targets: HashMap<u64, u64>,
    pub detective_targets: HashMap<u64, u64>,
    pub shaman_targets: HashMap<u64, u64>,
    pub priest_targets: HashMap<u64, u64>,
    pub priest_used_ids: HashSet<u64>,
    pub spy_targets: HashMap<u64, Vec<u64>>,
    pub spy_bonus_pending: HashSet<u64>,
    pub spy_contacts_this_night: Vec<u64>,
    pub contractor_contracts: HashMap<u64, ((u64, Role), (u64, Role))>,
    pub contractor_contacts_this_night: Vec<u64>,
    pub thief_used_days: HashMap<u64, u32>,
    pub thief_stolen_roles: HashMap<u64, Role>,
    pub thief_contacted: HashSet<u64>,
    pub witch_targets: HashMap<u64, u64>,
    pub witch_contacted: HashSet<u64>,
    pub witch_contacts_this_night: Vec<u64>,
    pub witch_curse_applied_actor_ids: HashSet<u64>,
    pub godfather_targets: HashMap<u64, u64>,
    pub terrorist_targets: HashMap<u64, u64>,
    pub terrorist_execution_targets: HashMap<u64, u64>,
    pub terrorist_action_submitted: HashSet<u64>,
    pub frog_user_ids: HashSet<u64>,
    pub soldier_bulletproof_used: HashSet<u64>,
    pub purified_dead_ids: HashSet<u64>,
    pub publicly_revealed_ids: HashSet<u64>,
    pub agent_discovered_ids: HashSet<u64>,
    pub day_votes: HashMap<u64, Option<u64>>,
    pub confirm_votes: HashMap<u64, bool>,
    pub police_result_announced: bool,
    pub spy_contacted: HashSet<u64>,
    /// 사기꾼 변장: fraudster → (사기 대상 시민, 변장 직업)
    pub fraudster_disguises: HashMap<u64, (u64, Role)>,
    pub fraudster_contacted: HashSet<u64>,
    /// 이번 밤 마피아팀 공격으로 교섭이 발동한 사기꾼: (id, 본인이 표적이었는지)
    pub fraudster_contacts_this_night: Vec<(u64, bool)>,
    pub contractor_contacted: HashSet<u64>,
    pub scientist_contacted: HashSet<u64>,
    pub scientist_revive_used_ids: HashSet<u64>,
    pub scientist_pending_revive_ids: HashSet<u64>,
    pub madam_contacted: HashSet<u64>,
    pub madam_seduced_ids: HashSet<u64>,
    pub madam_seduction_release_days: HashMap<u64, u32>,
    pub godfather_contacted: HashSet<u64>,
    pub revealed_judge_ids: HashSet<u64>,
    pub cult_targets: HashMap<u64, u64>,
    pub fanatic_targets: HashMap<u64, u64>,
    pub culted_ids: HashSet<u64>,
    pub cult_bells_this_night: u32,
    pub joker_won: bool,
    pub joker_winner_id: Option<u64>,
    pub death_order: Vec<u64>,
    pub rating_events: HashMap<u64, Vec<RatingEvent>>,
    pub rating_action_counts: HashMap<u64, u32>,
}

#[derive(Debug, Clone, Default)]
pub struct GameCounts {
    pub mafia_count: usize,
    pub doctor_count: usize,
    pub police_count: usize,
    pub agent_count: usize,
    pub vigilante_count: usize,
    pub inspector_count: usize,
    pub joker_count: usize,
    pub special_roles: Vec<Role>,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerAssignmentHistory {
    pub games: i64,
    pub mafia_role_games: i64,
    pub role_counts: HashMap<Role, i64>,
    pub recent_roles: Vec<Role>,
}

#[derive(Debug, Clone)]
pub struct RatingEvent {
    pub points: i64,
    pub reason: String,
}

impl MafiaGame {
    pub fn new(
        players: Vec<(u64, String)>,
        mafia_count: usize,
        doctor_count: usize,
        police_count: usize,
        special_roles: Vec<Role>,
    ) -> Result<Self> {
        Self::new_with_counts(
            players,
            GameCounts {
                mafia_count,
                doctor_count,
                police_count,
                special_roles,
                ..Default::default()
            },
        )
    }

    pub fn new_with_counts(players: Vec<(u64, String)>, counts: GameCounts) -> Result<Self> {
        Self::new_with_counts_balanced(players, counts, &HashMap::new())
    }

    pub fn new_with_counts_balanced(
        players: Vec<(u64, String)>,
        counts: GameCounts,
        assignment_history: &HashMap<u64, PlayerAssignmentHistory>,
    ) -> Result<Self> {
        validate_counts(&players, &counts)?;

        let mut roles = Vec::with_capacity(players.len());
        roles.extend(std::iter::repeat_n(Role::Mafia, counts.mafia_count));
        roles.extend(std::iter::repeat_n(Role::Doctor, counts.doctor_count));
        roles.extend(std::iter::repeat_n(Role::Police, counts.police_count));
        roles.extend(std::iter::repeat_n(Role::Agent, counts.agent_count));
        roles.extend(std::iter::repeat_n(Role::Vigilante, counts.vigilante_count));
        roles.extend(std::iter::repeat_n(Role::Inspector, counts.inspector_count));
        roles.extend(std::iter::repeat_n(Role::Joker, counts.joker_count));
        roles.extend(counts.special_roles);
        roles.extend(std::iter::repeat_n(
            Role::Citizen,
            players.len() - roles.len(),
        ));

        let players = assign_roles_balanced(players, roles, assignment_history);
        let players_by_id = players
            .iter()
            .enumerate()
            .map(|(index, player)| (player.user_id, index))
            .collect();

        let mut game = Self {
            players,
            players_by_id,
            phase: Phase::Night,
            day_number: 1,
            mafia_targets: HashMap::new(),
            mafia_display_targets: HashMap::new(),
            doctor_targets: HashMap::new(),
            nurse_targets: HashMap::new(),
            nurse_prescription_targets: HashMap::new(),
            nurse_contacted: HashSet::new(),
            nurse_contacts_this_night: Vec::new(),
            gangster_targets: HashMap::new(),
            gangster_used_ids: HashSet::new(),
            gangster_blocked_vote_days: HashMap::new(),
            police_targets: HashMap::new(),
            thief_police_targets: HashMap::new(),
            inspector_targets: HashMap::new(),
            inspector_used_ids: HashSet::new(),
            civil_servant_targets: HashMap::new(),
            paparazzi_shared_days: HashSet::new(),
            vigilante_targets: HashMap::new(),
            pending_deception_notices: Vec::new(),
            pending_soldier_watch_notices: Vec::new(),
            fraudster_blocked_by_soldier: HashMap::new(),
            player_tiers: HashMap::new(),
            tier_abilities: HashMap::new(),
            last_wills: HashMap::new(),
            escaped_on_day: HashMap::new(),
            pending_tier_ability_notices: Vec::new(),
            cleanup_masked_ids: HashSet::new(),
            loudspeaker_used_days: HashSet::new(),
            loudspeaker_spent_ids: HashSet::new(),
            concealed_kill_failure: false,
            snipe_armed: false,
            pending_night_raid_reveals: Vec::new(),
            poisoned_death_days: HashMap::new(),
            honeytrap_noticed: HashSet::new(),
            detective_live_last: HashMap::new(),
            pending_detective_live_notices: Vec::new(),
            debut_vote_penalty_ids: HashSet::new(),
            pending_tier_ability_contacts: Vec::new(),
            condolence_stolen_this_night: HashSet::new(),
            amnesia_suppressed_ids: HashSet::new(),
            pending_analysis_notices: HashMap::new(),
            intuition_hints: HashMap::new(),
            vigilante_known_enemy_ids: HashMap::new(),
            vigilante_investigation_used_ids: HashSet::new(),
            vigilante_execution_used_ids: HashSet::new(),
            reporter_targets: HashMap::new(),
            reporter_skip_submitted: HashSet::new(),
            reporter_used_ids: HashSet::new(),
            hacker_targets: HashMap::new(),
            hacker_pending_results: HashMap::new(),
            hacker_used_ids: HashSet::new(),
            hacker_proxy_targets: HashMap::new(),
            psychologist_used_days: HashMap::new(),
            hypnotist_targets: HashMap::new(),
            hypnotized_targets: HashMap::new(),
            hypnotist_skip_night_days: HashMap::new(),
            mercenary_client_ids: HashMap::new(),
            mercenary_contract_received_ids: HashSet::new(),
            mercenary_armed_ids: HashSet::new(),
            mercenary_targets: HashMap::new(),
            detective_targets: HashMap::new(),
            shaman_targets: HashMap::new(),
            priest_targets: HashMap::new(),
            priest_used_ids: HashSet::new(),
            spy_targets: HashMap::new(),
            spy_bonus_pending: HashSet::new(),
            spy_contacts_this_night: Vec::new(),
            contractor_contracts: HashMap::new(),
            contractor_contacts_this_night: Vec::new(),
            thief_used_days: HashMap::new(),
            thief_stolen_roles: HashMap::new(),
            thief_contacted: HashSet::new(),
            witch_targets: HashMap::new(),
            witch_contacted: HashSet::new(),
            witch_contacts_this_night: Vec::new(),
            witch_curse_applied_actor_ids: HashSet::new(),
            godfather_targets: HashMap::new(),
            terrorist_targets: HashMap::new(),
            terrorist_execution_targets: HashMap::new(),
            terrorist_action_submitted: HashSet::new(),
            frog_user_ids: HashSet::new(),
            soldier_bulletproof_used: HashSet::new(),
            purified_dead_ids: HashSet::new(),
            publicly_revealed_ids: HashSet::new(),
            agent_discovered_ids: HashSet::new(),
            day_votes: HashMap::new(),
            confirm_votes: HashMap::new(),
            police_result_announced: false,
            spy_contacted: HashSet::new(),
            fraudster_disguises: HashMap::new(),
            fraudster_contacted: HashSet::new(),
            fraudster_contacts_this_night: Vec::new(),
            contractor_contacted: HashSet::new(),
            scientist_contacted: HashSet::new(),
            scientist_revive_used_ids: HashSet::new(),
            scientist_pending_revive_ids: HashSet::new(),
            madam_contacted: HashSet::new(),
            madam_seduced_ids: HashSet::new(),
            madam_seduction_release_days: HashMap::new(),
            godfather_contacted: HashSet::new(),
            revealed_judge_ids: HashSet::new(),
            cult_targets: HashMap::new(),
            fanatic_targets: HashMap::new(),
            culted_ids: HashSet::new(),
            cult_bells_this_night: 0,
            joker_won: false,
            joker_winner_id: None,
            death_order: Vec::new(),
            rating_events: HashMap::new(),
            rating_action_counts: HashMap::new(),
        };
        game.assign_mercenary_clients();
        game.assign_fraudster_disguises();
        Ok(game)
    }

    pub fn mark_rating_action(&mut self, user_id: u64) {
        *self.rating_action_counts.entry(user_id).or_default() += 1;
    }

    pub fn record_rating_event(&mut self, user_id: u64, points: i64, reason: impl Into<String>) {
        if points == 0 {
            return;
        }
        self.rating_events
            .entry(user_id)
            .or_default()
            .push(RatingEvent {
                points,
                reason: reason.into(),
            });
    }

    pub fn get_player(&self, user_id: u64) -> Option<&Player> {
        self.players_by_id
            .get(&user_id)
            .and_then(|index| self.players.get(*index))
    }

    pub fn get_player_mut(&mut self, user_id: u64) -> Option<&mut Player> {
        let index = *self.players_by_id.get(&user_id)?;
        self.players.get_mut(index)
    }

    pub fn alive_players(&self) -> Vec<&Player> {
        self.players.iter().filter(|player| player.alive).collect()
    }

    pub fn dead_players(&self) -> Vec<&Player> {
        self.players.iter().filter(|player| !player.alive).collect()
    }

    pub fn unpurified_dead_players(&self) -> Vec<&Player> {
        self.players
            .iter()
            .filter(|player| !player.alive && !self.purified_dead_ids.contains(&player.user_id))
            .collect()
    }

    pub fn alive_role_count(&self, role: Role) -> usize {
        self.players
            .iter()
            .filter(|player| player.alive && player.role == role)
            .count()
    }

    pub fn is_mafia_team(&self, player: &Player) -> bool {
        player.role.is_mafia_team()
    }

    pub fn is_cult_team(&self, player: &Player) -> bool {
        player.role == Role::CultLeader || self.culted_ids.contains(&player.user_id)
    }

    pub fn is_known_mafia_team(&self, player: &Player) -> bool {
        match player.role {
            Role::Mafia | Role::Villain => true,
            Role::Spy => self.spy_contacted.contains(&player.user_id),
            Role::Contractor => self.contractor_contacted.contains(&player.user_id),
            Role::Fraudster => self.fraudster_contacted.contains(&player.user_id),
            Role::Thief => self.thief_contacted.contains(&player.user_id),
            Role::Witch => self.witch_contacted.contains(&player.user_id),
            Role::Scientist => self.scientist_contacted.contains(&player.user_id),
            Role::Madam => self.madam_contacted.contains(&player.user_id),
            Role::Godfather => self.godfather_contacted.contains(&player.user_id),
            _ => false,
        }
    }

    pub fn is_police_detected_mafia_team(&self, player: &Player) -> bool {
        if self.is_hypocrite_active(player) {
            return false;
        }
        match player.role {
            Role::Godfather => false,
            _ => self.is_known_mafia_team(player),
        }
    }

    pub fn is_citizen_team(&self, player: &Player) -> bool {
        !self.is_mafia_team(player) && !self.is_cult_team(player) && player.role != Role::Joker
    }

    pub(crate) fn terrorist_retaliation_target(&self, terrorist: &Player) -> Option<Player> {
        if !self.has_terrorist_ability(terrorist) {
            return None;
        }
        // [망각술] 저주 상태로 죽은 테러리스트는 지목 반격이 발동하지 않는다.
        if self.amnesia_suppressed_ids.contains(&terrorist.user_id) {
            return None;
        }
        let target_id = self.terrorist_targets.get(&terrorist.user_id).copied()?;
        let target = self.get_player(target_id)?.clone();
        if !target.alive {
            return None;
        }
        self.terrorist_blast_allowed(terrorist, &target)
            .then_some(target)
    }

    /// 지목 반격이 이 대상을 죽일 수 있는가. 접선하지 않은 보조 마피아는
    /// 아직 시민처럼 보이므로 반격에 터지지 않는다.
    fn terrorist_blast_allowed(&self, terrorist: &Player, target: &Player) -> bool {
        if self.is_mafia_team(target) && !self.is_known_mafia_team(target) {
            return false;
        }
        self.retaliation_team_key(terrorist) != self.retaliation_team_key(target)
    }

    pub fn begin_terrorist_final_defense(&mut self, actor_id: u64) -> Vec<Player> {
        if self.phase != Phase::FinalDefense {
            return Vec::new();
        }
        let Some(actor) = self.get_player(actor_id) else {
            return Vec::new();
        };
        if !actor.alive || !self.has_terrorist_ability(actor) {
            return Vec::new();
        }
        self.terrorist_execution_targets.remove(&actor_id);
        let mut targets = self
            .alive_players()
            .into_iter()
            .filter(|player| player.user_id != actor_id)
            .cloned()
            .collect::<Vec<_>>();
        targets.sort_by_key(|player| player.name.to_lowercase());
        targets
    }

    pub fn submit_terrorist_final_defense_target(
        &mut self,
        actor_id: u64,
        target_id: u64,
    ) -> Result<String> {
        if self.phase != Phase::FinalDefense {
            bail!("지금은 최후의 반론 시간이 아닙니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if !self.has_terrorist_ability(&actor) {
            bail!("테러리스트 능력이 없습니다.");
        }
        if actor_id == target_id {
            bail!("테러리스트는 자기 자신을 지목할 수 없습니다.");
        }
        let target = self.require_alive(target_id)?.clone();
        self.terrorist_execution_targets.insert(actor_id, target_id);
        Ok(format!("습격 대상: {}", target.name))
    }

    pub(crate) fn terrorist_execution_target(&self, terrorist: &Player) -> Option<Player> {
        if !self.has_terrorist_ability(terrorist) {
            return None;
        }
        let target_id = self
            .terrorist_execution_targets
            .get(&terrorist.user_id)
            .copied()?;
        let target = self.get_player(target_id)?.clone();
        if !target.alive {
            return None;
        }
        if terrorist.role == Role::Terrorist {
            self.is_known_mafia_team(&target).then_some(target)
        } else {
            self.terrorist_blast_allowed(terrorist, &target)
                .then_some(target)
        }
    }

    fn has_terrorist_ability(&self, player: &Player) -> bool {
        player.role == Role::Terrorist
            || (player.role == Role::Thief
                && self.thief_stolen_roles.get(&player.user_id) == Some(&Role::Terrorist))
    }

    fn retaliation_team_key(&self, player: &Player) -> &'static str {
        if self.is_cult_team(player) {
            "cult"
        } else if self.is_mafia_team(player) {
            "mafia"
        } else if player.role == Role::Joker {
            "joker"
        } else {
            "citizen"
        }
    }

    pub fn is_frog(&self, player: &Player) -> bool {
        player.alive && self.frog_user_ids.contains(&player.user_id)
    }

    fn hypnotist_can_act_at_night(&self, player: &Player) -> bool {
        player.alive
            && player.role == Role::Hypnotist
            && self.hypnotist_skip_night_days.get(&player.user_id) != Some(&self.day_number)
            && self
                .players
                .iter()
                .any(|target| target.alive && target.user_id != player.user_id)
    }

    fn hypnotist_reveal_text(&self, target: &Player) -> String {
        if self.team_key(target) == "citizen" {
            "시민팀".to_string()
        } else {
            self.visible_role(target).value().to_string()
        }
    }

    pub fn mercenary_client(&self, mercenary_id: u64) -> Option<&Player> {
        let client_id = self.mercenary_client_ids.get(&mercenary_id)?;
        self.get_player(*client_id)
    }

    pub fn mercenary_for_client(&self, client_id: u64) -> Option<&Player> {
        self.mercenary_client_ids
            .iter()
            .find_map(|(mercenary_id, mapped_client_id)| {
                (*mapped_client_id == client_id)
                    .then(|| self.get_player(*mercenary_id))
                    .flatten()
            })
    }

    pub fn receive_mercenary_contracts(&mut self) -> Vec<(Player, Player)> {
        let pairs = self
            .mercenary_client_ids
            .iter()
            .filter_map(|(mercenary_id, client_id)| {
                let mercenary = self.get_player(*mercenary_id)?;
                let client = self.get_player(*client_id)?;
                (mercenary.alive && client.alive).then(|| (mercenary.clone(), client.clone()))
            })
            .collect::<Vec<_>>();
        let mut newly_received = Vec::new();
        for (mercenary, client) in pairs {
            if self
                .mercenary_contract_received_ids
                .insert(mercenary.user_id)
            {
                newly_received.push((mercenary, client));
            }
        }
        newly_received
    }

    fn assign_mercenary_clients(&mut self) {
        let mercenary_ids = self
            .players
            .iter()
            .filter(|player| player.role == Role::Mercenary)
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        let mut rng = system_random::rng();
        for mercenary_id in mercenary_ids {
            let mut candidates = self
                .players
                .iter()
                .filter(|player| player.user_id != mercenary_id && self.is_citizen_team(player))
                .map(|player| player.user_id)
                .collect::<Vec<_>>();
            candidates.shuffle(&mut rng);
            if let Some(client_id) = candidates.into_iter().next() {
                self.mercenary_client_ids.insert(mercenary_id, client_id);
            }
        }
    }

    /// [사기] 게임 시작 시 사기꾼마다 시민팀 한 명을 무작위로 골라 정체를 알아내고
    /// 그 직업으로 변장한다.
    fn assign_fraudster_disguises(&mut self) {
        let fraudster_ids = self
            .players
            .iter()
            .filter(|player| player.role == Role::Fraudster)
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        let mut rng = system_random::rng();
        for fraudster_id in fraudster_ids {
            let mut candidates = self
                .players
                .iter()
                .filter(|player| player.user_id != fraudster_id && self.is_citizen_team(player))
                .map(|player| (player.user_id, player.role))
                .collect::<Vec<_>>();
            candidates.shuffle(&mut rng);
            if let Some((target_id, target_role)) = candidates.into_iter().next() {
                // [불침번] 군인을 고르면 사기가 무효가 되고 군인이 사기꾼의 정체를 안다.
                if target_role == Role::Soldier {
                    self.fraudster_blocked_by_soldier
                        .insert(fraudster_id, target_id);
                } else {
                    self.fraudster_disguises
                        .insert(fraudster_id, (target_id, target_role));
                }
            }
        }
    }

    /// 개인 티어 배정: 2티어 50% / 3티어 35% / 4티어 15%. 같은 능력이 여러
    /// 명에게 겹칠 수 있다. 4티어 풀은 소속에 따라 다르다 (마피아 본대 /
    /// 보조 마피아 / 그 외).
    /// 무작위성이 게임 로직 테스트를 흔들지 않도록 생성자가 아니라 실제 게임
    /// 시작(start_game)에서 호출한다.
    pub fn assign_tier_abilities(&mut self) {
        use crate::model::{TIER3_ABILITIES, tier4_pool};
        let order = self.players.clone();
        let mut rng = system_random::rng();
        for player in order {
            let roll = rng.next_u64() % 100;
            let tier: u8 = if roll < 40 {
                2
            } else if roll < 70 {
                3
            } else if roll < 85 {
                4
            } else if roll < 95 {
                5
            } else {
                6
            };
            self.player_tiers.insert(player.user_id, tier);
            let abilities: Vec<TierAbility> = match tier {
                3 => {
                    vec![TIER3_ABILITIES[(rng.next_u64() % TIER3_ABILITIES.len() as u64) as usize]]
                }
                4..=6 => {
                    // 5티어는 2개, 6티어는 3개. 4티어 이상 풀이 그보다 작으면
                    // (예: 시민팀 풀은 유언·확성 2개) 3티어 능력으로 채운다.
                    let want = tier as usize - 3;
                    let mut pool = tier4_pool(player.role);
                    pool.shuffle(&mut rng);
                    if pool.len() < want {
                        let mut filler = TIER3_ABILITIES.to_vec();
                        filler.shuffle(&mut rng);
                        pool.extend(filler);
                    }
                    pool.truncate(want.min(pool.len()));
                    pool
                }
                _ => Vec::new(),
            };
            if !abilities.is_empty() {
                self.tier_abilities.insert(player.user_id, abilities);
            }
        }
        self.prepare_intuition_hints();
    }

    /// [직감] 보유 청부업자에게 시민팀 한 명의 직업 힌트를 만든다. 역할 DM에
    /// 함께 전달돼 첫 밤 청부 예측부터 쓸 수 있다.
    pub(crate) fn prepare_intuition_hints(&mut self) {
        use rand::prelude::IndexedRandom;
        let holders = self
            .players
            .iter()
            .filter(|player| {
                player.alive && self.has_tier_ability(player.user_id, TierAbility::Intuition)
            })
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        if holders.is_empty() {
            return;
        }
        let citizens = self
            .players
            .iter()
            .filter(|player| self.is_citizen_team(player))
            .cloned()
            .collect::<Vec<_>>();
        let mut rng = system_random::rng();
        for holder_id in holders {
            let Some(target) = citizens.choose(&mut rng) else {
                continue;
            };
            self.intuition_hints.insert(
                holder_id,
                format!(
                    "[직감] {}님의 직업은 {}입니다.",
                    target.name,
                    target.role.value()
                ),
            );
        }
    }

    /// [부검] 사망자가 생길 때마다 보유자(스파이)가 자동 조사한다.
    pub(crate) fn queue_autopsy_notices(&mut self, dead: &[Player]) {
        if dead.is_empty() {
            return;
        }
        let holders = self
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && !self.is_frog(player)
                    && self.has_tier_ability(player.user_id, TierAbility::Autopsy)
            })
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        if holders.is_empty() {
            return;
        }
        for victim in dead {
            let Some(real) = self.get_player(victim.user_id) else {
                continue;
            };
            let line = format!(
                "[부검] {}님의 직업은 {}이었습니다.",
                real.name,
                real.role.value()
            );
            for holder_id in &holders {
                if *holder_id == victim.user_id {
                    continue;
                }
                self.pending_tier_ability_notices
                    .push((*holder_id, line.clone()));
            }
        }
    }

    /// [자객] 마피아팀에 혼자 남은 스파이 보유자가 이번 밤 조사한 대상들.
    pub(crate) fn assassin_execution_targets(&self) -> Vec<Player> {
        let alive_team = self
            .players
            .iter()
            .filter(|player| player.alive && self.is_mafia_team(player) && !self.is_frog(player))
            .collect::<Vec<_>>();
        if alive_team.len() != 1 {
            return Vec::new();
        }
        let spy = alive_team[0];
        if spy.role != Role::Spy || !self.has_tier_ability(spy.user_id, TierAbility::Assassin) {
            return Vec::new();
        }
        self.spy_targets
            .get(&spy.user_id)
            .map(|targets| {
                targets
                    .iter()
                    .filter_map(|target_id| self.get_player(*target_id).cloned())
                    .filter(|target| target.alive)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// [미인계] 시민팀 능력이 보유자를 대상으로 하면 보유자가 사용자의 직업을
    /// 알게 된다. 요원은 발동하지 않고, 같은 (사용자, 보유자) 쌍은 밤마다 한
    /// 번만 알린다.
    pub(crate) fn note_honeytrap_use(&mut self, actor_id: u64, target_id: u64) {
        if actor_id == target_id {
            return;
        }
        let Some(actor) = self.get_player(actor_id).cloned() else {
            return;
        };
        let Some(target) = self.get_player(target_id).cloned() else {
            return;
        };
        if !target.alive
            || self.is_frog(&target)
            || !self.has_tier_ability(target_id, TierAbility::Honeytrap)
        {
            return;
        }
        if !self.is_citizen_team(&actor) || actor.role == Role::Agent {
            return;
        }
        if !self.honeytrap_noticed.insert((actor_id, target_id)) {
            return;
        }
        self.pending_tier_ability_notices.push((
            target_id,
            format!(
                "[미인계] 당신에게 능력을 사용한 {}님의 직업은 {}입니다.",
                actor.name,
                actor.role.value()
            ),
        ));
    }

    /// [망각술] 살아있는 보유 마녀 목록.
    pub(crate) fn amnesia_witch_ids(&self) -> Vec<u64> {
        self.players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Witch
                    && !self.is_frog(player)
                    && self.has_tier_ability(player.user_id, TierAbility::Amnesia)
            })
            .map(|player| player.user_id)
            .collect()
    }

    /// [분석] 부활한 과학자에게 전달할 공격자 정보를 꺼낸다.
    pub fn take_analysis_notice(&mut self, user_id: u64) -> Option<String> {
        self.pending_analysis_notices.remove(&user_id)
    }

    /// [후계자] 마피아 본대가 전멸하면 보유 도둑이 마피아가 된다.
    pub(crate) fn ensure_thief_succession(&mut self) -> Vec<u64> {
        if self
            .players
            .iter()
            .any(|player| player.alive && player.role == Role::Mafia)
        {
            return Vec::new();
        }
        let ids = self
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Thief
                    && !self.is_frog(player)
                    && self.has_tier_ability(player.user_id, TierAbility::Successor)
            })
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        for id in &ids {
            if let Some(player) = self.get_player_mut(*id) {
                player.role = Role::Mafia;
            }
            self.thief_contacted.insert(*id);
            self.pending_tier_ability_notices.push((
                *id,
                "[후계자] 마피아의 능력을 이어받아 마피아가 되었습니다.".to_string(),
            ));
            self.pending_tier_ability_contacts.push(*id);
        }
        ids
    }

    /// 실시간 추적용: 이 플레이어가 지금 이번 밤 능력을 겨누고 있는 대상.
    /// 마피아는 본인이 고른 표시 대상을 기준으로 한다.
    pub(crate) fn live_action_target(&self, watched: &Player) -> Option<u64> {
        match watched.role {
            Role::Mafia => self.mafia_display_targets.get(&watched.user_id).copied(),
            Role::Thief => self.resolved_thief_action_target(watched),
            Role::Doctor => self.doctor_targets.get(&watched.user_id).copied(),
            Role::Nurse => self
                .nurse_targets
                .get(&watched.user_id)
                .or_else(|| self.nurse_prescription_targets.get(&watched.user_id))
                .copied(),
            Role::Gangster => self.gangster_targets.get(&watched.user_id).copied(),
            Role::Police => self.police_targets.get(&watched.user_id).copied(),
            Role::Inspector => self.inspector_targets.get(&watched.user_id).copied(),
            Role::Vigilante => self.vigilante_targets.get(&watched.user_id).copied(),
            Role::Hypnotist => self.hypnotist_targets.get(&watched.user_id).copied(),
            Role::Mercenary => self.mercenary_targets.get(&watched.user_id).copied(),
            Role::Reporter => self.reporter_targets.get(&watched.user_id).copied(),
            Role::Detective => self.detective_targets.get(&watched.user_id).copied(),
            Role::Shaman => self.shaman_targets.get(&watched.user_id).copied(),
            Role::Priest => self.priest_targets.get(&watched.user_id).copied(),
            Role::Spy => self
                .spy_targets
                .get(&watched.user_id)
                .and_then(|targets| targets.last().copied()),
            Role::Contractor => self
                .contractor_contracts
                .get(&watched.user_id)
                .map(|contract| contract.0.0),
            Role::Witch => self.witch_targets.get(&watched.user_id).copied(),
            Role::Terrorist => self.terrorist_targets.get(&watched.user_id).copied(),
            Role::Godfather => self.godfather_targets.get(&watched.user_id).copied(),
            Role::CultLeader => self.cult_targets.get(&watched.user_id).copied(),
            Role::Fanatic => self.fanatic_targets.get(&watched.user_id).copied(),
            _ => None,
        }
    }

    /// 실시간 추적: 방금 밤 행동을 낸 플레이어를 추적 중인 사탐들에게 알림을
    /// 쌓는다. 같은 대상 재제출은 무시하고, 처음이면 사용 알림, 다르면 변경
    /// 알림을 만든다.
    pub(crate) fn queue_detective_live_updates(&mut self, actor_id: u64) {
        if self.phase != Phase::Night {
            return;
        }
        let Some(watched) = self.get_player(actor_id).cloned() else {
            return;
        };
        let watchers = self
            .detective_targets
            .iter()
            .filter(|(_, target_id)| **target_id == actor_id)
            .map(|(detective_id, _)| *detective_id)
            .filter(|detective_id| {
                *detective_id != actor_id
                    && self.get_player(*detective_id).is_some_and(|detective| {
                        detective.alive
                            && !self.is_frog(detective)
                            && (detective.role == Role::Detective
                                || self.thief_night_role(detective) == Some(Role::Detective))
                    })
            })
            .collect::<Vec<_>>();
        if watchers.is_empty() {
            return;
        }
        let Some(current_id) = self.live_action_target(&watched) else {
            return;
        };
        let Some(target_name) = self
            .get_player(current_id)
            .map(|player| player.name.clone())
        else {
            return;
        };
        for detective_id in watchers {
            let previous = self
                .detective_live_last
                .insert((detective_id, actor_id), current_id);
            let line = match previous {
                Some(previous_id) if previous_id == current_id => continue,
                Some(_) => format!(
                    "[추적] {} 님이 대상을 {} 님으로 바꿨습니다.",
                    watched.name, target_name
                ),
                None => format!(
                    "[추적] {} 님이 {} 님에게 능력을 사용했습니다.",
                    watched.name, target_name
                ),
            };
            self.pending_detective_live_notices
                .push((detective_id, line));
        }
    }

    /// 실시간 추적 알림 대기열을 꺼낸다 (러너가 즉시 DM으로 전달).
    pub fn take_detective_live_notices(&mut self) -> Vec<(u64, String)> {
        std::mem::take(&mut self.pending_detective_live_notices)
    }

    /// [승부수] 살아있는 마피아 본대가 보유자 한 명뿐인 상태인가.
    pub(crate) fn all_in_active(&self) -> bool {
        let alive_mafia = self
            .players
            .iter()
            .filter(|player| player.alive && player.role == Role::Mafia && !self.is_frog(player))
            .collect::<Vec<_>>();
        alive_mafia.len() == 1 && self.has_tier_ability(alive_mafia[0].user_id, TierAbility::AllIn)
    }

    /// 살아있는 보유자가 있는지 (마피아팀 패시브 판정용).
    pub(crate) fn mafia_team_has_tier_ability(&self, ability: TierAbility) -> bool {
        self.tier_abilities.iter().any(|(user_id, held)| {
            held.contains(&ability)
                && self.get_player(*user_id).is_some_and(|player| {
                    player.alive && self.is_mafia_team(player) && !self.is_frog(player)
                })
        })
    }

    pub fn player_tier_abilities(&self, user_id: u64) -> Vec<TierAbility> {
        self.tier_abilities
            .get(&user_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn has_tier_ability(&self, user_id: u64, ability: TierAbility) -> bool {
        self.tier_abilities
            .get(&user_id)
            .is_some_and(|held| held.contains(&ability))
    }

    /// 살아있는 마피아팀 보유자들의 id (중복 배정 허용).
    pub(crate) fn mafia_tier_ability_holders(&self, ability: TierAbility) -> Vec<u64> {
        self.tier_abilities
            .iter()
            .filter(|(user_id, held)| {
                held.contains(&ability)
                    && self.get_player(**user_id).is_some_and(|player| {
                        player.alive && self.is_mafia_team(player) && !self.is_frog(player)
                    })
            })
            .map(|(user_id, _)| *user_id)
            .collect()
    }

    /// [확성] 보유자가 밤에도 전체 채팅을 쓸 수 있는지.
    pub fn is_loudspeaker_active(&self, player: &Player) -> bool {
        player.alive
            && self.has_tier_ability(player.user_id, TierAbility::Loudspeaker)
            && !self.is_frog(player)
            && !self.is_madam_seduced(player)
            // 인당 게임 중 1회. 한 번 쓰면 게임 끝까지 다시 못 쓴다.
            && !self.loudspeaker_spent_ids.contains(&player.user_id)
            // 밤마다 전체에서 단 1회. 누군가 먼저 쓰면 다른 보유자도 그 밤에는 못 쓴다.
            && !self.loudspeaker_used_days.contains(&self.day_number)
    }

    /// 형사 판정용 팀. 다른 조사(경찰·심리학자)와 달리 접선 여부와 무관하게 실제
    /// 소속으로 판정한다 — 마피아팀·교주팀 대상은 항상 "시민팀이 아닙니다"가 되고
    /// 알림도 가지 않는다. 변장 사기꾼만은 예외로 시민으로 판정되어 속인다.
    pub(crate) fn inspector_team_key(&self, player: &Player) -> &'static str {
        if self.is_hypocrite_active(player) {
            // [위선] 첫 밤 동안 의사(시민팀)로 판정된다.
            return "citizen";
        }
        if self.is_disguised_fraudster(player)
            && !self.fraudster_contacted.contains(&player.user_id)
        {
            return "citizen";
        }
        if self.is_cult_team(player) {
            "cult"
        } else if self.is_mafia_team(player) {
            "mafia"
        } else if player.role == Role::Joker {
            "joker"
        } else {
            "citizen"
        }
    }

    /// [확성] 사용을 소모한다. 그 밤은 보유자 전체가 더 못 쓰고(밤당 1회),
    /// 사용한 본인은 게임 끝까지 다시 못 쓴다(인당 1회).
    pub fn mark_loudspeaker_used(&mut self, user_id: u64) {
        let day = self.day_number;
        self.loudspeaker_used_days.insert(day);
        self.loudspeaker_spent_ids.insert(user_id);
    }

    /// 확성 능력 보유자 전원 (사용 여부와 무관).
    pub fn loudspeaker_holders(&self) -> Vec<Player> {
        self.players
            .iter()
            .filter(|player| self.has_tier_ability(player.user_id, TierAbility::Loudspeaker))
            .cloned()
            .collect()
    }

    /// 대상에게 걸린 치료가 전부 자기 자신이 쓴 것인지 ([야습] 판정).
    pub(crate) fn protection_is_self_heal_only(&self, target_id: u64) -> bool {
        let healers = self
            .doctor_targets
            .iter()
            .chain(&self.nurse_targets)
            .chain(&self.nurse_prescription_targets)
            .filter(|(_, healed_id)| **healed_id == target_id)
            .map(|(healer_id, _)| *healer_id)
            .collect::<Vec<_>>();
        !healers.is_empty() && healers.iter().all(|healer_id| *healer_id == target_id)
    }

    /// 역할 안내 DM용: 사기꾼의 사기 대상과 변장 직업.
    pub fn fraudster_disguise_info(&self, fraudster_id: u64) -> Option<(Player, Role)> {
        let (target_id, disguised_role) = self.fraudster_disguises.get(&fraudster_id)?;
        let target = self.get_player(*target_id)?.clone();
        Some((target, *disguised_role))
    }

    fn mercenary_can_block_mafia_win(&self) -> bool {
        self.players.iter().any(|player| {
            player.alive
                && player.role == Role::Mercenary
                && self.mercenary_armed_ids.contains(&player.user_id)
                && self
                    .players
                    .iter()
                    .any(|target| target.alive && target.user_id != player.user_id)
        })
    }

    pub fn is_madam_seduced(&self, player: &Player) -> bool {
        player.alive && self.madam_seduced_ids.contains(&player.user_id)
    }

    pub fn visible_role(&self, player: &Player) -> Role {
        if self.is_frog(player) {
            Role::Frog
        } else if self.cleanup_masked_ids.contains(&player.user_id) {
            // [수습] 가려진 사망자는 어떤 조사에서도 시민으로 보인다.
            Role::Citizen
        } else if self.is_hypocrite_active(player) {
            // [위선] 첫 밤 동안 조사 판정이 의사로 나온다.
            Role::Doctor
        } else if let Some((_, disguised_role)) = self.fraudster_disguises.get(&player.user_id) {
            // 사기꾼은 조사 판정이 변장한 시민 직업으로 나온다.
            *disguised_role
        } else {
            player.role
        }
    }

    /// [위선] 첫 번째 밤 동안 조사 판정이 의사로 나오는 상태인가.
    pub(crate) fn is_hypocrite_active(&self, player: &Player) -> bool {
        player.alive
            && self.phase == Phase::Night
            && self.day_number == 1
            && self.has_tier_ability(player.user_id, TierAbility::Hypocrisy)
    }

    /// 살아있고 아직 접선하지 않은 변장 사기꾼인가. 조사 판정을 속이는 기준.
    pub fn is_disguised_fraudster(&self, player: &Player) -> bool {
        player.alive
            && player.role == Role::Fraudster
            && self.fraudster_disguises.contains_key(&player.user_id)
    }

    pub fn can_mafia_attack(&self, player: &Player, _attacker_id: Option<u64>) -> bool {
        player.alive
    }

    pub fn is_publicly_revealed(&self, player: &Player) -> bool {
        self.publicly_revealed_ids.contains(&player.user_id)
    }

    pub fn spy_can_use_bonus_action(&self, actor_id: u64) -> bool {
        self.phase == Phase::Night
            && self.is_alive(actor_id)
            && self.spy_bonus_pending.contains(&actor_id)
    }

    pub fn contractor_can_use_contract(&self, actor_id: u64) -> bool {
        let Some(actor) = self.get_player(actor_id) else {
            return false;
        };
        self.phase == Phase::Night
            && actor.alive
            && !self.is_frog(actor)
            && (actor.role == Role::Contractor
                || (actor.role == Role::Thief
                    && self.thief_stolen_roles.get(&actor_id) == Some(&Role::Contractor)))
            && self.day_number >= 2
            && self.contractor_contract_targets(actor).len() >= 2
    }

    pub fn contractor_contract_targets(&self, actor: &Player) -> Vec<Player> {
        self.players
            .iter()
            .filter(|player| {
                player.alive
                    && player.user_id != actor.user_id
                    && !self.is_publicly_revealed(player)
            })
            .cloned()
            .collect()
    }

    fn team_key(&self, player: &Player) -> &'static str {
        if self.is_hypocrite_active(player) {
            // [위선] 첫 밤 동안 의사(시민팀)로 판정된다.
            "citizen"
        } else if self.is_cult_team(player) {
            "cult"
        } else if self.is_known_mafia_team(player) {
            "mafia"
        } else if player.role == Role::Joker {
            "joker"
        } else {
            "citizen"
        }
    }

    pub fn ensure_godfather_auto_contact(&mut self) -> Vec<u64> {
        if self.day_number < 3 {
            return Vec::new();
        }
        let ids = self
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Godfather
                    && !self.godfather_contacted.contains(&player.user_id)
            })
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        for id in &ids {
            self.godfather_contacted.insert(*id);
        }
        ids
    }

    fn contact_mafia_team_member(&mut self, player: &Player) {
        match player.role {
            Role::Spy => {
                self.spy_contacted.insert(player.user_id);
            }
            Role::Contractor => {
                self.contractor_contacted.insert(player.user_id);
            }
            Role::Fraudster => {
                self.fraudster_contacted.insert(player.user_id);
            }
            Role::Thief => {
                self.thief_contacted.insert(player.user_id);
            }
            Role::Witch => {
                self.witch_contacted.insert(player.user_id);
            }
            Role::Scientist => {
                self.scientist_contacted.insert(player.user_id);
            }
            Role::Madam => {
                self.madam_contacted.insert(player.user_id);
            }
            Role::Godfather => {
                self.godfather_contacted.insert(player.user_id);
            }
            _ => {}
        }
    }

    fn mark_dead(&mut self, user_id: u64) -> Option<Player> {
        let index = *self.players_by_id.get(&user_id)?;
        if !self.players[index].alive {
            return None;
        }
        self.players[index].alive = false;
        self.death_order.push(user_id);
        // [망각술] 저주(개구리) 상태로 죽으면 사망 시 직업 능력이 발동하지 않는다.
        let amnesia_suppressed =
            self.frog_user_ids.contains(&user_id) && !self.amnesia_witch_ids().is_empty();
        if amnesia_suppressed {
            self.amnesia_suppressed_ids.insert(user_id);
            let victim_name = self.players[index].name.clone();
            for holder_id in self.amnesia_witch_ids() {
                self.pending_tier_ability_notices.push((
                    holder_id,
                    format!(
                        "[망각술] 저주받은 {victim_name}님이 사망해 직업 능력 발동을 막았습니다."
                    ),
                ));
            }
        }
        self.frog_user_ids.remove(&user_id);
        self.day_votes.remove(&user_id);
        self.confirm_votes.remove(&user_id);
        self.day_votes
            .retain(|_, target_id| target_id.is_none_or(|id| id != user_id));
        if !amnesia_suppressed
            && self.players[index].role == Role::Scientist
            && self.scientist_revive_used_ids.insert(user_id)
        {
            self.scientist_pending_revive_ids.insert(user_id);
            self.scientist_contacted.insert(user_id);
        }
        Some(self.players[index].clone())
    }

    pub fn consume_cult_bells(&mut self) -> u32 {
        let count = self.cult_bells_this_night;
        self.cult_bells_this_night = 0;
        count
    }

    pub fn ensure_fanatic_reincarnation(&mut self) -> Vec<u64> {
        if self
            .players
            .iter()
            .any(|player| player.alive && player.role == Role::CultLeader)
        {
            return Vec::new();
        }
        let Some(index) = self.players.iter().position(|player| {
            player.alive
                && player.role == Role::Fanatic
                && self.culted_ids.contains(&player.user_id)
        }) else {
            return Vec::new();
        };
        self.players[index].role = Role::CultLeader;
        self.culted_ids.insert(self.players[index].user_id);
        vec![self.players[index].user_id]
    }

    pub fn winner(&self) -> Option<Winner> {
        if self.joker_won {
            return Some(Winner::Joker);
        }
        if let Some(winner) = self.prophet_winner() {
            return Some(winner);
        }
        if let Some(winner) = self.time_limit_winner() {
            return Some(winner);
        }
        let alive = self.alive_players();
        let mafia_alive = alive
            .iter()
            .filter(|player| self.is_known_mafia_team(player))
            .count();
        let cult_alive = alive
            .iter()
            .filter(|player| self.is_cult_team(player))
            .count();
        let non_cult_alive = alive.len().saturating_sub(cult_alive);
        let cult_leader_alive = alive.iter().any(|player| player.role == Role::CultLeader);
        if cult_leader_alive && cult_alive > 0 && cult_alive >= non_cult_alive {
            return Some(Winner::Cult);
        }
        let non_mafia_alive = alive.len().saturating_sub(mafia_alive);
        if mafia_alive == 0 {
            if self.has_pending_scientist_revive() {
                return None;
            }
            return Some(Winner::Citizen);
        }
        if mafia_alive >= non_mafia_alive {
            if self.revealed_judge_alive() {
                return None;
            }
            if self.mercenary_can_block_mafia_win() {
                return None;
            }
            return Some(Winner::Mafia);
        }
        None
    }

    /// [시한부] 생존자가 절반 이하 + 2번째 밤 이후에 살아있는 보유자가 있으면
    /// 그 보유자의 팀이 즉시 승리한다. 도주로 살아남은 상태(사망 판정)나
    /// 개구리 상태에서는 발동하지 않는다.
    fn time_limit_winner(&self) -> Option<Winner> {
        let reached_second_night =
            self.day_number > 2 || (self.day_number == 2 && self.phase == Phase::Night);
        if !reached_second_night {
            return None;
        }
        let alive = self.alive_players();
        if alive.len() * 2 > self.players.len() {
            return None;
        }
        let holder = alive.iter().find(|player| {
            self.has_tier_ability(player.user_id, TierAbility::TimeLimit)
                && !self.is_frog(player)
                && !self.escaped_on_day.contains_key(&player.user_id)
                && (self.is_mafia_team(player) || self.is_cult_team(player))
        })?;
        if self.is_cult_team(holder) {
            Some(Winner::Cult)
        } else {
            Some(Winner::Mafia)
        }
    }

    pub fn winning_prophet(&self) -> Option<&Player> {
        if self.phase != Phase::Day || self.day_number < 4 {
            return None;
        }
        self.players
            .iter()
            .filter(|player| player.alive && player.role == Role::Prophet)
            .min_by_key(|player| player.name.to_lowercase())
    }

    fn prophet_winner(&self) -> Option<Winner> {
        let prophet = self.winning_prophet()?;
        if self.is_cult_team(prophet) {
            Some(Winner::Cult)
        } else if self.is_mafia_team(prophet) {
            Some(Winner::Mafia)
        } else {
            Some(Winner::Citizen)
        }
    }

    fn active_judge(&self) -> Option<Player> {
        let mut judges = self
            .players
            .iter()
            .filter(|player| player.alive && player.role == Role::Judge)
            .cloned()
            .collect::<Vec<_>>();
        if judges.is_empty() {
            return None;
        }
        judges.sort_by_key(|player| player.name.to_lowercase());
        judges
            .iter()
            .find(|judge| self.revealed_judge_ids.contains(&judge.user_id))
            .cloned()
            .or_else(|| judges.into_iter().next())
    }

    fn revealed_judge_alive(&self) -> bool {
        self.players.iter().any(|player| {
            player.alive
                && player.role == Role::Judge
                && self.revealed_judge_ids.contains(&player.user_id)
        })
    }

    pub fn reveal_roles(&self) -> String {
        let mut players = self.players.clone();
        players.sort_by_key(|player| player.name.to_lowercase());
        players
            .into_iter()
            .map(|player| {
                format!(
                    "- {}: {}{}",
                    player.name,
                    player.role.value(),
                    if player.alive { "" } else { " (사망)" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Activity UI용: 전체 플레이어 슬라이스 (생존 + 사망)
    pub fn all_players(&self) -> &[Player] {
        &self.players
    }

    /// Activity UI용: 플레이어가 오늘 밤 지목한 대상 (아직 미제출이면 None)
    pub fn get_night_action_target(&self, user_id: u64) -> Option<u64> {
        let player = self.get_player(user_id)?;
        let role = if player.role == Role::Thief {
            self.thief_night_role(player)?
        } else {
            player.role
        };
        let maps: &[&HashMap<u64, u64>] = match role {
            Role::Mafia => &[&self.mafia_targets],
            Role::Doctor => &[&self.doctor_targets],
            Role::Nurse => &[&self.nurse_targets, &self.nurse_prescription_targets],
            Role::Gangster => &[&self.gangster_targets],
            Role::Police if player.role == Role::Thief => &[&self.thief_police_targets],
            Role::Police => &[&self.police_targets],
            Role::Inspector => &[&self.inspector_targets],
            Role::Agent => &[&self.detective_targets],
            Role::Vigilante => &[&self.vigilante_targets],
            Role::Hypnotist => &[&self.hypnotist_targets],
            Role::Mercenary => &[&self.mercenary_targets],
            Role::Godfather => &[&self.godfather_targets],
            Role::CultLeader => &[&self.cult_targets],
            Role::Fanatic => &[&self.fanatic_targets],
            Role::Shaman => &[&self.shaman_targets],
            Role::Witch => &[&self.witch_targets],
            Role::Priest => &[&self.priest_targets],
            Role::Terrorist => &[&self.terrorist_targets],
            Role::Spy => {
                return self
                    .spy_targets
                    .get(&user_id)
                    .and_then(|v| v.first())
                    .copied();
            }
            _ => &[],
        };
        maps.iter().find_map(|m| m.get(&user_id).copied())
    }

    /// Activity UI용: 현재 낮 투표 득표 집계 (targetId → 득표수)
    pub fn current_vote_counts(&self) -> HashMap<u64, usize> {
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for (voter_id, target_opt) in &self.day_votes {
            if !self.is_alive(*voter_id) || self.vote_blocked(*voter_id) {
                continue;
            }
            if let Some(target) = target_opt {
                if self.is_alive(*target) {
                    *counts.entry(*target).or_insert(0) += 1;
                }
            }
        }
        counts
    }

    pub fn current_skip_vote_count(&self) -> usize {
        self.day_votes
            .iter()
            .filter(|(voter_id, target_id)| {
                self.is_alive(**voter_id) && !self.vote_blocked(**voter_id) && target_id.is_none()
            })
            .count()
    }

    /// Activity UI용: 찬반 투표 현황 (찬성수, 반대수)
    pub fn current_confirm_counts(&self) -> (usize, usize) {
        let yes = self
            .confirm_votes
            .iter()
            .filter(|(voter_id, approve)| self.is_alive(**voter_id) && **approve)
            .count();
        let no = self
            .confirm_votes
            .iter()
            .filter(|(voter_id, approve)| self.is_alive(**voter_id) && !**approve)
            .count();
        (yes, no)
    }

    pub fn public_status(&self) -> String {
        let alive_players = self.alive_players();
        let dead_players = self.dead_players();
        let alive = alive_players
            .iter()
            .map(|player| player.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let dead = dead_players
            .iter()
            .map(|player| player.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}일차 / 현재 단계: {}\n생존자({}명): {}\n사망자: {}",
            self.day_number,
            self.phase.value(),
            alive_players.len(),
            alive,
            if dead.is_empty() { "없음" } else { &dead }
        )
    }

    fn require_alive(&self, user_id: u64) -> Result<&Player> {
        let player = self.require_player(user_id)?;
        if !player.alive {
            bail!("사망한 참가자는 행동할 수 없습니다.");
        }
        Ok(player)
    }

    fn require_player(&self, user_id: u64) -> Result<&Player> {
        self.get_player(user_id)
            .ok_or_else(|| anyhow::anyhow!("게임 참가자가 아닙니다."))
    }

    fn proxy_target_id(&self, target_id: u64) -> u64 {
        let Some(target) = self.get_player(target_id) else {
            return target_id;
        };
        if !target.alive || target.role != Role::Hacker {
            return target_id;
        }
        let Some(proxy_id) = self.hacker_proxy_targets.get(&target.user_id).copied() else {
            return target_id;
        };
        if self.is_alive(proxy_id) {
            proxy_id
        } else {
            target_id
        }
    }

    fn is_alive(&self, user_id: u64) -> bool {
        self.get_player(user_id).is_some_and(|player| player.alive)
    }

    fn is_stolen_godfather_actor(&self, user_id: u64) -> bool {
        self.get_player(user_id).is_some_and(|player| {
            player.role == Role::Thief
                && self.thief_stolen_roles.get(&user_id) == Some(&Role::Godfather)
        })
    }

    fn is_stolen_doctor_actor(&self, user_id: u64) -> bool {
        self.get_player(user_id).is_some_and(|player| {
            player.role == Role::Thief
                && self.thief_stolen_roles.get(&user_id) == Some(&Role::Doctor)
        })
    }

    fn majority_target(&self, targets: &HashMap<u64, u64>) -> Option<u64> {
        let live_targets = targets
            .iter()
            .filter(|(actor_id, target_id)| self.is_alive(**actor_id) && self.is_alive(**target_id))
            .map(|(_, target_id)| *target_id)
            .collect::<Vec<_>>();
        let voter_count = live_targets.len();
        if voter_count == 0 {
            return None;
        }
        let counts = count_values(live_targets);
        let highest = counts.values().copied().max()?;
        let tied = counts
            .iter()
            .filter(|(_, count)| **count == highest)
            .map(|(target_id, _)| *target_id)
            .collect::<Vec<_>>();
        if tied.len() != 1 || highest < majority_required(voter_count) {
            None
        } else {
            Some(tied[0])
        }
    }

    fn spy_actions_used(&self, actor_id: u64) -> usize {
        self.spy_targets.get(&actor_id).map_or(0, Vec::len)
    }

    fn spy_action_limit(&self, actor_id: u64) -> usize {
        if self.spy_bonus_pending.contains(&actor_id) {
            2
        } else {
            1
        }
    }

    fn contractor_can_act(&self, player: &Player) -> bool {
        self.day_number >= 2 && self.contractor_contract_targets(player).len() >= 2
    }

    fn reporter_can_act(&self, player: &Player, alive: &[Player]) -> bool {
        self.day_number >= 2 && !self.reporter_used_ids.contains(&player.user_id) && alive.len() > 1
    }

    fn vote_weight(&self, voter_id: u64) -> i32 {
        if self.vote_blocked(voter_id) {
            return 0;
        }
        self.get_player(voter_id).map_or(1, |voter| {
            let base = if voter.alive && voter.role == Role::Politician {
                2
            } else {
                1
            };
            // [데뷔] 투표권 한 표 박탈 (0표 밑으로는 내려가지 않는다).
            if self.debut_vote_penalty_ids.contains(&voter_id) {
                (base - 1).max(0)
            } else {
                base
            }
        })
    }

    fn vote_blocked(&self, voter_id: u64) -> bool {
        self.gangster_blocked_vote_days.get(&voter_id) == Some(&self.day_number)
    }

    fn advance_to_next_night(&mut self) {
        self.expire_madam_seductions();
        self.expire_vote_blocks();
        self.phase = Phase::Night;
        self.day_number += 1;
    }

    fn expire_vote_blocks(&mut self) {
        let day = self.day_number;
        self.gangster_blocked_vote_days
            .retain(|_, block_day| *block_day > day);
    }

    fn expire_madam_seductions(&mut self) {
        let day = self.day_number;
        let expired = self
            .madam_seduction_release_days
            .iter()
            .filter(|(_, release_day)| **release_day <= day)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in expired {
            self.madam_seduced_ids.remove(&id);
            self.madam_seduction_release_days.remove(&id);
        }
    }

    fn action_insert(&mut self, map: RoleActionMap, actor_id: u64, target_id: u64) {
        match map {
            RoleActionMap::Doctor => {
                self.doctor_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Gangster => {
                self.gangster_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Police => {
                self.police_targets.insert(actor_id, target_id);
            }
            RoleActionMap::ThiefPolice => {
                self.thief_police_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Inspector => {
                self.inspector_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Detective => {
                self.detective_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Priest => {
                self.priest_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Witch => {
                self.witch_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Terrorist => {
                self.terrorist_targets.insert(actor_id, target_id);
            }
            RoleActionMap::Mercenary => {
                self.mercenary_targets.insert(actor_id, target_id);
            }
        };
    }
}

#[derive(Debug, Clone, Copy)]
enum RoleActionMap {
    Doctor,
    Gangster,
    Police,
    ThiefPolice,
    Inspector,
    Detective,
    Priest,
    Witch,
    Terrorist,
    Mercenary,
}

const ROLE_ASSIGNMENT_RANDOM_JITTER: u64 = 50_000;

fn assign_roles_balanced(
    mut players: Vec<(u64, String)>,
    mut roles: Vec<Role>,
    assignment_history: &HashMap<u64, PlayerAssignmentHistory>,
) -> Vec<Player> {
    if players.is_empty() {
        return Vec::new();
    }
    let mut rng = system_random::rng();
    players.shuffle(&mut rng);
    roles.shuffle(&mut rng);

    let total_players = players.len();
    let mafia_slots = roles.iter().filter(|role| role.is_mafia_team()).count();
    let role_slots = roles
        .iter()
        .copied()
        .fold(HashMap::new(), |mut counts, role| {
            *counts.entry(role).or_default() += 1_usize;
            counts
        });
    let empty_history = PlayerAssignmentHistory::default();
    let mut costs = Vec::with_capacity(total_players);
    for (user_id, _) in &players {
        let history = assignment_history.get(user_id).unwrap_or(&empty_history);
        let mut row = Vec::with_capacity(total_players);
        for role in &roles {
            let base_cost = role_assignment_cost(
                history,
                *role,
                total_players,
                mafia_slots,
                role_slots.get(role).copied().unwrap_or(1),
            );
            let random_jitter = (rng.next_u64() % (ROLE_ASSIGNMENT_RANDOM_JITTER + 1)) as i64;
            row.push(base_cost.saturating_add(random_jitter));
        }
        costs.push(row);
    }
    let role_by_player = minimum_cost_assignment(&costs);

    players
        .into_iter()
        .enumerate()
        .map(|(index, (user_id, name))| Player::new(user_id, name, roles[role_by_player[index]]))
        .collect()
}

fn role_assignment_cost(
    history: &PlayerAssignmentHistory,
    role: Role,
    total_players: usize,
    mafia_slots: usize,
    same_role_slots: usize,
) -> i64 {
    const MAFIA_RECENCY_COSTS: [i64; 3] = [80_000_000, 24_000_000, 6_000_000];
    const ROLE_RECENCY_COSTS: [i64; 3] = [12_000_000, 4_000_000, 1_000_000];

    let expected_role_rate = same_role_slots as i64 * 1_000 / total_players as i64;
    let role_games = history.role_counts.get(&role).copied().unwrap_or(0);
    let mut cost = smoothed_assignment_rate(role_games, history.games, expected_role_rate) * 2_000;

    for (index, recent_role) in history.recent_roles.iter().take(3).enumerate() {
        if *recent_role == role {
            cost += ROLE_RECENCY_COSTS[index];
        }
    }

    if role.is_mafia_team() {
        let expected_mafia_rate = mafia_slots as i64 * 1_000 / total_players as i64;
        cost +=
            smoothed_assignment_rate(history.mafia_role_games, history.games, expected_mafia_rate)
                * 10_000;
        for (index, recent_role) in history.recent_roles.iter().take(3).enumerate() {
            if recent_role.is_mafia_team() {
                cost += MAFIA_RECENCY_COSTS[index];
            }
        }
    }
    cost
}

fn smoothed_assignment_rate(count: i64, games: i64, expected_rate: i64) -> i64 {
    const PRIOR_GAMES: i64 = 4;
    let games = games.max(0);
    let count = count.max(0);
    (count
        .saturating_mul(1_000)
        .saturating_add(expected_rate.saturating_mul(PRIOR_GAMES))
        / games.saturating_add(PRIOR_GAMES))
    .min(10_000)
}

fn minimum_cost_assignment(costs: &[Vec<i64>]) -> Vec<usize> {
    let size = costs.len();
    let mut row_potential = vec![0_i64; size + 1];
    let mut column_potential = vec![0_i64; size + 1];
    let mut matched_row = vec![0_usize; size + 1];
    let mut previous_column = vec![0_usize; size + 1];

    for row in 1..=size {
        matched_row[0] = row;
        let mut column = 0;
        let mut minimum = vec![i64::MAX / 4; size + 1];
        let mut used = vec![false; size + 1];
        loop {
            used[column] = true;
            let current_row = matched_row[column];
            let mut delta = i64::MAX / 4;
            let mut next_column = 0;
            for candidate_column in 1..=size {
                if used[candidate_column] {
                    continue;
                }
                let reduced_cost = costs[current_row - 1][candidate_column - 1]
                    - row_potential[current_row]
                    - column_potential[candidate_column];
                if reduced_cost < minimum[candidate_column] {
                    minimum[candidate_column] = reduced_cost;
                    previous_column[candidate_column] = column;
                }
                if minimum[candidate_column] < delta {
                    delta = minimum[candidate_column];
                    next_column = candidate_column;
                }
            }
            for candidate_column in 0..=size {
                if used[candidate_column] {
                    row_potential[matched_row[candidate_column]] += delta;
                    column_potential[candidate_column] -= delta;
                } else {
                    minimum[candidate_column] -= delta;
                }
            }
            column = next_column;
            if matched_row[column] == 0 {
                break;
            }
        }
        loop {
            let prior = previous_column[column];
            matched_row[column] = matched_row[prior];
            column = prior;
            if column == 0 {
                break;
            }
        }
    }

    let mut assignment = vec![0_usize; size];
    for column in 1..=size {
        assignment[matched_row[column] - 1] = column - 1;
    }
    assignment
}

fn validate_counts(players: &[(u64, String)], counts: &GameCounts) -> Result<()> {
    if players.len() < 3 {
        bail!("최소 3명이 필요합니다.");
    }
    if players.len() > 24 {
        bail!("투표 스킵 선택지를 포함해야 해서 최대 24명까지 지원합니다.");
    }
    if players
        .iter()
        .map(|(user_id, _)| *user_id)
        .collect::<HashSet<_>>()
        .len()
        != players.len()
    {
        bail!("중복된 참가자가 있습니다.");
    }
    let investigation_role_count = [
        counts.police_count > 0,
        counts.agent_count
            + counts
                .special_roles
                .iter()
                .filter(|role| **role == Role::Agent)
                .count()
            > 0,
        counts.vigilante_count
            + counts
                .special_roles
                .iter()
                .filter(|role| **role == Role::Vigilante)
                .count()
            > 0,
        counts.inspector_count
            + counts
                .special_roles
                .iter()
                .filter(|role| **role == Role::Inspector)
                .count()
            > 0,
    ]
    .into_iter()
    .filter(|value| *value)
    .count();
    if investigation_role_count > 1 {
        bail!("경찰, 요원, 자경단원, 형사는 한 게임에 함께 배정할 수 없습니다.");
    }
    if counts.agent_count > 0 && counts.special_roles.contains(&Role::Agent) {
        bail!("요원 수가 중복 배정되었습니다.");
    }
    if counts.vigilante_count > 0 && counts.special_roles.contains(&Role::Vigilante) {
        bail!("자경단원 수가 중복 배정되었습니다.");
    }
    if counts.inspector_count > 0 && counts.special_roles.contains(&Role::Inspector) {
        bail!("형사 수가 중복 배정되었습니다.");
    }
    let mut role_counts = HashMap::<Role, usize>::new();
    for role in &counts.special_roles {
        *role_counts.entry(*role).or_default() += 1;
    }
    let duplicate_roles = role_counts
        .iter()
        .filter(|(role, count)| **count > 1 && !(**role == Role::Lover && **count == 2))
        .map(|(role, _)| role.value())
        .collect::<Vec<_>>();
    if !duplicate_roles.is_empty() {
        bail!("같은 특수 역할은 한 게임에 한 번만 선택됩니다.");
    }
    let special_count = counts.mafia_count
        + counts.doctor_count
        + counts.police_count
        + counts.agent_count
        + counts.vigilante_count
        + counts.inspector_count
        + counts.joker_count
        + counts.special_roles.len();
    let mercenary_count = counts
        .special_roles
        .iter()
        .filter(|role| **role == Role::Mercenary)
        .count();
    if mercenary_count > 0 {
        let citizen_fill_count = players.len().saturating_sub(special_count);
        let citizen_team_count = counts.doctor_count
            + counts.police_count
            + counts.agent_count
            + counts.vigilante_count
            + counts.inspector_count
            + citizen_fill_count
            + counts
                .special_roles
                .iter()
                .filter(|role| !role.is_mafia_team() && **role != Role::Joker)
                .count();
        if citizen_team_count <= mercenary_count {
            bail!("용병 의뢰인이 될 시민팀 플레이어가 부족합니다.");
        }
    }
    if special_count > players.len() {
        bail!("직업 수의 합계가 참가자 수보다 많습니다.");
    }
    let mafia_team_count = counts.mafia_count
        + counts
            .special_roles
            .iter()
            .filter(|role| role.is_mafia_team())
            .count();
    if mafia_team_count < 1 {
        bail!("마피아 계열은 최소 1명이어야 합니다.");
    }
    if mafia_team_count >= players.len() - mafia_team_count {
        bail!("시작할 때 시민 진영이 마피아 팀보다 많아야 합니다.");
    }
    Ok(())
}

pub const fn majority_required(voter_count: usize) -> usize {
    (voter_count + 1) / 2
}

fn count_values(values: impl IntoIterator<Item = u64>) -> HashMap<u64, usize> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
}

fn reported_protected_id(
    protected_ids: &HashSet<u64>,
    mafia_target_id: Option<u64>,
    godfather_target_id: Option<u64>,
    majority_protected_id: Option<u64>,
) -> Option<u64> {
    if mafia_target_id.is_some_and(|id| protected_ids.contains(&id)) {
        return mafia_target_id;
    }
    if godfather_target_id.is_some_and(|id| protected_ids.contains(&id)) {
        return godfather_target_id;
    }
    if majority_protected_id.is_some() {
        return majority_protected_id;
    }
    protected_ids.iter().copied().min()
}

#[cfg(test)]
mod tests;
