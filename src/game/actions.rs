// game/actions.rs
// 역할: 플레이어의 낮·밤 행동 제출 처리 (해킹, 숙청, 심리학, 도벽, 밤 행동, 청부, 소생 등)

#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

use crate::model::{CONTRACTOR_GUESS_ROLES, Phase, Player, Role};
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};

use super::{MafiaGame, RoleActionMap};

impl MafiaGame {
    pub fn submit_hacker_action(&mut self, actor_id: u64, target_id: u64) -> Result<String> {
        if self.phase != Phase::Day {
            bail!("해킹은 낮에만 사용할 수 있습니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if actor.role != Role::Hacker {
            bail!("해커만 해킹을 사용할 수 있습니다.");
        }
        if self.is_madam_seduced(&actor) {
            bail!("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.");
        }
        if self.hacker_used_ids.contains(&actor_id) {
            bail!("해킹은 이미 사용했습니다.");
        }
        let target = self.require_alive(target_id)?.clone();
        if actor_id == target_id {
            bail!("해커는 자기 자신을 해킹할 수 없습니다.");
        }
        self.hacker_targets.insert(actor_id, target_id);
        self.hacker_pending_results.insert(actor_id, target_id);
        self.hacker_proxy_targets.insert(actor_id, target_id);
        self.hacker_used_ids.insert(actor_id);
        self.mark_rating_action(actor_id);
        self.record_rating_event(actor_id, 2, "해킹 실행");
        Ok(format!("해킹 대상: {}", target.name))
    }

    pub fn submit_vigilante_investigation(
        &mut self,
        actor_id: u64,
        target_id: u64,
    ) -> Result<String> {
        if self.phase != Phase::Day {
            bail!("자경단원 조사는 낮에만 사용할 수 있습니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if actor.role != Role::Vigilante {
            bail!("자경단원만 숙청 조사를 사용할 수 있습니다.");
        }
        if self.is_madam_seduced(&actor) {
            bail!("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.");
        }
        if self.vigilante_investigation_used_ids.contains(&actor_id) {
            bail!("자경단원 조사는 이미 사용했습니다.");
        }
        let target = self.require_alive(target_id)?.clone();
        if actor_id == target_id {
            bail!("자경단원은 자기 자신을 조사할 수 없습니다.");
        }
        self.vigilante_pending_results.insert(actor_id, target_id);
        self.vigilante_investigation_used_ids.insert(actor_id);
        self.mark_rating_action(actor_id);
        Ok(format!("숙청 조사 대상: {}", target.name))
    }

    pub fn consume_vigilante_results(&mut self) -> HashMap<u64, String> {
        let pending = std::mem::take(&mut self.vigilante_pending_results);
        let mut results = HashMap::new();
        for (actor_id, target_id) in pending {
            let Some(actor) = self.get_player(actor_id).cloned() else {
                continue;
            };
            let Some(target) = self.get_player(target_id).cloned() else {
                continue;
            };
            if !actor.alive {
                continue;
            }
            let result_text = if self.is_known_mafia_team(&target) {
                self.vigilante_known_enemy_ids
                    .entry(actor_id)
                    .or_default()
                    .insert(target_id);
                self.record_rating_event(actor_id, 3, "숙청 조사로 마피아팀 확인");
                "마피아팀입니다"
            } else {
                "마피아팀이 아닙니다"
            };
            results.insert(
                actor_id,
                format!("[숙청] {} 님은 **{}**.", target.name, result_text),
            );
        }
        results
    }

    pub fn submit_psychologist_observation(
        &mut self,
        actor_id: u64,
        first_target_id: u64,
        second_target_id: u64,
    ) -> Result<String> {
        if self.phase != Phase::Day {
            bail!("심리학자 관찰은 낮에만 사용할 수 있습니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if actor.role != Role::Psychologist {
            bail!("심리학자만 관찰을 사용할 수 있습니다.");
        }
        if self.is_madam_seduced(&actor) {
            bail!("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.");
        }
        if self.psychologist_used_days.get(&actor_id) == Some(&self.day_number) {
            bail!("오늘은 이미 관찰을 사용했습니다.");
        }
        if first_target_id == second_target_id {
            bail!("서로 다른 두 명을 선택해야 합니다.");
        }
        if actor_id == first_target_id || actor_id == second_target_id {
            bail!("심리학자는 자기 자신을 관찰 대상으로 고를 수 없습니다.");
        }
        let first = self.require_alive(first_target_id)?.clone();
        let second = self.require_alive(second_target_id)?.clone();
        self.psychologist_used_days
            .insert(actor_id, self.day_number);
        self.mark_rating_action(actor_id);
        self.record_rating_event(actor_id, 2, "심리학자 관찰 실행");
        let relation = if self.team_key(&first) == self.team_key(&second) {
            "같은 팀입니다"
        } else {
            "다른 팀입니다"
        };
        Ok(format!(
            "[관찰] {} 님과 {} 님은 **{}**.",
            first.name, second.name, relation
        ))
    }

    pub fn submit_thief_steal(&mut self, actor_id: u64, target_id: u64) -> Result<String> {
        if self.phase != Phase::Vote {
            bail!("도둑의 도벽은 투표 시간에만 사용할 수 있습니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if actor.role != Role::Thief {
            bail!("도둑만 도벽을 사용할 수 있습니다.");
        }
        if self.is_frog(&actor) {
            bail!("개구리 상태에서는 능력을 사용할 수 없습니다.");
        }
        if self.thief_used_days.get(&actor_id) == Some(&self.day_number) {
            bail!("오늘은 이미 도벽을 사용했습니다.");
        }
        let target = self.require_alive(target_id)?.clone();
        if actor_id == target_id {
            bail!("도둑은 자기 자신을 훔칠 수 없습니다.");
        }
        self.thief_used_days.insert(actor_id, self.day_number);
        self.thief_stolen_roles.insert(actor_id, target.role);
        self.mark_rating_action(actor_id);
        self.record_rating_event(actor_id, 3, "도벽 실행");
        let contacted_now = self.is_mafia_team(&target) && self.thief_contacted.insert(actor_id);
        let mut lines = vec![format!(
            "[도벽] {} 님의 직업 능력을 훔쳤습니다.",
            target.name
        )];
        if self.thief_night_role(&actor).is_some() {
            lines.push(format!(
                "다음 밤까지 **{}** 능력을 사용할 수 있습니다.",
                target.role.value()
            ));
        } else if target.role == Role::Agent {
            lines.push("다음 밤 결과 때 요원 지령을 받습니다.".to_string());
        } else {
            lines.push(format!(
                "**{}**은/는 도둑이 다음 밤에 사용할 수 있는 선택형 밤 능력이 없습니다.",
                target.role.value()
            ));
        }
        if contacted_now {
            self.record_rating_event(actor_id, 2, "도벽으로 마피아팀 접선");
            lines.push("[교련] 마피아 직업을 훔쳐 마피아팀과 접선했습니다.".to_string());
        }
        Ok(lines.join("\n"))
    }

    pub fn submit_night_action(&mut self, actor_id: u64, target_id: Option<u64>) -> Result<String> {
        if self.phase != Phase::Night {
            bail!("지금은 밤이 아닙니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if self.is_frog(&actor) {
            bail!("개구리 상태에서는 밤 행동을 사용할 수 없습니다.");
        }
        if self.is_madam_seduced(&actor) && !self.is_mafia_team(&actor) {
            bail!("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.");
        }
        if actor.role == Role::Thief {
            let result = self.submit_stolen_night_action(&actor, target_id);
            if result.is_ok() {
                self.mark_rating_action(actor_id);
            }
            return result;
        }

        let result = match actor.role {
            Role::Mafia => self.submit_target_action(
                actor_id,
                target_id,
                "공격 대상을 선택해야 합니다.",
                None,
                false,
                |game, actor_id, selected, target_id| {
                    let previous = game.mafia_display_targets.get(&actor_id).copied();
                    let proxy = game.proxy_target_id(target_id);
                    game.mafia_targets.insert(actor_id, proxy);
                    game.mafia_display_targets.insert(actor_id, target_id);
                    Ok(if previous == Some(target_id) {
                        format!("공격 대상 유지: {}", selected.name)
                    } else if previous.is_some() {
                        format!("공격 대상 변경: {}", selected.name)
                    } else {
                        format!("공격 대상: {}", selected.name)
                    })
                },
            ),
            Role::Doctor => self.once_target_action(
                actor_id,
                target_id,
                "보호 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Doctor,
                None,
                "보호 대상",
            ),
            Role::Nurse => self.submit_nurse_action(actor_id, target_id),
            Role::Gangster => self.once_target_action(
                actor_id,
                target_id,
                "공갈 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Gangster,
                Some("건달은 자기 자신을 공갈할 수 없습니다."),
                "공갈 대상",
            ),
            Role::Police => self.once_target_action(
                actor_id,
                target_id,
                "조사 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Police,
                Some("경찰은 자기 자신을 조사할 수 없습니다."),
                "조사 투표 대상",
            ),
            Role::Vigilante => self.submit_vigilante_night_action(actor_id, target_id),
            Role::Hypnotist => self.submit_hypnotist_action(actor_id, target_id),
            Role::Mercenary => self.submit_mercenary_action(actor_id, target_id),
            Role::Reporter => self.submit_reporter_action(actor_id, target_id, ""),
            Role::Detective => self.once_target_action(
                actor_id,
                target_id,
                "추적 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Detective,
                Some("사립탐정은 자기 자신을 추적할 수 없습니다."),
                "추적 대상",
            ),
            Role::Shaman => self.submit_dead_target_action(
                actor_id,
                target_id,
                "성불 대상을 선택해야 합니다.",
                RoleActionMap::Shaman,
                "영매는 사망한 참가자만 성불할 수 있습니다.",
                "이미 성불한 사망자입니다.",
                "성불 대상",
            ),
            Role::Priest => self.submit_priest_action(actor_id, target_id, ""),
            Role::Spy => self.submit_spy_action(actor_id, target_id, ""),
            Role::Terrorist => self.once_target_action(
                actor_id,
                target_id,
                "지목 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Terrorist,
                Some("테러리스트는 자기 자신을 지목할 수 없습니다."),
                "지목 대상",
            ),
            Role::Witch => self.once_target_action(
                actor_id,
                target_id,
                "저주 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Witch,
                Some("마녀는 자기 자신을 저주할 수 없습니다."),
                "저주 대상",
            ),
            Role::Godfather => self.submit_godfather_action(actor_id, target_id, ""),
            Role::CultLeader => self.submit_cult_action(actor_id, target_id),
            Role::Fanatic => self.submit_fanatic_action(actor_id, target_id),
            _ => bail!("{}은/는 밤 행동이 없습니다.", actor.role.value()),
        };
        if result.is_ok() {
            self.mark_rating_action(actor_id);
        }
        result
    }

    fn submit_target_action<F>(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        missing: &str,
        self_error: Option<&str>,
        _once: bool,
        apply: F,
    ) -> Result<String>
    where
        F: FnOnce(&mut Self, u64, Player, u64) -> Result<String>,
    {
        let Some(target_id) = target_id else {
            bail!("{}", missing);
        };
        let selected = self.require_alive(target_id)?.clone();
        if actor_id == target_id {
            if let Some(message) = self_error {
                bail!("{}", message);
            }
        }
        apply(self, actor_id, selected, target_id)
    }

    fn once_target_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        missing: &str,
        _duplicate: &str,
        map: RoleActionMap,
        self_error: Option<&str>,
        label: &str,
    ) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("{}", missing);
        };
        if actor_id == target_id {
            if let Some(message) = self_error {
                bail!("{}", message);
            }
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        self.action_insert(map, actor_id, proxy);
        if matches!(map, RoleActionMap::Terrorist) {
            self.terrorist_action_submitted.insert(actor_id);
        }
        Ok(format!("{}: {}", label, selected.name))
    }

    fn submit_nurse_action(&mut self, actor_id: u64, target_id: Option<u64>) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("대상을 선택해야 합니다.");
        };
        let selected = self.require_alive(target_id)?.clone();
        if self.nurse_contacted.contains(&actor_id) {
            if self.alive_role_count(Role::Doctor) > 0 {
                bail!("의사가 살아있는 동안 간호사는 치료 대상을 직접 선택하지 않습니다.");
            }
            let proxy = self.proxy_target_id(target_id);
            self.nurse_prescription_targets.remove(&actor_id);
            self.nurse_targets.insert(actor_id, proxy);
            return Ok(format!("치료 대상: {}", selected.name));
        }
        if actor_id == target_id {
            bail!("간호사는 자기 자신을 처방할 수 없습니다.");
        }
        let proxy = self.proxy_target_id(target_id);
        self.nurse_targets.remove(&actor_id);
        self.nurse_prescription_targets.insert(actor_id, proxy);
        Ok(format!("처방 대상: {}", selected.name))
    }

    fn submit_vigilante_night_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
    ) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("숙청 대상을 선택해야 합니다.");
        };
        if self.vigilante_execution_used_ids.contains(&actor_id) {
            bail!("숙청 처형은 이미 사용했습니다.");
        }
        if actor_id == target_id {
            bail!("자경단원은 자기 자신을 숙청할 수 없습니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        let selected = self.require_alive(target_id)?.clone();
        let available = self
            .vigilante_execution_targets(&actor)
            .into_iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if !available.contains(&target_id) {
            bail!("자경단원은 살아있는 다른 플레이어만 숙청할 수 있습니다.");
        }
        let proxy = self.proxy_target_id(target_id);
        self.vigilante_targets.insert(actor_id, proxy);
        Ok(format!("숙청 대상: {}", selected.name))
    }

    fn submit_mercenary_action(&mut self, actor_id: u64, target_id: Option<u64>) -> Result<String> {
        if !self.mercenary_armed_ids.contains(&actor_id) {
            bail!("용병은 아직 처형 의뢰를 수행할 수 없습니다.");
        }
        self.once_target_action(
            actor_id,
            target_id,
            "처형 대상을 선택해야 합니다.",
            "",
            RoleActionMap::Mercenary,
            Some("용병은 자기 자신을 처형할 수 없습니다."),
            "처형 대상",
        )
    }

    fn submit_hypnotist_action(&mut self, actor_id: u64, target_id: Option<u64>) -> Result<String> {
        if self.hypnotist_skip_night_days.get(&actor_id) == Some(&self.day_number) {
            bail!("최면을 해제한 다음 밤에는 최면을 걸 수 없습니다.");
        }
        let Some(target_id) = target_id else {
            bail!("최면 대상을 선택해야 합니다.");
        };
        if actor_id == target_id {
            bail!("최면술사는 자기 자신에게 최면을 걸 수 없습니다.");
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        self.hypnotist_targets.insert(actor_id, proxy);
        Ok(format!("[{}님에게 최면을 겁니다.]", selected.name))
    }

    pub fn submit_hypnotist_wake(&mut self, actor_id: u64) -> Result<String> {
        if self.phase != Phase::Day {
            bail!("최면 해제는 낮에만 사용할 수 있습니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if actor.role != Role::Hypnotist {
            bail!("최면술사만 최면을 해제할 수 있습니다.");
        }
        if self.is_frog(&actor) {
            bail!("개구리 상태에서는 능력을 사용할 수 없습니다.");
        }
        if self.is_madam_seduced(&actor) {
            bail!("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.");
        }
        let Some(target_ids) = self.hypnotized_targets.remove(&actor_id) else {
            bail!("해제할 최면 대상이 없습니다.");
        };
        if target_ids.is_empty() {
            bail!("해제할 최면 대상이 없습니다.");
        }
        self.hypnotist_skip_night_days
            .insert(actor_id, self.day_number + 1);
        self.mark_rating_action(actor_id);

        let mut targets = target_ids
            .into_iter()
            .filter_map(|target_id| self.get_player(target_id).cloned())
            .collect::<Vec<_>>();
        targets.sort_by_key(|player| player.name.to_lowercase());
        let non_citizen_count = targets
            .iter()
            .filter(|target| !self.is_citizen_team(target))
            .count();
        if non_citizen_count > 0 {
            self.record_rating_event(
                actor_id,
                (non_citizen_count as i64 * 3).min(9),
                "최면 해제로 비시민 직업 확인",
            );
        } else {
            self.record_rating_event(actor_id, 1, "최면 해제 실행");
        }
        Ok(targets
            .into_iter()
            .map(|target| {
                format!(
                    "[{}님에게 걸린 최면을 해제합니다. {}님 : {}]",
                    target.name,
                    target.name,
                    self.hypnotist_reveal_text(&target)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn submit_reporter_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        prefix: &str,
    ) -> Result<String> {
        if self.day_number < 2 {
            bail!("엠바고로 첫 번째 낮에는 기사를 낼 수 없습니다.");
        }
        if self.reporter_used_ids.contains(&actor_id) {
            bail!("기자는 특종을 이미 사용했습니다.");
        }
        let Some(target_id) = target_id else {
            self.reporter_targets.remove(&actor_id);
            self.reporter_skip_submitted.insert(actor_id);
            return Ok(format!("{prefix}이번 밤에는 특종을 사용하지 않습니다."));
        };
        if actor_id == target_id {
            bail!("기자는 자기 자신을 취재할 수 없습니다.");
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        self.reporter_skip_submitted.remove(&actor_id);
        self.reporter_targets.insert(actor_id, proxy);
        Ok(format!("{prefix}특종 대상: {}", selected.name))
    }

    fn submit_dead_target_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        missing: &str,
        map: RoleActionMap,
        alive_error: &str,
        purified_error: &str,
        label: &str,
    ) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("{}", missing);
        };
        let target = self.require_player(target_id)?.clone();
        if target.alive {
            bail!("{}", alive_error);
        }
        if self.purified_dead_ids.contains(&target.user_id) {
            bail!("{}", purified_error);
        }
        self.action_insert(map, actor_id, target_id);
        Ok(format!("{}: {}", label, target.name))
    }

    fn submit_priest_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        prefix: &str,
    ) -> Result<String> {
        if self.priest_used_ids.contains(&actor_id) {
            bail!("소생은 이미 사용했습니다.");
        }
        let result = self.submit_dead_target_action(
            actor_id,
            target_id,
            "소생 대상을 선택해야 합니다.",
            RoleActionMap::Priest,
            "성직자는 사망한 참가자만 소생시킬 수 있습니다.",
            "성불 상태인 사망자는 소생시킬 수 없습니다.",
            "소생 대상",
        )?;
        Ok(format!("{prefix}{result}"))
    }

    fn submit_spy_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        prefix: &str,
    ) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("첩보 대상을 선택해야 합니다.");
        };
        if self.spy_actions_used(actor_id) >= self.spy_action_limit(actor_id) {
            bail!("이미 이번 밤 행동을 선택했습니다.");
        }
        if actor_id == target_id {
            bail!("스파이는 자기 자신을 지목할 수 없습니다.");
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        let target = self.require_player(proxy)?.clone();
        self.spy_targets.entry(actor_id).or_default().push(proxy);
        let mut lines = vec![format!(
            "{prefix}[첩보] {} 님의 직업은 **{}** 입니다.",
            target.name,
            self.visible_role(&target).value()
        )];
        if target.role == Role::Mafia && !self.spy_contacted.contains(&actor_id) {
            self.spy_contacted.insert(actor_id);
            self.spy_bonus_pending.insert(actor_id);
            self.spy_contacts_this_night.push(actor_id);
            lines.push(
                "[접선] 마피아와 접선했습니다. 이번 밤에 한 번 더 첩보를 사용할 수 있습니다."
                    .to_string(),
            );
        }
        if self.spy_bonus_pending.contains(&actor_id) && self.spy_actions_used(actor_id) >= 2 {
            self.spy_bonus_pending.remove(&actor_id);
        }
        if prefix.is_empty() {
            Ok(lines.join("\n"))
        } else {
            Ok(format!("{prefix}첩보 대상: {}", selected.name))
        }
    }

    fn submit_godfather_action(
        &mut self,
        actor_id: u64,
        target_id: Option<u64>,
        prefix: &str,
    ) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("확정 처치 대상을 선택해야 합니다.");
        };
        self.ensure_godfather_auto_contact();
        let stolen = self.is_stolen_godfather_actor(actor_id);
        if !stolen && !self.godfather_contacted.contains(&actor_id) {
            bail!("대부는 세 번째 밤부터 마피아 팀과 자동 접선되어 행동할 수 있습니다.");
        }
        if actor_id == target_id {
            bail!("대부는 자기 자신을 지목할 수 없습니다.");
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        self.godfather_targets.insert(actor_id, proxy);
        Ok(format!("{prefix}확정 처치 대상: {}", selected.name))
    }

    fn submit_cult_action(&mut self, actor_id: u64, target_id: Option<u64>) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("포교 대상을 선택해야 합니다.");
        };
        if self.day_number % 2 != 1 {
            bail!("교주는 홀수날 밤에만 포교할 수 있습니다.");
        }
        if actor_id == target_id {
            bail!("교주는 자기 자신을 포교할 수 없습니다.");
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        let target = self.require_player(proxy)?.clone();
        if self.is_cult_team(&target) {
            bail!("이미 교주팀인 대상은 포교할 수 없습니다.");
        }
        self.cult_targets.insert(actor_id, proxy);
        Ok(format!("포교 대상: {}", selected.name))
    }

    fn submit_fanatic_action(&mut self, actor_id: u64, target_id: Option<u64>) -> Result<String> {
        let Some(target_id) = target_id else {
            bail!("추종 대상을 선택해야 합니다.");
        };
        if actor_id == target_id {
            bail!("광신도는 자기 자신을 추종할 수 없습니다.");
        }
        let selected = self.require_alive(target_id)?.clone();
        let proxy = self.proxy_target_id(target_id);
        self.fanatic_targets.insert(actor_id, proxy);
        Ok(format!("추종 대상: {}", selected.name))
    }

    fn submit_stolen_night_action(
        &mut self,
        actor: &Player,
        target_id: Option<u64>,
    ) -> Result<String> {
        let actor_id = actor.user_id;
        let Some(stolen_role) = self.thief_night_role(actor) else {
            bail!("오늘 밤 사용할 수 있는 도벽 능력이 없습니다.");
        };
        let prefix = format!("[도벽: {}] ", stolen_role.value());
        match stolen_role {
            Role::Mafia => {
                let Some(target_id) = target_id else {
                    bail!("공격 대상을 선택해야 합니다.");
                };
                let selected = self.require_alive(target_id)?.clone();
                let previous = self.mafia_display_targets.get(&actor_id).copied();
                let proxy = self.proxy_target_id(target_id);
                self.mafia_targets.insert(actor_id, proxy);
                self.mafia_display_targets.insert(actor_id, target_id);
                Ok(if previous.is_some() && previous != Some(target_id) {
                    format!("{prefix}공격 대상 변경: {}", selected.name)
                } else {
                    format!("{prefix}공격 대상: {}", selected.name)
                })
            }
            Role::Doctor => self.once_target_action(
                actor_id,
                target_id,
                "보호 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Doctor,
                None,
                &format!("{prefix}보호 대상"),
            ),
            Role::Nurse => self
                .submit_nurse_action(actor_id, target_id)
                .map(|message| format!("{prefix}{message}")),
            Role::Police => self.once_target_action(
                actor_id,
                target_id,
                "조사 대상을 선택해야 합니다.",
                "",
                RoleActionMap::ThiefPolice,
                Some("자기 자신은 조사할 수 없습니다."),
                &format!("{prefix}조사 대상"),
            ),
            Role::Vigilante => self
                .submit_vigilante_night_action(actor_id, target_id)
                .map(|message| format!("{prefix}{message}")),
            Role::Reporter => self.submit_reporter_action(actor_id, target_id, &prefix),
            Role::Detective => self.once_target_action(
                actor_id,
                target_id,
                "추적 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Detective,
                Some("자기 자신은 추적할 수 없습니다."),
                &format!("{prefix}추적 대상"),
            ),
            Role::Spy => self.submit_spy_action(actor_id, target_id, &prefix),
            Role::Contractor => bail!("청부는 전용 선택 메뉴로 사용해야 합니다."),
            Role::Shaman => self.submit_dead_target_action(
                actor_id,
                target_id,
                "성불 대상을 선택해야 합니다.",
                RoleActionMap::Shaman,
                "성불은 사망자에게만 사용할 수 있습니다.",
                "이미 성불된 사망자입니다.",
                &format!("{prefix}성불 대상"),
            ),
            Role::Priest => self.submit_priest_action(actor_id, target_id, &prefix),
            Role::Witch => self.once_target_action(
                actor_id,
                target_id,
                "저주 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Witch,
                Some("자기 자신은 저주할 수 없습니다."),
                &format!("{prefix}저주 대상"),
            ),
            Role::Godfather => self.submit_godfather_action(actor_id, target_id, &prefix),
            Role::Terrorist => self.once_target_action(
                actor_id,
                target_id,
                "지목 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Terrorist,
                Some("자기 자신은 지목할 수 없습니다."),
                &format!("{prefix}지목 대상"),
            ),
            Role::CultLeader => self
                .submit_cult_action(actor_id, target_id)
                .map(|message| format!("{prefix}{message}")),
            Role::Fanatic => self
                .submit_fanatic_action(actor_id, target_id)
                .map(|message| format!("{prefix}{message}")),
            Role::Gangster => self.once_target_action(
                actor_id,
                target_id,
                "공갈 대상을 선택해야 합니다.",
                "",
                RoleActionMap::Gangster,
                Some("자기 자신은 공갈할 수 없습니다."),
                &format!("{prefix}공갈 대상"),
            ),
            _ => bail!("훔친 직업은 이번 밤에 사용할 수 있는 능력이 없습니다."),
        }
    }

    pub fn submit_contractor_contract(
        &mut self,
        actor_id: u64,
        first_target_id: u64,
        first_role: Role,
        second_target_id: u64,
        second_role: Role,
    ) -> Result<String> {
        if self.phase != Phase::Night {
            bail!("지금은 밤이 아닙니다.");
        }
        let actor = self.require_alive(actor_id)?.clone();
        if actor.role != Role::Contractor
            && !(actor.role == Role::Thief
                && self.thief_stolen_roles.get(&actor_id) == Some(&Role::Contractor))
        {
            bail!("청부업자만 청부를 사용할 수 있습니다.");
        }
        if self.is_frog(&actor) {
            bail!("개구리 상태에서는 밤 행동을 사용할 수 없습니다.");
        }
        if self.day_number < 2 {
            bail!("청부는 두 번째 밤부터 사용할 수 있습니다.");
        }
        if first_target_id == second_target_id {
            bail!("청부 대상 두 명은 서로 달라야 합니다.");
        }
        if !CONTRACTOR_GUESS_ROLES.contains(&first_role)
            || !CONTRACTOR_GUESS_ROLES.contains(&second_role)
        {
            bail!("청부로 추측할 수 없는 직업입니다.");
        }
        if actor_id == first_target_id || actor_id == second_target_id {
            bail!("청부업자는 자기 자신을 지목할 수 없습니다.");
        }
        let first = self.require_alive(first_target_id)?.clone();
        let second = self.require_alive(second_target_id)?.clone();
        if first.role.is_investigation_role() || second.role.is_investigation_role() {
            bail!("경찰, 요원, 자경단원은 청부 대상으로 지목할 수 없습니다.");
        }
        if self.is_publicly_revealed(&first) || self.is_publicly_revealed(&second) {
            bail!("게임 채널에 직업이 공개된 사람은 청부 대상으로 지목할 수 없습니다.");
        }
        self.contractor_contracts.insert(
            actor_id,
            ((first.user_id, first_role), (second.user_id, second_role)),
        );
        self.mark_rating_action(actor_id);
        Ok(format!(
            "[청부] 암살 대상을 선택했습니다.\n- {}: {}\n- {}: {}",
            first.name,
            first_role.value(),
            second.name,
            second_role.value()
        ))
    }

    pub fn restore_frogs(&mut self) -> Vec<Player> {
        let ids = self.frog_user_ids.drain().collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| self.get_player(id).cloned())
            .collect()
    }

    pub fn revive_pending_scientists(&mut self) -> Vec<Player> {
        let ids = self
            .scientist_pending_revive_ids
            .drain()
            .collect::<Vec<_>>();
        let mut revived = Vec::new();
        for id in ids {
            if let Some(index) = self.players_by_id.get(&id).copied() {
                if !self.players[index].alive {
                    self.players[index].alive = true;
                    self.scientist_contacted.insert(id);
                    self.publicly_revealed_ids.insert(id);
                    revived.push(self.players[index].clone());
                }
            }
        }
        revived
    }

    pub fn has_pending_scientist_revive(&self) -> bool {
        self.scientist_pending_revive_ids
            .iter()
            .any(|id| self.get_player(*id).is_some_and(|player| !player.alive))
    }
}
