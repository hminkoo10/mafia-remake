// 역할: 마피아 게임 루프(game_loop), 밤/낮/투표 단계 진행(run_night, run_day, run_vote),
//        역할 배분, 야간 행동 DM, 경찰 결과 공지

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::{
    COMPLETED_REPLAY_LIMIT, CONFIRM_VOTE_SECONDS, Context, ContractorContractDraft,
    DAY_EXTENSION_VOTE_SECONDS, DISCUSSION_EXTENSION_SECONDS, Data, Error, PRIVATE_CHAT_ROLES,
    RunningGame,
};
use crate::channel::*;
use crate::commands::{draw_lb_text, fill_circle, fill_rect, image_color, truncate_for_board};
use crate::embed::*;
use ab_glyph::FontArc;
use anyhow::{Context as AnyhowContext, Result, bail};
use dashmap::{DashMap, mapref::entry::Entry};
use image::{ImageFormat, Rgb, RgbImage};
use mafia_remake::config;
use mafia_remake::game::{MafiaGame, majority_required};
use mafia_remake::model::{
    ConfirmVoteResult, ContractorGuessRoleGroup, NightResult, Phase, Player, Role, VoteResult,
    Winner, contractor_guessable_roles,
};
use mafia_remake::stats;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};

mod day_vote;
mod night;
mod results;
pub(crate) use self::day_vote::*;
pub(crate) use self::night::*;
pub(crate) use self::results::*;

pub async fn game_loop(
    ctx: serenity::Context,
    data: Data,
    running: Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let result = game_loop_inner(&ctx, &data, &running).await;
    if let Err(error) = &result {
        eprintln!("game loop failed; forcing cleanup: {error:?}");
    }
    cleanup_game(&ctx, &data, &running).await;
    let guild_id = running.read().await.guild_id;
    remove_current_entry(&data.games, guild_id, &running);
    result
}

async fn game_loop_inner(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    setup_game_channels(ctx, data, running).await?;
    {
        let running_read = running.read().await;
        let game = &running_read.game;
        send_channel_embed(
            &ctx.http,
            running_read.channel_id,
            public_game_settings_text(game, &config, "게임 방 설정입니다."),
            "방 설정",
            serenity::Colour::GOLD,
            vec![],
        )
        .await?;
        send_channel_embed(
            &ctx.http,
            running_read.channel_id,
            game_rule_text(game, &config, running_read.reveal_death_roles),
            "게임 설명",
            serenity::Colour::GOLD,
            vec![],
        )
        .await?;
    }
    send_roles(ctx, running, &config).await;
    upsert_game_status(ctx, running).await;
    loop {
        {
            let running_read = running.read().await;
            if running_read.game.phase == Phase::Ended {
                break;
            }
        }
        run_night(ctx, data, running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        if announce_winner(ctx, data, running).await? {
            break;
        }
        run_day(ctx, data, running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        run_vote(ctx, data, running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        if announce_winner(ctx, data, running).await? {
            break;
        }
    }
    Ok(())
}

fn remove_current_entry<K, T>(entries: &DashMap<K, Arc<T>>, key: K, current: &Arc<T>) -> bool
where
    K: Eq + Hash,
{
    match entries.entry(key) {
        Entry::Occupied(entry) if Arc::ptr_eq(entry.get(), current) => {
            entry.remove();
            true
        }
        Entry::Occupied(_) | Entry::Vacant(_) => false,
    }
}

pub async fn send_roles(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
) {
    let (guild_id, channel_id, payloads) = {
        let running_read = running.read().await;
        let payloads = running_read
            .game
            .players
            .iter()
            .map(|player| {
                let anonymous_notice = if running_read.anonymous_enabled {
                    let alias = running_read
                        .anonymous_aliases
                        .get(&player.user_id)
                        .cloned()
                        .unwrap_or_else(|| "익명".to_string());
                    format!(
                        "\n\n익명 이름: **{alias}**\n채팅은 서버에 생성된 본인 익명 입력 채널에서만 진행하세요."
                    )
                } else {
                    String::new()
                };
                (
                    player.clone(),
                    format!(
                        "{}\n\n방 설정\n{}\n\n게임 설명\n{}\n\n본인 역할 설명은 `/마피아능력`, 전체 역할 설명은 `/역할설명`으로 다시 확인할 수 있습니다.{}",
                        role_message(&running_read.game, player),
                        public_game_settings_text(
                            &running_read.game,
                            config,
                            "현재 게임 설정입니다."
                        ),
                        game_rule_text(
                            &running_read.game,
                            config,
                            running_read.reveal_death_roles
                        ),
                        anonymous_notice
                    ),
                )
            })
            .collect::<Vec<_>>();
        (running_read.guild_id, running_read.channel_id, payloads)
    };
    let mut failures = Vec::new();
    for (player, message) in payloads {
        if let Err(error) =
            send_player_secret_detailed(ctx, running, &player, message, vec![]).await
        {
            eprintln!(
                "secret delivery failed: stage=role_assignment guild_id={} channel_id={} user_id={} player={:?} {}",
                guild_id.get(),
                channel_id.get(),
                player.user_id,
                player.name,
                error.log_detail(),
            );
            failures.push(format!("{} ({})", player.name, error.public_reason()));
        }
    }
    if !failures.is_empty() {
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "비밀 메시지를 보낼 수 없는 참가자와 원인:\n{}\n\n서버 콘솔에는 Discord 원문 오류와 채널/사용자 ID를 기록했습니다.",
                failures.join("\n")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let _ = send_channel_embed(
        &ctx.http,
        channel_id,
        "역할 배정이 끝났습니다. 각자 비밀 메시지와 역할별 비공개 채널을 확인하세요.",
        "역할 배정 완료",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
}

pub fn role_message(game: &MafiaGame, player: &Player) -> String {
    let team = if game.is_cult_team(player) {
        "교주팀"
    } else if game.is_mafia_team(player) {
        "마피아팀"
    } else if player.role == Role::Joker {
        "중립"
    } else {
        "시민팀"
    };
    let mut message = format!(
        "당신의 역할은 **{}** 입니다.\n진영: **{}**\n\n{}",
        player.role.value(),
        team,
        role_short_guide(player.role)
    );
    // [사기] 사기꾼은 게임 시작 시 사기 대상과 변장 직업을 바로 알게 된다.
    if player.role == Role::Fraudster {
        if let Some((target, disguised_role)) = game.fraudster_disguise_info(player.user_id) {
            message.push_str(&format!(
                "\n\n[사기] {}님의 직업은 **{}**입니다.\n당신은 **{}**{} 변장했습니다. 조사 판정이 {}{} 표시됩니다.\n[교섭] 당신 또는 사기 대상이 마피아팀의 처형 대상이 되면 마피아팀과 접선하며, 당신은 마피아팀에게 처형되지 않습니다.",
                target.name,
                disguised_role.value(),
                disguised_role.value(),
                mafia_remake::model::korean_ro_particle(disguised_role.value()),
                disguised_role.value(),
                mafia_remake::model::korean_ro_particle(disguised_role.value()),
            ));
        } else if game
            .fraudster_blocked_by_soldier
            .contains_key(&player.user_id)
        {
            // [불침번]에 막힌 경우. 군인의 정체는 사기꾼에게 알려주지 않는다.
            message.push_str(
                "\n\n[사기] 사기 대상이 불침번을 서고 있어 변장에 실패했습니다. 이번 게임에는 변장 없이 사기꾼 그대로 판정됩니다.",
            );
        }
    }
    // 개인 티어 안내 (비공개).
    let tier = game.player_tiers.get(&player.user_id).copied().unwrap_or(2);
    let abilities = game.player_tier_abilities(player.user_id);
    if abilities.is_empty() {
        message.push_str(&format!(
            "\n\n당신의 티어: **{}티어** (티어 능력 없음)",
            tier
        ));
    } else {
        message.push_str(&format!("\n\n당신의 티어: **{}티어**", tier));
        for ability in &abilities {
            message.push_str(&format!(
                "\n티어 능력 [{}]: {}",
                ability.value(),
                ability.description()
            ));
        }
        // [직감] 청부업자에게 시민팀 직업 힌트를 함께 전달한다.
        if let Some(hint) = game.intuition_hints.get(&player.user_id) {
            message.push_str(&format!("\n{hint}"));
        }
    }
    // [불침번] 군인은 게임 시작 시 자신을 노린 사기를 막아낸 사실을 안다.
    if player.role == Role::Soldier {
        let blocked_fraudsters = game
            .fraudster_blocked_by_soldier
            .iter()
            .filter(|(_, soldier_id)| **soldier_id == player.user_id)
            .filter_map(|(fraudster_id, _)| game.get_player(*fraudster_id))
            .map(|fraudster| {
                format!(
                    "\n\n[불침번] 사기꾼 {}님의 사기를 막아냈습니다.",
                    fraudster.name
                )
            })
            .collect::<String>();
        message.push_str(&blocked_fraudsters);
    }
    message
}

fn mercenary_contract_received_message() -> &'static str {
    "누군가로부터 의뢰를 받았습니다."
}

pub fn role_short_guide(role: Role) -> &'static str {
    match role {
        Role::Mafia => "밤마다 제거할 대상을 선택합니다.",
        Role::Doctor => "밤마다 보호할 대상을 선택합니다.",
        Role::Police => {
            "밤마다 한 명을 조사해 즉시 결과를 확인합니다. 대상은 제출 후 바꿀 수 없습니다."
        }
        Role::Agent => "밤마다 시민팀 지령 정보를 받습니다.",
        Role::Vigilante => "낮에 조사(1회, 즉시 결과)하고 밤에 숙청할 수 있습니다.",
        Role::Inspector => {
            "게임 중 한 번만 수사할 수 있고, 결과는 제출 즉시 나옵니다. 같은 팀이면 직업을 확인하며 대상에게 자신의 정체를 알립니다. 다른 팀이면 \"시민팀이 아닙니다\"만 나오고 대상은 알지 못합니다."
        }
        Role::Detective => "밤 행동의 이동 경로를 추적합니다.",
        Role::CivilServant => {
            "밤마다 직업 하나를 조회해 그 직업을 가진 플레이어를 사망자까지 포함해 알아냅니다. 경찰 계열과 시민은 조회할 수 없습니다."
        }
        Role::Paparazzi => {
            "하루에 한 번, 시민팀이 처음으로 알아낸 다른 사람의 직업 정보를 함께 공유받습니다."
        }
        Role::Shaman => "사망자를 성불하고 직업을 확인합니다.",
        Role::Priest => "사망자를 한 번 소생시킬 수 있습니다.",
        Role::Reporter => "두 번째 밤부터 특종으로 직업을 공개합니다.",
        Role::Hacker => "낮에 해킹해 직업을 확인하고 능력을 우회합니다.",
        Role::Terrorist => "지목한 위험 대상을 함께 데려갈 수 있습니다.",
        Role::Lover => "연인과 정보를 공유하고 서로를 지킵니다.",
        Role::Soldier => {
            "마피아 공격을 한 번 버티고, 불침번으로 스파이의 첩보·도둑의 도벽·사기꾼의 사기·청부업자의 청부를 막아내며 그 정체를 알아냅니다."
        }
        Role::Spy => {
            "밤마다 한 명의 직업을 알아내고, 마피아를 찾아내면 그 밤 한 번 더 첩보를 사용합니다."
        }
        Role::Fraudster => {
            "시민 한 명의 직업으로 변장해 조사를 속이고, 변장 대상이나 자신이 마피아의 표적이 되면 접선합니다."
        }
        Role::Contractor => "두 명의 직업을 맞히면 암살합니다.",
        Role::Thief => {
            "마지막으로 지목 투표한 대상의 능력을 훔칩니다. 결과는 투표가 끝난 뒤 전달됩니다."
        }
        Role::Witch => "밤에 대상을 개구리로 저주합니다.",
        Role::Scientist => {
            "처음부터 마피아팀입니다. 첫 사망 전에는 미접선 보조처럼 시민 판정을 받고, 첫 사망 후 접선되어 다음 밤 부활합니다."
        }
        Role::Madam => "지목 투표로 선택한 대상을 유혹합니다.",
        Role::Godfather => "세 번째 밤부터 확정 처치합니다.",
        Role::CultLeader => "홀수날 밤마다 포교합니다.",
        Role::Fanatic => "교주팀 여부를 확인하고 교주를 찾습니다.",
        Role::Joker => "낮 처형으로 단독 승리합니다.",
        Role::Politician => "투표가 2표이며 처형 면역이 있습니다.",
        Role::Judge => "찬반투표 결과를 뒤집을 수 있습니다.",
        Role::Gangster => "밤에 한 명의 다음 낮 투표권을 빼앗습니다.",
        Role::Prophet => "4번째 낮까지 생존하면 소속팀이 승리합니다.",
        Role::Psychologist => "낮에 두 명이 같은 팀인지 봅니다.",
        Role::Hypnotist => "밤마다 최면을 누적하고 낮에 한꺼번에 깨워 비시민 직업을 확인합니다.",
        Role::Mercenary => "의뢰인이 밤에 사망한 뒤 밤마다 한 명을 처형할 수 있습니다.",
        Role::Graverobber => "첫날 사망자의 직업을 이어받습니다.",
        _ => "낮 토론과 투표로 승리를 노리세요.",
    }
}

pub fn death_role_text(running: &RunningGame, player: &Player) -> String {
    if running.reveal_death_roles {
        format!("직업은 **{}** 입니다.", player.role.value())
    } else {
        "직업은 공개되지 않습니다.".to_string()
    }
}

#[cfg(test)]
mod tests;
