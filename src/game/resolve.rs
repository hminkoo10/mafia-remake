// game/resolve.rs
// 역할: 밤 행동 결산 (마피아 공격, 치료, 경찰 조사, 각종 특수 능력 결산), 저주·성불·소생 처리

#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

use crate::model::{NightResult, Phase, Player, Role, TierAbility};
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
        self.civil_servant_targets.remove(&actor_id);
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
        // [저격] 다음 밤 장전: 이번 밤 마피아팀 처형 선언이 있었지만 아무도
        // 죽이지 못했을 때만 장전된다 (성공하거나 선언이 없었으면 해제).
        self.snipe_armed = (mafia_target.is_some() || godfather_target.is_some())
            && killed_by_mafia_team_ids.is_empty();
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
        let terrorist_retaliations = self.resolve_terrorist_night_retaliations(&mut killed_players);
        // [수습] 마피아팀이 죽인 대상의 직업을 보유자에게 알려주고, 시민팀이면
        // 발표·이후 조사에서 '시민'으로 보이게 바꾼다 (레이팅은 시작 직업 기준).
        self.resolve_cleanup(&mut killed_players, &killed_by_mafia_team_ids);
        // [유언] 밤에 죽은 유언 보유자의 유언을 아침에 공개한다.
        let published_wills = killed_players
            .iter()
            .filter(|player| {
                self.tier_abilities.get(&player.user_id) == Some(&TierAbility::LastWill)
            })
            .filter_map(|player| {
                let will = self.last_wills.get(&player.user_id)?.clone();
                Some((player.name.clone(), will))
            })
            .collect::<Vec<_>>();
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
        // 파파라치 이슈용: 시민팀이 이번 밤 알아낸 "다른 플레이어의 정확한 직업" 목록.
        // (우선순위, 알아낸 사람, 대상, 직업) — 우선순위가 낮을수록 먼저 알아낸 것으로 본다.
        let mut role_reveals: Vec<(u8, u64, u64, Role)> = Vec::new();
        let civil_servant_results =
            self.resolve_civil_servant_results(&blocked_actor_ids, &mut role_reveals);
        let (inspector_results, inspector_target_notices) =
            self.resolve_inspector_results(&blocked_actor_ids, &mut role_reveals);
        let (spy_results, spy_contacts) = self.resolve_spy_results(&blocked_actor_ids);
        let godfather_results = self.resolve_godfather_results(&blocked_actor_ids);
        let (shaman_results, shaman_purifications) =
            self.resolve_shaman_results(&blocked_actor_ids, &mut role_reveals);
        self.apply_hypnotist_targets(&blocked_actor_ids);
        let (nurse_results, nurse_contacts) = self.resolve_nurse_results(&blocked_actor_ids);
        let gangster_results = self.resolve_gangster_results(&blocked_actor_ids);
        let (cult_results, cult_bells) = self.resolve_cult_results(&blocked_actor_ids);
        let (fanatic_results, fanatic_bells) = self.resolve_fanatic_results(&blocked_actor_ids);
        let mut fanatic_inherits = self.ensure_fanatic_reincarnation();
        let (priest_results, priest_revives) = self.resolve_priest_results(&killed_players);
        let graverobber_results = self.resolve_graverobbers(&killed_players);
        let agent_results = self.resolve_agent_results(&blocked_actor_ids, &mut role_reveals);
        let reporter_results = self.resolve_reporter_results(
            &killed_players
                .iter()
                .map(|player| player.user_id)
                .collect::<HashSet<_>>(),
            &mut role_reveals,
        );
        for id in self.ensure_fanatic_reincarnation() {
            if !fanatic_inherits.contains(&id) {
                fanatic_inherits.push(id);
            }
        }

        let paparazzi_results = self.resolve_paparazzi_issue(&role_reveals);
        let (fraudster_results, fraudster_contacts) =
            self.resolve_fraudster_results(&blocked_actor_ids, &role_reveals);
        self.queue_wanted_notices();
        self.queue_directive_notices();
        let soldier_watch_results = self.drain_soldier_watch_notices();
        let tier_ability_results = self.drain_tier_ability_notices();
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
            civil_servant_results,
            paparazzi_results,
            fraudster_results,
            fraudster_contacts,
            soldier_watch_results,
            quiet_night: self.concealed_kill_failure,
            tier_ability_results,
            published_wills,
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
        self.concealed_kill_failure = false;
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
        self.civil_servant_targets.clear();
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

    /// 공무원 조회 결산. 지목한 직업의 생존 보유자를 알려주고, 없으면 없다고
    /// 알려준다(그날 밤 능력은 이미 소모됨). 조회 성공은 파파라치 이슈의
    /// 최우선 공유 후보다.
    fn resolve_civil_servant_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
        role_reveals: &mut Vec<(u8, u64, u64, Role)>,
    ) -> HashMap<u64, String> {
        use crate::model::korean_ro_particle;
        let mut results = HashMap::new();
        let mut rating_actor_ids = Vec::new();
        for (actor_id, queried_role) in self.civil_servant_targets.clone() {
            if blocked_actor_ids.contains(&actor_id) {
                continue;
            }
            let Some(actor) = self.get_player(actor_id) else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            // 사망자도 조회에 걸린다. 생존자만 세면 "없습니다" 결과에서 사망자의
            // 직업이 역산되는 것을 막을 수 없고, 규칙상으로도 사망자의 직업 정보를
            // 조회로 알아낼 수 있어야 한다.
            let holders = self
                .players
                .iter()
                .filter(|player| {
                    player.user_id != actor_id && self.visible_role(player) == queried_role
                })
                .cloned()
                .collect::<Vec<_>>();
            if holders.is_empty() {
                results.insert(
                    actor_id,
                    "[해당 직업을 보유한 플레이어가 없습니다.]".to_string(),
                );
                continue;
            }
            let names = holders
                .iter()
                .map(|player| format!("{}님", player.name))
                .collect::<Vec<_>>()
                .join(", ");
            results.insert(
                actor_id,
                format!(
                    "[{names}이 {}{} 조회되었습니다.]",
                    queried_role.value(),
                    korean_ro_particle(queried_role.value())
                ),
            );
            rating_actor_ids.push(actor_id);
            for holder in &holders {
                role_reveals.push((0, actor_id, holder.user_id, queried_role));
            }
        }
        for actor_id in rating_actor_ids {
            self.record_rating_event(actor_id, 3, "조회로 직업 확인");
        }
        results
    }

    /// 파파라치 이슈: 시민팀이 이번 하루 중 처음으로 "다른 플레이어의 정확한 직업"을
    /// 알아냈을 때 그 정보를 함께 받는다. 팀만 알아내는 능력(경찰·자경단원 등),
    /// 자기 자신에 대한 정보, 마피아팀(도둑)이 훔친 능력으로 알아낸 정보는 제외한다.
    fn resolve_paparazzi_issue(
        &mut self,
        role_reveals: &[(u8, u64, u64, Role)],
    ) -> HashMap<u64, String> {
        let mut reveals = role_reveals.to_vec();
        reveals.sort_unstable_by_key(|(priority, actor_id, target_id, _)| {
            (*priority, *actor_id, *target_id)
        });
        let Some((_, _, target_id, revealed_role)) =
            reveals.into_iter().find(|(_, actor_id, target_id, _)| {
                actor_id != target_id
                    && self
                        .get_player(*actor_id)
                        .is_some_and(|actor| self.is_citizen_team(actor))
            })
        else {
            return HashMap::new();
        };
        let Some(target_name) = self.get_player(target_id).map(|player| player.name.clone()) else {
            return HashMap::new();
        };
        self.share_issue_with_paparazzi(self.day_number, &target_name, revealed_role)
    }

    /// 하루 한 번뿐인 이슈 공유를 실행한다. 밤 결산과 낮 해킹 결산이 같은 판단을
    /// 공유하며, 한 번 발동하면 받을 수 있는 파파라치가 없었더라도 그날 몫은 소모된다.
    pub(crate) fn share_issue_with_paparazzi(
        &mut self,
        day: u32,
        target_name: &str,
        revealed_role: Role,
    ) -> HashMap<u64, String> {
        if !self.paparazzi_shared_days.insert(day) {
            return HashMap::new();
        }
        let recipient_ids = self
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && player.role == Role::Paparazzi
                    && !self.is_frog(player)
                    && !self.is_madam_seduced(player)
            })
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        let mut results = HashMap::new();
        for paparazzi_id in recipient_ids {
            self.record_rating_event(paparazzi_id, 2, "이슈로 직업 정보 공유");
            results.insert(
                paparazzi_id,
                format!(
                    "[{target_name}님이 {} 직업이라는 정보를 공유받았습니다.]",
                    revealed_role.value()
                ),
            );
        }
        results
    }

    /// [수습] 처리. 보유자가 살아있는 마피아팀일 때만 발동한다.
    fn resolve_cleanup(
        &mut self,
        killed_players: &mut [Player],
        killed_by_mafia_team_ids: &HashSet<u64>,
    ) {
        let holder_ids = self
            .mafia_tier_ability_holders(TierAbility::Cleanup)
            .into_iter()
            .filter(|holder_id| {
                !killed_players
                    .iter()
                    .any(|player| player.user_id == *holder_id)
            })
            .collect::<Vec<_>>();
        if holder_ids.is_empty() {
            return;
        }
        for victim in killed_players
            .iter_mut()
            .filter(|player| killed_by_mafia_team_ids.contains(&player.user_id))
        {
            let original_role = victim.role;
            for holder_id in &holder_ids {
                self.pending_tier_ability_notices.push((
                    *holder_id,
                    format!(
                        "[수습] {}님의 직업은 {}이었습니다.",
                        victim.name,
                        original_role.value()
                    ),
                ));
            }
            let is_citizen = self
                .get_player(victim.user_id)
                .is_some_and(|player| self.is_citizen_team(player));
            if is_citizen && original_role != Role::Citizen {
                // 실제 role은 그대로 두고 판정만 가린다. 발표용 사본만 시민으로 바꾼다.
                self.cleanup_masked_ids.insert(victim.user_id);
                victim.role = Role::Citizen;
            }
        }
    }

    /// [수배] 첫 번째 낮이 될 때(첫 밤 결산) 접선하지 않은 마피아팀 명단을
    /// 보유자에게 알린다. 밤 사망 처리 후에 불러 아침 생존자 기준으로 잡는다.
    fn queue_wanted_notices(&mut self) {
        if self.day_number != 1 {
            return;
        }
        let holders = self.mafia_tier_ability_holders(TierAbility::Wanted);
        if holders.is_empty() {
            return;
        }
        let uncontacted = self
            .players
            .iter()
            .filter(|player| {
                player.alive && self.is_mafia_team(player) && !self.is_known_mafia_team(player)
            })
            .map(|player| player.name.clone())
            .collect::<Vec<_>>();
        let line = if uncontacted.is_empty() {
            "[수배] 접선하지 않은 마피아팀이 없습니다.".to_string()
        } else {
            format!("[수배] 접선하지 않은 마피아팀: {}", uncontacted.join(", "))
        };
        for holder_id in holders {
            self.pending_tier_ability_notices
                .push((holder_id, line.clone()));
        }
    }

    /// [지령] 첫 번째 낮이 될 때: 마피아·청부업자 보유자는 경찰 계열 생존자
    /// 한 명이 누구인지, 그 외 보조·교주 보유자는 정체가 밝혀지지 않은 시민팀
    /// 한 명의 직업을 안다. 도굴꾼이 퍼블 경찰 계열을 도굴했으면 도굴꾼의
    /// 역할이 이미 경찰 계열로 바뀐 뒤라(resolve_graverobbers) 도굴꾼으로 뜬다.
    fn queue_directive_notices(&mut self) {
        if self.day_number != 1 {
            return;
        }
        let holders = self
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && !self.is_frog(player)
                    && self.tier_abilities.get(&player.user_id) == Some(&TierAbility::Directive)
            })
            .cloned()
            .collect::<Vec<_>>();
        if holders.is_empty() {
            return;
        }
        let police_line = self
            .players
            .iter()
            .filter(|player| player.alive && player.role.is_investigation_role())
            .cloned()
            .collect::<Vec<_>>();
        let hidden_citizens = self
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && self.is_citizen_team(player)
                    && !self.publicly_revealed_ids.contains(&player.user_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut rng = system_random::rng();
        for holder in holders {
            let line = if holder.role == Role::Mafia || holder.role == Role::Contractor {
                match police_line.choose(&mut rng) {
                    None => "[지령] 경찰 계열 생존자가 없습니다.".to_string(),
                    Some(target) => {
                        format!("[지령] {}님은 경찰 계열 직업입니다.", target.name)
                    }
                }
            } else {
                match hidden_citizens.choose(&mut rng) {
                    None => "[지령] 정체가 밝혀지지 않은 시민팀 생존자가 없습니다.".to_string(),
                    Some(target) => format!(
                        "[지령] {}님의 직업은 {}입니다.",
                        target.name,
                        self.visible_role(target).value()
                    ),
                }
            };
            self.pending_tier_ability_notices
                .push((holder.user_id, line));
        }
    }

    /// 티어 능력(무법·야습·수습) 알림 대기열을 결산 맵으로 꺼낸다.
    fn drain_tier_ability_notices(&mut self) -> HashMap<u64, String> {
        let mut results: HashMap<u64, String> = HashMap::new();
        for (user_id, line) in std::mem::take(&mut self.pending_tier_ability_notices) {
            if !self.is_alive(user_id) {
                continue;
            }
            results
                .entry(user_id)
                .and_modify(|text| {
                    text.push('\n');
                    text.push_str(&line);
                })
                .or_insert(line);
        }
        results
    }

    /// [불침번] 이번 밤 군인이 막아낸 능력 알림을 결산 맵으로 꺼낸다. 죽은 군인은
    /// 받지 못한다.
    fn drain_soldier_watch_notices(&mut self) -> HashMap<u64, String> {
        let mut results: HashMap<u64, String> = HashMap::new();
        for (soldier_id, line) in std::mem::take(&mut self.pending_soldier_watch_notices) {
            if !self.is_alive(soldier_id) {
                continue;
            }
            results
                .entry(soldier_id)
                .and_modify(|text| {
                    text.push('\n');
                    text.push_str(&line);
                })
                .or_insert(line);
        }
        results
    }

    /// 사기꾼 결산: 교섭 접선 안내와 "속임" 판정 알림.
    /// 조사 계열이 변장 사기꾼을 평가하면 사기꾼에게 "[000님을 속였습니다.]"가 간다.
    fn resolve_fraudster_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
        role_reveals: &[(u8, u64, u64, Role)],
    ) -> (HashMap<u64, String>, Vec<u64>) {
        let mut results: HashMap<u64, String> = HashMap::new();
        let mut contacts = Vec::new();
        let push_line = |map: &mut HashMap<u64, String>, id: u64, line: String| {
            map.entry(id)
                .and_modify(|text| {
                    text.push('\n');
                    text.push_str(&line);
                })
                .or_insert(line);
        };

        for (fraudster_id, self_targeted) in std::mem::take(&mut self.fraudster_contacts_this_night)
        {
            if !self.is_alive(fraudster_id) || blocked_actor_ids.contains(&fraudster_id) {
                continue;
            }
            let line = if self_targeted {
                "[교섭] 마피아팀의 처형 대상이 되었지만 사기꾼은 처형되지 않습니다. 마피아팀과 접선했습니다."
            } else {
                "[교섭] 사기 대상이 마피아팀의 처형 대상이 되어 마피아팀과 접선했습니다."
            };
            push_line(&mut results, fraudster_id, line.to_string());
            contacts.push(fraudster_id);
            self.record_rating_event(fraudster_id, 2, "교섭으로 마피아팀 접선");
        }

        // 속임 판정 수집: 사기꾼별로 이번 밤 자신을 평가한 조사자 목록(중복 제거).
        let mut deceived: HashMap<u64, Vec<u64>> = HashMap::new();
        let note = |map: &mut HashMap<u64, Vec<u64>>, fraudster_id: u64, actor_id: u64| {
            let actors = map.entry(fraudster_id).or_default();
            if !actors.contains(&actor_id) {
                actors.push(actor_id);
            }
        };
        // 직업을 그대로 알아낸 조사 (조회·수사·지령·성불)
        for (_, actor_id, target_id, _) in role_reveals {
            let Some(target) = self.get_player(*target_id) else {
                continue;
            };
            if self.is_disguised_fraudster(target) && actor_id != target_id {
                note(&mut deceived, *target_id, *actor_id);
            }
        }
        // 경찰 조사(도둑의 경찰 조사 포함): 마피아 여부 판정을 속인다.
        // 이미 접선한 사기꾼은 표준 규칙대로 마피아로 판정되므로 속임이 아니다.
        for (actor_id, target_id) in self.police_targets.iter().chain(&self.thief_police_targets) {
            if blocked_actor_ids.contains(actor_id) || !self.is_alive(*actor_id) {
                continue;
            }
            let Some(target) = self.get_player(*target_id) else {
                continue;
            };
            if self.is_disguised_fraudster(target)
                && !self.fraudster_contacted.contains(&target.user_id)
            {
                note(&mut deceived, *target_id, *actor_id);
            }
        }
        // 스파이 첩보
        for (actor_id, target_ids) in &self.spy_targets {
            if blocked_actor_ids.contains(actor_id) || !self.is_alive(*actor_id) {
                continue;
            }
            for target_id in target_ids {
                let Some(target) = self.get_player(*target_id) else {
                    continue;
                };
                if self.is_disguised_fraudster(target) {
                    note(&mut deceived, *target_id, *actor_id);
                }
            }
        }
        let mut rating_events = Vec::new();
        for (fraudster_id, actor_ids) in deceived {
            if blocked_actor_ids.contains(&fraudster_id) {
                continue;
            }
            for actor_id in actor_ids {
                let Some(actor_name) = self.get_player(actor_id).map(|actor| actor.name.clone())
                else {
                    continue;
                };
                push_line(
                    &mut results,
                    fraudster_id,
                    format!("[{actor_name}님을 속였습니다.]"),
                );
                rating_events.push(fraudster_id);
            }
        }
        for fraudster_id in rating_events {
            self.record_rating_event(fraudster_id, 2, "변장으로 조사 속임");
        }
        (results, contacts)
    }

    fn resolve_inspector_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
        role_reveals: &mut Vec<(u8, u64, u64, Role)>,
    ) -> (HashMap<u64, String>, HashMap<u64, String>) {
        let mut results = HashMap::new();
        let mut target_notices = HashMap::new();
        // 수사를 실제로 수행한 형사. 대상이 다른 팀이라 결과가 없어도 1회용을 소모한다.
        let mut used_actor_ids = Vec::new();
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
            if !actor.alive {
                continue;
            }
            used_actor_ids.push(*actor_id);
            // 수사 대상이 이 밤에 죽어도 수사 자체는 이미 끝났으므로 결과는 전달한다.
            // 다만 이미 죽은 대상에게는 형사의 정체를 알리지 않는다.
            if self.inspector_team_key(actor) == self.inspector_team_key(target) {
                if target.alive {
                    target_notices.insert(
                        target.user_id,
                        format!("[형사 {}님이 당신을 수사했습니다.]", actor.name),
                    );
                }
                results.insert(
                    *actor_id,
                    format!(
                        "[{}님의 직업은 {}입니다.]",
                        target.name,
                        self.visible_role(target).value()
                    ),
                );
                role_reveals.push((1, *actor_id, *target_id, self.visible_role(target)));
            }
        }
        self.inspector_used_ids.extend(used_actor_ids);
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
        killed_players: &mut Vec<Player>,
    ) -> Vec<(Player, Player)> {
        let mut retaliations = Vec::new();
        let dead_terrorists = killed_players.clone();
        for terrorist in dead_terrorists {
            let Some(target) = self.terrorist_retaliation_target(&terrorist) else {
                continue;
            };
            if let Some(killed) = self.mark_dead(target.user_id) {
                killed_players.push(killed.clone());
                retaliations.push((terrorist, killed));
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
                    if target.role == Role::Soldier {
                        lines.push(format!(
                            "[첩보] {} 님은 불침번을 서고 있어 정보를 알아내지 못했습니다.",
                            target.name
                        ));
                    } else {
                        lines.push(format!(
                            "[첩보] {} 님의 직업은 **{}** 입니다.",
                            target.name,
                            self.visible_role(target).value()
                        ));
                    }
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
            // [불침번] 청부 대상에 군인이 있으면 청부 전체가 무효가 되고, 군인이
            // 청부업자의 정체를 안다. 접선 판정도 일어나지 않는다.
            let watching_soldiers = targets
                .iter()
                .filter_map(|(target, _)| {
                    target
                        .as_ref()
                        .filter(|target| target.alive && target.role == Role::Soldier)
                        .map(|target| target.user_id)
                })
                .collect::<Vec<_>>();
            if !watching_soldiers.is_empty() {
                for soldier_id in watching_soldiers {
                    self.pending_soldier_watch_notices.push((
                        soldier_id,
                        format!("[불침번] 청부업자 {}님의 청부를 막아냈습니다.", actor.name),
                    ));
                }
                results.insert(
                    actor_id,
                    "[청부] 대상이 불침번을 서고 있어 청부가 무산되었습니다.".to_string(),
                );
                continue;
            }
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
        role_reveals: &mut Vec<(u8, u64, u64, Role)>,
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
            role_reveals.push((3, actor_id, target.user_id, self.visible_role(&target)));
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
        role_reveals: &mut Vec<(u8, u64, u64, Role)>,
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
                // 특종도 "직업을 명확하게 알아낸" 경우라 이슈 트리거다. 본인 특종은
                // 파파라치 결산의 actor != target 가드가 걸러낸다. 공개 정보이므로
                // 우선순위는 비공개 조사들(0~3) 뒤로 둔다.
                role_reveals.push((4, actor_id, target.user_id, visible_role));
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
                results.insert(
                    mercenary_id,
                    "[의뢰] 의뢰인이 사망했습니다. 이제 밤마다 플레이어 한 명을 처형할 수 있습니다."
                        .to_string(),
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

    fn resolve_agent_results(
        &mut self,
        blocked_actor_ids: &HashSet<u64>,
        role_reveals: &mut Vec<(u8, u64, u64, Role)>,
    ) -> HashMap<u64, String> {
        let surviving_players = self
            .players
            .iter()
            .filter(|player| player.alive && !blocked_actor_ids.contains(&player.user_id))
            .cloned()
            .collect::<Vec<_>>();
        let agents = self
            .players
            .iter()
            .filter(|player| player.alive || blocked_actor_ids.contains(&player.user_id))
            .filter(|player| {
                player.role == Role::Agent
                    || self.thief_stolen_roles.get(&player.user_id) == Some(&Role::Agent)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut results = HashMap::new();
        for agent in agents {
            let candidates = surviving_players
                .iter()
                .filter(|player| {
                    player.user_id != agent.user_id
                        && (self.is_citizen_team(player)
                            || (self.is_disguised_fraudster(player)
                                && !self.fraudster_contacted.contains(&player.user_id)))
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
            role_reveals.push((2, agent.user_id, target.user_id, self.visible_role(&target)));
            results.insert(
                agent.user_id,
                format!(
                    "[공작] 지령이 도착했습니다.\n{} 님의 직업은 **{}** 입니다.",
                    target.name,
                    self.visible_role(&target).value()
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
        // [교섭] 사기꾼 본인이 마피아팀 처형 대상이 되면 죽지 않고 접선한다.
        if target.role == Role::Fraudster {
            if !self.fraudster_contacted.contains(&target.user_id) {
                self.fraudster_contacts_this_night
                    .push((target.user_id, true));
                self.contact_mafia_team_member(&target);
            }
            return;
        }
        // 사기 대상이 처형 대상이 되면 사기꾼이 접선한다. 처형 성공 여부와 무관하므로
        // 보호 판정보다 먼저 처리하고, 공격 자체는 평소대로 진행한다.
        let watching_fraudster_ids = self
            .fraudster_disguises
            .iter()
            .filter(|(_, (disguise_target_id, _))| *disguise_target_id == target.user_id)
            .map(|(fraudster_id, _)| *fraudster_id)
            .collect::<Vec<_>>();
        for fraudster_id in watching_fraudster_ids {
            if self.fraudster_contacted.contains(&fraudster_id) {
                continue;
            }
            let Some(fraudster) = self.get_player(fraudster_id).cloned() else {
                continue;
            };
            if !fraudster.alive {
                continue;
            }
            self.fraudster_contacts_this_night
                .push((fraudster_id, false));
            self.contact_mafia_team_member(&fraudster);
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
        // [무법] 경찰을 노린 마피아팀 공격은 치료를 무시한다.
        // [야습] 첫날 밤에는 자기 자신에게 쓴 치료를 무시한다.
        // [저격] 전날 밤 처형이 실패했다면 이번 밤은 모든 보호를 무시한다.
        let lawless_pierce = target.role.is_investigation_role()
            && self.mafia_team_has_tier_ability(TierAbility::Lawless);
        let night_raid_pierce = self.day_number == 1
            && self.mafia_team_has_tier_ability(TierAbility::NightRaid)
            && self.protection_is_self_heal_only(target.user_id);
        let snipe_pierce = self.snipe_armed && self.mafia_team_has_tier_ability(TierAbility::Snipe);
        let pierce_protection = lawless_pierce || night_raid_pierce || snipe_pierce;
        if pierce_protection
            && (enhanced_protection_ids.contains(&target.user_id)
                || protected_ids.contains(&target.user_id))
        {
            let (ability, reason) = if lawless_pierce {
                (
                    TierAbility::Lawless,
                    "경찰 계열이라 보호를 무시하고 처형했습니다",
                )
            } else if night_raid_pierce {
                (
                    TierAbility::NightRaid,
                    "첫날 밤 자가 치료를 무시하고 처형했습니다",
                )
            } else {
                (TierAbility::Snipe, "모든 보호를 무시하고 처형했습니다")
            };
            for holder_id in self.mafia_tier_ability_holders(ability) {
                self.pending_tier_ability_notices.push((
                    holder_id,
                    format!("[{}] {}님의 {reason}.", ability.value(), target.name),
                ));
            }
        }
        if !pierce_protection {
            if enhanced_protection_ids.contains(&target.user_id) {
                self.mark_concealed_kill_failure(&target);
                return;
            }
            if !ignore_doctor && protected_ids.contains(&target.user_id) {
                self.mark_concealed_kill_failure(&target);
                return;
            }
        }
        if allow_soldier_block
            && !snipe_pierce
            && target.role == Role::Soldier
            && !self.soldier_bulletproof_used.contains(&target.user_id)
        {
            self.soldier_bulletproof_used.insert(target.user_id);
            // [은폐] 방탄은 소모되지만 공개 문구와 정체 공개가 사라진다.
            if self.mark_concealed_kill_failure(&target) {
                return;
            }
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

    /// [은폐] 처형 실패를 조용한 밤으로 가린다. 보유자가 있으면 true를 돌려주고
    /// 보유자들에게만 실패 사실을 알린다.
    fn mark_concealed_kill_failure(&mut self, target: &Player) -> bool {
        let holders = self.mafia_tier_ability_holders(TierAbility::Concealment);
        if holders.is_empty() {
            return false;
        }
        if !self.concealed_kill_failure {
            self.concealed_kill_failure = true;
        }
        let line = format!(
            "[은폐] {}님 처형 실패를 조용한 밤으로 가렸습니다.",
            target.name
        );
        for holder_id in holders {
            self.pending_tier_ability_notices
                .push((holder_id, line.clone()));
        }
        true
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
