// game/mod.rs
// 역할: MafiaGame 구조체 정의, 생성자, 기본 플레이어 조회, 팀 판별, 승리 조건,
//        공유 유틸리티 메서드 (majority_target, mark_dead, ensure_fanatic_reincarnation 등)

#![allow(clippy::collapsible_if, clippy::too_many_arguments, clippy::type_complexity)]

pub mod actors;
pub mod actions;
pub mod resolve;
pub mod vote;

use crate::model::{Phase, Player, Role, Winner};
use anyhow::{Result, bail};
use rand::seq::SliceRandom;
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

        let mut rng = rand::rng();
        roles.shuffle(&mut rng);
        let mut shuffled_players = players;
        shuffled_players.shuffle(&mut rng);

        let players = shuffled_players
            .into_iter()
            .zip(roles)
            .map(|((user_id, name), role)| Player::new(user_id, name, role))
            .collect::<Vec<_>>();
        let players_by_id = players
            .iter()
            .enumerate()
            .map(|(index, player)| (player.user_id, index))
            .collect();

        Ok(Self {
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
            rating_events: HashMap::new(),
            rating_action_counts: HashMap::new(),
        })
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

    pub fn is_citizen_team(&self, player: &Player) -> bool {
        !self.is_mafia_team(player) && !self.is_cult_team(player) && player.role != Role::Joker
    }

    pub fn is_frog(&self, player: &Player) -> bool {
        player.alive && self.frog_user_ids.contains(&player.user_id)
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
                    && !player.role.is_investigation_role()
                    && !self.is_publicly_revealed(player)
            })
            .cloned()
            .collect()
    }

    fn team_key(&self, player: &Player) -> &'static str {
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
            return Some(self.players[index].clone());
        }
        self.players[index].alive = false;
        self.frog_user_ids.remove(&user_id);
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
            return Some(Winner::Mafia);
        }
        None
    }

    fn prophet_winner(&self) -> Option<Winner> {
        if self.phase != Phase::Day || self.day_number < 4 {
            return None;
        }
        let prophet = self
            .players
            .iter()
            .filter(|player| player.alive && player.role == Role::Prophet)
            .min_by_key(|player| player.name.to_lowercase())?;
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
        let role = self.get_player(user_id)?.role;
        let maps: &[&HashMap<u64, u64>] = match role {
            Role::Mafia | Role::Gangster => &[&self.mafia_targets],
            Role::Doctor => &[&self.doctor_targets],
            Role::Police => &[&self.police_targets],
            Role::Agent => &[&self.detective_targets],
            Role::Vigilante => &[&self.vigilante_targets],
            Role::Godfather => &[&self.godfather_targets],
            Role::CultLeader => &[&self.cult_targets],
            Role::Fanatic => &[&self.fanatic_targets],
            Role::Shaman => &[&self.shaman_targets],
            Role::Witch => &[&self.witch_targets],
            Role::Priest => &[&self.priest_targets],
            Role::Terrorist => &[&self.terrorist_targets],
            Role::Spy => return self.spy_targets.get(&user_id).and_then(|v| v.first()).copied(),
            _ => &[],
        };
        maps.iter().find_map(|m| m.get(&user_id).copied())
    }

    /// Activity UI용: 현재 낮 투표 득표 집계 (targetId → 득표수)
    pub fn current_vote_counts(&self) -> HashMap<u64, usize> {
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for target_opt in self.day_votes.values() {
            if let Some(&target) = target_opt.as_ref() {
                *counts.entry(target).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Activity UI용: 찬반 투표 현황 (찬성수, 반대수)
    pub fn current_confirm_counts(&self) -> (usize, usize) {
        let yes = self.confirm_votes.values().filter(|&&v| v).count();
        let no = self.confirm_votes.values().filter(|&&v| !v).count();
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
        if tied.len() != 1 || highest <= voter_count / 2 {
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

    fn action_contains(&self, map: RoleActionMap, actor_id: u64) -> bool {
        match map {
            RoleActionMap::Doctor => self.doctor_targets.contains_key(&actor_id),
            RoleActionMap::Gangster => self.gangster_targets.contains_key(&actor_id),
            RoleActionMap::Police => self.police_targets.contains_key(&actor_id),
            RoleActionMap::Detective => self.detective_targets.contains_key(&actor_id),
            RoleActionMap::Shaman => self.shaman_targets.contains_key(&actor_id),
            RoleActionMap::Priest => self.priest_targets.contains_key(&actor_id),
            RoleActionMap::Witch => self.witch_targets.contains_key(&actor_id),
            RoleActionMap::Terrorist => self.terrorist_action_submitted.contains(&actor_id),
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
        };
    }

}

#[derive(Debug, Clone, Copy)]
enum RoleActionMap {
    Doctor,
    Gangster,
    Police,
    Detective,
    Shaman,
    Priest,
    Witch,
    Terrorist,
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
    ]
    .into_iter()
    .filter(|value| *value)
    .count();
    if investigation_role_count > 1 {
        bail!("경찰, 요원, 자경단원은 한 게임에 함께 배정할 수 없습니다.");
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

    #[test]
    fn indexes_players_by_id() {
        let game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
        assert_eq!(game.get_player(2).unwrap().name, "Two");
        assert!(game.get_player(999).is_none());
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
        assert!(events
            .iter()
            .any(|event| event.points == 5 && event.reason.contains("치료 성공")));
    }
}
