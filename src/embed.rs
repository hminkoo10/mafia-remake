// 역할: Discord 메시지 임베드, 버튼 컴포넌트, DM/채널 메시지 전송 헬퍼

#![allow(unused_imports)]

use super::{Context, Data, Error, RunningGame};
use anyhow::{Context as AnyhowContext, Result};
use mafia_remake::config;
use mafia_remake::model::{Player, Role};
use poise::serenity_prelude as serenity;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

const EMBED_BROADCAST_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretDeliveryRoute {
    AnonymousChannel,
    MemoChannel,
    DirectMessage,
    NotRequired,
}

#[derive(Debug, Clone, Default)]
pub struct SecretDeliveryFailure {
    pub anonymous_enabled: bool,
    pub anonymous_channel_id: Option<serenity::ChannelId>,
    pub anonymous_channel_error: Option<String>,
    pub memo_channel_id: Option<serenity::ChannelId>,
    pub memo_channel_error: Option<String>,
    pub dm_user_error: Option<String>,
    pub dm_send_error: Option<String>,
}

impl SecretDeliveryFailure {
    pub fn public_reason(&self) -> String {
        let mut reasons = Vec::new();
        if self.anonymous_enabled {
            reasons.push(if self.anonymous_channel_id.is_some() {
                "익명 개인 채널 전송 실패"
            } else {
                "익명 개인 채널 없음"
            });
        }
        reasons.push(if self.memo_channel_id.is_some() {
            "개인 메모 채널 전송 실패"
        } else {
            "개인 메모 채널 없음"
        });
        reasons.push(if self.dm_user_error.is_some() {
            "Discord 사용자 조회 실패"
        } else {
            "DM 전송 실패"
        });
        reasons.join(" / ")
    }

    pub fn log_detail(&self) -> String {
        format!(
            "anonymous_enabled={} anonymous_channel_id={:?} anonymous_error={:?} memo_channel_id={:?} memo_error={:?} dm_user_error={:?} dm_send_error={:?}",
            self.anonymous_enabled,
            self.anonymous_channel_id.map(|id| id.get()),
            self.anonymous_channel_error,
            self.memo_channel_id.map(|id| id.get()),
            self.memo_channel_error,
            self.dm_user_error,
            self.dm_send_error,
        )
    }
}

enum DirectSecretError {
    User(String),
    Send(String),
}

async fn send_direct_secret_message(
    ctx: &serenity::Context,
    player: &Player,
    message: String,
    components: Vec<serenity::CreateActionRow>,
) -> std::result::Result<(), DirectSecretError> {
    let user = serenity::UserId::new(player.user_id)
        .to_user(ctx)
        .await
        .map_err(|error| DirectSecretError::User(format!("{error:?}")))?;
    user.direct_message(
        ctx,
        serenity::CreateMessage::new()
            .embed(make_embed(message, "비밀 메시지", serenity::Colour::GOLD))
            .components(components),
    )
    .await
    .map(|_| ())
    .map_err(|error| DirectSecretError::Send(format!("{error:?}")))
}

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
        for chunk in targets.chunks(EMBED_BROADCAST_CONCURRENCY) {
            let mut deliveries = JoinSet::new();
            for &channel_id in chunk {
                let http = ctx.http.clone();
                let message = message.clone();
                let title = title.to_string();
                let components = components.clone();
                deliveries.spawn(async move {
                    let _ = send_channel_embed(
                        http.as_ref(),
                        channel_id,
                        message,
                        &title,
                        color,
                        components,
                    )
                    .await;
                });
            }
            while deliveries.join_next().await.is_some() {}
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
    send_player_secret_detailed(ctx, running, player, message, components)
        .await
        .is_ok()
}

pub async fn send_player_secret_detailed(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    message: impl Into<String>,
    components: Vec<serenity::CreateActionRow>,
) -> std::result::Result<SecretDeliveryRoute, SecretDeliveryFailure> {
    let message = message.into();
    let (anonymous_enabled, anonymous_channel_id, memo_channel_id) = {
        let running_read = running.read().await;
        (
            running_read.anonymous_enabled,
            running_read
                .anonymous_enabled
                .then(|| {
                    running_read
                        .anonymous_input_channel_ids
                        .get(&player.user_id)
                        .copied()
                })
                .flatten(),
            running_read.memo_channel_ids.get(&player.user_id).copied(),
        )
    };

    let mut failure = SecretDeliveryFailure {
        anonymous_enabled,
        anonymous_channel_id,
        memo_channel_id,
        ..Default::default()
    };
    if let Some(channel_id) = anonymous_channel_id {
        match send_channel_embed(
            &ctx.http,
            channel_id,
            message.clone(),
            "비밀 메시지",
            serenity::Colour::GOLD,
            components.clone(),
        )
        .await
        {
            Ok(_) => return Ok(SecretDeliveryRoute::AnonymousChannel),
            Err(error) => failure.anonymous_channel_error = Some(format!("{error:?}")),
        }
    }

    if !anonymous_enabled {
        match send_direct_secret_message(ctx, player, message.clone(), components.clone()).await {
            Ok(()) => return Ok(SecretDeliveryRoute::DirectMessage),
            Err(DirectSecretError::User(error)) => failure.dm_user_error = Some(error),
            Err(DirectSecretError::Send(error)) => failure.dm_send_error = Some(error),
        }
    }

    if let Some(channel_id) = memo_channel_id.filter(|id| Some(*id) != anonymous_channel_id) {
        match send_channel_embed(
            &ctx.http,
            channel_id,
            message.clone(),
            "비밀 메시지",
            serenity::Colour::GOLD,
            components.clone(),
        )
        .await
        {
            Ok(_) => return Ok(SecretDeliveryRoute::MemoChannel),
            Err(error) => failure.memo_channel_error = Some(format!("{error:?}")),
        }
    }

    if anonymous_enabled {
        match send_direct_secret_message(ctx, player, message, components).await {
            Ok(()) => return Ok(SecretDeliveryRoute::DirectMessage),
            Err(DirectSecretError::User(error)) => failure.dm_user_error = Some(error),
            Err(DirectSecretError::Send(error)) => failure.dm_send_error = Some(error),
        }
    }
    Err(failure)
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

fn has_workspace_marker(path: &Path) -> bool {
    path.join(".env").is_file()
        || path.join("config.json").is_file()
        || path.join("Cargo.toml").is_file()
}

fn find_workspace_from(start: impl AsRef<Path>) -> Option<PathBuf> {
    start
        .as_ref()
        .ancestors()
        .find(|path| has_workspace_marker(path))
        .map(Path::to_path_buf)
}

pub fn workspace_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("MAFIA_WORKDIR") {
        let root = PathBuf::from(root);
        if root.is_dir() {
            return Ok(root);
        }
    }

    if let Ok(current_dir) = std::env::current_dir()
        && let Some(root) = find_workspace_from(current_dir)
    {
        return Ok(root);
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
        && let Some(root) = find_workspace_from(exe_dir)
    {
        return Ok(root);
    }

    std::env::current_dir().context("현재 작업 디렉터리를 확인하지 못했습니다.")
}

pub fn load_workspace_env() -> Result<PathBuf> {
    let root = workspace_root()?;
    let env_path = root.join(".env");
    if env_path.is_file() {
        dotenvy::from_path(&env_path)
            .with_context(|| format!("{} 파일을 읽지 못했습니다.", env_path.display()))?;
    }
    Ok(root)
}

#[allow(dead_code)]
pub fn workspace_path(file_name: &str) -> Result<PathBuf> {
    Ok(workspace_root()?.join(file_name))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_delivery_failure_explains_every_attempt() {
        let failure = SecretDeliveryFailure {
            anonymous_enabled: true,
            anonymous_channel_id: None,
            memo_channel_id: Some(serenity::ChannelId::new(20)),
            memo_channel_error: Some("forbidden".to_string()),
            dm_send_error: Some("cannot message user".to_string()),
            ..Default::default()
        };

        assert_eq!(
            failure.public_reason(),
            "익명 개인 채널 없음 / 개인 메모 채널 전송 실패 / DM 전송 실패"
        );
        assert!(failure.log_detail().contains("memo_channel_id=Some(20)"));
        assert!(failure.log_detail().contains("cannot message user"));
    }
}
