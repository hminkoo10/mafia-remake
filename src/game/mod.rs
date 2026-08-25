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
    /// [은폐] 이번 밤 마피아팀 처형 실패가 조용한 밤으로 가려졌는지.
    pub concealed_kill_failure: bool,
    /// [저격] 전날 밤 마피아팀 처형이 실패해 이번 밤 관통이 장전된 상태인지.
    pub snipe_armed: bool,
    /// [야습] 이번 밤 관통된 자가 치료 의사(아침에 전체 공개).
    pub pending_night_raid_reveals: Vec<Player>,
    /// [독살] 중독된 플레이어 → 사망하는 밤의 day_number.
    pub poisoned_death_days: HashMap<u64, u32>,
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
            concealed_kill_failure: false,
            snipe_armed: false,
            pending_night_raid_reveals: Vec::new(),
            poisoned_death_days: HashMap::new(),
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
        let target_id = self.terrorist_targets.get(&terrorist.user_id).copied()?;
        let target = self.get_player(target_id)?.clone();
        if !target.alive {
            return None;
        }
        (self.retaliation_team_key(terrorist) != self.retaliation_team_key(&target))
            .then_some(target)
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
            (self.retaliation_team_key(terrorist) != self.retaliation_team_key(&target))
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
                    // 5티어는 2개, 6티어는 3개. 풀이 그보다 작으면 풀 크기까지만.
                    let mut pool = tier4_pool(player.role);
                    pool.shuffle(&mut rng);
                    pool.truncate((tier as usize - 3).min(pool.len()));
                    pool
                }
                _ => Vec::new(),
            };
            if !abilities.is_empty() {
                self.tier_abilities.insert(player.user_id, abilities);
            }
        }
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

    /// [확성] 이번 밤 사용을 소모한다 (보유자 전체 공유, 밤당 1회).
    pub fn mark_loudspeaker_used(&mut self) {
        let day = self.day_number;
        self.loudspeaker_used_days.insert(day);
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
        self.frog_user_ids.remove(&user_id);
        self.day_votes.remove(&user_id);
        self.confirm_votes.remove(&user_id);
        self.day_votes
            .retain(|_, target_id| target_id.is_none_or(|id| id != user_id));
        if self.players[index].role == Role::Scientist
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
            if voter.alive && voter.role == Role::Politician {
                2
            } else {
                1
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
            RoleActionMap::Shaman => {
                self.shaman_targets.insert(actor_id, target_id);
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
    Shaman,
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
mod tests {
    use super::*;

    fn basic_players() -> Vec<(u64, String)> {
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ]
    }

    fn special_mafia_player(role: Role, index: usize) -> Player {
        Player::new(900 + index as u64, format!("{role:?}"), role)
    }

    #[test]
    fn indexes_players_by_id() {
        let game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
        assert_eq!(game.get_player(2).unwrap().name, "Two");
        assert!(game.get_player(999).is_none());
    }

    #[test]
    fn balanced_assignment_avoids_consecutive_mafia_roles() {
        let players = (1..=6)
            .map(|user_id| (user_id, format!("P{user_id}")))
            .collect::<Vec<_>>();
        let mut history = HashMap::new();
        for user_id in 1..=6 {
            let was_mafia = user_id <= 2;
            history.insert(
                user_id,
                PlayerAssignmentHistory {
                    games: 4,
                    mafia_role_games: if was_mafia { 4 } else { 0 },
                    role_counts: HashMap::from([(
                        if was_mafia {
                            Role::Mafia
                        } else {
                            Role::Citizen
                        },
                        4,
                    )]),
                    recent_roles: vec![if was_mafia {
                        Role::Mafia
                    } else {
                        Role::Citizen
                    }],
                },
            );
        }

        let game = MafiaGame::new_with_counts_balanced(
            players,
            GameCounts {
                mafia_count: 2,
                ..Default::default()
            },
            &history,
        )
        .unwrap();
        let mafia_ids = game
            .players
            .iter()
            .filter(|player| player.role.is_mafia_team())
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();

        assert!(!mafia_ids.contains(&1));
        assert!(!mafia_ids.contains(&2));
    }

    #[test]
    fn assignment_log_adjusts_role_probability_cost() {
        let rarely_doctor = PlayerAssignmentHistory {
            games: 12,
            role_counts: HashMap::from([(Role::Doctor, 0)]),
            ..Default::default()
        };
        let often_doctor = PlayerAssignmentHistory {
            games: 12,
            role_counts: HashMap::from([(Role::Doctor, 5)]),
            ..Default::default()
        };

        let rare_cost = role_assignment_cost(&rarely_doctor, Role::Doctor, 8, 2, 1);
        let often_cost = role_assignment_cost(&often_doctor, Role::Doctor, 8, 2, 1);

        assert!(often_cost - rare_cost > ROLE_ASSIGNMENT_RANDOM_JITTER as i64);
    }

    #[test]
    fn assignment_history_reduces_inspector_probability() {
        let rarely_inspector = PlayerAssignmentHistory {
            games: 12,
            role_counts: HashMap::from([(Role::Inspector, 0)]),
            ..Default::default()
        };
        let often_inspector = PlayerAssignmentHistory {
            games: 12,
            role_counts: HashMap::from([(Role::Inspector, 5)]),
            ..Default::default()
        };

        let rare_cost = role_assignment_cost(&rarely_inspector, Role::Inspector, 8, 2, 1);
        let often_cost = role_assignment_cost(&often_inspector, Role::Inspector, 8, 2, 1);

        assert!(often_cost - rare_cost > ROLE_ASSIGNMENT_RANDOM_JITTER as i64);
    }

    #[test]
    fn base_inspector_count_is_assigned() {
        let game = MafiaGame::new_with_counts(
            basic_players(),
            GameCounts {
                mafia_count: 1,
                inspector_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            game.players
                .iter()
                .filter(|player| player.role == Role::Inspector)
                .count(),
            1
        );
    }

    #[test]
    fn balanced_assignment_evenly_rotates_teams_and_roles() {
        let players = (1..=8)
            .map(|user_id| (user_id, format!("P{user_id}")))
            .collect::<Vec<_>>();
        let mut history = HashMap::<u64, PlayerAssignmentHistory>::new();
        let mut previous_mafia_ids = HashSet::new();

        for _ in 0..32 {
            let game = MafiaGame::new_with_counts_balanced(
                players.clone(),
                GameCounts {
                    mafia_count: 2,
                    doctor_count: 1,
                    police_count: 1,
                    ..Default::default()
                },
                &history,
            )
            .unwrap();
            let mafia_ids = game
                .players
                .iter()
                .filter(|player| player.role.is_mafia_team())
                .map(|player| player.user_id)
                .collect::<HashSet<_>>();
            if !previous_mafia_ids.is_empty() {
                assert!(mafia_ids.is_disjoint(&previous_mafia_ids));
            }

            for player in &game.players {
                let entry = history.entry(player.user_id).or_default();
                entry.games += 1;
                if player.role.is_mafia_team() {
                    entry.mafia_role_games += 1;
                }
                *entry.role_counts.entry(player.role).or_default() += 1;
                entry.recent_roles.insert(0, player.role);
                entry.recent_roles.truncate(3);
            }
            previous_mafia_ids = mafia_ids;
        }

        for role in [Role::Mafia, Role::Doctor, Role::Police] {
            let counts = (1..=8)
                .map(|user_id| {
                    history[&user_id]
                        .role_counts
                        .get(&role)
                        .copied()
                        .unwrap_or(0)
                })
                .collect::<Vec<_>>();
            assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1);
        }
    }

    #[test]
    fn uncontacted_mafia_specials_are_citizen_for_investigations() {
        let game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();

        for (index, role) in crate::model::MAFIA_SPECIAL_ROLES
            .iter()
            .copied()
            .enumerate()
        {
            let player = special_mafia_player(role, index);

            assert!(
                !game.is_police_detected_mafia_team(&player),
                "{role:?} should not be police-detected as mafia before contact"
            );
            assert_eq!(
                game.team_key(&player),
                "citizen",
                "{role:?} should be citizen team for relation investigations before contact"
            );
        }
    }

    #[test]
    fn contacted_mafia_specials_are_mafia_for_investigations() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();

        for (index, role) in crate::model::MAFIA_SPECIAL_ROLES
            .iter()
            .copied()
            .enumerate()
        {
            let player = special_mafia_player(role, index);
            game.contact_mafia_team_member(&player);

            assert_eq!(
                game.team_key(&player),
                "mafia",
                "{role:?} should be mafia team for relation investigations after contact"
            );
            if role == Role::Godfather {
                assert!(
                    !game.is_police_detected_mafia_team(&player),
                    "Godfather should keep police concealment even after contact"
                );
            } else {
                assert!(
                    game.is_police_detected_mafia_team(&player),
                    "{role:?} should be police-detected as mafia after contact"
                );
            }
        }
    }

    #[test]
    fn contractor_can_target_hidden_investigation_roles() {
        let players = (1..=8)
            .map(|user_id| (user_id, format!("P{user_id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 0, 0, Vec::new()).unwrap();
        for (user_id, role) in [
            (1, Role::Contractor),
            (2, Role::Police),
            (3, Role::Agent),
            (4, Role::Vigilante),
            (5, Role::Inspector),
            (6, Role::Judge),
            (7, Role::Citizen),
            (8, Role::Mafia),
        ] {
            game.get_player_mut(user_id).unwrap().role = role;
        }
        game.publicly_revealed_ids.insert(6);
        game.phase = Phase::Night;
        game.day_number = 2;
        let contractor = game.get_player(1).unwrap().clone();

        let target_ids = game
            .contractor_contract_targets(&contractor)
            .into_iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();

        assert_eq!(target_ids, HashSet::from([2, 3, 4, 5, 7, 8]));
        for role in [Role::Police, Role::Agent, Role::Vigilante, Role::Inspector] {
            assert!(!crate::model::is_contractor_guess_role(role));
        }
        assert!(crate::model::is_contractor_guess_role(Role::Detective));
        assert!(
            game.submit_contractor_contract(1, 2, Role::Police, 3, Role::Citizen)
                .is_err()
        );
        assert!(
            game.submit_contractor_contract(1, 2, Role::Citizen, 3, Role::Mafia)
                .is_ok()
        );
    }

    #[test]
    fn winning_prophet_is_exposed_for_victory_announcement() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.get_player_mut(2).unwrap().role = Role::Prophet;
        game.phase = Phase::Day;
        game.day_number = 4;

        assert_eq!(game.winner(), Some(Winner::Citizen));
        assert_eq!(game.winning_prophet().map(|player| player.user_id), Some(2));
    }

    #[test]
    fn scientist_is_mafia_team_but_hidden_until_first_death() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.get_player_mut(2).unwrap().role = Role::Scientist;
        let scientist = game.get_player(2).unwrap().clone();

        assert!(game.is_mafia_team(&scientist));
        assert!(!game.is_citizen_team(&scientist));
        assert!(!game.is_known_mafia_team(&scientist));

        game.mark_dead(scientist.user_id).unwrap();
        let dead_scientist = game.get_player(scientist.user_id).unwrap();

        assert!(game.scientist_contacted.contains(&scientist.user_id));
        assert!(game.is_mafia_team(dead_scientist));
        assert!(!game.is_citizen_team(dead_scientist));
        assert!(game.is_known_mafia_team(dead_scientist));
    }

    #[test]
    fn agent_directive_ignores_uncontacted_mafia_specials() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Agent),
            (3, Role::Spy),
            (4, Role::Mafia),
            (5, Role::Joker),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        let result = game.resolve_night().unwrap();

        assert!(!game.agent_discovered_ids.contains(&3));
        assert!(
            result
                .agent_results
                .get(&2)
                .is_some_and(|text| !text.contains("Three"))
        );
    }

    #[test]
    fn agent_directive_reports_frog_instead_of_original_role() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Agent),
            (3, Role::Doctor),
            (4, Role::Mafia),
            (5, Role::Joker),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.frog_user_ids.insert(3);

        let result = game.resolve_night().unwrap();
        let directive = result.agent_results.get(&2).unwrap();

        assert!(directive.contains(Role::Frog.value()), "{directive}");
        assert!(!directive.contains(Role::Doctor.value()), "{directive}");
    }

    #[test]
    fn agent_receives_directive_when_killed_the_same_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Agent),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(1, Some(2)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 2)
        );
        assert!(result.agent_results.contains_key(&2));
    }

    /// 티어 배정: 전원이 2~6티어를 받고, 4티어 이상 능력은 시작 역할의
    /// 풀에서 티어에 맞는 개수(4=1, 5=2, 6=3, 풀이 작으면 풀 크기)만큼 서로
    /// 다른 능력으로 나온다. 같은 능력이 여러 플레이어에게 겹칠 수는 있다.
    #[test]
    fn tier_abilities_follow_group_pools() {
        use crate::model::tier4_pool;
        for _ in 0..20 {
            let players = (1..=10)
                .map(|id| (id as u64, format!("P{id}")))
                .collect::<Vec<_>>();
            let mut game = MafiaGame::new(players, 2, 1, 1, vec![Role::Spy, Role::Madam]).unwrap();
            game.assign_tier_abilities();

            assert_eq!(game.player_tiers.len(), 10);
            for player in &game.players {
                let tier = game.player_tiers[&player.user_id];
                assert!((2..=6).contains(&tier), "{tier}");
                let abilities = game.player_tier_abilities(player.user_id);
                match tier {
                    2 => assert!(abilities.is_empty(), "{:?} {abilities:?}", player.role),
                    3 => {
                        assert_eq!(abilities.len(), 1, "{abilities:?}");
                        assert_eq!(abilities[0].tier(), 3, "{abilities:?}");
                    }
                    _ => {
                        let pool = tier4_pool(player.role);
                        let expected = (tier as usize - 3).min(pool.len());
                        assert_eq!(
                            abilities.len(),
                            expected,
                            "{:?} {tier} {abilities:?}",
                            player.role
                        );
                        let unique = abilities.iter().collect::<HashSet<_>>();
                        assert_eq!(unique.len(), abilities.len(), "{abilities:?}");
                        for ability in &abilities {
                            assert!(pool.contains(ability), "{:?} {ability:?}", player.role);
                        }
                    }
                }
            }
        }
    }

    /// 티어 확률(2티어 40% / 3티어 30% / 4티어 15% / 5티어 10% / 6티어 5%)이
    /// 실제 분포로 나오는지 대량 표본으로 확인한다. 허용 오차 ±3%p는 표본
    /// 20,000명 기준 표준편차의 8배 이상이라 사실상 플레이크가 나지 않는다.
    #[test]
    fn tier_probabilities_match_the_declared_distribution() {
        let mut counts = [0u32; 5];
        let mut total = 0u32;
        for _ in 0..2000 {
            let players = (1..=10)
                .map(|id| (id as u64, format!("P{id}")))
                .collect::<Vec<_>>();
            let mut game = MafiaGame::new(players, 2, 1, 1, Vec::new()).unwrap();
            game.assign_tier_abilities();
            for tier in game.player_tiers.values() {
                counts[(*tier - 2) as usize] += 1;
                total += 1;
            }
        }
        assert_eq!(total, 20_000);
        let percent = |count: u32| count as f64 * 100.0 / total as f64;
        for (index, expected) in [40.0, 30.0, 15.0, 10.0, 5.0].into_iter().enumerate() {
            let share = percent(counts[index]);
            assert!(
                (share - expected).abs() <= 3.0,
                "{}티어 {share:.2}% (기대 {expected}%)",
                index + 2
            );
        }
    }

    /// 능력 배정이 풀 안에서 균등하게 나오는지 대량 표본으로 확인한다.
    /// 3티어 풀과 역할별 4티어 이상 풀마다 각 능력의 비율이 균등 기대치
    /// ±6%p 안이어야 한다 (다중 배정도 서로 다른 능력을 균등 추출하므로
    /// 능력별 점유율 기대치는 1/풀 크기 그대로다).
    #[test]
    fn tier_ability_rolls_are_uniform_within_each_pool() {
        use crate::model::{TIER3_ABILITIES, tier4_pool};
        let mut tier3: HashMap<TierAbility, u32> = HashMap::new();
        let mut tier4_by_role: HashMap<Role, HashMap<TierAbility, u32>> = HashMap::new();
        for _ in 0..10_000 {
            let players = (1..=10)
                .map(|id| (id as u64, format!("P{id}")))
                .collect::<Vec<_>>();
            let mut game = MafiaGame::new(players, 2, 1, 1, vec![Role::Spy, Role::Madam]).unwrap();
            game.assign_tier_abilities();
            for player in &game.players {
                for ability in game.player_tier_abilities(player.user_id) {
                    let bucket = if ability.tier() == 3 {
                        &mut tier3
                    } else {
                        tier4_by_role.entry(player.role).or_default()
                    };
                    *bucket.entry(ability).or_default() += 1;
                }
            }
        }
        let check = |label: &str, counts: &HashMap<TierAbility, u32>, pool: &[TierAbility]| {
            let total: u32 = counts.values().sum();
            let expected = 100.0 / pool.len() as f64;
            for ability in pool {
                let share = counts.get(ability).copied().unwrap_or(0) as f64 * 100.0 / total as f64;
                assert!(
                    (share - expected).abs() <= 6.0,
                    "{label} {ability:?}: {share:.2}% (기대 {expected:.2}%, 표본 {total})"
                );
            }
        };
        check("3티어", &tier3, TIER3_ABILITIES);
        for (role, counts) in &tier4_by_role {
            check(role.value(), counts, &tier4_pool(*role));
        }
    }

    /// [시한부] 절반 이하 + 2번째 밤 생존 시 보유자의 팀이 즉시 승리한다.
    /// 포교된 보유자는 교주팀 승리가 된다.
    #[test]
    fn time_limit_wins_for_the_holders_team_at_half_survivors() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 2, 0, 0, vec![Role::Spy]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Mafia),
            (3, Role::Spy),
            (4, Role::CultLeader),
            (5, Role::Citizen),
            (6, Role::Citizen),
            (7, Role::Citizen),
            (8, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::TimeLimit]);

        // 아직 첫 밤: 발동하지 않는다.
        assert_eq!(game.winner(), None);

        // 2번째 밤이지만 생존자가 절반보다 많으면 발동하지 않는다.
        game.day_number = 2;
        game.phase = Phase::Night;
        for id in [5, 6, 7] {
            game.get_player_mut(id).unwrap().alive = false;
        }
        assert_eq!(game.winner(), None);

        // 절반(4명) 이하가 되면 마피아팀 승리.
        game.get_player_mut(8).unwrap().alive = false;
        assert_eq!(game.winner(), Some(Winner::Mafia));

        // 보유자가 죽으면 발동하지 않는다.
        game.get_player_mut(1).unwrap().alive = false;
        assert_ne!(game.winner(), Some(Winner::Mafia));

        // 포교된 보조 보유자는 교주팀 승리.
        game.tier_abilities.insert(3, vec![TierAbility::TimeLimit]);
        game.culted_ids.insert(3);
        assert_eq!(game.winner(), Some(Winner::Cult));
    }

    /// [밀정] 두 번째 낮이 되면 보유 보조가 자동으로 마피아와 접선한다.
    #[test]
    fn inside_man_auto_contacts_on_the_second_day() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Spy]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Spy),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
            (6, Role::Citizen),
            (7, Role::Citizen),
            (8, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(2, vec![TierAbility::InsideMan]);

        // 첫 밤 결산: 아직 접선하지 않는다.
        let result = game.resolve_night().unwrap();
        assert!(result.tier_ability_contacts.is_empty());
        assert!(!game.spy_contacted.contains(&2));

        // 2일차 밤 결산(두 번째 낮): 자동 접선.
        game.phase = Phase::Night;
        game.day_number = 2;
        let result = game.resolve_night().unwrap();
        assert_eq!(result.tier_ability_contacts, vec![2]);
        assert!(game.spy_contacted.contains(&2));
        assert!(result.tier_ability_results[&2].contains("[밀정]"));
    }

    /// [수배] 첫 낮이 될 때 접선하지 않은 마피아팀 명단이 보유자에게 오고,
    /// 이미 접선한 보조와 둘째 밤 이후는 제외된다.
    #[test]
    fn wanted_lists_uncontacted_mafia_team_on_first_day() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Spy, Role::Madam]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Spy),
            (3, Role::Madam),
            (4, Role::Citizen),
            (5, Role::Citizen),
            (6, Role::Citizen),
            (7, Role::Citizen),
            (8, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Wanted]);
        game.madam_contacted.insert(3);

        let result = game.resolve_night().unwrap();
        let notice = &result.tier_ability_results[&1];
        assert!(notice.contains("[수배]"), "{notice}");
        assert!(notice.contains("P2"), "{notice}");
        assert!(!notice.contains("P3"), "{notice}");

        // 둘째 밤부터는 다시 오지 않는다.
        game.phase = Phase::Night;
        game.day_number = 2;
        let result = game.resolve_night().unwrap();
        assert!(!result.tier_ability_results.contains_key(&1));
    }

    /// [지령] 첫 낮에 마피아·청부업자 보유자는 경찰 계열이 누군지, 보조·교주
    /// 보유자는 미공개 시민팀 한 명의 직업을 안다.
    #[test]
    fn directive_gives_role_appropriate_intel_on_first_day() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 0, 1, vec![Role::Spy]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Spy),
            (3, Role::Police),
            (4, Role::Doctor),
            (5, Role::Citizen),
            (6, Role::Citizen),
            (7, Role::Citizen),
            (8, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Directive]);
        game.tier_abilities.insert(2, vec![TierAbility::Directive]);
        // 정체가 공개된 시민은 지령 대상에서 빠진다. 4~8 중 4만 남기고 공개해
        // 보조 지령 결과를 결정적으로 만든다.
        for id in [3, 5, 6, 7, 8] {
            game.publicly_revealed_ids.insert(id);
        }

        let result = game.resolve_night().unwrap();
        let mafia_notice = &result.tier_ability_results[&1];
        assert_eq!(mafia_notice, "[지령] P3님은 경찰 계열 직업입니다.");
        let spy_notice = &result.tier_ability_results[&2];
        assert_eq!(spy_notice, "[지령] P4님의 직업은 의사입니다.");

        // 둘째 밤부터는 오지 않는다.
        game.phase = Phase::Night;
        game.day_number = 2;
        let result = game.resolve_night().unwrap();
        assert!(!result.tier_ability_results.contains_key(&1));
        assert!(!result.tier_ability_results.contains_key(&2));
    }

    /// [위선] 첫 밤 동안 조사가 의사로 판정하고, 둘째 밤부터는 원래대로다.
    #[test]
    fn hypocrisy_passes_first_night_investigations_as_doctor() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Citizen),
            (3, Role::Inspector),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Hypocrisy]);

        let mafia = game.get_player(1).unwrap().clone();
        // 경찰 판정: 첫 밤에는 마피아팀이 아니라고 나온다.
        assert!(!game.is_police_detected_mafia_team(&mafia));
        // 형사 판정: 같은 시민팀으로 보여 직업이 '의사'로 공개된다.
        let immediate = game.submit_night_action(3, Some(1)).unwrap();
        assert!(
            immediate.contains("[One님의 직업은 의사입니다.]"),
            "{immediate}"
        );

        // 둘째 밤부터는 원래 판정으로 돌아온다.
        game.day_number = 2;
        assert!(game.is_police_detected_mafia_team(&mafia));
        assert_eq!(game.visible_role(&mafia), Role::Mafia);
    }

    /// [은폐] 처형 실패(치료·방탄)가 조용한 밤으로 가려지고, 군인 방탄은
    /// 소모되지만 공개 문구·정체 공개가 사라진다.
    #[test]
    fn concealment_hides_failed_kills_as_a_quiet_night() {
        // 치료 실패: quiet_night가 서고 보유자에게만 알림이 간다.
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Doctor),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities
            .insert(1, vec![TierAbility::Concealment]);
        game.mafia_targets.insert(1, 3);
        game.doctor_targets.insert(2, 3);

        let result = game.resolve_night().unwrap();
        assert!(result.quiet_night);
        assert!(game.get_player(3).unwrap().alive);
        assert!(result.tier_ability_results[&1].contains("[은폐]"));

        // 군인 방탄: 방탄은 소모되지만 공개 목록과 정체 공개가 비어 있다.
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Soldier),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities
            .insert(1, vec![TierAbility::Concealment]);
        game.mafia_targets.insert(1, 2);

        let result = game.resolve_night().unwrap();
        assert!(result.quiet_night);
        assert!(result.soldier_blocks.is_empty());
        assert!(game.get_player(2).unwrap().alive);
        assert!(game.soldier_bulletproof_used.contains(&2));
        assert!(!game.publicly_revealed_ids.contains(&2));

        // 보유자가 없으면 기존대로 공개된다.
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Soldier),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.mafia_targets.insert(1, 2);
        let result = game.resolve_night().unwrap();
        assert!(!result.quiet_night);
        assert_eq!(result.soldier_blocks.len(), 1);
    }

    /// [저격] 전날 밤 처형이 실패하면 다음 밤은 치료·방탄을 모두 관통하고,
    /// 성공한 밤 다음에는 발동하지 않는다.
    #[test]
    fn snipe_pierces_all_protection_after_a_failed_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Doctor),
            (3, Role::Soldier),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Snipe]);

        // 1일차 밤: 치료에 막혀 실패 → 저격 장전.
        game.mafia_targets.insert(1, 4);
        game.doctor_targets.insert(2, 4);
        game.resolve_night().unwrap();
        assert!(game.get_player(4).unwrap().alive);
        assert!(game.snipe_armed);

        // 2일차 밤: 치료 중인 대상도 관통해 처형한다.
        game.phase = Phase::Night;
        game.day_number = 2;
        game.mafia_targets.insert(1, 4);
        game.doctor_targets.insert(2, 4);
        let result = game.resolve_night().unwrap();
        assert!(!game.get_player(4).unwrap().alive);
        assert!(result.tier_ability_results[&1].contains("[저격]"));
        // 성공했으니 장전 해제.
        assert!(!game.snipe_armed);

        // 3일차 밤: 군인 방탄도 저격이 장전됐을 때만 관통된다. 우선 실패로 장전.
        game.phase = Phase::Night;
        game.day_number = 3;
        game.mafia_targets.insert(1, 5);
        game.doctor_targets.insert(2, 5);
        game.resolve_night().unwrap();
        assert!(game.snipe_armed);
        game.phase = Phase::Night;
        game.day_number = 4;
        game.mafia_targets.insert(1, 3);
        let result = game.resolve_night().unwrap();
        assert!(!game.get_player(3).unwrap().alive, "{result:?}");
        assert!(result.soldier_blocks.is_empty());
    }

    /// [독살] 시민팀 처형 실패 시 중독되어 다음 밤에 죽고, 마피아팀 보조는
    /// 면역이다.
    #[test]
    fn poison_kills_a_protected_citizen_one_day_later() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Doctor),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Poison]);

        // 1일차 밤: 치료에 막혀 실패 → 중독.
        game.mafia_targets.insert(1, 3);
        game.doctor_targets.insert(2, 3);
        let result = game.resolve_night().unwrap();
        assert!(game.get_player(3).unwrap().alive);
        assert!(
            result.tier_ability_results[&1].contains("[독살]"),
            "{result:?}"
        );
        assert_eq!(game.poisoned_death_days.get(&3), Some(&2));

        // 2일차 밤 결산: 중독 사망.
        game.phase = Phase::Night;
        game.day_number = 2;
        let result = game.resolve_night().unwrap();
        assert!(!game.get_player(3).unwrap().alive);
        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 3),
            "{:?}",
            result.killed_players
        );
        assert!(game.poisoned_death_days.is_empty());
    }

    /// [독살] 교주·광신도·마피아팀 보조는 중독되지 않는다.
    #[test]
    fn poison_does_not_affect_cult_or_mafia_support() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 1, 0, vec![Role::Spy]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Doctor),
            (3, Role::Spy),
            (4, Role::CultLeader),
            (5, Role::Citizen),
            (6, Role::Citizen),
            (7, Role::Citizen),
            (8, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Poison]);

        // 교주를 치료에 막혀 처형 실패 → 중독 안 됨.
        game.mafia_targets.insert(1, 4);
        game.doctor_targets.insert(2, 4);
        game.resolve_night().unwrap();
        assert!(game.poisoned_death_days.is_empty());
    }

    /// [승부수] 마지막 마피아의 처형은 치료·방탄을 모두 무시하고,
    /// 다른 마피아가 살아있으면 발동하지 않는다.
    #[test]
    fn all_in_kills_unconditionally_when_last_mafia_remains() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 2, 1, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Mafia),
            (3, Role::Doctor),
            (4, Role::Soldier),
            (5, Role::Citizen),
            (6, Role::Citizen),
            (7, Role::Citizen),
            (8, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::AllIn]);

        // 동료 마피아가 살아있으면 발동하지 않는다 (치료에 막힌다).
        game.mafia_targets.insert(1, 5);
        game.doctor_targets.insert(3, 5);
        game.resolve_night().unwrap();
        assert!(game.get_player(5).unwrap().alive);

        // 혼자 남으면 치료도 방탄도 무시한다.
        game.get_player_mut(2).unwrap().alive = false;
        game.phase = Phase::Night;
        game.day_number = 2;
        game.mafia_targets.insert(1, 4);
        game.doctor_targets.insert(3, 4);
        let result = game.resolve_night().unwrap();
        assert!(!game.get_player(4).unwrap().alive, "{result:?}");
        assert!(result.soldier_blocks.is_empty());
        assert!(result.tier_ability_results[&1].contains("[승부수]"));
    }

    /// [퇴마] 마피아팀이 죽인 비마피아팀 희생자가 성불된다.
    #[test]
    fn exorcism_purifies_non_mafia_victims() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Citizen),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Exorcism]);
        game.mafia_targets.insert(1, 3);

        let result = game.resolve_night().unwrap();
        assert!(!game.get_player(3).unwrap().alive);
        assert!(game.purified_dead_ids.contains(&3));
        assert!(
            result.tier_ability_results[&1].contains("[퇴마]"),
            "{result:?}"
        );
    }

    /// [확성]은 밤마다 보유자 전체에서 1회뿐이다. 먼저 쓰면 나머지는 그 밤에
    /// 못 쓰고, 다음 밤에는 다시 쓸 수 있다.
    #[test]
    fn loudspeaker_is_shared_once_per_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.tier_abilities.clear();
        game.tier_abilities
            .insert(4, vec![TierAbility::Loudspeaker]);
        game.tier_abilities
            .insert(5, vec![TierAbility::Loudspeaker]);

        let fourth = game.get_player(4).unwrap().clone();
        let fifth = game.get_player(5).unwrap().clone();
        assert!(game.is_loudspeaker_active(&fourth));
        assert!(game.is_loudspeaker_active(&fifth));

        // 4번이 먼저 사용하면 그 밤에는 5번도(그리고 4번 본인도) 못 쓴다.
        game.mark_loudspeaker_used();
        assert!(!game.is_loudspeaker_active(&fourth));
        assert!(!game.is_loudspeaker_active(&fifth));

        // 다음 밤에는 다시 사용할 수 있다.
        game.day_number += 1;
        assert!(game.is_loudspeaker_active(&fourth));
        assert!(game.is_loudspeaker_active(&fifth));
    }

    /// [무법] 경찰을 노린 공격은 치료를 무시한다.
    #[test]
    fn lawless_pierces_doctor_protection_on_police() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.player_tiers.insert(1, 4);
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Lawless]);

        game.submit_night_action(3, Some(2)).unwrap(); // 의사가 경찰 보호
        game.submit_night_action(1, Some(2)).unwrap(); // 마피아가 경찰 공격
        let result = game.resolve_night().unwrap();

        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 2),
            "{:?}",
            result.killed_players
        );
        assert!(
            result
                .tier_ability_results
                .get(&1)
                .is_some_and(|text| text.contains("[무법]")),
            "{:?}",
            result.tier_ability_results
        );
    }

    /// [무법] 경찰뿐 아니라 형사 등 경찰 계열 전체를 관통해 처형한다.
    #[test]
    fn lawless_pierces_protection_on_any_investigation_role() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Lawless]);

        game.submit_night_action(3, Some(2)).unwrap();
        game.submit_night_action(1, Some(2)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 2),
            "{:?}",
            result.killed_players
        );
    }

    /// [야습] 첫날 밤 자가 치료만 무시한다. 남이 치료해 준 경우는 못 뚫는다.
    #[test]
    fn night_raid_pierces_only_self_heal_on_night_one() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::NightRaid]);

        // 의사 자가 치료 → 야습이 뚫고, 의사 정체가 전체 공개된다.
        game.submit_night_action(3, Some(3)).unwrap();
        game.submit_night_action(1, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();
        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 3),
            "{:?}",
            result.killed_players
        );
        assert!(
            result
                .night_raid_reveals
                .iter()
                .any(|player| player.user_id == 3),
            "{:?}",
            result.night_raid_reveals
        );
        assert!(game.publicly_revealed_ids.contains(&3));
    }

    /// [수습] 마피아팀이 죽인 시민팀의 직업이 '시민'으로 바뀌고 보유자가 원 직업을 안다.
    #[test]
    fn cleanup_hides_the_victims_role_and_informs_the_holder() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Cleanup]);

        game.submit_night_action(1, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(
            result
                .tier_ability_results
                .get(&1)
                .is_some_and(|text| text.contains("의사")),
            "{:?}",
            result.tier_ability_results
        );
        // 발표용 사망자 목록은 '시민'으로 가려지지만 실제 직업은 유지된다
        // (역할 기반 내부 로직이 깨지지 않도록 판정만 가린다).
        assert_eq!(
            result
                .killed_players
                .iter()
                .find(|player| player.user_id == 3)
                .map(|player| player.role),
            Some(Role::Citizen)
        );
        let victim = game.get_player(3).unwrap().clone();
        assert_eq!(victim.role, Role::Doctor);
        assert_eq!(game.visible_role(&victim), Role::Citizen);
    }

    /// [도주] 처형 대신 도주하고, 다음날 투표 시작 때 사망한다.
    #[test]
    fn escape_defers_the_execution_to_the_next_vote() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(1, vec![TierAbility::Escape]);

        // 1을 지목해 찬반 가결.
        game.phase = Phase::Day;
        game.start_vote().unwrap();
        for voter in [2, 3, 4, 5] {
            game.submit_day_vote(voter, Some(1)).unwrap();
        }
        game.resolve_nomination_vote().unwrap();
        game.start_confirmation_vote().unwrap();
        for voter in [2, 3, 4, 5] {
            game.submit_confirmation_vote(voter, true).unwrap();
        }
        let confirm = game.resolve_confirmation_vote(1).unwrap();

        assert!(confirm.executed.is_none());
        assert_eq!(
            confirm.escaped.as_ref().map(|player| player.user_id),
            Some(1)
        );
        assert!(game.get_player(1).unwrap().alive);

        // 다음날 투표 시작 → 사망.
        game.phase = Phase::Day;
        let executed = game.start_vote().unwrap();
        assert_eq!(
            executed
                .iter()
                .map(|player| player.user_id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert!(!game.get_player(1).unwrap().alive);
        // 도주는 1회뿐 — 능력은 소모됐다.
        assert!(game.tier_abilities.get(&1).is_none());
    }

    /// [유언] 밤에 죽으면 유언이 공개된다. 살아있으면 공개되지 않는다.
    #[test]
    fn last_will_is_published_only_when_the_writer_dies_at_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.tier_abilities.clear();
        game.tier_abilities.insert(4, vec![TierAbility::LastWill]);

        game.submit_last_will(4, "마피아는 1번입니다").unwrap();
        // 유언 능력이 없는 사람은 작성 불가.
        assert!(game.submit_last_will(5, "테스트").is_err());

        // 첫 밤: 죽지 않음 → 공개 없음.
        game.submit_night_action(1, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();
        assert!(result.published_wills.is_empty());

        // 다음 밤: 작성자가 죽음 → 공개.
        game.phase = Phase::Night;
        game.day_number += 1;
        game.submit_night_action(1, Some(4)).unwrap();
        let result = game.resolve_night().unwrap();
        assert_eq!(
            result.published_wills,
            vec![("Four".to_string(), "마피아는 1번입니다".to_string())]
        );
    }

    /// [불침번] 스파이가 군인을 첩보하면 정보가 막히고 군인이 스파이의 정체를 안다.
    #[test]
    fn soldier_watch_blocks_spy_espionage_and_reveals_the_spy() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Spy]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Spy),
            (3, Role::Soldier),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        let reply = game.submit_night_action(2, Some(3)).unwrap();
        assert!(reply.contains("불침번"), "{reply}");
        assert!(!reply.contains("군인"), "{reply}");

        let result = game.resolve_night().unwrap();
        assert_eq!(
            result.soldier_watch_results.get(&3).map(String::as_str),
            Some("[불침번] 스파이 Two님의 첩보를 막아냈습니다.")
        );
        // 밤 결산 요약에서도 직업이 새지 않는다.
        if let Some(recap) = result.spy_results.get(&2) {
            assert!(!recap.contains("군인"), "{recap}");
        }
    }

    /// [불침번] 도둑이 군인에게 도벽을 쓰면 훔치지 못하고 군인이 도둑의 정체를 안다.
    #[test]
    fn soldier_watch_blocks_the_thief_steal() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Thief]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Thief),
            (3, Role::Soldier),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        game.submit_day_vote(2, Some(3)).unwrap();
        let vote_result = game.resolve_nomination_vote().unwrap();

        assert!(
            vote_result
                .thief_steal_results
                .get(&2)
                .is_some_and(|text| text.contains("불침번")),
            "{:?}",
            vote_result.thief_steal_results
        );
        assert_eq!(
            vote_result.thief_steal_results.get(&3).map(String::as_str),
            Some("[불침번] 도둑 Two님의 도벽을 막아냈습니다.")
        );
        assert!(game.thief_stolen_roles.is_empty());
        // 도벽 시도 자체는 소모된다.
        assert_eq!(game.thief_used_days.get(&2), Some(&1));
    }

    /// [불침번] 사기꾼이 군인을 사기 대상으로 고르면 변장이 무효가 되고, 군인은
    /// 게임 시작 안내에서 사기꾼의 정체를 안다.
    #[test]
    fn soldier_watch_blocks_the_fraudster_disguise() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Fraudster]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Fraudster),
            (3, Role::Soldier),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.fraudster_disguises.clear();
        game.fraudster_blocked_by_soldier.clear();
        // 군인이 무작위로 뽑힌 상황을 재현한다.
        game.fraudster_blocked_by_soldier.insert(2, 3);

        let fraudster = game.get_player(2).unwrap().clone();
        assert_eq!(game.visible_role(&fraudster), Role::Fraudster);
        assert!(!game.is_disguised_fraudster(&fraudster));
        assert!(game.fraudster_disguise_info(2).is_none());
    }

    /// [불침번] 청부 대상에 군인이 있으면 청부 전체가 무산되고 접선도 없다.
    #[test]
    fn soldier_watch_voids_the_contract_naming_a_soldier() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Contractor]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Contractor),
            (3, Role::Soldier),
            (4, Role::Doctor),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.phase = Phase::Night;
        game.day_number = 2;
        game.contractor_contracts
            .insert(2, ((3, Role::Soldier), (4, Role::Doctor)));

        let result = game.resolve_night().unwrap();

        assert!(
            result
                .contractor_results
                .get(&2)
                .is_some_and(|text| text.contains("불침번")),
            "{:?}",
            result.contractor_results
        );
        assert_eq!(
            result.soldier_watch_results.get(&3).map(String::as_str),
            Some("[불침번] 청부업자 Two님의 청부를 막아냈습니다.")
        );
        assert!(result.contractor_kills.is_empty());
        assert!(!game.contractor_contacted.contains(&2));
        assert!(game.get_player(4).unwrap().alive);
    }

    /// 스파이는 마피아를 찾아낸 밤마다 첩보를 한 번 더 쓸 수 있다 (최초 접선에만
    /// 주어지던 보너스를 매 밤으로 확장).
    #[test]
    fn spy_gets_a_bonus_action_every_night_a_mafia_is_found() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Spy]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Spy),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        let first = game.submit_night_action(2, Some(1)).unwrap();
        assert!(first.contains("한 번 더"), "{first}");
        assert!(first.contains("[접선]"), "{first}");
        // 보너스로 두 번째 첩보 사용 가능, 세 번째는 불가.
        game.submit_night_action(2, Some(3)).unwrap();
        assert!(game.submit_night_action(2, Some(4)).is_err());
        game.resolve_night().unwrap();

        // 다음 밤에도 마피아를 찾아내면 다시 한 번 더 쓸 수 있다.
        game.phase = Phase::Night;
        game.day_number += 1;
        let next = game.submit_night_action(2, Some(1)).unwrap();
        assert!(next.contains("한 번 더"), "{next}");
        // 이미 접선한 상태라 접선 안내는 반복되지 않는다.
        assert!(!next.contains("[접선]"), "{next}");
        assert!(game.submit_night_action(2, Some(4)).is_ok());
    }

    /// 사기꾼 기본 배치: 1 마피아, 2 사기꾼(3=의사로 변장), 3 의사, 4 경찰, 5 시민.
    fn fraudster_test_game() -> MafiaGame {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Fraudster),
            (3, Role::Doctor),
            (4, Role::Police),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.fraudster_disguises.clear();
        game.fraudster_disguises.insert(2, (3, Role::Doctor));
        game
    }

    #[test]
    fn fraudster_disguise_changes_role_judgment_and_deceives_investigators() {
        let mut game = fraudster_test_game();
        game.get_player_mut(4).unwrap().role = Role::Inspector;

        let fraudster = game.get_player(2).unwrap().clone();
        assert_eq!(game.visible_role(&fraudster), Role::Doctor);
        assert!(!game.is_known_mafia_team(&fraudster));

        // 형사가 사기꾼을 수사하면 변장 직업이 나오고, 사기꾼은 속임 알림을 받는다.
        game.submit_night_action(4, Some(2)).unwrap();
        let result = game.resolve_night().unwrap();
        assert_eq!(
            result.inspector_results.get(&4).map(String::as_str),
            Some("[Two님의 직업은 의사입니다.]")
        );
        assert!(
            result
                .fraudster_results
                .get(&2)
                .is_some_and(|text| text.contains("[Four님을 속였습니다.]")),
            "{:?}",
            result.fraudster_results
        );
    }

    #[test]
    fn fraudster_deceives_the_police_team_check() {
        let mut game = fraudster_test_game();

        game.submit_night_action(4, Some(2)).unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(result.police_target_is_mafia, Some(false));
        assert!(
            result
                .fraudster_results
                .get(&2)
                .is_some_and(|text| text.contains("[Four님을 속였습니다.]")),
            "{:?}",
            result.fraudster_results
        );
    }

    #[test]
    fn fraudster_survives_mafia_attack_and_contacts_the_team() {
        let mut game = fraudster_test_game();

        game.submit_night_action(1, Some(2)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(game.get_player(2).unwrap().alive);
        assert!(result.killed_players.is_empty());
        assert_eq!(result.fraudster_contacts, vec![2]);
        assert!(
            result
                .fraudster_results
                .get(&2)
                .is_some_and(|text| text.contains("[교섭]")),
            "{:?}",
            result.fraudster_results
        );
        let fraudster = game.get_player(2).unwrap().clone();
        assert!(game.is_known_mafia_team(&fraudster));
    }

    /// 사기 대상이 표적이 되면 공격 성공 여부와 무관하게 접선한다.
    #[test]
    fn attack_on_the_disguise_target_contacts_the_fraudster() {
        let mut game = fraudster_test_game();

        game.submit_night_action(1, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(!game.get_player(3).unwrap().alive);
        assert_eq!(result.fraudster_contacts, vec![2]);
        let fraudster = game.get_player(2).unwrap().clone();
        assert!(game.is_known_mafia_team(&fraudster));
    }

    #[test]
    fn fraudster_gets_a_disguise_at_game_start() {
        let players = (1..=8)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let game = MafiaGame::new(players, 1, 1, 1, vec![Role::Fraudster]).unwrap();
        let fraudster = game
            .players
            .iter()
            .find(|player| player.role == Role::Fraudster)
            .unwrap();

        let (target_id, disguised_role) = game.fraudster_disguises[&fraudster.user_id];
        let target = game.get_player(target_id).unwrap();
        assert!(game.is_citizen_team(target));
        assert_eq!(target.role, disguised_role);
        assert_ne!(target_id, fraudster.user_id);
    }

    /// 공무원/파파라치 기본 배치: 1 마피아, 2 공무원, 3 의사, 4 파파라치, 5 시민.
    fn civil_servant_test_game() -> MafiaGame {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::CivilServant),
            (3, Role::Doctor),
            (4, Role::Paparazzi),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game
    }

    #[test]
    fn civil_servant_query_reveals_the_role_holder_and_shares_with_paparazzi() {
        let mut game = civil_servant_test_game();

        let ack = game.submit_civil_servant_query(2, Role::Doctor).unwrap();
        assert_eq!(ack, "[의사를 조회합니다.]");

        let result = game.resolve_night().unwrap();

        assert_eq!(
            result.civil_servant_results.get(&2).map(String::as_str),
            Some("[Three님이 의사로 조회되었습니다.]")
        );
        assert_eq!(
            result.paparazzi_results.get(&4).map(String::as_str),
            Some("[Three님이 의사 직업이라는 정보를 공유받았습니다.]")
        );
    }

    /// 사망자도 조회에 걸린다.
    #[test]
    fn civil_servant_query_matches_dead_players() {
        let mut game = civil_servant_test_game();
        game.mark_dead(3);

        game.submit_civil_servant_query(2, Role::Doctor).unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(
            result.civil_servant_results.get(&2).map(String::as_str),
            Some("[Three님이 의사로 조회되었습니다.]")
        );
    }

    /// 기자 특종도 이슈 트리거다. 공개 발표라도 하루 몫을 소모한다.
    #[test]
    fn reporter_scoop_triggers_the_paparazzi_issue() {
        let mut game = civil_servant_test_game();
        game.get_player_mut(2).unwrap().role = Role::Reporter;
        game.day_number = 2;

        game.reporter_targets.insert(2, 3);
        let result = game.resolve_night().unwrap();

        assert!(result.reporter_results.contains_key(&2));
        assert_eq!(
            result.paparazzi_results.get(&4).map(String::as_str),
            Some("[Three님이 의사 직업이라는 정보를 공유받았습니다.]")
        );
    }

    /// 기자가 자신을 특종한 경우는 "다른 사람의 직업"이 아니므로 트리거가 아니다.
    #[test]
    fn reporter_self_scoop_does_not_trigger_the_issue() {
        let mut game = civil_servant_test_game();
        game.get_player_mut(2).unwrap().role = Role::Reporter;
        game.day_number = 2;

        game.reporter_targets.insert(2, 2);
        let result = game.resolve_night().unwrap();

        assert!(result.paparazzi_results.is_empty());
    }

    #[test]
    fn civil_servant_query_without_holder_consumes_the_night_use() {
        let mut game = civil_servant_test_game();

        game.submit_civil_servant_query(2, Role::Prophet).unwrap();
        // 같은 밤에는 다시 시도할 수 없다.
        assert!(game.submit_civil_servant_query(2, Role::Doctor).is_err());

        let result = game.resolve_night().unwrap();
        assert_eq!(
            result.civil_servant_results.get(&2).map(String::as_str),
            Some("[해당 직업을 보유한 플레이어가 없습니다.]")
        );
        // 알아낸 직업이 없으므로 파파라치에게도 공유되지 않는다.
        assert!(result.paparazzi_results.is_empty());

        // 다음 밤에는 다시 조회할 수 있다.
        game.phase = Phase::Night;
        game.day_number += 1;
        assert!(game.submit_civil_servant_query(2, Role::Doctor).is_ok());
    }

    #[test]
    fn civil_servant_cannot_query_police_lineage_or_citizen() {
        let mut game = civil_servant_test_game();

        for role in [
            Role::Police,
            Role::Agent,
            Role::Vigilante,
            Role::Inspector,
            Role::Citizen,
            Role::Mafia,
            Role::CivilServant,
        ] {
            assert!(
                game.submit_civil_servant_query(2, role).is_err(),
                "{role:?} must not be queryable"
            );
        }
    }

    #[test]
    fn paparazzi_shares_only_the_first_reveal_and_only_once_per_day() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
                (6, "Six".to_string()),
            ],
            1,
            0,
            0,
            Vec::new(),
        )
        .unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::CivilServant),
            (3, Role::Doctor),
            (4, Role::Paparazzi),
            (5, Role::Inspector),
            (6, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        // 같은 밤에 공무원 조회(의사)와 형사 수사(시민)가 함께 성공해도
        // 공유되는 것은 우선순위가 높은 조회 결과 하나뿐이다.
        game.submit_civil_servant_query(2, Role::Doctor).unwrap();
        game.submit_night_action(5, Some(6)).unwrap();
        let result = game.resolve_night().unwrap();

        let shared = result.paparazzi_results.get(&4).unwrap();
        assert!(shared.contains("Three"), "{shared}");
        assert!(shared.contains("의사"), "{shared}");
        assert!(!shared.contains("Six"), "{shared}");

        // 같은 날에는 두 번 공유되지 않았고, 다음 날에는 다시 공유된다.
        game.phase = Phase::Night;
        game.day_number += 1;
        game.submit_civil_servant_query(2, Role::Paparazzi).unwrap();
        let next_result = game.resolve_night().unwrap();
        let next_shared = next_result.paparazzi_results.get(&4).unwrap();
        assert!(next_shared.contains("파파라치"), "{next_shared}");
    }

    /// 실제 게임 순서 재현: 낮 1 해킹 → (해킹 결과는 밤 2 시작에 전달) → 밤 2 조회.
    /// 해킹 공유의 하루 몫은 해킹이 일어난 날(1일)에서 차감돼야 하고, 밤 2의 조회
    /// 공유(2일 몫)를 막으면 안 된다.
    #[test]
    fn day_hack_share_does_not_consume_the_next_days_issue() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
                (6, "Six".to_string()),
            ],
            1,
            0,
            0,
            Vec::new(),
        )
        .unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::CivilServant),
            (3, Role::Doctor),
            (4, Role::Paparazzi),
            (5, Role::Hacker),
            (6, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        // 밤 1: 아무 조사도 없이 지나간다.
        game.resolve_night().unwrap();
        // 낮 1: 해커가 해킹한다.
        game.submit_hacker_action(5, 6).unwrap();
        // 투표가 끝나 다음 밤으로 넘어간다 (day_number 1 → 2).
        game.start_vote().unwrap();
        game.resolve_nomination_vote().unwrap();
        assert_eq!(game.day_number, 2);

        // 밤 2 시작: 해킹 결과가 전달되고, 공유는 1일 몫으로 처리된다.
        let hacker_results = game.consume_hacker_results();
        let hack_share = hacker_results.get(&4).unwrap();
        assert!(hack_share.contains("Six"), "{hack_share}");
        assert!(game.paparazzi_shared_days.contains(&1));
        assert!(!game.paparazzi_shared_days.contains(&2));

        // 밤 2의 조회 공유는 2일 몫으로 정상 동작해야 한다.
        game.submit_civil_servant_query(2, Role::Doctor).unwrap();
        let result = game.resolve_night().unwrap();
        let night_share = result.paparazzi_results.get(&4).unwrap();
        assert!(night_share.contains("의사"), "{night_share}");
    }

    /// 밤 1 조사가 먼저 공유되면 같은 날(1일) 낮 해킹은 이미 몫을 쓴 뒤라 공유되지
    /// 않는다 — "하루 중 가장 먼저 알아낸 정보만".
    #[test]
    fn night_share_beats_the_same_days_hack() {
        let mut game = civil_servant_test_game();
        game.get_player_mut(5).unwrap().role = Role::Hacker;

        game.submit_civil_servant_query(2, Role::Doctor).unwrap();
        let result = game.resolve_night().unwrap();
        assert!(result.paparazzi_results.contains_key(&4));

        game.submit_hacker_action(5, 3).unwrap();
        game.start_vote().unwrap();
        game.resolve_nomination_vote().unwrap();
        let hacker_results = game.consume_hacker_results();
        assert!(hacker_results.contains_key(&5));
        assert!(!hacker_results.contains_key(&4), "{hacker_results:?}");
    }

    #[test]
    fn paparazzi_is_not_triggered_by_team_only_information() {
        let mut game = civil_servant_test_game();
        game.get_player_mut(2).unwrap().role = Role::Police;

        // 경찰 조사는 마피아 여부(팀)만 알아내므로 이슈가 발동하지 않는다.
        game.submit_night_action(2, Some(1)).unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(result.police_target_is_mafia, Some(true));
        assert!(result.paparazzi_results.is_empty());
    }

    /// 도둑(마피아팀)이 훔친 능력으로 알아낸 정보는 "시민팀이 알아낸 정보"가
    /// 아니므로 파파라치에게 공유되지 않는다.
    #[test]
    fn paparazzi_ignores_reveals_made_by_the_mafia_team() {
        let mut game = civil_servant_test_game();
        game.get_player_mut(2).unwrap().role = Role::Thief;
        game.thief_stolen_roles.insert(2, Role::CivilServant);

        game.submit_civil_servant_query(2, Role::Doctor).unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(
            result.civil_servant_results.get(&2).map(String::as_str),
            Some("[Three님이 의사로 조회되었습니다.]")
        );
        assert!(result.paparazzi_results.is_empty());
    }

    #[test]
    fn inspector_reveals_same_team_role_and_notifies_target() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(
            result.inspector_results.get(&2).map(String::as_str),
            Some("[Three님의 직업은 의사입니다.]")
        );
        assert_eq!(
            result.inspector_target_notices.get(&3).map(String::as_str),
            Some("[형사 Two님이 당신을 수사했습니다.]")
        );
    }

    /// 경찰은 대상을 고른 즉시(밤이 끝나기 전) 자기 선택에 대한 결과를 볼 수 있어야
    /// 하고, 대상을 바꾸면 바꾼 대상의 결과가 나와야 한다.
    #[test]
    fn police_result_is_available_as_soon_as_a_target_is_chosen() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        assert_eq!(game.police_result_for_actor(2), None);

        game.submit_night_action(2, Some(1)).unwrap();
        let mafia_result = game.police_result_for_actor(2).unwrap();
        assert!(mafia_result.contains("One"), "{mafia_result}");
        assert!(mafia_result.contains("마피아팀입니다"), "{mafia_result}");
    }

    /// 결과가 즉시 나오므로 같은 밤에 대상을 바꾸면 연속 조사가 된다. 첫 제출로
    /// 고정하고, 다음 밤에는 다시 조사할 수 있다.
    #[test]
    fn police_investigation_locks_after_the_first_submission() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(1)).unwrap();
        let error = game.submit_night_action(2, Some(3)).unwrap_err();
        assert!(error.to_string().contains("이미 이번 밤"), "{error}");
        // 결과는 첫 대상 그대로다.
        let result = game.police_result_for_actor(2).unwrap();
        assert!(result.contains("One"), "{result}");
        // 잠긴 행동은 변경 가능 목록에 없어야 밤 조기 종료가 막히지 않는다.
        let police = game.get_player(2).unwrap().clone();
        assert!(!game.night_action_can_be_changed(&police));

        game.resolve_night().unwrap();
        game.phase = Phase::Night;
        game.day_number += 1;
        assert!(game.submit_night_action(2, Some(3)).is_ok());
    }

    #[test]
    fn inspector_receives_result_when_target_dies_the_same_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(3)).unwrap();
        game.submit_night_action(1, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 3)
        );
        assert_eq!(
            result.inspector_results.get(&2).map(String::as_str),
            Some("[Three님의 직업은 의사입니다.]")
        );
        assert!(!result.inspector_target_notices.contains_key(&3));
    }

    #[test]
    fn inspector_investigation_is_single_use_per_game() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        // 경찰 계열 공통 규칙: 결과가 제출 즉시 나오므로 대상을 바꿀 수 없다.
        let immediate = game.submit_night_action(2, Some(3)).unwrap();
        assert!(
            immediate.contains("[Three님의 직업은 의사입니다.]"),
            "{immediate}"
        );
        assert!(game.inspector_used_ids.contains(&2));
        let error = game.submit_night_action(2, Some(4)).unwrap_err();
        assert!(error.to_string().contains("한 번만"), "{error}");
        // 밤 종료 시에도 결과 기록(리플레이/대상 알림)은 그대로 남는다.
        assert!(
            game.resolve_night()
                .unwrap()
                .inspector_results
                .contains_key(&2)
        );

        game.phase = Phase::Night;
        assert!(
            !game
                .night_action_actors()
                .iter()
                .any(|actor| actor.user_id == 2)
        );
        assert!(game.submit_night_action(2, Some(3)).is_err());
    }

    /// 다른 팀 수사는 즉시 "시민팀이 아닙니다"만 나오고 1회용은 소모된다.
    #[test]
    fn inspector_gets_an_immediate_no_result_for_another_team() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        let immediate = game.submit_night_action(2, Some(1)).unwrap();
        assert!(
            immediate.contains("[One님은 시민팀이 아닙니다.]"),
            "{immediate}"
        );
        assert!(game.inspector_used_ids.contains(&2));
    }

    /// 형사는 접선 여부와 무관하게 실제 소속으로 판정한다: 접선 전 마피아 보조나
    /// 교주팀도 "시민팀이 아닙니다"가 나오고, 대상에게 알림이 가지 않는다.
    #[test]
    fn inspector_judges_by_real_team_without_notifying_the_target() {
        for enemy_role in [Role::Spy, Role::CultLeader] {
            let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
            for (id, role) in [
                (1, Role::Mafia),
                (2, Role::Inspector),
                (3, enemy_role),
                (4, Role::Citizen),
                (5, Role::Citizen),
            ] {
                game.get_player_mut(id).unwrap().role = role;
            }
            assert!(!game.is_known_mafia_team(game.get_player(3).unwrap()));

            let immediate = game.submit_night_action(2, Some(3)).unwrap();
            assert!(
                immediate.contains("[Three님은 시민팀이 아닙니다.]"),
                "{enemy_role:?}: {immediate}"
            );

            let result = game.resolve_night().unwrap();
            assert!(!result.inspector_results.contains_key(&2));
            assert!(!result.inspector_target_notices.contains_key(&3));
        }
    }

    /// 자경단원 숙청 조사도 제출 즉시 결과가 나온다.
    #[test]
    fn vigilante_investigation_returns_the_result_immediately() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Vigilante]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Vigilante),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.phase = Phase::Day;

        let citizen_result = game.submit_vigilante_investigation(2, 3).unwrap();
        assert!(
            citizen_result.contains("[숙청] Three 님은 **마피아팀이 아닙니다**."),
            "{citizen_result}"
        );
        // 게임 중 1회라 재조사는 막힌다.
        assert!(game.submit_vigilante_investigation(2, 1).is_err());
    }

    /// 다른 팀을 수사하면 결과가 없지만 1회용은 그대로 소모된다.
    #[test]
    fn inspector_single_use_is_consumed_even_without_a_result() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(1)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(!result.inspector_results.contains_key(&2));
        assert!(game.inspector_used_ids.contains(&2));
    }

    #[test]
    fn inspector_does_not_reveal_or_notify_other_team() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(1)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(!result.inspector_results.contains_key(&2));
        assert!(!result.inspector_target_notices.contains_key(&1));
    }

    #[test]
    fn public_status_lists_alive_and_dead_players() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.get_player_mut(2).unwrap().alive = false;
        let status = game.public_status();
        assert!(status.contains("1일차 / 현재 단계: 밤"));
        assert!(status.contains("생존자(4명)"));
        assert!(status.contains("사망자: Two"));
    }

    #[test]
    fn stolen_terrorist_retaliates_against_citizen_team_when_thief_dies_at_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Thief),
            (3, Role::Citizen),
            (4, Role::CultLeader),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.thief_stolen_roles.insert(2, Role::Terrorist);
        game.terrorist_targets.insert(2, 3);
        game.mafia_targets.insert(1, 2);

        let result = game.resolve_night().unwrap();

        assert!(!game.get_player(2).unwrap().alive);
        assert!(!game.get_player(3).unwrap().alive);
        assert!(
            result
                .terrorist_retaliations
                .iter()
                .any(|(terrorist, target)| terrorist.user_id == 2 && target.user_id == 3)
        );
    }

    #[test]
    fn terrorist_retaliates_against_cult_team_when_citizen_team_terrorist_dies() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Terrorist),
            (3, Role::CultLeader),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.terrorist_targets.insert(2, 3);
        game.mafia_targets.insert(1, 2);

        let result = game.resolve_night().unwrap();

        assert!(!game.get_player(2).unwrap().alive);
        assert!(!game.get_player(3).unwrap().alive);
        assert!(
            result
                .terrorist_retaliations
                .iter()
                .any(|(terrorist, target)| terrorist.user_id == 2 && target.user_id == 3)
        );
    }

    #[test]
    fn terrorist_does_not_retaliate_against_same_team_target() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Terrorist),
            (3, Role::Citizen),
            (4, Role::CultLeader),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.terrorist_targets.insert(2, 3);
        game.mafia_targets.insert(1, 2);

        let result = game.resolve_night().unwrap();

        assert!(!game.get_player(2).unwrap().alive);
        assert!(game.get_player(3).unwrap().alive);
        assert!(result.terrorist_retaliations.is_empty());
    }

    #[test]
    fn stolen_terrorist_retaliates_when_thief_is_executed() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Thief),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.phase = Phase::FinalDefense;
        game.thief_stolen_roles.insert(2, Role::Terrorist);
        game.begin_terrorist_final_defense(2);
        game.submit_terrorist_final_defense_target(2, 3).unwrap();
        game.start_confirmation_vote().unwrap();
        game.confirm_votes.insert(1, true);

        let result = game.resolve_confirmation_vote(2).unwrap();

        assert_eq!(
            result.executed.as_ref().map(|player| player.user_id),
            Some(2)
        );
        assert!(result.extra_killed.iter().any(|player| player.user_id == 3));
        assert!(!game.get_player(2).unwrap().alive);
        assert!(!game.get_player(3).unwrap().alive);
    }

    #[test]
    fn terrorist_night_target_is_not_reused_when_executed_by_vote() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Terrorist),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.phase = Phase::ConfirmVote;
        game.terrorist_targets.insert(2, 1);
        game.confirm_votes.insert(3, true);

        let result = game.resolve_confirmation_vote(2).unwrap();

        assert_eq!(
            result.executed.as_ref().map(|player| player.user_id),
            Some(2)
        );
        assert!(result.extra_killed.is_empty());
        assert!(game.get_player(1).unwrap().alive);
    }

    #[test]
    fn terrorist_attacks_mafia_selected_during_final_defense() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Terrorist),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.phase = Phase::FinalDefense;

        let targets = game.begin_terrorist_final_defense(2);
        assert!(targets.iter().any(|player| player.user_id == 1));
        assert_eq!(
            game.submit_terrorist_final_defense_target(2, 1).unwrap(),
            "습격 대상: One"
        );
        game.start_confirmation_vote().unwrap();
        game.confirm_votes.insert(3, true);

        let result = game.resolve_confirmation_vote(2).unwrap();

        assert!(result.extra_killed.iter().any(|player| player.user_id == 1));
        assert!(!game.get_player(1).unwrap().alive);
    }

    #[test]
    fn terrorist_attacks_only_contacted_mafia_support_during_execution() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Terrorist),
            (3, Role::Spy),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.spy_contacted.insert(3);
        game.phase = Phase::FinalDefense;
        game.begin_terrorist_final_defense(2);
        game.submit_terrorist_final_defense_target(2, 3).unwrap();
        game.start_confirmation_vote().unwrap();
        game.confirm_votes.insert(4, true);

        let result = game.resolve_confirmation_vote(2).unwrap();

        assert!(result.extra_killed.iter().any(|player| player.user_id == 3));
        assert!(!game.get_player(3).unwrap().alive);
    }

    #[test]
    fn terrorist_does_not_attack_uncontacted_mafia_support_during_execution() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Terrorist),
            (3, Role::Spy),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.phase = Phase::FinalDefense;
        game.begin_terrorist_final_defense(2);
        game.submit_terrorist_final_defense_target(2, 3).unwrap();
        game.start_confirmation_vote().unwrap();
        game.confirm_votes.insert(4, true);

        let result = game.resolve_confirmation_vote(2).unwrap();

        assert!(result.extra_killed.is_empty());
        assert!(game.get_player(3).unwrap().alive);
    }

    #[test]
    fn mark_dead_reports_a_player_once() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();

        assert_eq!(game.mark_dead(1).map(|player| player.user_id), Some(1));
        assert!(game.mark_dead(1).is_none());
        assert_eq!(game.death_order, vec![1]);
    }

    #[test]
    fn mark_dead_removes_stale_vote_state() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.phase = Phase::Day;
        game.start_vote().unwrap();
        game.day_votes.insert(1, Some(2));
        game.day_votes.insert(3, Some(2));
        game.day_votes.insert(4, None);
        game.confirm_votes.insert(1, true);
        game.confirm_votes.insert(4, false);

        game.mark_dead(1).unwrap();
        game.mark_dead(2).unwrap();

        assert!(!game.day_votes.contains_key(&1));
        assert!(!game.day_votes.values().any(|target| *target == Some(2)));
        assert_eq!(game.current_vote_counts().get(&2), None);
        assert_eq!(game.current_skip_vote_count(), 1);
        assert_eq!(game.current_confirm_counts(), (0, 1));
    }

    #[test]
    fn confirmation_vote_executes_at_half_or_more_yes() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.phase = Phase::FinalDefense;
        game.start_confirmation_vote().unwrap();

        for voter_id in [1, 2, 3] {
            game.submit_confirmation_vote(voter_id, true).unwrap();
        }
        for voter_id in [4, 5] {
            game.submit_confirmation_vote(voter_id, false).unwrap();
        }

        let result = game.resolve_confirmation_vote(5).unwrap();

        assert!(result.approved);
        assert_eq!(result.executed.unwrap().user_id, 5);
    }

    #[test]
    fn gangster_vote_block_does_not_change_confirmation_majority() {
        let players = (1..=7)
            .map(|id| (id, format!("Player {id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 0, 0, Vec::new()).unwrap();
        game.get_player_mut(7).unwrap().role = Role::Citizen;
        game.phase = Phase::ConfirmVote;
        game.gangster_blocked_vote_days.insert(6, game.day_number);

        for voter_id in [1, 2, 3] {
            game.submit_confirmation_vote(voter_id, true).unwrap();
        }
        for voter_id in [4, 5, 6] {
            game.submit_confirmation_vote(voter_id, false).unwrap();
        }

        let result = game.resolve_confirmation_vote(7).unwrap();

        assert!(!result.approved);
        assert!(result.tied);
        assert!(result.executed.is_none());
        assert_eq!(result.vote_counts.get(&true).copied(), Some(3));
        assert_eq!(result.vote_counts.get(&false).copied(), Some(3));
        assert_eq!(result.weighted_vote_counts.get(&true).copied(), Some(3));
        assert_eq!(result.weighted_vote_counts.get(&false).copied(), Some(3));
    }

    #[test]
    fn politician_vote_displays_one_but_counts_as_two_for_nomination() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.get_player_mut(1).unwrap().role = Role::Politician;
        game.phase = Phase::Vote;

        game.submit_day_vote(1, Some(2)).unwrap();
        game.submit_day_vote(3, Some(4)).unwrap();

        let result = game.resolve_nomination_vote().unwrap();

        assert_eq!(
            result.executed.as_ref().map(|player| player.user_id),
            Some(2)
        );
        assert_eq!(result.vote_counts.get(&Some(2)).copied(), Some(1));
        assert_eq!(result.weighted_vote_counts.get(&Some(2)).copied(), Some(2));
    }

    #[test]
    fn politician_does_not_weight_confirmation_vote() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        game.get_player_mut(1).unwrap().role = Role::Politician;
        game.get_player_mut(2).unwrap().role = Role::Citizen;
        game.phase = Phase::ConfirmVote;

        game.submit_confirmation_vote(1, true).unwrap();
        game.submit_confirmation_vote(3, false).unwrap();

        let result = game.resolve_confirmation_vote(2).unwrap();

        assert!(!result.approved);
        assert!(result.tied);
        assert!(result.executed.is_none());
        assert_eq!(result.vote_counts.get(&true).copied(), Some(1));
        assert_eq!(result.vote_counts.get(&false).copied(), Some(1));
        assert_eq!(result.weighted_vote_counts.get(&true).copied(), Some(1));
        assert_eq!(result.weighted_vote_counts.get(&false).copied(), Some(1));
    }

    fn mercenary_test_game() -> MafiaGame {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Mercenary),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.mercenary_client_ids.clear();
        game.assign_mercenary_clients();
        game
    }

    #[test]
    fn mercenary_client_is_citizen_team_player() {
        let game = mercenary_test_game();
        let client = game.mercenary_client(2).unwrap();

        assert_ne!(client.user_id, 2);
        assert!(game.is_citizen_team(client));
    }

    #[test]
    fn mercenary_arms_when_client_dies_first_night() {
        let mut game = mercenary_test_game();
        let mafia_id = 1;
        let client = game.mercenary_client(2).unwrap().clone();
        let client_id = client.user_id;

        game.submit_night_action(mafia_id, Some(client_id)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.killed_players.iter().any(|p| p.user_id == client_id));
        assert!(game.mercenary_armed_ids.contains(&2));
        assert!(game.mercenary_contract_received_ids.contains(&2));
        assert_eq!(
            result.mercenary_results.get(&2).map(String::as_str),
            Some("[의뢰] 의뢰인이 사망했습니다. 이제 밤마다 플레이어 한 명을 처형할 수 있습니다.")
        );
        assert!(!result.mercenary_results[&2].contains(&client.name));
    }

    #[test]
    fn mercenary_arms_after_contracted_client_dies_at_night() {
        let mut game = mercenary_test_game();
        let mafia_id = 1;
        let client = game.mercenary_client(2).unwrap().clone();
        let client_id = client.user_id;
        assert_eq!(game.receive_mercenary_contracts().len(), 1);
        game.phase = Phase::Night;
        game.day_number = 2;

        game.submit_night_action(mafia_id, Some(client_id)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.killed_players.iter().any(|p| p.user_id == client_id));
        assert!(game.mercenary_armed_ids.contains(&2));
        assert_eq!(
            result.mercenary_results.get(&2).map(String::as_str),
            Some("[의뢰] 의뢰인이 사망했습니다. 이제 밤마다 플레이어 한 명을 처형할 수 있습니다.")
        );
        assert!(!result.mercenary_results[&2].contains(&client.name));
    }

    #[test]
    fn armed_mercenary_blocks_mafia_majority_win() {
        let mut game = mercenary_test_game();
        for id in [3, 4, 5] {
            game.get_player_mut(id).unwrap().alive = false;
        }

        assert_eq!(game.winner(), Some(Winner::Mafia));
        game.mercenary_armed_ids.insert(2);
        assert_eq!(game.winner(), None);
    }

    #[test]
    fn mercenary_executes_independently_at_night() {
        let mut game = mercenary_test_game();
        game.mercenary_armed_ids.insert(2);

        game.submit_night_action(2, Some(1)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.mercenary_kills.iter().any(|p| p.user_id == 1));
        assert!(result.killed_players.iter().any(|p| p.user_id == 1));
    }

    #[test]
    fn mercenary_kill_is_canceled_when_mercenary_dies_same_night() {
        let mut game = mercenary_test_game();
        game.mercenary_armed_ids.insert(2);

        game.submit_night_action(1, Some(2)).unwrap();
        game.submit_night_action(2, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.killed_players.iter().any(|p| p.user_id == 2));
        assert!(!result.killed_players.iter().any(|p| p.user_id == 3));
        assert!(result.mercenary_kills.is_empty());
        assert!(!result.mercenary_results.contains_key(&2));
        assert!(game.get_player(3).unwrap().alive);
    }

    #[test]
    fn police_result_is_canceled_when_police_dies_same_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Police),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(1, Some(2)).unwrap();
        game.submit_night_action(2, Some(1)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.killed_players.iter().any(|p| p.user_id == 2));
        assert!(result.police_target.is_none());
        assert_eq!(result.police_target_is_mafia, None);
    }

    #[test]
    fn doctor_protection_is_canceled_when_doctor_dies_same_night() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Doctor),
            (3, Role::Citizen),
            (4, Role::Godfather),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game.godfather_contacted.insert(4);

        game.submit_night_action(1, Some(2)).unwrap();
        game.submit_night_action(2, Some(3)).unwrap();
        game.submit_night_action(4, Some(3)).unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.killed_players.iter().any(|p| p.user_id == 2));
        assert!(result.killed_players.iter().any(|p| p.user_id == 3));
        assert!(result.protected.is_none());
    }

    #[test]
    fn doctor_can_change_night_target_before_morning() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Doctor),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(3)).unwrap();
        game.submit_night_action(2, Some(4)).unwrap();
        game.submit_night_action(1, Some(3)).unwrap();

        assert_eq!(game.doctor_targets.get(&2), Some(&4));
        assert!(!game.should_finish_night_early());

        let result = game.resolve_night().unwrap();

        assert_eq!(result.protected.unwrap().user_id, 4);
        assert!(
            result
                .killed_players
                .iter()
                .any(|player| player.user_id == 3)
        );
    }

    #[test]
    fn vigilante_can_change_execution_target_before_morning() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Vigilante),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(3)).unwrap();
        game.submit_night_action(2, Some(1)).unwrap();

        assert_eq!(game.vigilante_targets.get(&2), Some(&1));
        assert!(!game.vigilante_execution_used_ids.contains(&2));

        let result = game.resolve_night().unwrap();

        assert!(
            result
                .vigilante_kills
                .iter()
                .any(|player| player.user_id == 1)
        );
        assert!(game.vigilante_execution_used_ids.contains(&2));
    }

    #[test]
    fn cult_leader_change_does_not_convert_previous_target() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::CultLeader),
            (3, Role::Citizen),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.submit_night_action(2, Some(3)).unwrap();
        game.submit_night_action(2, Some(4)).unwrap();

        assert!(!game.culted_ids.contains(&3));
        assert!(!game.culted_ids.contains(&4));

        let result = game.resolve_night().unwrap();

        assert!(!game.culted_ids.contains(&3));
        assert!(game.culted_ids.contains(&4));
        assert_eq!(result.cult_bells, 1);
    }

    fn hypnotist_test_game() -> MafiaGame {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Hypnotist),
            (3, Role::Doctor),
            (4, Role::CultLeader),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        game
    }

    #[test]
    fn hypnotist_accumulates_targets_until_wake() {
        let mut game = hypnotist_test_game();

        game.submit_night_action(2, Some(1)).unwrap();
        game.resolve_night().unwrap();
        assert!(
            game.hypnotized_targets
                .get(&2)
                .is_some_and(|targets| targets.contains(&1))
        );

        game.advance_to_next_night();
        game.submit_night_action(2, Some(3)).unwrap();
        game.resolve_night().unwrap();

        let result = game.submit_hypnotist_wake(2).unwrap();
        assert!(result.contains("One님 : 마피아"));
        assert!(result.contains("Three님 : 시민팀"));
        assert!(!game.hypnotized_targets.contains_key(&2));
    }

    #[test]
    fn hypnotist_wake_blocks_next_night_action() {
        let mut game = hypnotist_test_game();

        game.submit_night_action(2, Some(4)).unwrap();
        game.resolve_night().unwrap();
        let result = game.submit_hypnotist_wake(2).unwrap();

        assert!(result.contains("Four님 : 교주"));
        game.advance_to_next_night();
        assert!(
            !game
                .night_action_actors()
                .iter()
                .any(|player| player.user_id == 2)
        );
    }

    #[test]
    fn stolen_police_result_is_independent_from_police_vote() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
                (6, "Six".to_string()),
            ],
            1,
            0,
            1,
            vec![Role::Thief],
        )
        .unwrap();
        let thief_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Thief)
            .unwrap()
            .user_id;
        let police_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Police)
            .unwrap()
            .user_id;
        let targets = game
            .players
            .iter()
            .filter(|player| player.user_id != thief_id && player.user_id != police_id)
            .take(2)
            .map(|player| (player.user_id, player.name.clone()))
            .collect::<Vec<_>>();
        let (police_target_id, police_target_name) = targets[0].clone();
        let (thief_target_id, thief_target_name) = targets[1].clone();

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        let vote_message = game.submit_day_vote(thief_id, Some(police_id)).unwrap();
        assert!(vote_message.contains("투표 대상"));
        assert!(vote_message.contains("[도벽]"));
        // 훔친 직업은 투표 응답이 아니라 투표 결산 후에야 알려준다.
        assert!(!vote_message.contains("경찰"), "{vote_message}");
        let vote_result = game.resolve_nomination_vote().unwrap();
        assert!(
            vote_result
                .thief_steal_results
                .get(&thief_id)
                .is_some_and(|text| text.contains("경찰")),
            "{:?}",
            vote_result.thief_steal_results
        );
        game.phase = Phase::Night;
        game.submit_night_action(police_id, Some(police_target_id))
            .unwrap();
        game.submit_night_action(thief_id, Some(thief_target_id))
            .unwrap();

        assert_eq!(game.police_targets.get(&police_id), Some(&police_target_id));
        assert!(!game.police_targets.contains_key(&thief_id));
        assert_eq!(
            game.thief_police_targets.get(&thief_id),
            Some(&thief_target_id)
        );
        assert_eq!(
            game.get_night_action_target(police_id),
            Some(police_target_id)
        );
        assert_eq!(
            game.get_night_action_target(thief_id),
            Some(thief_target_id)
        );
        assert!(
            game.police_result_for_actor(thief_id)
                .unwrap()
                .contains(&thief_target_name)
        );
        assert!(
            !game
                .police_result_for_actor(thief_id)
                .unwrap()
                .contains(&police_target_name)
        );

        let result = game.resolve_night().unwrap();

        assert_eq!(result.police_target.unwrap().user_id, police_target_id);
        let thief_result = result.thief_police_results.get(&thief_id).unwrap();
        assert!(thief_result.contains(&thief_target_name));
        assert!(!thief_result.contains(&police_target_name));
    }

    #[test]
    fn thief_stealing_vigilante_can_act_at_night() {
        let mut game = MafiaGame::new_with_counts(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            GameCounts {
                mafia_count: 1,
                vigilante_count: 1,
                special_roles: vec![Role::Thief],
                ..Default::default()
            },
        )
        .unwrap();
        let thief_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Thief)
            .unwrap()
            .user_id;
        let vigilante_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Vigilante)
            .unwrap()
            .user_id;
        let target_id = game
            .players
            .iter()
            .find(|player| player.user_id != thief_id && player.user_id != vigilante_id)
            .unwrap()
            .user_id;

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        let vote_message = game.submit_day_vote(thief_id, Some(vigilante_id)).unwrap();
        assert!(!vote_message.contains("자경단원"), "{vote_message}");
        let vote_result = game.resolve_nomination_vote().unwrap();
        assert!(
            vote_result
                .thief_steal_results
                .get(&thief_id)
                .is_some_and(|text| text.contains("자경단원")),
            "{:?}",
            vote_result.thief_steal_results
        );

        game.phase = Phase::Night;
        assert!(
            game.night_action_actors()
                .iter()
                .any(|player| player.user_id == thief_id)
        );
        let action_message = game.submit_night_action(thief_id, Some(target_id)).unwrap();

        assert!(action_message.contains("[도벽: 자경단원]"));
        assert_eq!(game.vigilante_targets.get(&thief_id), Some(&target_id));
    }

    #[test]
    fn thief_stealing_mafia_contacts_and_can_attack() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            0,
            vec![Role::Thief],
        )
        .unwrap();
        let mafia_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Mafia)
            .unwrap()
            .user_id;
        let thief_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Thief)
            .unwrap()
            .user_id;
        let target_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Citizen)
            .unwrap()
            .user_id;

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        let vote_message = game.submit_day_vote(thief_id, Some(mafia_id)).unwrap();
        // 접선도 투표 결산 시점에 이뤄진다.
        assert!(!vote_message.contains("접선"), "{vote_message}");
        assert!(!game.thief_contacted.contains(&thief_id));
        let vote_result = game.resolve_nomination_vote().unwrap();
        let thief = game.get_player(thief_id).unwrap().clone();

        assert!(
            vote_result
                .thief_steal_results
                .get(&thief_id)
                .is_some_and(|text| text.contains("마피아팀과 접선했습니다")),
            "{:?}",
            vote_result.thief_steal_results
        );
        assert_eq!(
            vote_result
                .thief_newly_contacted
                .iter()
                .map(|player| player.user_id)
                .collect::<Vec<_>>(),
            vec![thief_id]
        );
        assert!(game.thief_contacted.contains(&thief_id));
        assert!(game.is_known_mafia_team(&thief));
        assert_eq!(game.thief_night_role(&thief), Some(Role::Mafia));

        game.phase = Phase::Night;
        assert!(
            game.night_action_actors()
                .iter()
                .any(|player| player.user_id == thief_id)
        );
        assert!(game.submit_night_action(thief_id, Some(target_id)).is_ok());
        assert_eq!(game.mafia_targets.get(&thief_id), Some(&target_id));
    }

    /// 투표를 바꿔가며 여러 명의 직업을 알아내는 것을 막는다: 훔치는 대상은 마지막
    /// 지목 하나뿐이고, 결과도 결산 때 한 번만 나온다.
    #[test]
    fn thief_steal_follows_only_the_final_vote_target() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Thief]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Thief),
            (3, Role::Doctor),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        let first = game.submit_day_vote(2, Some(3)).unwrap();
        assert!(!first.contains("의사"), "{first}");
        let second = game.submit_day_vote(2, Some(1)).unwrap();
        assert!(!second.contains("마피아"), "{second}");

        let vote_result = game.resolve_nomination_vote().unwrap();

        // 마지막 지목(마피아)만 훔쳤고, 결과도 하나뿐이다.
        assert_eq!(vote_result.thief_steal_results.len(), 1);
        assert!(
            vote_result
                .thief_steal_results
                .get(&2)
                .is_some_and(|text| text.contains("One") && !text.contains("의사")),
            "{:?}",
            vote_result.thief_steal_results
        );
        let thief = game.get_player(2).unwrap().clone();
        assert_eq!(game.thief_night_role(&thief), Some(Role::Mafia));
    }

    #[test]
    fn police_does_not_detect_uncontacted_spy_as_mafia_team() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            1,
            vec![Role::Spy],
        )
        .unwrap();
        let police_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Police)
            .unwrap()
            .user_id;
        let spy_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Spy)
            .unwrap()
            .user_id;

        game.submit_night_action(police_id, Some(spy_id)).unwrap();

        assert!(game.police_result_ready());
        assert_eq!(game.current_police_result().1, Some(false));
        assert_eq!(
            game.resolve_night().unwrap().police_target_is_mafia,
            Some(false)
        );
    }

    #[test]
    fn police_detects_contacted_spy_as_mafia_team() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            1,
            vec![Role::Spy],
        )
        .unwrap();
        let police_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Police)
            .unwrap()
            .user_id;
        let spy_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Spy)
            .unwrap()
            .user_id;
        game.spy_contacted.insert(spy_id);

        game.submit_night_action(police_id, Some(spy_id)).unwrap();

        assert!(game.police_result_ready());
        assert_eq!(game.current_police_result().1, Some(true));
        assert_eq!(
            game.resolve_night().unwrap().police_target_is_mafia,
            Some(true)
        );
    }

    #[test]
    fn psychologist_treats_uncontacted_spy_and_citizen_as_same_team() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            0,
            vec![Role::Psychologist, Role::Spy],
        )
        .unwrap();
        game.phase = Phase::Day;
        let psychologist_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Psychologist)
            .unwrap()
            .user_id;
        let spy = game
            .players
            .iter()
            .find(|player| player.role == Role::Spy)
            .unwrap()
            .clone();
        let citizen = game
            .players
            .iter()
            .find(|player| player.role == Role::Citizen)
            .unwrap()
            .clone();

        assert_eq!(game.team_key(&spy), game.team_key(&citizen));
        assert!(
            game.submit_psychologist_observation(psychologist_id, spy.user_id, citizen.user_id)
                .is_ok()
        );
    }

    #[test]
    fn psychologist_treats_contacted_spy_and_citizen_as_different_team() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            0,
            vec![Role::Psychologist, Role::Spy],
        )
        .unwrap();
        let spy = game
            .players
            .iter()
            .find(|player| player.role == Role::Spy)
            .unwrap()
            .clone();
        let citizen = game
            .players
            .iter()
            .find(|player| player.role == Role::Citizen)
            .unwrap()
            .clone();
        game.spy_contacted.insert(spy.user_id);

        assert_ne!(game.team_key(&spy), game.team_key(&citizen));
    }

    #[test]
    fn vigilante_does_not_execute_uncontacted_spy() {
        let mut game = MafiaGame::new_with_counts(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            GameCounts {
                mafia_count: 1,
                vigilante_count: 1,
                special_roles: vec![Role::Spy],
                ..Default::default()
            },
        )
        .unwrap();
        let vigilante_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Vigilante)
            .unwrap()
            .user_id;
        let spy_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Spy)
            .unwrap()
            .user_id;

        game.submit_night_action(vigilante_id, Some(spy_id))
            .unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.vigilante_kills.is_empty());
        assert!(game.get_player(spy_id).unwrap().alive);
    }

    #[test]
    fn vigilante_executes_contacted_spy() {
        let mut game = MafiaGame::new_with_counts(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            GameCounts {
                mafia_count: 1,
                vigilante_count: 1,
                special_roles: vec![Role::Spy],
                ..Default::default()
            },
        )
        .unwrap();
        let vigilante_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Vigilante)
            .unwrap()
            .user_id;
        let spy_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Spy)
            .unwrap()
            .user_id;
        game.spy_contacted.insert(spy_id);

        game.submit_night_action(vigilante_id, Some(spy_id))
            .unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(
            result
                .vigilante_kills
                .iter()
                .map(|player| player.user_id)
                .collect::<Vec<_>>(),
            vec![spy_id]
        );
        assert!(!game.get_player(spy_id).unwrap().alive);
    }

    #[test]
    fn police_does_not_detect_uncontacted_witch_as_mafia_team() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            1,
            vec![Role::Witch],
        )
        .unwrap();
        let police_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Police)
            .unwrap()
            .user_id;
        let witch_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Witch)
            .unwrap()
            .user_id;

        game.submit_night_action(police_id, Some(witch_id)).unwrap();

        assert!(game.police_result_ready());
        assert_eq!(game.current_police_result().1, Some(false));
        assert_eq!(
            game.resolve_night().unwrap().police_target_is_mafia,
            Some(false)
        );
    }

    #[test]
    fn police_detects_contacted_witch_as_mafia_team() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Two".to_string()),
                (3, "Three".to_string()),
                (4, "Four".to_string()),
                (5, "Five".to_string()),
            ],
            1,
            0,
            1,
            vec![Role::Witch],
        )
        .unwrap();
        let police_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Police)
            .unwrap()
            .user_id;
        let witch_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Witch)
            .unwrap()
            .user_id;
        game.witch_contacted.insert(witch_id);

        game.submit_night_action(police_id, Some(witch_id)).unwrap();

        assert!(game.police_result_ready());
        assert_eq!(game.current_police_result().1, Some(true));
        assert_eq!(
            game.resolve_night().unwrap().police_target_is_mafia,
            Some(true)
        );
    }

    #[test]
    fn citizen_wins_when_known_mafia_dead() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
        let mafia_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Mafia)
            .unwrap()
            .user_id;
        game.get_player_mut(mafia_id).unwrap().alive = false;
        assert_eq!(game.winner(), Some(Winner::Citizen));
    }

    #[test]
    fn doctor_blocks_mafia_majority_attack() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
        let mafia = game
            .players
            .iter()
            .find(|p| p.role == Role::Mafia)
            .unwrap()
            .user_id;
        let doctor = game
            .players
            .iter()
            .find(|p| p.role == Role::Doctor)
            .unwrap()
            .user_id;
        let target = game
            .players
            .iter()
            .find(|p| p.role == Role::Citizen)
            .unwrap()
            .user_id;
        game.submit_night_action(mafia, Some(target)).unwrap();
        game.submit_night_action(doctor, Some(target)).unwrap();
        let result = game.resolve_night().unwrap();
        assert!(result.killed.is_none());
        assert_eq!(result.protected.unwrap().user_id, target);
        let events = game.rating_events.get(&doctor).unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.points == 5 && event.reason.contains("치료 성공"))
        );
    }

    #[test]
    fn single_submitted_mafia_attack_resolves_even_if_other_mafia_waits() {
        let mut game = MafiaGame::new(basic_players(), 2, 0, 0, Vec::new()).unwrap();
        let mafia = game
            .players
            .iter()
            .filter(|player| player.role == Role::Mafia)
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        let target = game
            .players
            .iter()
            .find(|player| player.role == Role::Citizen)
            .unwrap()
            .user_id;

        game.submit_night_action(mafia[0], Some(target)).unwrap();
        let result = game.resolve_night().unwrap();

        assert_eq!(result.killed.unwrap().user_id, target);
    }

    #[test]
    fn split_submitted_mafia_attacks_do_not_resolve() {
        let mut game = MafiaGame::new(basic_players(), 2, 0, 0, Vec::new()).unwrap();
        let mafia = game
            .players
            .iter()
            .filter(|player| player.role == Role::Mafia)
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        let targets = game
            .players
            .iter()
            .filter(|player| player.role == Role::Citizen)
            .map(|player| player.user_id)
            .take(2)
            .collect::<Vec<_>>();

        game.submit_night_action(mafia[0], Some(targets[0]))
            .unwrap();
        game.submit_night_action(mafia[1], Some(targets[1]))
            .unwrap();
        let result = game.resolve_night().unwrap();

        assert!(result.killed.is_none());
    }

    #[test]
    fn madam_seduction_lasts_until_following_vote_ends() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Madam]).unwrap();
        let madam_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Madam)
            .unwrap()
            .user_id;
        let doctor_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .unwrap()
            .user_id;

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        game.submit_day_vote(madam_id, Some(doctor_id)).unwrap();
        let other_voter_ids = game
            .alive_players()
            .into_iter()
            .filter(|player| player.user_id != madam_id)
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        for voter_id in other_voter_ids {
            game.submit_day_vote(voter_id, None).unwrap();
        }
        game.resolve_nomination_vote().unwrap();
        assert!(game.madam_seduced_ids.contains(&doctor_id));
        assert!(
            !game
                .night_action_actors()
                .iter()
                .any(|player| player.user_id == doctor_id)
        );

        game.resolve_night().unwrap();
        assert!(game.madam_seduced_ids.contains(&doctor_id));

        game.start_vote().unwrap();
        let voter_ids = game
            .alive_players()
            .into_iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        for voter_id in voter_ids {
            game.submit_day_vote(voter_id, None).unwrap();
        }
        game.resolve_nomination_vote().unwrap();
        assert!(!game.madam_seduced_ids.contains(&doctor_id));
        assert!(!game.madam_seduction_release_days.contains_key(&doctor_id));
        assert!(
            game.night_action_actors()
                .iter()
                .any(|player| player.user_id == doctor_id)
        );
        assert!(game.submit_night_action(doctor_id, Some(madam_id)).is_ok());
    }

    #[test]
    fn dead_madam_vote_does_not_seduce() {
        let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Madam]).unwrap();
        let madam_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Madam)
            .unwrap()
            .user_id;
        let doctor_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .unwrap()
            .user_id;

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        game.submit_day_vote(madam_id, Some(doctor_id)).unwrap();
        game.mark_dead(madam_id).unwrap();
        for voter_id in game
            .alive_players()
            .into_iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>()
        {
            game.submit_day_vote(voter_id, None).unwrap();
        }

        let result = game.resolve_nomination_vote().unwrap();

        assert!(result.madam_seduced.is_empty());
        assert!(!game.madam_seduced_ids.contains(&doctor_id));
    }

    #[test]
    fn madam_cannot_vote_for_herself() {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Madam]).unwrap();
        let madam_id = game
            .players
            .iter()
            .find(|player| player.role == Role::Madam)
            .unwrap()
            .user_id;

        game.phase = Phase::Day;
        game.start_vote().unwrap();
        let error = game.submit_day_vote(madam_id, Some(madam_id)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("마담은 자기 자신에게 투표할 수 없습니다.")
        );
        assert!(!game.day_votes.contains_key(&madam_id));
    }
}
