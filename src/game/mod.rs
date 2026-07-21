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

use crate::model::{Phase, Player, Role, Winner};
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
    pub vigilante_targets: HashMap<u64, u64>,
    pub vigilante_pending_results: HashMap<u64, u64>,
    pub vigilante_known_enemy_ids: HashMap<u64, HashSet<u64>>,
    pub vigilante_investigation_used_ids: HashSet<u64>,
    pub vigilante_execution_used_ids: HashSet<u64>,
    pub reporter_targets: HashMap<u64, u64>,
    pub reporter_skip_submitted: HashSet<u64>,
    pub reporter_used_ids: HashSet<u64>,
    pub hacker_targets: HashMap<u64, u64>,
    pub hacker_pending_results: HashMap<u64, u64>,
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
            vigilante_targets: HashMap::new(),
            vigilante_pending_results: HashMap::new(),
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
            Role::Thief => self.thief_contacted.contains(&player.user_id),
            Role::Witch => self.witch_contacted.contains(&player.user_id),
            Role::Scientist => self.scientist_contacted.contains(&player.user_id),
            Role::Madam => self.madam_contacted.contains(&player.user_id),
            Role::Godfather => self.godfather_contacted.contains(&player.user_id),
            _ => false,
        }
    }

    pub fn is_police_detected_mafia_team(&self, player: &Player) -> bool {
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
        } else {
            player.role
        }
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
        if self.is_cult_team(player) {
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
        counts
            .special_roles
            .iter()
            .any(|role| *role == Role::Inspector),
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
            assert!(!crate::model::CONTRACTOR_GUESS_ROLES.contains(&role));
        }
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
        assert!(vote_message.contains("자경단원"));

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
        let thief = game.get_player(thief_id).unwrap().clone();

        assert!(vote_message.contains("마피아팀과 접선했습니다"));
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
