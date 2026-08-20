// game/vote.rs
// 역할: 낮 투표 (지목·찬반), 최후변론, 처형 결산 처리

#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

use crate::model::{ConfirmVoteResult, Phase, Role, VoteResult};
use anyhow::{Result, bail};
use std::collections::HashMap;

use super::MafiaGame;

impl MafiaGame {
    pub fn start_vote(&mut self) -> Result<()> {
        if self.phase != Phase::Day {
            bail!("낮 단계에서만 투표를 시작할 수 있습니다.");
        }
        self.phase = Phase::Vote;
        self.day_votes.clear();
        self.confirm_votes.clear();
        Ok(())
    }

    pub fn submit_day_vote(&mut self, voter_id: u64, target_id: Option<u64>) -> Result<String> {
        if self.phase != Phase::Vote {
            bail!("지금은 투표 시간이 아닙니다.");
        }
        let voter = self.require_alive(voter_id)?.clone();
        if self.vote_blocked(voter.user_id) {
            self.day_votes.insert(voter.user_id, None);
            return Ok("공갈당해 이번 지목 투표권이 없습니다.".to_string());
        }
        let Some(target_id) = target_id else {
            self.day_votes.insert(voter.user_id, None);
            return Ok("투표 대상: 스킵".to_string());
        };
        let target = self.require_alive(target_id)?.clone();
        if voter.role == Role::Madam && voter.user_id == target.user_id {
            bail!("마담은 자기 자신에게 투표할 수 없습니다.");
        }
        self.day_votes.insert(voter.user_id, Some(target.user_id));
        let mut lines = vec![format!("투표 대상: {}", target.name)];
        // 도벽은 투표 종료 시 마지막 지목 대상에게 적용된다. 여기서 결과를 주면
        // 투표를 바꿔가며 여러 명의 직업을 연속으로 알아낼 수 있어 결과를 미룬다.
        if voter.role == Role::Thief
            && voter.user_id != target.user_id
            && !self.is_frog(&voter)
            && self.thief_used_days.get(&voter.user_id) != Some(&self.day_number)
        {
            lines.push(
                "[도벽] 투표가 끝나면 마지막으로 지목한 대상의 능력을 훔칩니다. 결과는 투표 종료 후에 전달됩니다."
                    .to_string(),
            );
        }
        Ok(lines.join("\n\n"))
    }

    /// 투표 종료 시 도둑들의 도벽을 최종 지목 대상으로 결산한다.
    fn resolve_thief_steals(
        &mut self,
        live_votes: &HashMap<u64, Option<u64>>,
    ) -> (HashMap<u64, String>, Vec<crate::model::Player>) {
        let mut results = HashMap::new();
        let mut newly_contacted = Vec::new();
        let thief_votes = live_votes
            .iter()
            .filter_map(|(voter_id, target_id)| Some((*voter_id, (*target_id)?)))
            .filter(|(voter_id, _)| {
                self.get_player(*voter_id)
                    .is_some_and(|voter| voter.role == Role::Thief)
            })
            .collect::<Vec<_>>();
        for (thief_id, target_id) in thief_votes {
            let Some((message, contacted_now, blocked_by_soldier)) =
                self.resolve_thief_steal(thief_id, target_id)
            else {
                continue;
            };
            results.insert(thief_id, message);
            if contacted_now {
                if let Some(thief) = self.get_player(thief_id).cloned() {
                    newly_contacted.push(thief);
                }
            }
            // [불침번] 군인은 자신을 노린 도벽을 막아내고 도둑의 정체를 안다.
            if let Some(soldier_id) = blocked_by_soldier {
                if let Some(thief_name) = self.get_player(thief_id).map(|thief| thief.name.clone())
                {
                    results.insert(
                        soldier_id,
                        format!("[불침번] 도둑 {thief_name}님의 도벽을 막아냈습니다."),
                    );
                }
            }
        }
        (results, newly_contacted)
    }

    pub fn resolve_nomination_vote(&mut self) -> Result<VoteResult> {
        if self.phase != Phase::Vote {
            bail!("투표 단계만 정산할 수 있습니다.");
        }
        let live_votes = self
            .day_votes
            .iter()
            .filter(|(voter_id, target_id)| {
                self.is_alive(**voter_id)
                    && !self.vote_blocked(**voter_id)
                    && target_id.is_none_or(|id| self.is_alive(id))
            })
            .map(|(voter_id, target_id)| (*voter_id, *target_id))
            .collect::<HashMap<_, _>>();
        let blocked_voters = self
            .players
            .iter()
            .filter(|player| player.alive && self.vote_blocked(player.user_id))
            .cloned()
            .collect::<Vec<_>>();
        let (madam_seduced, madam_newly_contacted) = self.apply_madam_seduction(&live_votes);
        let (thief_steal_results, thief_newly_contacted) = self.resolve_thief_steals(&live_votes);
        if live_votes.is_empty() {
            self.advance_to_next_night();
            return Ok(VoteResult {
                blocked_voters,
                madam_seduced,
                madam_newly_contacted,
                thief_steal_results,
                thief_newly_contacted,
                ..Default::default()
            });
        }
        let mut weighted_counts: HashMap<Option<u64>, i32> = HashMap::new();
        let mut display_counts: HashMap<Option<u64>, i32> = HashMap::new();
        for (voter_id, target_id) in &live_votes {
            *weighted_counts.entry(*target_id).or_default() += self.vote_weight(*voter_id);
            *display_counts.entry(*target_id).or_default() += 1;
        }
        let highest = weighted_counts.values().copied().max().unwrap_or(0);
        let top = weighted_counts
            .iter()
            .filter(|(_, count)| **count == highest)
            .map(|(target_id, _)| *target_id)
            .collect::<Vec<_>>();
        if top.len() != 1 {
            self.advance_to_next_night();
            return Ok(VoteResult {
                tied: true,
                weighted_vote_counts: weighted_counts,
                vote_counts: display_counts,
                madam_seduced,
                madam_newly_contacted,
                blocked_voters,
                thief_steal_results,
                thief_newly_contacted,
                ..Default::default()
            });
        }
        if top[0].is_none() {
            self.advance_to_next_night();
            return Ok(VoteResult {
                skipped: true,
                weighted_vote_counts: weighted_counts,
                vote_counts: display_counts,
                madam_seduced,
                madam_newly_contacted,
                blocked_voters,
                thief_steal_results,
                thief_newly_contacted,
                ..Default::default()
            });
        }
        let nominated = top[0].and_then(|id| self.get_player(id).cloned());
        self.phase = Phase::FinalDefense;
        Ok(VoteResult {
            executed: nominated,
            weighted_vote_counts: weighted_counts,
            vote_counts: display_counts,
            madam_seduced,
            madam_newly_contacted,
            blocked_voters,
            thief_steal_results,
            thief_newly_contacted,
            ..Default::default()
        })
    }

    pub fn resolve_vote(&mut self) -> Result<VoteResult> {
        self.resolve_nomination_vote()
    }

    pub fn start_confirmation_vote(&mut self) -> Result<()> {
        if self.phase != Phase::FinalDefense {
            bail!("최후변론 뒤에만 찬반투표를 시작할 수 있습니다.");
        }
        self.phase = Phase::ConfirmVote;
        self.confirm_votes.clear();
        Ok(())
    }

    pub fn submit_confirmation_vote(&mut self, voter_id: u64, approve: bool) -> Result<String> {
        if self.phase != Phase::ConfirmVote {
            bail!("지금은 찬반투표 시간이 아닙니다.");
        }
        let voter = self.require_alive(voter_id)?.clone();
        self.confirm_votes.insert(voter.user_id, approve);
        Ok(if approve {
            "찬성에 투표했습니다.".to_string()
        } else {
            "반대에 투표했습니다.".to_string()
        })
    }

    pub fn resolve_confirmation_vote(&mut self, target_id: u64) -> Result<ConfirmVoteResult> {
        if self.phase != Phase::ConfirmVote {
            bail!("찬반투표 단계만 정산할 수 있습니다.");
        }
        let live_votes = self
            .confirm_votes
            .iter()
            .filter(|(voter_id, _)| self.is_alive(**voter_id))
            .map(|(voter_id, approve)| (*voter_id, *approve))
            .collect::<HashMap<_, _>>();
        let mut vote_counts = HashMap::<bool, i32>::new();
        for approve in live_votes.values() {
            *vote_counts.entry(*approve).or_default() += 1;
        }
        let yes = *vote_counts.get(&true).unwrap_or(&0);
        let no = *vote_counts.get(&false).unwrap_or(&0);
        let target = self.get_player(target_id).cloned();
        let submitted_vote_count = yes + no;
        let required_yes = submitted_vote_count / 2 + 1;
        let normal_approved = target
            .as_ref()
            .is_some_and(|target| target.alive && submitted_vote_count > 0 && yes >= required_yes);
        let mut approved = normal_approved;
        let judge = self.active_judge();
        let judge_choice = judge
            .as_ref()
            .and_then(|judge| live_votes.get(&judge.user_id).copied());
        let mut decided_by_judge = false;
        if let Some(judge) = judge.as_ref() {
            if self.revealed_judge_ids.contains(&judge.user_id) {
                approved = target.as_ref().is_some_and(|target| target.alive)
                    && judge_choice.unwrap_or(false);
                decided_by_judge = true;
            } else if judge_choice.is_some_and(|choice| choice != normal_approved) {
                self.revealed_judge_ids.insert(judge.user_id);
                self.publicly_revealed_ids.insert(judge.user_id);
                approved = target.as_ref().is_some_and(|target| target.alive)
                    && judge_choice.unwrap_or(false);
                decided_by_judge = true;
            }
        }
        let tied = !decided_by_judge && !normal_approved && yes == no;
        let blocked_by_politician = approved
            && target
                .as_ref()
                .is_some_and(|target| target.role == Role::Politician);
        let mut executed = None;
        let mut extra_killed = Vec::new();
        if blocked_by_politician {
            if let Some(target) = target.as_ref() {
                self.publicly_revealed_ids.insert(target.user_id);
            }
        } else if approved {
            if let Some(target) = target.as_ref() {
                executed = self.mark_dead(target.user_id);
                if target.role == Role::Joker {
                    self.joker_won = true;
                    self.joker_winner_id = Some(target.user_id);
                }
                if let Some(retaliation_target) = self.terrorist_execution_target(target)
                    && let Some(killed) = self.mark_dead(retaliation_target.user_id)
                {
                    extra_killed.push(killed);
                }
            }
        }
        self.terrorist_execution_targets.clear();
        self.ensure_fanatic_reincarnation();
        self.advance_to_next_night();
        Ok(ConfirmVoteResult {
            executed,
            approved,
            tied,
            blocked_by_politician,
            extra_killed,
            weighted_vote_counts: vote_counts.clone(),
            vote_counts,
            judge: if decided_by_judge { judge } else { None },
            judge_choice: if decided_by_judge { judge_choice } else { None },
            decided_by_judge,
        })
    }
}
