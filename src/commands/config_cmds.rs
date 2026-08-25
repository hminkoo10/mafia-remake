// commands/config_cmds.rs — 게임 설정·블랙리스트 관리 명령어

use super::*;

#[poise::command(
    slash_command,
    rename = "마피아설정",
    description_localized("ko", "마피아 게임 기본 설정을 변경합니다.")
)]
#[allow(clippy::too_many_arguments)]
pub async fn configure_game(
    ctx: Context<'_>,
    #[description = "마피아 수"] mafia: Option<u32>,
    #[description = "의사 활성화 여부"] doctor: Option<bool>,
    #[description = "경찰 활성화 여부"] police: Option<bool>,
    #[description = "시민 특수룰 수"] citizen_special: Option<u32>,
    #[description = "마피아 특수룰 수"] mafia_special: Option<u32>,
    #[description = "중립 특수룰 수"] neutral_special: Option<u32>,
    #[description = "낮 채팅 슬로우모드 초. 기본 3초"] slowmode: Option<u64>,
    #[description = "사망 시 직업 공개 여부"] death_role_reveal: Option<bool>,
    #[description = "낮에 경찰 조사 성공 여부 공개 여부"] police_status_reveal: Option<bool>,
    #[description = "아침 생존 마피아 수 공개 여부"] mafia_count_reveal: Option<bool>,
    #[description = "사립탐정 활성화 여부"] detective: Option<bool>,
    #[description = "형사 활성화 여부"] inspector: Option<bool>,
    #[description = "영매 활성화 여부"] shaman: Option<bool>,
    #[description = "도굴꾼 활성화 여부"] graverobber: Option<bool>,
    #[description = "스파이 활성화 여부"] spy: Option<bool>,
    #[description = "청부업자 활성화 여부"] contractor: Option<bool>,
    #[description = "마녀 활성화 여부"] witch: Option<bool>,
    #[description = "과학자 활성화 여부"] scientist: Option<bool>,
    #[description = "대부 활성화 여부"] godfather: Option<bool>,
    #[description = "조커 활성화 여부"] joker: Option<bool>,
    #[description = "정치인 활성화 여부"] politician: Option<bool>,
    #[description = "판사 활성화 여부"] judge: Option<bool>,
    #[description = "기자 활성화 여부"] reporter: Option<bool>,
    #[description = "해커 활성화 여부"] hacker: Option<bool>,
    #[description = "테러리스트 활성화 여부"] terrorist: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let previous = config_write.clone();
    if let Some(value) = mafia {
        if value < 1 {
            reply_embed(
                ctx,
                "마피아는 최소 1명이어야 합니다.",
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        config_write.default_mafia_count = value;
    }
    if let Some(value) = doctor {
        config_write.default_doctor_count = u32::from(value);
    }
    if let Some(value) = police {
        config_write.default_police_count = u32::from(value);
    }
    if let Some(value) = citizen_special {
        config_write.citizen_special_count = value;
    }
    if let Some(value) = mafia_special {
        config_write.mafia_special_count = value;
    }
    if let Some(value) = neutral_special {
        config_write.neutral_special_count = value;
    }
    if let Some(value) = slowmode {
        config_write.chat_slowmode_seconds = value;
    }
    if let Some(value) = death_role_reveal {
        config_write.reveal_death_roles = value;
    }
    if let Some(value) = police_status_reveal {
        config_write.reveal_public_police_status = value;
    }
    if let Some(value) = mafia_count_reveal {
        config_write.reveal_morning_mafia_count = value;
    }
    if let Some(value) = detective {
        config_write.enable_detective = value;
    }
    if let Some(value) = inspector {
        config_write.enable_inspector = value;
    }
    if let Some(value) = shaman {
        config_write.enable_shaman = value;
    }
    if let Some(value) = graverobber {
        config_write.enable_graverobber = value;
    }
    if let Some(value) = spy {
        config_write.enable_spy = value;
    }
    if let Some(value) = contractor {
        config_write.enable_contractor = value;
    }
    if let Some(value) = witch {
        config_write.enable_witch = value;
    }
    if let Some(value) = scientist {
        config_write.enable_scientist = value;
    }
    if let Some(value) = godfather {
        config_write.enable_godfather = value;
    }
    if let Some(value) = joker {
        config_write.enable_joker = value;
    }
    if let Some(value) = politician {
        config_write.enable_politician = value;
    }
    if let Some(value) = judge {
        config_write.enable_judge = value;
    }
    if let Some(value) = reporter {
        config_write.enable_reporter = value;
    }
    if let Some(value) = hacker {
        config_write.enable_hacker = value;
    }
    if let Some(value) = terrorist {
        config_write.enable_terrorist = value;
    }
    let validation = choose_special_roles(&config_write)
        .and_then(|special_roles| selected_role_counts(&config_write, &special_roles))
        .map(|role_counts| {
            let minimum_players = minimum_player_count(&role_counts);
            let max_players = effective_max_player_count(&config_write);
            (minimum_players, max_players)
        });
    match validation {
        Ok((minimum_players, max_players)) if max_players < minimum_players => {
            *config_write = previous;
            reply_embed(
                ctx,
                format!("현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."),
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        Err(error) => {
            *config_write = previous;
            reply_embed(
                ctx,
                error.to_string(),
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        _ => {}
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아인원설정",
    description_localized("ko", "마피아 게임 모집 최대 인원과 모집 시간을 설정합니다.")
)]
pub async fn configure_player_limit(
    ctx: Context<'_>,
    #[description = "최대 참가 인원. 0은 제한 없음(봇 최대 24명)"] max_players: Option<u32>,
    #[description = "참가자 모집 시간(초). 기본 60초"] 모집시간: Option<u64>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if max_players.is_none() && 모집시간.is_none() {
        reply_embed(
            ctx,
            "변경할 값을 하나 이상 입력하세요.",
            "설정 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    if max_players.is_some_and(|value| value as usize > MAX_GAME_PLAYERS) {
        reply_embed(
            ctx,
            format!("최대 인원은 {MAX_GAME_PLAYERS}명 이하로 설정해야 합니다."),
            "설정 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    if 모집시간.is_some_and(|value| {
        !(config::MIN_RECRUITMENT_SECONDS..=config::MAX_RECRUITMENT_SECONDS).contains(&value)
    }) {
        reply_embed(
            ctx,
            format!(
                "모집 시간은 {}~{}초 사이여야 합니다.",
                config::MIN_RECRUITMENT_SECONDS,
                config::MAX_RECRUITMENT_SECONDS
            ),
            "설정 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(value) = max_players {
        config_write.max_player_count = value;
    }
    if let Some(value) = 모집시간 {
        config_write.recruitment_seconds = value;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 인원 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아익명설정",
    description_localized("ko", "마피아 게임 익명 채팅 사용 여부를 설정합니다.")
)]
pub async fn configure_anonymous_mode(
    ctx: Context<'_>,
    #[description = "익명 채팅 사용 여부"] enabled: bool,
    #[description = "익명 이름을 동물로 할지 숫자로 할지 선택합니다."] 이름방식: Option<
        AnonymousNameMode,
    >,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.anonymous_mode = enabled;
    if let Some(name_mode) = 이름방식 {
        config_write.anonymous_name_mode = name_mode.value().to_string();
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 익명 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아웹설정",
    description_localized(
        "ko",
        "브라우저에서 게임 설정을 편집할 수 있는 1회용 링크를 발급합니다. (관리자 전용)"
    )
)]
pub async fn web_configure_game(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "웹 설정",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let user = ctx.author();
    let token = web_settings::issue_session(
        &ctx.data().web_sessions,
        guild_id.get(),
        user.id.get(),
        user.name.clone(),
    );
    let url = format!(
        "{}{}/{}",
        ctx.data().web_base_url.trim_end_matches('/'),
        web_settings::settings_path(),
        token
    );
    let minutes = web_settings::session_ttl_minutes();
    reply_embed(
        ctx,
        format!(
            "아래 링크에서 마피아 게임 설정을 편집할 수 있습니다.\n{url}\n\n⚠️ 이 링크는 **{}** 님만 사용할 수 있고, **{minutes}분 동안 1회**만 유효합니다. 다른 사람과 공유하지 마세요.",
            user.name
        ),
        "웹 설정 링크 발급",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아추가설정",
    description_localized("ko", "추가 역할 묶음을 설정합니다.")
)]
#[allow(clippy::too_many_arguments)]
pub async fn configure_extra_roles(
    ctx: Context<'_>,
    nurse: Option<bool>,
    lover: Option<bool>,
    priest: Option<bool>,
    madam: Option<bool>,
    gangster: Option<bool>,
    prophet: Option<bool>,
    psychologist: Option<bool>,
    hypnotist: Option<bool>,
    mercenary: Option<bool>,
    thief: Option<bool>,
    soldier: Option<bool>,
    civil_servant: Option<bool>,
    paparazzi: Option<bool>,
    fraudster: Option<bool>,
    cult_team: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(v) = nurse {
        config_write.enable_nurse = v;
    }
    if let Some(v) = lover {
        config_write.enable_lover = v;
    }
    if let Some(v) = priest {
        config_write.enable_priest = v;
    }
    if let Some(v) = madam {
        config_write.enable_madam = v;
    }
    if let Some(v) = gangster {
        config_write.enable_gangster = v;
    }
    if let Some(v) = prophet {
        config_write.enable_prophet = v;
    }
    if let Some(v) = psychologist {
        config_write.enable_psychologist = v;
    }
    if let Some(v) = hypnotist {
        config_write.enable_hypnotist = v;
    }
    if let Some(v) = mercenary {
        config_write.enable_mercenary = v;
    }
    if let Some(v) = civil_servant {
        config_write.enable_civil_servant = v;
    }
    if let Some(v) = paparazzi {
        config_write.enable_paparazzi = v;
    }
    if let Some(v) = fraudster {
        config_write.enable_fraudster = v;
    }
    if let Some(v) = thief {
        config_write.enable_thief = v;
    }
    if let Some(v) = soldier {
        config_write.enable_soldier = v;
    }
    if let Some(v) = cult_team {
        config_write.enable_cult_team = v;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 추가 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아수사설정",
    description_localized("ko", "수사직 후보를 설정합니다.")
)]
pub async fn configure_investigation_role(
    ctx: Context<'_>,
    agent: Option<bool>,
    vigilante: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(v) = agent {
        config_write.use_agent = v;
    }
    if let Some(v) = vigilante {
        config_write.use_vigilante = v;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 수사 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아비활성화",
    description_localized("ko", "마피아 게임 시작을 비활성화합니다.")
)]
pub async fn disable_mafia_game(ctx: Context<'_>) -> Result<(), Error> {
    set_game_enabled(ctx, false).await
}

#[poise::command(
    slash_command,
    rename = "마피아활성화",
    description_localized("ko", "마피아 게임 시작을 활성화합니다.")
)]
pub async fn enable_mafia_game(ctx: Context<'_>) -> Result<(), Error> {
    set_game_enabled(ctx, true).await
}

pub async fn set_game_enabled(ctx: Context<'_>, enabled: bool) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.game_enabled = enabled;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if enabled {
            "마피아 게임을 활성화했습니다. 이제 새 게임을 시작할 수 있습니다."
        } else {
            "마피아 게임을 비활성화했습니다. 새 게임을 시작할 수 없습니다."
        },
        "마피아 게임",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트추가",
    description_localized("ko", "마피아 게임 참가 블랙리스트에 유저를 추가합니다.")
)]
pub async fn add_to_blacklist(
    ctx: Context<'_>,
    #[description = "블랙리스트에 추가할 유저"] 유저: serenity::User,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let id = 유저.id.get();
    let changed = !config_write.blacklist_user_ids.contains(&id);
    if changed {
        config_write.blacklist_user_ids.push(id);
        config_write.blacklist_user_ids.sort_unstable();
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if changed {
            format!(
                "{} 님을 블랙리스트에 추가했습니다. 이제 게임에 참가할 수 없습니다.",
                유저.name
            )
        } else {
            format!("{} 님은 이미 블랙리스트에 있습니다.", 유저.name)
        },
        "블랙리스트",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트해제",
    description_localized("ko", "마피아 게임 참가 블랙리스트에서 유저를 제거합니다.")
)]
pub async fn remove_from_blacklist(
    ctx: Context<'_>,
    #[description = "블랙리스트에서 해제할 유저"] 유저: serenity::User,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let id = 유저.id.get();
    let before = config_write.blacklist_user_ids.len();
    config_write
        .blacklist_user_ids
        .retain(|user_id| *user_id != id);
    let changed = config_write.blacklist_user_ids.len() != before;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if changed {
            format!(
                "{} 님을 블랙리스트에서 해제했습니다. 이제 게임에 참가할 수 있습니다.",
                유저.name
            )
        } else {
            format!("{} 님은 블랙리스트에 없습니다.", 유저.name)
        },
        "블랙리스트",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트목록",
    description_localized("ko", "마피아 게임 참가 블랙리스트 목록을 확인합니다.")
)]
pub async fn show_blacklist(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let config_read = ctx.data().config.read().await;
    let text = if config_read.blacklist_user_ids.is_empty() {
        "블랙리스트가 비어 있습니다.".to_string()
    } else {
        config_read
            .blacklist_user_ids
            .iter()
            .take(50)
            .enumerate()
            .map(|(i, id)| format!("{}. `{id}`", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    drop(config_read);
    reply_embed(ctx, text, "블랙리스트", serenity::Colour::GOLD, true).await?;
    Ok(())
}
