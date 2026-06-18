// 역할: Discord 메시지 임베드, 버튼 컴포넌트, DM/채널 메시지 전송 헬퍼

#![allow(unused_imports)]

use super::{Context, Data, Error, RunningGame};
use anyhow::{Context as AnyhowContext, Result};
use mafia_remake::config;
use mafia_remake::model::{Player, Role};
use poise::serenity_prelude as serenity;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub fn make_embed(
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
) -> serenity::CreateEmbed {
    let message = message.into();
    let mut lines = message.lines();
    let description = if let Some(first) = lines.next() {
        let first = if first.contains("**") {
            first.to_string()
        } else {
            format!("**{first}**")
        };
        std::iter::once(first)
            .chain(lines.map(str::to_string))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        message
    };

    serenity::CreateEmbed::new()
        .title(format!("[마피아] {title}"))
        .description(description)
        .color(color)
        .author(serenity::CreateEmbedAuthor::new("마피아 게임 알림"))
        .footer(serenity::CreateEmbedFooter::new("마피아 게임 진행 메시지"))
}

pub async fn reply_embed(
    ctx: Context<'_>,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    ephemeral: bool,
) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .embed(make_embed(message, title, color))
            .ephemeral(ephemeral),
    )
    .await?;
    Ok(())
}

pub async fn send_channel_embed(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    components: Vec<serenity::CreateActionRow>,
) -> serenity::Result<serenity::Message> {
    channel_id
        .send_message(
            http,
            serenity::CreateMessage::new()
                .embed(make_embed(message, title, color))
                .components(components),
        )
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn send_game_embed(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    components: Vec<serenity::CreateActionRow>,
    include_dead: bool,
    broadcast: bool,
) -> serenity::Result<serenity::Message> {
    let message = message.into();
    let (channel_id, anonymous_enabled, targets) = {
        let running_read = running.read().await;
        let targets = if broadcast && running_read.anonymous_enabled {
            let players = if include_dead {
                running_read.game.players.clone()
            } else {
                running_read
                    .game
                    .alive_players()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            };
            players
                .into_iter()
                .filter_map(|player| {
                    running_read
                        .anonymous_input_channel_ids
                        .get(&player.user_id)
                        .copied()
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        (
            running_read.channel_id,
            running_read.anonymous_enabled,
            targets,
        )
    };
    let sent = send_channel_embed(
        &ctx.http,
        channel_id,
        message.clone(),
        title,
        color,
        components.clone(),
    )
    .await?;
    if broadcast && anonymous_enabled {
        for channel_id in targets {
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                message.clone(),
                title,
                color,
                components.clone(),
            )
            .await;
        }
    }
    Ok(sent)
}

pub async fn send_player_secret(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    message: impl Into<String>,
    components: Vec<serenity::CreateActionRow>,
) -> bool {
    let message = message.into();
    let anonymous_channel_id = {
        let running_read = running.read().await;
        running_read
            .anonymous_enabled
            .then(|| {
                running_read
                    .anonymous_input_channel_ids
                    .get(&player.user_id)
                    .copied()
            })
            .flatten()
    };
    if let Some(channel_id) = anonymous_channel_id
        && send_channel_embed(
            &ctx.http,
            channel_id,
            message.clone(),
            "비밀 메시지",
            serenity::Colour::GOLD,
            components.clone(),
        )
        .await
        .is_ok()
    {
        return true;
    }
    let Ok(user) = serenity::UserId::new(player.user_id).to_user(ctx).await else {
        return false;
    };
    user.direct_message(
        ctx,
        serenity::CreateMessage::new()
            .embed(make_embed(message, "비밀 메시지", serenity::Colour::GOLD))
            .components(components),
    )
    .await
    .is_ok()
}

pub fn duration_text(seconds: u64) -> String {
    if seconds.is_multiple_of(60) {
        format!("{}분", seconds / 60)
    } else {
        format!("{seconds}초")
    }
}

pub fn day_skip_components(
    guild_id: serenity::GuildId,
    disabled: bool,
    confirmed: bool,
) -> Vec<serenity::CreateActionRow> {
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("skipday:{}", guild_id.get()))
            .label(if confirmed {
                "투표 확정"
            } else {
                "바로 투표"
            })
            .style(serenity::ButtonStyle::Primary)
            .disabled(disabled),
    ])]
}

pub fn day_extension_components(
    guild_id: serenity::GuildId,
    disabled: bool,
    confirmed: bool,
) -> Vec<serenity::CreateActionRow> {
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("extendday:{}", guild_id.get()))
            .label(if confirmed {
                "연장 확정"
            } else {
                "1분 연장"
            })
            .style(serenity::ButtonStyle::Secondary)
            .disabled(disabled),
    ])]
}

pub async fn send_component_private(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    message: impl Into<String>,
) -> serenity::Result<()> {
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(message, "마피아 게임", serenity::Colour::RED))
                    .ephemeral(true),
            ),
        )
        .await
}

pub async fn ack_component(ctx: &serenity::Context, component: &serenity::ComponentInteraction) {
    let _ = component
        .create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
        .await;
}

pub fn workspace_path(file_name: &str) -> Result<PathBuf> {
    Ok(std::env::current_dir()
        .context("현재 작업 디렉터리를 확인하지 못했습니다.")?
        .join(file_name))
}

pub fn display_name(member: &serenity::Member) -> String {
    member
        .nick
        .clone()
        .or_else(|| member.user.global_name.clone())
        .unwrap_or_else(|| member.user.name.clone())
}

pub async fn role_by_name(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    name: &str,
) -> Result<Option<serenity::Role>> {
    let roles = guild_id.roles(&ctx.http).await?;
    Ok(roles.into_values().find(|role| role.name == name))
}
