// game/resolve.rs
// 역할: 밤 행동 결산 (마피아 공격, 치료, 경찰 조사, 각종 특수 능력 결산), 저주·성불·소생 처리

#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

use crate::model::{NightResult, Phase, Player, Role};
use crate::system_random;
use anyhow::{Result, bail};
use rand::prelude::IndexedRandom;
use std::collections::{HashMap, HashSet};

use super::{MafiaGame, reported_protected_id};

impl MafiaGame {
    pub fn apply_witch_curses(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (Vec<Player>, Vec<u64>) {
        let mut cursed_players = Vec::new();
        let mut contacts = Vec::new();
        let targets = self.witch_targets.clone();
        for (actor_id, target_id) in targets {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            if !self.witch_curse_applied_actor_ids.insert(actor_id) {
                continue;
            }
            let actor_alive = self.get_player(actor_id).is_some_and(|actor| actor.alive);
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor_alive || self.is_frog(&target) || !target.alive {
                continue;
            }
            self.frog_user_ids.insert(target.user_id);
            self.clear_night_action(target.user_id);
            cursed_players.push(target.clone());
            self.resolve_priest_cult_after_curse(&target);
            if target.role == Role::Mafia && self.witch_contacted.insert(actor_id) {
                self.witch_contacts_this_night.push(actor_id);
                contacts.push(actor_id);
            }
        }
        (cursed_players, contacts)
    }

    fn night_protection(
        &self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashSet<u64>, Option<u64>, HashSet<u64>) {
        let mut healing_targets = self
            .doctor_targets
            .iter()
            .filter(|(actor_id, _)| {
                !blocked_actor_ids.contains(actor_id)
                    && self
                        .get_player(**actor_id)
                        .is_some_and(|actor| actor.role == Role::Doctor)
            })
            .map(|(actor_id, target_id)| (*actor_id, *target_id))
            .collect::<HashMap<_, _>>();
        let stolen_doctor_target_ids = self
            .doctor_targets
            .iter()
            .filter(|(actor_id, target_id)| {
                !blocked_actor_ids.contains(actor_id)
                    && self.is_stolen_doctor_actor(**actor_id)
                    && self.is_alive(**actor_id)
                    && self.is_alive(**target_id)
            })
            .map(|(_, target_id)| *target_id)
            .collect::<HashSet<_>>();
        if self.alive_role_count(Role::Doctor) == 0 {
            healing_targets.extend(
                self.nurse_targets
                    .iter()
                    .filter(|(actor_id, _)| !blocked_actor_ids.contains(actor_id))
                    .map(|(actor_id, target_id)| (*actor_id, *target_id)),
            );
        }
        let majority_protected_id = self.majority_target(&healing_targets);
        let mut protected_ids = stolen_doctor_target_ids;
        if let Some(id) = majority_protected_id {
            protected_ids.insert(id);
        }
        let enhanced_protection_ids = if majority_protected_id.is_some()
            && self.nurse_enhanced_heal_active(blocked_actor_ids)
        {
            protected_ids.clone()
        } else {
            HashSet::new()
        };
        (
            protected_ids,
            majority_protected_id,
            enhanced_protection_ids,
        )
    }

    fn attack_blocked_by_protection(
        &self,
        target: Option<&Player>,
        ignore_doctor: bool,
        protected_ids: &HashSet<u64>,
        enhanced_protection_ids: &HashSet<u64>,
    ) -> bool {
        let Some(target) = target else {
            return false;
        };
        enhanced_protection_ids.contains(&target.user_id)
            || (!ignore_doctor && protected_ids.contains(&target.user_id))
    }

    fn resolve_priest_cult_after_curse(&mut self, target: &Player) {
        if target.role != Role::Priest || self.culted_ids.contains(&target.user_id) {
            return;
        }
        for (actor_id, target_id) in self.cult_targets.clone() {
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            if target_id == target.user_id && actor.alive && actor.role == Role::CultLeader {
                self.culted_ids.insert(target.user_id);
                self.cult_bells_this_night += 1;
                return;
            }
        }
    }

    fn clear_night_action(&mut self, actor_id: u64) {
        self.mafia_targets.remove(&actor_id);
        self.mafia_display_targets.remove(&actor_id);
        self.doctor_targets.remove(&actor_id);
        self.nurse_targets.remove(&actor_id);
        self.nurse_prescription_targets.remove(&actor_id);
        self.gangster_targets.remove(&actor_id);
        self.police_targets.remove(&actor_id);
        self.thief_police_targets.remove(&actor_id);
        self.inspector_targets.remove(&actor_id);
        self.vigilante_targets.remove(&actor_id);
        self.hypnotist_targets.remove(&actor_id);
        self.mercenary_targets.remove(&actor_id);
        self.detective_targets.remove(&actor_id);
        self.shaman_targets.remove(&actor_id);
        self.priest_targets.remove(&actor_id);
        self.godfather_targets.remove(&actor_id);
        self.terrorist_action_submitted.remove(&actor_id);
        self.reporter_targets.remove(&actor_id);
        self.reporter_skip_submitted.remove(&actor_id);
        self.spy_targets.remove(&actor_id);
        self.spy_bonus_pending.remove(&actor_id);
        self.contractor_contracts.remove(&actor_id);
        self.witch_targets.remove(&actor_id);
        self.witch_curse_applied_actor_ids.remove(&actor_id);
    }

    pub fn resolve_night(&mut self) -> Result<NightResult> {
        if self.phase != Phase::Night {
            bail!("밤 단계만 정산할 수 있습니다.");
        }

        self.ensure_godfather_auto_contact();
        let godfather_attackers = self
            .godfather_targets
            .iter()
            .filter(|(actor_id, _)| {
                self.godfather_contacted.contains(actor_id)
                    || self.is_stolen_godfather_actor(**actor_id)
            })
            .map(|(actor_id, target_id)| (*actor_id, *target_id))
            .collect::<HashMap<_, _>>();
        let mafia_target_id = self.majority_target(&self.mafia_targets);
        let (protected_ids, _, enhanced_protection_ids) = self.night_protection(&HashSet::new());
        let godfather_target_id = self.majority_target(&godfather_attackers);

        let mafia_target = mafia_target_id.and_then(|id| self.get_player(id).cloned());
        let godfather_target = godfather_target_id.and_then(|id| self.get_player(id).cloned());

        let mut killed_players: Vec<Player> = Vec::new();
        let mut killed_by_mafia_team_ids = HashSet::new();
        let mut soldier_blocks = Vec::new();
        let mut lover_sacrifices = Vec::new();
        let initial_protected_ids = protected_ids.clone();
        let initial_enhanced_protection_ids = enhanced_protection_ids.clone();

        self.resolve_mafia_team_attack(
            mafia_target.as_ref(),
            false,
            true,
            &protected_ids,
            &enhanced_protection_ids,
            &mut killed_players,
            &mut killed_by_mafia_team_ids,
            &mut soldier_blocks,
            &mut lover_sacrifices,
        );
        self.resolve_mafia_team_attack(
            godfather_target.as_ref(),
            true,
            false,
            &protected_ids,
            &enhanced_protection_ids,
            &mut killed_players,
            &mut killed_by_mafia_team_ids,
            &mut soldier_blocks,
            &mut lover_sacrifices,
        );

        let mut blocked_actor_ids = killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let (protected_ids, majority_protected_id, enhanced_protection_ids) =
            self.night_protection(&blocked_actor_ids);
        if self.attack_blocked_by_protection(
            mafia_target.as_ref(),
            false,
            &initial_protected_ids,
            &initial_enhanced_protection_ids,
        ) && !self.attack_blocked_by_protection(
            mafia_target.as_ref(),
            false,
            &protected_ids,
            &enhanced_protection_ids,
        ) {
            self.resolve_mafia_team_attack(
                mafia_target.as_ref(),
                false,
                true,
                &protected_ids,
                &enhanced_protection_ids,
                &mut killed_players,
                &mut killed_by_mafia_team_ids,
                &mut soldier_blocks,
                &mut lover_sacrifices,
            );
        }
        if self.attack_blocked_by_protection(
            godfather_target.as_ref(),
            true,
            &initial_protected_ids,
            &initial_enhanced_protection_ids,
        ) && !self.attack_blocked_by_protection(
            godfather_target.as_ref(),
            true,
            &protected_ids,
            &enhanced_protection_ids,
        ) {
            self.resolve_mafia_team_attack(
                godfather_target.as_ref(),
                true,
                false,
                &protected_ids,
                &enhanced_protection_ids,
                &mut killed_players,
                &mut killed_by_mafia_team_ids,
                &mut soldier_blocks,
                &mut lover_sacrifices,
            );
        }
        let protected_id = reported_protected_id(
            &protected_ids,
            mafia_target_id,
            godfather_target_id,
            majority_protected_id,
        );
        let protected = protected_id.and_then(|id| self.get_player(id).cloned());
        blocked_actor_ids = killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        self.apply_witch_curses(&blocked_actor_ids);
        let timed_cult_bells = self.consume_cult_bells();
        let witch_contacts = self.witch_contacts_this_night.clone();
        let (contractor_results, contractor_contacts, contractor_kills) =
            self.resolve_contractor_results(&blocked_actor_ids);

        for target in &contractor_kills {
            self.kill_player(
                target.user_id,
                true,
                &mut killed_players,
                &mut killed_by_mafia_team_ids,
            );
        }
        blocked_actor_ids = killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let (vigilante_results, vigilante_kills) =
            self.resolve_vigilante_results(&blocked_actor_ids);
        for target in &vigilante_kills {
            self.kill_player(
                target.user_id,
                false,
                &mut killed_players,
                &mut killed_by_mafia_team_ids,
            );
        }
        blocked_actor_ids = killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let (mut mercenary_results, mercenary_kills) =
            self.resolve_mercenary_results(&blocked_actor_ids);
        for target in &mercenary_kills {
            self.kill_player(
                target.user_id,
                false,
                &mut killed_players,
                &mut killed_by_mafia_team_ids,
            );
        }
        let terrorist_retaliations = self
            .resolve_terrorist_night_retaliations(&killed_by_mafia_team_ids, &mut killed_players);
        for (actor_id, message) in self.activate_mercenaries_for_killed_clients(&killed_players) {
            mercenary_results
                .entry(actor_id)
                .and_modify(|text| {
                    text.push('\n');
                    text.push_str(&message);
                })
                .or_insert(message);
        }
        blocked_actor_ids = killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let (police_target, police_target_is_mafia) =
            self.current_police_result_excluding(&blocked_actor_ids);
        let police_target_id = police_target.as_ref().map(|player| player.user_id);
        let thief_police_results = self.thief_police_results_excluding(&blocked_actor_ids);
        let detective_results = self.resolve_detective_results(
            &blocked_actor_ids,
            mafia_target_id,
            protected_id,
            police_target_id,
            godfather_target_id,
        );
        let (inspector_results, inspector_target_notices) =
            self.resolve_inspector_results(&blocked_actor_ids);
        let (spy_results, spy_contacts) = self.resolve_spy_results(&blocked_actor_ids);
        let godfather_results = self.resolve_godfather_results(&blocked_actor_ids);
        let (shaman_results, shaman_purifications) =
            self.resolve_shaman_results(&blocked_actor_ids);
        self.apply_hypnotist_targets(&blocked_actor_ids);
        let (nurse_results, nurse_contacts) = self.resolve_nurse_results(&blocked_actor_ids);
        let gangster_results = self.resolve_gangster_results(&blocked_actor_ids);
        let (cult_results, cult_bells) = self.resolve_cult_results(&blocked_actor_ids);
        let (fanatic_results, fanatic_bells) = self.resolve_fanatic_results(&blocked_actor_ids);
        let mut fanatic_inherits = self.ensure_fanatic_reincarnation();
        let (priest_results, priest_revives) = self.resolve_priest_results(&killed_players);
        let graverobber_results = self.resolve_graverobbers(&killed_players);
        let agent_results = self.resolve_agent_results(&blocked_actor_ids);
        let reporter_results = self.resolve_reporter_results(
            &killed_players
                .iter()
                .map(|player| player.user_id)
                .collect::<HashSet<_>>(),
        );
        for id in self.ensure_fanatic_reincarnation() {
            if !fanatic_inherits.contains(&id) {
                fanatic_inherits.push(id);
            }
        }

        let result = NightResult {
            killed: killed_players.first().cloned(),
            protected,
            mafia_target,
            police_target_is_mafia,
            police_target,
            thief_police_results,
            killed_players,
            detective_results,
            inspector_results,
            inspector_target_notices,
            spy_results,
            spy_contacts,
            contractor_results,
            contractor_contacts,
            contractor_kills,
            witch_contacts,
            godfather_results,
            shaman_results,
            shaman_purifications,
            graverobber_results,
            terrorist_retaliations,
            soldier_blocks,
            lover_sacrifices,
            priest_results,
            priest_revives,
            agent_results,
            reporter_results,
            hacker_results: HashMap::new(),
            vigilante_results,
            vigilante_kills,
            mercenary_results,
            mercenary_kills,
            nurse_results,
            nurse_contacts,
            cult_results,
            fanatic_results,
            fanatic_inherits,
            gangster_results,
            cult_bells: timed_cult_bells + cult_bells + fanatic_bells,
            ..Default::default()
        };
        self.record_night_rating_events(&result);
        self.record_night_action_usage(&result);
        self.clear_night_maps();
        self.phase = Phase::Day;
        // Madam seductions expire when the following day's vote ends.

        Ok(result)
    }

    fn record_night_rating_events(&mut self, result: &NightResult) {
        let killed_ids = result
            .killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if let (Some(mafia_target), Some(protected)) = (&result.mafia_target, &result.protected)
            && mafia_target.user_id == protected.user_id
            && !killed_ids.contains(&mafia_target.user_id)
        {
            let doctors = self
                .doctor_targets
                .iter()
                .filter_map(|(&actor_id, &target_id)| {
                    (target_id == protected.user_id).then_some(actor_id)
                })
                .collect::<Vec<_>>();
            for actor_id in doctors {
                self.record_rating_event(actor_id, 5, "마피아 공격 치료 성공");
            }
        }

        if result.police_target_is_mafia == Some(true)
            && let Some(target) = &result.police_target
        {
            let police = self
                .police_targets
                .iter()
                .filter_map(|(&actor_id, &target_id)| {
                    (target_id == target.user_id).then_some(actor_id)
                })
                .collect::<Vec<_>>();
            for actor_id in police {
                self.record_rating_event(actor_id, 4, "경찰 조사로 마피아팀 확인");
            }
        }

        let vigilante_kills = result
            .vigilante_kills
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let vigilantes = self
            .vigilante_targets
            .iter()
            .filter_map(|(&actor_id, &target_id)| {
                let target = self.get_player(target_id)?;
                (vigilante_kills.contains(&target_id) && self.is_mafia_team(target))
                    .then_some(actor_id)
            })
            .collect::<Vec<_>>();
        for actor_id in vigilantes {
            self.record_rating_event(actor_id, 6, "숙청으로 마피아팀 처형");
        }

        let mercenary_kill_ids = result
            .mercenary_kills
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let mercenaries = self
            .mercenary_targets
            .iter()
            .filter_map(|(&actor_id, &target_id)| {
                mercenary_kill_ids.contains(&target_id).then_some(actor_id)
            })
            .collect::<Vec<_>>();
        for actor_id in mercenaries {
            self.record_rating_event(actor_id, 6, "의뢰 처형 성공");
        }

        for actor_id in &result.spy_contacts {
            self.record_rating_event(*actor_id, 4, "첩보로 마피아팀 접선");
        }
        for actor_id in &result.contractor_contacts {
            self.record_rating_event(*actor_id, 4, "청부 추측으로 마피아팀 접선");
        }
        for actor_id in &result.witch_contacts {
            self.record_rating_event(*actor_id, 4, "저주로 마피아팀 접선");
        }
        for actor_id in &result.nurse_contacts {
            self.record_rating_event(*actor_id, 3, "처방으로 의사 접선");
        }
        for actor_id in &result.fanatic_inherits {
            self.record_rating_event(*actor_id, 5, "광신도 재림으로 교주 능력 승계");
        }
        for player in &result.soldier_blocks {
            self.record_rating_event(player.user_id, 5, "군인 방탄 발동");
        }
        for (terrorist, target) in &result.terrorist_retaliations {
            if self.rating_team_key(terrorist) != self.rating_team_key(target) {
                self.record_rating_event(terrorist.user_id, 6, "테러리스트 반격으로 적팀 처치");
            }
        }
        for (lover, _) in &result.lover_sacrifices {
            self.record_rating_event(lover.user_id, 5, "연인 희생으로 상대 보호");
        }
        for actor_id in result.gangster_results.keys() {
            self.record_rating_event(*actor_id, 3, "공갈 성공");
        }
        for actor_id in result.agent_results.keys() {
            self.record_rating_event(*actor_id, 3, "요원 지령으로 시민 직업 확인");
        }
        for actor_id in result.inspector_results.keys() {
            self.record_rating_event(*actor_id, 3, "형사 수사로 같은 팀 직업 확인");
        }
        for (actor_id, text) in &result.cult_results {
            if text.contains("포교했습니다") {
                self.record_rating_event(*actor_id, 5, "포교 성공");
            }
        }
        for (actor_id, text) in &result.fanatic_results {
            if text.contains("포교했습니다") {
                self.record_rating_event(*actor_id, 4, "광신도 추종으로 포교 성공");
            }
        }
        let shamans = self
            .shaman_targets
            .iter()
            .filter_map(|(&actor_id, target_id)| {
                result
                    .shaman_purifications
                    .contains(target_id)
                    .then_some(actor_id)
            })
            .collect::<Vec<_>>();
        for actor_id in shamans {
            self.record_rating_event(actor_id, 3, "성불로 직업 정보 확보");
        }
        let revived_ids = result
            .priest_revives
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let priests = self
            .priest_targets
            .iter()
            .filter_map(|(&actor_id, &target_id)| {
                revived_ids.contains(&target_id).then_some(actor_id)
            })
            .collect::<Vec<_>>();
        for actor_id in priests {
            self.record_rating_event(actor_id, 6, "성직자 소생 성공");
        }
        let contractor_kill_ids = result
            .contractor_kills
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let contractors = self
            .contractor_contracts
            .iter()
            .filter_map(|(&actor_id, &((first_target, _), (second_target, _)))| {
                (contractor_kill_ids.contains(&first_target)
                    || contractor_kill_ids.contains(&second_target))
                .then_some(actor_id)
            })
            .collect::<Vec<_>>();
        for actor_id in contractors {
            self.record_rating_event(actor_id, 7, "청부 암살 성공");
        }
        for actor_id in result.graverobber_results.keys() {
            self.record_rating_event(*actor_id, 2, "도굴 성공");
        }
    }

    fn record_night_action_usage(&mut self, result: &NightResult) {
        for actor_id in result.vigilante_results.keys() {
            self.vigilante_execution_used_ids.insert(*actor_id);
        }
        for actor_id in result.reporter_results.keys() {
            self.reporter_used_ids.insert(*actor_id);
        }
        for actor_id in result.priest_results.keys() {
            self.priest_used_ids.insert(*actor_id);
        }
    }

    fn rating_team_key(&self, player: &Player) -> &'static str {
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

    fn clear_night_maps(&mut self) {
        self.mafia_targets.clear();
        self.mafia_display_targets.clear();
        self.doctor_targets.clear();
        self.nurse_targets.clear();
        self.nurse_prescription_targets.clear();
        self.nurse_contacts_this_night.clear();
        self.gangster_targets.clear();
        self.police_targets.clear();
        self.thief_police_targets.clear();
        self.inspector_targets.clear();
        self.vigilante_targets.clear();
        self.hypnotist_targets.clear();
        self.mercenary_targets.clear();
        self.reporter_targets.clear();
        self.reporter_skip_submitted.clear();
        self.detective_targets.clear();
        self.shaman_targets.clear();
        self.priest_targets.clear();
        self.spy_targets.clear();
        self.spy_bonus_pending.clear();
        self.spy_contacts_this_night.clear();
        self.contractor_contracts.clear();
        self.contractor_contacts_this_night.clear();
        self.witch_targets.clear();
        self.witch_contacts_this_night.clear();
        self.witch_curse_applied_actor_ids.clear();
        self.godfather_targets.clear();
        self.terrorist_action_submitted.clear();
        self.cult_targets.clear();
        self.fanatic_targets.clear();
        self.thief_stolen_roles.clear();
        self.cult_bells_this_night = 0;
        self.day_votes.clear();
        self.confirm_votes.clear();
        self.police_result_announced = false;
    }

    pub(super) fn apply_madam_seduction(
        &mut self,
        live_votes: &HashMap<u64, Option<u64>>,
    ) -> (Vec<Player>, Vec<Player>) {
        let mut seduced = Vec::new();
        let mut newly_contacted = Vec::new();
        for (voter_id, target_id) in live_votes {
            let Some(target_id) = target_id else {
                continue;
            };
            let Some(voter) = self.get_player(*voter_id).cloned() else {
                continue;
            };
            let Some(target) = self.get_player(*target_id).cloned() else {
                continue;
            };
            if !voter.alive
                || !target.alive
                || voter.role != Role::Madam
                || voter.user_id == target.user_id
            {
                continue;
            }
            if self.madam_seduced_ids.insert(target.user_id) {
                seduced.push(target.clone());
            }
            self.madam_seduction_release_days
                .insert(target.user_id, self.day_number + 1);
            if self.is_mafia_team(&target) {
                self.contact_mafia_team_member(&target);
                if self.madam_contacted.insert(voter.user_id) {
                    newly_contacted.push(voter);
                }
            }
        }
        (seduced, newly_contacted)
    }

    fn resolve_detective_results(
        &self,
        blocked_actor_ids: &HashSet<u64>,
        mafia_target_id: Option<u64>,
        protected_id: Option<u64>,
        police_target_id: Option<u64>,
        godfather_target_id: Option<u64>,
    ) -> HashMap<u64, String> {
        let mut results = HashMap::new();
        for (actor_id, watched_id) in &self.detective_targets {
            if blocked_actor_ids.contains(actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(*actor_id) else {
                continue;
            };
            let Some(watched) = self.get_player(*watched_id) else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            let action_target_id = self.resolved_action_target(
                watched,
                mafia_target_id,
                protected_id,
                police_target_id,
                godfather_target_id,
            );
            if let Some(action_target_id) = action_target_id {
                let target_name = self
                    .get_player(action_target_id)
                    .map(|player| player.name.clone())
                    .unwrap_or_else(|| action_target_id.to_string());
                results.insert(
                    *actor_id,
                    format!(
                        "{} 님은 밤에 {} 님에게 능력을 사용했습니다.",
                        watched.name, target_name
                    ),
                );
            } else {
                results.insert(
                    *actor_id,
                    format!("{} 님은 밤에 능력을 사용하지 않았습니다.", watched.name),
                );
            }
        }
        results
    }

    fn resolve_inspector_results(
        &self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, HashMap<u64, String>) {
        let mut results = HashMap::new();
        let mut target_notices = HashMap::new();
        for (actor_id, target_id) in &self.inspector_targets {
            if blocked_actor_ids.contains(actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(*actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(*target_id) else {
                continue;
            };
            if !actor.alive || !target.alive {
                continue;
            }
            target_notices.insert(
                target.user_id,
                format!("[형사 {}님이 당신을 수사했습니다.]", actor.name),
            );
            if self.team_key(actor) == self.team_key(target) {
                results.insert(
                    *actor_id,
                    format!(
                        "[{}님의 직업은 {}입니다.]",
                        target.name,
                        self.visible_role(target).value()
                    ),
                );
            }
        }
        (results, target_notices)
    }

    fn resolved_action_target(
        &self,
        watched: &Player,
        mafia_target_id: Option<u64>,
        protected_id: Option<u64>,
        police_target_id: Option<u64>,
        godfather_target_id: Option<u64>,
    ) -> Option<u64> {
        match watched.role {
            Role::Mafia => mafia_target_id,
            Role::Doctor => self.doctor_targets.get(&watched.user_id).copied(),
            Role::Nurse => self
                .nurse_targets
                .get(&watched.user_id)
                .or_else(|| self.nurse_prescription_targets.get(&watched.user_id))
                .copied(),
            Role::Gangster => self.gangster_targets.get(&watched.user_id).copied(),
            Role::Thief => self.resolved_thief_action_target(watched),
            Role::Police => self
                .police_targets
                .contains_key(&watched.user_id)
                .then_some(police_target_id)
                .flatten(),
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
            Role::Godfather => {
                if self.godfather_contacted.contains(&watched.user_id) {
                    godfather_target_id
                } else {
                    self.godfather_targets.get(&watched.user_id).copied()
                }
            }
            Role::CultLeader => self.cult_targets.get(&watched.user_id).copied(),
            Role::Fanatic => self.fanatic_targets.get(&watched.user_id).copied(),
            _ => {
                let _ = protected_id;
                None
            }
        }
    }

    fn resolved_thief_action_target(&self, watched: &Player) -> Option<u64> {
        match self.thief_night_role(watched) {
            Some(Role::Mafia) => self.mafia_targets.get(&watched.user_id).copied(),
            Some(Role::Doctor) => self.doctor_targets.get(&watched.user_id).copied(),
            Some(Role::Nurse) => self
                .nurse_targets
                .get(&watched.user_id)
                .or_else(|| self.nurse_prescription_targets.get(&watched.user_id))
                .copied(),
            Some(Role::Police) => self.thief_police_targets.get(&watched.user_id).copied(),
            Some(Role::Inspector) => self.inspector_targets.get(&watched.user_id).copied(),
            Some(Role::Vigilante) => self.vigilante_targets.get(&watched.user_id).copied(),
            Some(Role::Reporter) => self.reporter_targets.get(&watched.user_id).copied(),
            Some(Role::Detective) => self.detective_targets.get(&watched.user_id).copied(),
            Some(Role::Spy) => self
                .spy_targets
                .get(&watched.user_id)
                .and_then(|targets| targets.last().copied()),
            Some(Role::Contractor) => self
                .contractor_contracts
                .get(&watched.user_id)
                .map(|contract| contract.0.0),
            Some(Role::Shaman) => self.shaman_targets.get(&watched.user_id).copied(),
            Some(Role::Priest) => self.priest_targets.get(&watched.user_id).copied(),
            Some(Role::Witch) => self.witch_targets.get(&watched.user_id).copied(),
            Some(Role::Godfather) => self.godfather_targets.get(&watched.user_id).copied(),
            Some(Role::Terrorist) => self.terrorist_targets.get(&watched.user_id).copied(),
            Some(Role::Gangster) => self.gangster_targets.get(&watched.user_id).copied(),
            Some(Role::CultLeader) => self.cult_targets.get(&watched.user_id).copied(),
            Some(Role::Fanatic) => self.fanatic_targets.get(&watched.user_id).copied(),
            _ => None,
        }
    }

    fn resolve_terrorist_night_retaliations(
        &mut self,
        killed_by_mafia_team_ids: &HashSet<u64>,
        killed_players: &mut Vec<Player>,
    ) -> Vec<(Player, Player)> {
        let mut retaliations = Vec::new();
        for terrorist_id in killed_by_mafia_team_ids {
            let Some(terrorist) = self.get_player(*terrorist_id).cloned() else {
                continue;
            };
            if terrorist.role != Role::Terrorist {
                continue;
            }
            let Some(target_id) = self.terrorist_targets.get(terrorist_id).copied() else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if target.alive && self.is_mafia_team(&target) {
                if let Some(killed) = self.mark_dead(target.user_id) {
                    killed_players.push(killed.clone());
                    retaliations.push((terrorist, killed));
                }
            }
        }
        retaliations
    }

    fn resolve_spy_results(
        &self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, Vec<u64>) {
        let mut results = HashMap::new();
        for (actor_id, target_ids) in &self.spy_targets {
            if blocked_actor_ids.contains(actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(*actor_id) else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            let mut lines = Vec::new();
            for target_id in target_ids {
                if let Some(target) = self.get_player(*target_id) {
                    lines.push(format!(
                        "[첩보] {} 님의 직업은 **{}** 입니다.",
                        target.name,
                        self.visible_role(target).value()
                    ));
                }
            }
            if self.spy_contacts_this_night.contains(actor_id) {
                lines.push("[접선] 마피아와 접선했습니다.".to_string());
            }
            if !lines.is_empty() {
                results.insert(*actor_id, lines.join("\n"));
            }
        }
        (
            results,
            self.spy_contacts_this_night
                .iter()
                .copied()
                .filter(|actor_id| !blocked_actor_ids.contains(actor_id))
                .collect(),
        )
    }

    fn resolve_contractor_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, Vec<u64>, Vec<Player>) {
        let mut results = HashMap::new();
        let mut kills = Vec::new();
        let contracts = self.contractor_contracts.clone();
        for (actor_id, contract) in contracts {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id).cloned() else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            let targets = [
                (self.get_player(contract.0.0).cloned(), contract.0.1),
                (self.get_player(contract.1.0).cloned(), contract.1.1),
            ];
            let matched_mafia = targets.iter().any(|(target, guessed_role)| {
                target.as_ref().is_some_and(|target| {
                    target.alive && target.role == Role::Mafia && *guessed_role == Role::Mafia
                })
            });
            if matched_mafia {
                if actor.role == Role::Thief {
                    self.thief_contacted.insert(actor_id);
                } else {
                    self.contractor_contacted.insert(actor_id);
                }
                if !self.contractor_contacts_this_night.contains(&actor_id) {
                    self.contractor_contacts_this_night.push(actor_id);
                }
            }
            let success = targets.iter().all(|(target, guessed_role)| {
                target.as_ref().is_some_and(|target| {
                    target.alive
                        && self.is_citizen_team(target)
                        && target.role == *guessed_role
                        && !self.is_publicly_revealed(target)
                })
            });
            if !success {
                let mut text = "대상의 정보가 정확하지 않아 암살에 실패했습니다.".to_string();
                if matched_mafia {
                    text = format!("[동업] 마피아와 접선했습니다.\n{text}");
                }
                results.insert(actor_id, text);
                continue;
            }
            for (target, _) in targets {
                if let Some(target) = target {
                    if !kills.iter().any(|k: &Player| k.user_id == target.user_id) {
                        kills.push(target);
                    }
                }
            }
            let mut text = "청부가 성공했습니다. 대상 둘이 아침에 암살됩니다.".to_string();
            if matched_mafia {
                text = format!("[동업] 마피아와 접선했습니다.\n{text}");
            }
            results.insert(actor_id, text);
        }
        (
            results,
            self.contractor_contacts_this_night
                .iter()
                .copied()
                .filter(|actor_id| !blocked_actor_ids.contains(actor_id))
                .collect(),
            kills,
        )
    }

    fn resolve_godfather_results(&self, blocked_actor_ids: &HashSet<u64>) -> HashMap<u64, String> {
        self.godfather_targets
            .iter()
            .filter_map(|(actor_id, target_id)| {
                if blocked_actor_ids.contains(actor_id) {
                    return None;
                }
                let actor = self.get_player(*actor_id)?;
                let target = self.get_player(*target_id)?;
                (actor.alive && target.alive).then(|| {
                    (
                        *actor_id,
                        format!("{} 님을 확정 처치 대상으로 지목했습니다.", target.name),
                    )
                })
            })
            .collect()
    }

    fn resolve_shaman_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, Vec<u64>) {
        let mut results = HashMap::new();
        let mut purifications = Vec::new();
        for (actor_id, target_id) in self.shaman_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive || target.alive || self.purified_dead_ids.contains(&target.user_id) {
                continue;
            }
            self.purified_dead_ids.insert(target.user_id);
            purifications.push(target.user_id);
            results.insert(
                actor_id,
                format!(
                    "[성불] {} 님의 직업은 **{}** 입니다.\n대상은 사망자 채널에서 채팅할 수 없습니다.",
                    target.name,
                    self.visible_role(&target).value()
                ),
            );
        }
        (results, purifications)
    }

    fn resolve_reporter_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> HashMap<u64, String> {
        let mut results = HashMap::new();
        for (actor_id, target_id) in self.reporter_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            let visible_role = self.visible_role(&target);
            if visible_role != Role::Frog {
                self.publicly_revealed_ids.insert(target.user_id);
            }
            results.insert(
                actor_id,
                format!(
                    "[속보입니다! {}님이 {}이라는 소식입니다!]",
                    target.name,
                    visible_role.value()
                ),
            );
        }
        results
    }

    fn resolve_vigilante_results(
        &self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, Vec<Player>) {
        let mut results = HashMap::new();
        let mut kills = Vec::new();
        for (actor_id, target_id) in &self.vigilante_targets {
            if blocked_actor_ids.contains(actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(*actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(*target_id).cloned() else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            if target.alive && self.is_known_mafia_team(&target) {
                kills.push(target.clone());
                results.insert(
                    *actor_id,
                    format!("[숙청] {} 님을 처형했습니다.", target.name),
                );
            } else {
                results.insert(
                    *actor_id,
                    "[숙청] 대상이 마피아팀이 아니거나 이미 사망해 처형에 실패했습니다."
                        .to_string(),
                );
            }
        }
        (results, kills)
    }

    fn resolve_mercenary_results(
        &self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, Vec<Player>) {
        let mut results = HashMap::new();
        let mut kills = Vec::new();
        for (actor_id, target_id) in &self.mercenary_targets {
            if blocked_actor_ids.contains(actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(*actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(*target_id).cloned() else {
                continue;
            };
            if !actor.alive || !self.mercenary_armed_ids.contains(actor_id) {
                continue;
            }
            if target.alive {
                kills.push(target.clone());
                results.insert(
                    *actor_id,
                    format!("[의뢰] {} 님을 처형했습니다.", target.name),
                );
            } else {
                results.insert(
                    *actor_id,
                    "[의뢰] 대상이 이미 사망해 처형에 실패했습니다.".to_string(),
                );
            }
        }
        (results, kills)
    }

    fn apply_hypnotist_targets(&mut self, blocked_actor_ids: &HashSet<u64>) {
        for (actor_id, target_id) in self.hypnotist_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            if !actor.alive || actor.role != Role::Hypnotist {
                continue;
            }
            let Some(target) = self.get_player(target_id) else {
                continue;
            };
            if !target.alive {
                continue;
            }
            self.hypnotized_targets
                .entry(actor_id)
                .or_default()
                .insert(target_id);
        }
    }

    fn activate_mercenaries_for_killed_clients(
        &mut self,
        killed_players: &[Player],
    ) -> HashMap<u64, String> {
        let killed_ids = killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        let pairs = self.mercenary_client_ids.clone();
        let mut results = HashMap::new();
        for (mercenary_id, client_id) in pairs {
            if !killed_ids.contains(&client_id)
                || !self
                    .get_player(mercenary_id)
                    .is_some_and(|player| player.alive && player.role == Role::Mercenary)
            {
                continue;
            }
            if self.mercenary_armed_ids.insert(mercenary_id) {
                self.mercenary_contract_received_ids.insert(mercenary_id);
                let client_name = self
                    .get_player(client_id)
                    .map(|player| player.name.clone())
                    .unwrap_or_else(|| client_id.to_string());
                results.insert(
                    mercenary_id,
                    format!(
                        "[의뢰] 의뢰인 {} 님이 사망했습니다. 이제 밤마다 플레이어 한 명을 처형할 수 있습니다.",
                        client_name
                    ),
                );
            }
        }
        results
    }

    fn resolve_nurse_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, Vec<u64>) {
        let mut results = HashMap::new();
        for (actor_id, target_id) in self.nurse_prescription_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            if target.role == Role::Doctor {
                self.nurse_contacted.insert(actor_id);
                if !self.nurse_contacts_this_night.contains(&actor_id) {
                    self.nurse_contacts_this_night.push(actor_id);
                }
                results.insert(
                    actor_id,
                    format!(
                        "[처방] {} 님은 의사입니다. 의사와 접선했습니다.",
                        target.name
                    ),
                );
            } else {
                results.insert(
                    actor_id,
                    format!("[처방] {} 님은 의사가 아닙니다.", target.name),
                );
            }
        }
        for (actor_id, target_id) in &self.nurse_targets {
            if blocked_actor_ids.contains(actor_id) {
                continue;
            }
            if let (Some(actor), Some(target)) =
                (self.get_player(*actor_id), self.get_player(*target_id))
            {
                if actor.alive {
                    results.insert(
                        *actor_id,
                        format!("[치료] {} 님을 치료 대상으로 선택했습니다.", target.name),
                    );
                }
            }
        }
        (
            results,
            self.nurse_contacts_this_night
                .iter()
                .copied()
                .filter(|actor_id| !blocked_actor_ids.contains(actor_id))
                .collect(),
        )
    }

    fn resolve_gangster_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> HashMap<u64, String> {
        let mut results = HashMap::new();
        for (actor_id, target_id) in self.gangster_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive || !target.alive {
                continue;
            }
            self.gangster_used_ids.insert(actor_id);
            self.gangster_blocked_vote_days
                .insert(target.user_id, self.day_number);
            results.insert(
                actor_id,
                format!(
                    "[공갈] {} 님의 다음 낮 지목 투표권을 빼앗았습니다.",
                    target.name
                ),
            );
        }
        results
    }

    fn nurse_enhanced_heal_active(&self, blocked_actor_ids: &HashSet<u64>) -> bool {
        self.players.iter().any(|player| {
            player.alive
                && player.role == Role::Nurse
                && !blocked_actor_ids.contains(&player.user_id)
                && self.nurse_contacted.contains(&player.user_id)
        })
    }

    fn resolve_priest_results(
        &mut self,
        killed_players: &[Player],
    ) -> (HashMap<u64, String>, Vec<Player>) {
        let mut results = HashMap::new();
        let mut revived = Vec::new();
        let killed_ids = killed_players
            .iter()
            .map(|p| p.user_id)
            .collect::<HashSet<_>>();
        for (actor_id, target_id) in self.priest_targets.clone() {
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            if killed_ids.contains(&actor_id) || !actor.alive {
                continue;
            }
            let Some(target) = self.get_player(target_id).cloned() else {
                results.insert(
                    actor_id,
                    "[소생] 대상이 이미 살아있어 부활에 실패했습니다.".to_string(),
                );
                continue;
            };
            if target.alive {
                results.insert(
                    actor_id,
                    "[소생] 대상이 이미 살아있어 부활에 실패했습니다.".to_string(),
                );
                continue;
            }
            if self.purified_dead_ids.contains(&target.user_id) {
                results.insert(
                    actor_id,
                    "[소생] 대상이 성불 상태라 부활에 실패했습니다.".to_string(),
                );
                continue;
            }
            if let Some(index) = self.players_by_id.get(&target.user_id).copied() {
                self.players[index].alive = true;
                self.scientist_pending_revive_ids.remove(&target.user_id);
                let revived_player = self.players[index].clone();
                revived.push(revived_player.clone());
                results.insert(
                    actor_id,
                    format!("[소생] {} 님을 부활시켰습니다.", revived_player.name),
                );
            }
        }
        (results, revived)
    }

    fn resolve_cult_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, u32) {
        let mut results = HashMap::new();
        let mut cult_bells = 0;
        for (actor_id, target_id) in self.cult_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive
                || (actor.role != Role::CultLeader
                    && self.thief_stolen_roles.get(&actor_id) != Some(&Role::CultLeader))
                || !target.alive
            {
                continue;
            }
            if self.culted_ids.contains(&target.user_id) {
                results.insert(
                    actor_id,
                    format!(
                        "[포교] {} 님을 포교했습니다. 직업은 **{}** 입니다.",
                        target.name,
                        target.role.value()
                    ),
                );
                continue;
            }
            if self.is_mafia_team(&target) || target.role == Role::CultLeader {
                results.insert(actor_id, "[포교] 포교에 실패했습니다.".to_string());
                continue;
            }
            if target.role == Role::Priest {
                results.insert(actor_id, "[포교] 포교에 실패했습니다.".to_string());
                results.insert(
                    target.user_id,
                    format!(
                        "[신앙] 교주가 포교를 시도했습니다. 교주는 **{}** 님입니다.",
                        actor.name
                    ),
                );
                continue;
            }
            if self.culted_ids.insert(target.user_id) {
                cult_bells += 1;
            }
            results.insert(
                actor_id,
                format!(
                    "[포교] {} 님을 포교했습니다. 직업은 **{}** 입니다.",
                    target.name,
                    target.role.value()
                ),
            );
        }
        (results, cult_bells)
    }

    fn resolve_fanatic_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
    ) -> (HashMap<u64, String>, u32) {
        let mut results = HashMap::new();
        let mut cult_bells = 0;
        for (actor_id, target_id) in self.fanatic_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive
                || (actor.role != Role::Fanatic
                    && self.thief_stolen_roles.get(&actor_id) != Some(&Role::Fanatic))
            {
                continue;
            }
            let is_cult = self.is_cult_team(&target);
            if target.role == Role::CultLeader {
                if self.culted_ids.insert(actor_id) {
                    cult_bells += 1;
                }
            }
            let suffix = if is_cult {
                "교주팀입니다"
            } else {
                "교주팀이 아닙니다"
            };
            results.insert(
                actor_id,
                format!("[추종] {} 님은 **{}**.", target.name, suffix),
            );
        }
        (results, cult_bells)
    }

    fn resolve_agent_results(&mut self, blocked_actor_ids: &HashSet<u64>) -> HashMap<u64, String> {
        let alive = self
            .players
            .iter()
            .filter(|player| player.alive && !blocked_actor_ids.contains(&player.user_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut results = HashMap::new();
        for agent in alive.iter() {
            if agent.role != Role::Agent
                && self.thief_stolen_roles.get(&agent.user_id) != Some(&Role::Agent)
            {
                continue;
            }
            let candidates = alive
                .iter()
                .filter(|player| {
                    player.user_id != agent.user_id
                        && self.is_citizen_team(player)
                        && !self.agent_discovered_ids.contains(&player.user_id)
                        && !self.is_publicly_revealed(player)
                })
                .cloned()
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                results.insert(agent.user_id, "지령이 도착하지 않았습니다.".to_string());
                continue;
            }
            let mut rng = system_random::rng();
            let target = candidates.choose(&mut rng).cloned().unwrap();
            self.agent_discovered_ids.insert(target.user_id);
            results.insert(
                agent.user_id,
                format!(
                    "[공작] 지령이 도착했습니다.\n{} 님의 직업은 **{}** 입니다.",
                    target.name,
                    target.role.value()
                ),
            );
        }
        results
    }

    fn resolve_graverobbers(&mut self, killed_players: &[Player]) -> HashMap<u64, Role> {
        if self.day_number != 1 {
            return HashMap::new();
        }
        let inherited_role = killed_players
            .first()
            .map(|player| player.role)
            .unwrap_or(Role::Citizen);
        let mut results = HashMap::new();
        let graverobber_ids = self
            .players
            .iter()
            .filter(|player| player.alive && player.role == Role::Graverobber)
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        for id in graverobber_ids {
            if let Some(player) = self.get_player_mut(id) {
                player.role = inherited_role;
                results.insert(id, inherited_role);
            }
        }
        if !results.is_empty() {
            if let Some(robbed) = killed_players.first() {
                if let Some(player) = self.get_player_mut(robbed.user_id) {
                    player.role = if inherited_role.is_mafia_team() {
                        Role::Villain
                    } else {
                        Role::Citizen
                    };
                }
            }
        }
        results
    }

    fn lover_sacrifice_for(&self, target: &Player) -> Option<Player> {
        if target.role != Role::Lover {
            return None;
        }
        let alive_lovers = self
            .players
            .iter()
            .filter(|player| player.alive && player.role == Role::Lover)
            .cloned()
            .collect::<Vec<_>>();
        if alive_lovers.len() < 2 {
            return None;
        }
        alive_lovers
            .into_iter()
            .find(|lover| lover.user_id != target.user_id)
    }

    fn resolve_mafia_team_attack(
        &mut self,
        target: Option<&Player>,
        ignore_doctor: bool,
        allow_soldier_block: bool,
        protected_ids: &HashSet<u64>,
        enhanced_protection_ids: &HashSet<u64>,
        killed_players: &mut Vec<Player>,
        killed_by_mafia_team_ids: &mut HashSet<u64>,
        soldier_blocks: &mut Vec<Player>,
        lover_sacrifices: &mut Vec<(Player, Player)>,
    ) {
        let Some(target) = target.cloned() else {
            return;
        };
        if !target.alive {
            return;
        }
        if let Some(lover_savior) = self.lover_sacrifice_for(&target) {
            self.kill_player(
                lover_savior.user_id,
                true,
                killed_players,
                killed_by_mafia_team_ids,
            );
            lover_sacrifices.push((lover_savior, target));
            return;
        }
        if enhanced_protection_ids.contains(&target.user_id) {
            return;
        }
        if !ignore_doctor && protected_ids.contains(&target.user_id) {
            return;
        }
        if allow_soldier_block
            && target.role == Role::Soldier
            && !self.soldier_bulletproof_used.contains(&target.user_id)
        {
            self.soldier_bulletproof_used.insert(target.user_id);
            self.publicly_revealed_ids.insert(target.user_id);
            soldier_blocks.push(target);
            return;
        }
        self.kill_player(
            target.user_id,
            true,
            killed_players,
            killed_by_mafia_team_ids,
        );
    }

    fn kill_player(
        &mut self,
        user_id: u64,
        by_mafia_team: bool,
        killed_players: &mut Vec<Player>,
        killed_by_mafia_team_ids: &mut HashSet<u64>,
    ) {
        if let Some(killed) = self.mark_dead(user_id) {
            if !killed_players
                .iter()
                .any(|player| player.user_id == killed.user_id)
            {
                killed_players.push(killed.clone());
            }
            if by_mafia_team {
                killed_by_mafia_team_ids.insert(killed.user_id);
            }
        }
    }
}
