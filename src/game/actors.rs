// game/actors.rs
// 역할: 밤·낮·투표 단계에서 행동 가능한 플레이어 목록 조회, 제출 여부 확인, 경찰·해커 결과 처리

#![allow(clippy::collapsible_if, clippy::too_many_arguments, clippy::type_complexity)]

use crate::model::{Phase, Player, Role};
use std::collections::{HashMap, HashSet};

use super::{MafiaGame, count_values, majority_required};

impl MafiaGame {
    pub fn night_action_actors(&mut self) -> Vec<Player> {
        self.ensure_godfather_auto_contact();
        let alive = self
            .players
            .iter()
            .filter(|player| player.alive)
            .cloned()
            .collect::<Vec<_>>();
        let has_other_alive = alive.len() > 1;
        let unpurified_dead = self
            .players
            .iter()
            .filter(|player| !player.alive && !self.purified_dead_ids.contains(&player.user_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut actors = Vec::new();
        for player in alive.iter() {
            if self.is_frog(player) {
                continue;
            }
            if self.is_madam_seduced(player) && !self.is_mafia_team(player) {
                continue;
            }
            let acts = match player.role {
                Role::Mafia => alive
                    .iter()
                    .any(|target| self.can_mafia_attack(target, Some(player.user_id))),
                Role::Doctor => true,
                Role::Nurse => self.nurse_can_act(player, &alive),
                Role::Gangster => has_other_alive,
                Role::Thief => self.thief_can_act_at_night(player, &alive, &unpurified_dead),
                Role::Police | Role::Detective | Role::Spy | Role::Terrorist => has_other_alive,
                Role::Vigilante => !self.vigilante_execution_targets(player).is_empty(),
                Role::Reporter => self.reporter_can_act(player, &alive),
                Role::Contractor => self.contractor_can_act(player),
                Role::Witch => has_other_alive,
                Role::Shaman => !unpurified_dead.is_empty(),
                Role::Priest => {
                    !self.priest_used_ids.contains(&player.user_id) && !unpurified_dead.is_empty()
                }
                Role::Godfather => {
                    self.godfather_contacted.contains(&player.user_id) && has_other_alive
                }
                Role::CultLeader => self.cult_leader_can_act(player, &alive),
                Role::Fanatic => has_other_alive,
                _ => false,
            };
            if acts {
                actors.push(player.clone());
            }
        }
        actors
    }

    pub fn all_night_actions_submitted(&mut self) -> bool {
        self.phase == Phase::Night
            && self
                .night_action_actors()
                .iter()
                .all(|actor| self.night_action_submitted(actor))
    }

    pub fn has_changeable_mafia_action(&mut self) -> bool {
        self.night_action_actors().iter().any(|actor| {
            actor.role == Role::Mafia
                || (actor.role == Role::Thief && self.thief_night_role(actor) == Some(Role::Mafia))
        })
    }

    pub fn should_finish_night_early(&mut self) -> bool {
        self.all_night_actions_submitted() && !self.has_changeable_mafia_action()
    }

    fn night_action_submitted(&self, actor: &Player) -> bool {
        match actor.role {
            Role::Mafia => self.mafia_targets.contains_key(&actor.user_id),
            Role::Doctor => self.doctor_targets.contains_key(&actor.user_id),
            Role::Nurse => {
                self.nurse_targets.contains_key(&actor.user_id)
                    || self.nurse_prescription_targets.contains_key(&actor.user_id)
            }
            Role::Gangster => self.gangster_targets.contains_key(&actor.user_id),
            Role::Thief => self.stolen_night_action_submitted(actor),
            Role::Police => self.police_targets.contains_key(&actor.user_id),
            Role::Vigilante => self.vigilante_targets.contains_key(&actor.user_id),
            Role::Reporter => {
                self.reporter_targets.contains_key(&actor.user_id)
                    || self.reporter_skip_submitted.contains(&actor.user_id)
            }
            Role::Detective => self.detective_targets.contains_key(&actor.user_id),
            Role::Shaman => self.shaman_targets.contains_key(&actor.user_id),
            Role::Priest => self.priest_targets.contains_key(&actor.user_id),
            Role::Spy => {
                self.spy_targets
                    .get(&actor.user_id)
                    .is_some_and(|targets| !targets.is_empty())
                    && !self.spy_bonus_pending.contains(&actor.user_id)
            }
            Role::Contractor => self.contractor_contracts.contains_key(&actor.user_id),
            Role::Witch => self.witch_targets.contains_key(&actor.user_id),
            Role::Godfather => self.godfather_targets.contains_key(&actor.user_id),
            Role::Terrorist => self.terrorist_action_submitted.contains(&actor.user_id),
            Role::CultLeader => self.cult_targets.contains_key(&actor.user_id),
            Role::Fanatic => self.fanatic_targets.contains_key(&actor.user_id),
            _ => true,
        }
    }

    fn nurse_can_act(&self, player: &Player, alive: &[Player]) -> bool {
        if self.nurse_contacted.contains(&player.user_id) {
            return !alive.iter().any(|target| target.role == Role::Doctor) && !alive.is_empty();
        }
        alive.len() > 1
    }

    pub fn thief_night_role(&self, player: &Player) -> Option<Role> {
        if player.role != Role::Thief {
            return None;
        }
        let role = *self.thief_stolen_roles.get(&player.user_id)?;
        match role {
            Role::Mafia
            | Role::Doctor
            | Role::Police
            | Role::Reporter
            | Role::Detective
            | Role::Spy
            | Role::Contractor
            | Role::Shaman
            | Role::Priest
            | Role::Witch
            | Role::Godfather
            | Role::Terrorist
            | Role::Gangster => Some(role),
            _ => None,
        }
    }

    fn thief_can_act_at_night(
        &self,
        player: &Player,
        alive: &[Player],
        unpurified_dead: &[Player],
    ) -> bool {
        match self.thief_night_role(player) {
            Some(Role::Mafia | Role::Doctor) => !alive.is_empty(),
            Some(Role::Shaman) => !unpurified_dead.is_empty(),
            Some(Role::Priest) => {
                !unpurified_dead.is_empty() && !self.priest_used_ids.contains(&player.user_id)
            }
            Some(Role::Reporter) => self.reporter_can_act(player, alive),
            Some(_) => alive.len() > 1,
            None => false,
        }
    }

    fn stolen_night_action_submitted(&self, actor: &Player) -> bool {
        match self.thief_night_role(actor) {
            Some(Role::Mafia) => self.mafia_targets.contains_key(&actor.user_id),
            Some(Role::Doctor) => self.doctor_targets.contains_key(&actor.user_id),
            Some(Role::Police) => self.thief_police_targets.contains_key(&actor.user_id),
            Some(Role::Reporter) => {
                self.reporter_targets.contains_key(&actor.user_id)
                    || self.reporter_skip_submitted.contains(&actor.user_id)
            }
            Some(Role::Detective) => self.detective_targets.contains_key(&actor.user_id),
            Some(Role::Spy) => self
                .spy_targets
                .get(&actor.user_id)
                .is_some_and(|targets| !targets.is_empty()),
            Some(Role::Contractor) => self.contractor_contracts.contains_key(&actor.user_id),
            Some(Role::Shaman) => self.shaman_targets.contains_key(&actor.user_id),
            Some(Role::Priest) => {
                self.priest_targets.contains_key(&actor.user_id)
                    || self.priest_used_ids.contains(&actor.user_id)
            }
            Some(Role::Witch) => self.witch_targets.contains_key(&actor.user_id),
            Some(Role::Godfather) => self.godfather_targets.contains_key(&actor.user_id),
            Some(Role::Terrorist) => self.terrorist_action_submitted.contains(&actor.user_id),
            Some(Role::Gangster) => self.gangster_targets.contains_key(&actor.user_id),
            _ => true,
        }
    }

    fn cult_leader_can_act(&self, player: &Player, alive: &[Player]) -> bool {
        self.day_number % 2 == 1
            && alive.iter().any(|target| {
                target.user_id != player.user_id && !self.culted_ids.contains(&target.user_id)
            })
    }

    pub fn hacker_day_actors(&self) -> Vec<Player> {
        if self.phase != Phase::Day || self.alive_players().len() <= 1 {
            return Vec::new();
        }
        self.players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Hacker
                    && !self.is_madam_seduced(player)
                    && !self.hacker_used_ids.contains(&player.user_id)
            })
            .cloned()
            .collect()
    }

    pub fn vigilante_day_actors(&self) -> Vec<Player> {
        if self.phase != Phase::Day || self.alive_players().len() <= 1 {
            return Vec::new();
        }
        self.players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Vigilante
                    && !self.is_madam_seduced(player)
                    && !self
                        .vigilante_investigation_used_ids
                        .contains(&player.user_id)
            })
            .cloned()
            .collect()
    }

    pub fn vigilante_execution_targets(&self, actor: &Player) -> Vec<Player> {
        if actor.role != Role::Vigilante
            || !actor.alive
            || self.vigilante_execution_used_ids.contains(&actor.user_id)
        {
            return Vec::new();
        }
        self.players
            .iter()
            .filter(|player| player.alive && player.user_id != actor.user_id)
            .cloned()
            .collect()
    }

    pub fn consume_hacker_results(&mut self) -> HashMap<u64, String> {
        let pending = std::mem::take(&mut self.hacker_pending_results);
        let mut results = HashMap::new();
        for (actor_id, target_id) in pending {
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id) else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            results.insert(
                actor_id,
                format!(
                    "[해킹] {} 님의 직업은 **{}** 입니다.",
                    target.name,
                    self.visible_role(target).value()
                ),
            );
        }
        results
    }

    pub fn psychologist_day_actors(&self) -> Vec<Player> {
        if self.phase != Phase::Day || self.alive_players().len() < 3 {
            return Vec::new();
        }
        self.players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Psychologist
                    && !self.is_madam_seduced(player)
                    && self.psychologist_used_days.get(&player.user_id) != Some(&self.day_number)
            })
            .cloned()
            .collect()
    }

    pub fn thief_vote_actors(&self) -> Vec<Player> {
        if self.phase != Phase::Vote || self.alive_players().len() <= 1 {
            return Vec::new();
        }
        self.players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Thief
                    && !self.is_frog(player)
                    && self.thief_used_days.get(&player.user_id) != Some(&self.day_number)
            })
            .cloned()
            .collect()
    }

    fn police_action_actors(&mut self) -> Vec<Player> {
        self.night_action_actors()
            .into_iter()
            .filter(|player| player.role == Role::Police)
            .collect()
    }

    pub fn police_result_ready(&mut self) -> bool {
        let actors = self.police_action_actors();
        if actors.is_empty() {
            return false;
        }
        let actor_ids = actors
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let live_targets = self
            .police_targets
            .iter()
            .filter(|(actor_id, target_id)| {
                actor_ids.contains(actor_id)
                    && self.is_alive(**actor_id)
                    && self.is_alive(**target_id)
            })
            .map(|(actor_id, target_id)| (*actor_id, *target_id))
            .collect::<HashMap<_, _>>();
        if live_targets.is_empty() {
            return false;
        }
        if live_targets.len() == actors.len() {
            return true;
        }
        count_values(live_targets.values().copied())
            .values()
            .any(|count| *count >= majority_required(actors.len()))
    }

    pub fn current_police_result(&self) -> (Option<Player>, Option<bool>) {
        let police_targets = self
            .police_targets
            .iter()
            .filter_map(|(&actor_id, &target_id)| {
                let actor = self.get_player(actor_id)?;
                (actor.role == Role::Police).then_some((actor_id, target_id))
            })
            .collect::<HashMap<_, _>>();
        let target_id = self.majority_target(&police_targets);
        let target = target_id.and_then(|id| self.get_player(id).cloned());
        let is_mafia = target
            .as_ref()
            .map(|player| self.is_police_detected_mafia_team(player));
        (target, is_mafia)
    }

    pub fn police_result_message(&self) -> String {
        let (target, is_mafia) = self.current_police_result();
        let Some(target) = target else {
            return "경찰 조사 대상이 과반에 도달하지 못해 이번 밤 조사 결과가 없습니다.".to_string();
        };
        let result_text = if is_mafia.unwrap_or(false) {
            "마피아입니다"
        } else {
            "마피아가 아닙니다"
        };
        format!("조사 결과: {} 님은 **{}**.", target.name, result_text)
    }

    pub fn police_result_for_actor(&self, actor_id: u64) -> Option<String> {
        let actor = self.get_player(actor_id)?;
        if !actor.alive {
            return None;
        }
        let targets = match actor.role {
            Role::Thief if self.thief_night_role(actor) == Some(Role::Police) => {
                &self.thief_police_targets
            }
            _ => &self.police_targets,
        };
        let target_id = *targets.get(&actor_id)?;
        let target = self.get_player(target_id)?;
        if !target.alive {
            return None;
        }
        let result_text = if self.is_police_detected_mafia_team(target) {
            "마피아팀입니다"
        } else {
            "마피아팀이 아닙니다"
        };
        Some(format!("조사 결과: {} 님은 **{}**.", target.name, result_text))
    }

    pub fn thief_police_results(&self) -> HashMap<u64, String> {
        self.thief_police_targets
            .keys()
            .filter_map(|actor_id| {
                self.police_result_for_actor(*actor_id)
                    .map(|message| (*actor_id, message))
            })
            .collect()
    }

    pub fn consume_ready_police_result(&mut self) -> Option<String> {
        if self.police_result_announced || !self.police_result_ready() {
            return None;
        }
        self.police_result_announced = true;
        Some(self.police_result_message())
    }

    pub fn mark_police_result_announced(&mut self) {
        self.police_result_announced = true;
    }

    pub fn all_day_votes_submitted(&self) -> bool {
        self.phase == Phase::Vote
            && self.players.iter().filter(|p| p.alive).all(|player| {
                self.day_votes.contains_key(&player.user_id) || self.vote_blocked(player.user_id)
            })
    }

    pub fn all_confirm_votes_submitted(&self) -> bool {
        self.phase == Phase::ConfirmVote
            && self
                .players
                .iter()
                .filter(|p| p.alive)
                .all(|player| self.confirm_votes.contains_key(&player.user_id))
    }

}
