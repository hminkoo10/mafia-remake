// channel/recruitment.rs — 참가자 모집 임베드·역할 정리

use super::*;

pub const RECRUITMENT_STATUS_OPEN: &str = "\u{BAA8}\u{C9D1} \u{C911}\u{C785}\u{B2C8}\u{B2E4}.";
pub const RECRUITMENT_STATUS_CANCELLED: &str =
    "\u{BAA8}\u{C9D1}\u{C774} \u{CDE8}\u{C18C}\u{B418}\u{C5C8}\u{C2B5}\u{B2C8}\u{B2E4}.";

pub fn recruitment_embed(
    recruitment: &Recruitment,
    config: &config::BotConfig,
    status: &str,
) -> serenity::CreateEmbed {
    let mut joined = recruitment
        .joined_names
        .values()
        .cloned()
        .collect::<Vec<_>>();
    joined.sort_by_key(|name| name.to_lowercase());
    let joined_text = if joined.is_empty() {
        "아직 참가자가 없습니다.".to_string()
    } else {
        joined
            .iter()
            .enumerate()
            .map(|(idx, name)| format!("{}. {name}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut spectators = recruitment
        .spectator_names
        .values()
        .cloned()
        .collect::<Vec<_>>();
    spectators.sort_by_key(|name| name.to_lowercase());
    let spectator_text = if spectators.is_empty() {
        "아직 관전자가 없습니다.".to_string()
    } else {
        spectators
            .iter()
            .enumerate()
            .map(|(idx, name)| format!("{}. {name}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let shortage = recruitment
        .minimum_players
        .saturating_sub(recruitment.joined_ids.len());
    let minimum_text = if shortage == 0 {
        format!("최소 시작 인원 **{}명** 충족", recruitment.minimum_players)
    } else {
        format!(
            "최소 시작 인원 **{}명**까지 **{}명** 더 필요",
            recruitment.minimum_players, shortage
        )
    };
    let remaining = recruitment
        .max_players
        .saturating_sub(recruitment.joined_ids.len());
    let auto_start_text = match recruitment.auto_start_players {
        Some(count) => format!("자동시작: **{count}명**이 모이면 즉시 시작합니다."),
        None => {
            "자동시작: 설정되지 않았습니다. 주최자는 `자동시작` 버튼으로 인원을 정할 수 있습니다."
                .to_string()
        }
    };
    make_embed(
        format!(
            "최대 {}초 동안 참가자를 모집합니다.\n참가 버튼을 누르면 게임 참가자로 등록되고, '{}' 역할이 부여됩니다.\n관전 버튼을 누르면 '{SPECTATOR_ROLE}' 역할이 부여되고 게임 채널을 읽을 수 있습니다.\n주최자는 `시작` 버튼으로 즉시 시작하거나 `취소` 버튼으로 모집을 취소할 수 있습니다.\n{auto_start_text}\n\n역할 구성: {}\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n{}\n\n최대 참가 인원 **{}명**까지 **{}명** 더 참가 가능\n\n현재 참가자 **{}/{}명**\n{}\n\n현재 관전자 **{}명**\n{}\n\n{}",
            recruitment.recruitment_seconds,
            config.participant_role,
            public_role_count_text_from_counts(&recruitment.role_counts, None),
            if config.reveal_death_roles {
                "공개"
            } else {
                "비공개"
            },
            if config.reveal_public_police_status {
                "공개"
            } else {
                "비공개"
            },
            if config.reveal_morning_mafia_count {
                "공개"
            } else {
                "비공개"
            },
            minimum_text,
            recruitment.max_players,
            remaining,
            recruitment.joined_ids.len(),
            recruitment.max_players,
            joined_text,
            recruitment.spectator_ids.len(),
            spectator_text,
            status
        ),
        "참가자 모집",
        serenity::Colour::DARK_GREEN,
    )
}

pub fn recruitment_components(
    guild_id: serenity::GuildId,
    disabled: bool,
) -> Vec<serenity::CreateActionRow> {
    let guild_key = guild_id.get();
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("join:{guild_key}"))
            .label("참가")
            .style(serenity::ButtonStyle::Success)
            .disabled(disabled),
        serenity::CreateButton::new(format!("spectate:{guild_key}"))
            .label("관전")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("startnow:{guild_key}"))
            .label("시작")
            .style(serenity::ButtonStyle::Primary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("autostart:{guild_key}"))
            .label("자동시작")
            .style(serenity::ButtonStyle::Primary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("cancelrec:{guild_key}"))
            .label("취소")
            .style(serenity::ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}

pub fn auto_start_modal(
    guild_id: serenity::GuildId,
    recruitment: &Recruitment,
) -> serenity::CreateModal {
    let mut input = serenity::CreateInputText::new(
        serenity::InputTextStyle::Short,
        format!(
            "인원 ({}~{}명)",
            recruitment.minimum_players, recruitment.max_players
        ),
        "auto_start_players",
    )
    .placeholder(format!(
        "예: {} (이 인원이 모이면 즉시 시작)",
        recruitment.minimum_players
    ))
    .min_length(1)
    .max_length(3)
    .required(true);
    // Discord는 value가 실려 있으면 1자 이상을 요구한다. 아직 정해진 인원이 없을 때
    // 빈 문자열을 보내면 모달 응답이 400으로 실패하고, 인터랙션이 확인되지 않아
    // 사용자에게는 "봇이 적시에 응답하지 않았어요"로 보인다.
    if let Some(count) = recruitment.auto_start_players {
        input = input.value(count.to_string());
    }
    serenity::CreateModal::new(
        format!("autostart:{}", guild_id.get()),
        "자동시작 인원 설정",
    )
    .components(vec![serenity::CreateActionRow::InputText(input)])
}

/// 모집 취소 시 되돌려야 하는 (유저, 역할) 목록. 관전자 역할이 서버에 없으면
/// 관전자에게는 아무 역할도 부여되지 않았으므로 정리 대상도 아니다.
pub fn recruitment_role_removals(recruitment: &Recruitment) -> Vec<(u64, serenity::RoleId)> {
    let mut removals = recruitment
        .joined_ids
        .iter()
        .map(|user_id| (*user_id, recruitment.participant_role_id))
        .collect::<Vec<_>>();
    if let Some(spectator_role_id) = recruitment.spectator_role_id {
        removals.extend(
            recruitment
                .spectator_ids
                .iter()
                .map(|user_id| (*user_id, spectator_role_id)),
        );
    }
    removals.sort_unstable();
    removals
}

/// 자동시작 인원에 도달했는지. 참가 처리와 모달 제출이 같은 판단을 공유한다.
pub fn auto_start_reached(recruitment: &Recruitment) -> bool {
    recruitment
        .auto_start_players
        .is_some_and(|count| recruitment.joined_ids.len() >= count)
}

/// 모집이 취소되면 모집 중에 부여한 참가자/관전자 역할을 되돌린다. 게임이 시작되지
/// 않았으므로 `cleanup_game`이 돌지 않아, 여기서 정리하지 않으면 역할이 남는다.
pub async fn cleanup_recruitment_roles(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    recruitment: &Recruitment,
) {
    let removals = recruitment_role_removals(recruitment);
    for chunk in removals.chunks(DISCORD_WRITE_CONCURRENCY) {
        let mut jobs = JoinSet::new();
        for (user_id, role_id) in chunk.iter().copied() {
            let ctx = ctx.clone();
            jobs.spawn(async move {
                if let Err(error) = crate::http_pool::with_fallback(&ctx, |http| async move {
                    guild_id
                        .member(&http, serenity::UserId::new(user_id))
                        .await?
                        .remove_role(&http, role_id)
                        .await
                })
                .await
                {
                    eprintln!(
                        "failed to remove recruitment role: guild_id={} user_id={user_id} role_id={} error={error:?}",
                        guild_id.get(),
                        role_id.get()
                    );
                }
            });
        }
        while jobs.join_next().await.is_some() {}
    }
}

pub async fn update_recruitment_message(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    recruitment: &Recruitment,
    status: &str,
    disabled: bool,
) {
    update_recruitment_message_at(
        ctx,
        data,
        component.channel_id,
        component.message.id,
        guild_id,
        recruitment,
        status,
        disabled,
    )
    .await;
}

pub async fn update_recruitment_message_at(
    ctx: &serenity::Context,
    data: &Data,
    channel_id: serenity::ChannelId,
    message_id: serenity::MessageId,
    guild_id: serenity::GuildId,
    recruitment: &Recruitment,
    status: &str,
    disabled: bool,
) {
    let counter = data
        .recruitment_update_versions
        .entry(guild_id)
        .or_insert_with(|| Arc::new(std::sync::atomic::AtomicU64::new(0)))
        .clone();
    let version = counter.fetch_add(1, Ordering::AcqRel) + 1;
    let data = data.clone();
    let http = ctx.http.clone();
    let recruitment = recruitment.clone();
    let status = status.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(RECRUITMENT_UPDATE_DEBOUNCE).await;
        let Some(current_counter) = data
            .recruitment_update_versions
            .get(&guild_id)
            .map(|entry| entry.clone())
        else {
            return;
        };
        if current_counter.load(Ordering::Acquire) != version {
            return;
        }
        let config = data.config.read().await.clone();
        if let Err(error) = channel_id
            .edit_message(
                http.as_ref(),
                message_id,
                serenity::EditMessage::new()
                    .embed(recruitment_embed(&recruitment, &config, &status))
                    .components(recruitment_components(guild_id, disabled)),
            )
            .await
        {
            eprintln!(
                "failed to update recruitment message: guild_id={} channel_id={} message_id={} error={error:?}",
                guild_id.get(),
                channel_id.get(),
                message_id.get()
            );
        }
        if disabled && current_counter.load(Ordering::Acquire) == version {
            data.recruitment_update_versions
                .remove_if(&guild_id, |_, stored| Arc::ptr_eq(stored, &current_counter));
        }
    });
}
