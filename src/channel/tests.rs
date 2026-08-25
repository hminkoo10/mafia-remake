// channel 테스트 모듈 (src/channel.rs에서 분리)

use super::*;

fn role_chat_test_game() -> MafiaGame {
    MafiaGame::new(
        vec![
            (1, "p1".to_string()),
            (2, "p2".to_string()),
            (3, "p3".to_string()),
            (4, "p4".to_string()),
        ],
        1,
        1,
        1,
        Vec::new(),
    )
    .unwrap()
}

pub(crate) fn dead_chat_test_running() -> RunningGame {
    let game = role_chat_test_game();
    let initial_roles = game
        .players
        .iter()
        .map(|player| (player.user_id, player.role))
        .collect();
    let participant_user_ids = game.players.iter().map(|player| player.user_id).collect();
    RunningGame {
        guild_id: serenity::GuildId::new(1),
        channel_id: serenity::ChannelId::new(10),
        participant_user_ids,
        spectator_user_ids: Default::default(),
        game,
        reveal_death_roles: true,
        anonymous_enabled: false,
        started_at: Instant::now(),
        started_at_iso: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ended_at_iso: None,
        activity_game_key: "test-game".to_string(),
        phase_deadline: None,
        initial_roles,
        memos: Default::default(),
        game_status_message_id: None,
        game_status_text: None,
        anonymous_aliases: Default::default(),
        anonymous_original_names: Default::default(),
        anonymous_input_channel_ids: Default::default(),
        anonymous_input_channel_owners: Default::default(),
        anonymous_dead_input_channel_ids: Default::default(),
        anonymous_dead_input_channel_owners: Default::default(),
        dead_chat_unlocked_ids: Default::default(),
        pending_dead_chat_user_ids: Default::default(),
        dead_role_chat_visible_from_days: Default::default(),
        anonymous_shaman_input_channel_ids: Default::default(),
        anonymous_shaman_input_channel_owners: Default::default(),
        anonymous_role_input_channel_ids: Default::default(),
        anonymous_role_input_channels: Default::default(),
        anonymous_role_input_status_message_ids: Default::default(),
        anonymous_role_status_texts: Default::default(),
        anonymous_webhooks: Default::default(),
        anonymous_webhook_creation_locks: Default::default(),
        channel_role_ids: None,
        source_category_id: None,
        permission_overwrite_cache: Default::default(),
        verified_member_ids: Default::default(),
        personal_channel_creation_locks: Default::default(),
        original_game_channel_overwrites: Default::default(),
        game_channel_overwrites: Default::default(),
        member_channel_overwrites: Default::default(),
        original_slowmode_delays: Default::default(),
        channel_slowmode_cache: Default::default(),
        private_channel_ids: Default::default(),
        private_role_status_message_ids: Default::default(),
        private_role_status_texts: Default::default(),
        memo_channel_ids: Default::default(),
        shaman_channel_id: None,
        shaman_status_message_id: None,
        shaman_status_text: None,
        frog_game_channel_overwrites: Default::default(),
        madam_seduction_channel_overwrites: Default::default(),
        day_chat_open: true,
        final_defense_user_id: None,
        day_skip_voter_ids: Default::default(),
        day_skip_confirmed: false,
        day_extension_voter_ids: Default::default(),
        day_extension_active: false,
        day_extension_confirmed: false,
        night_timed_events_due: false,
        contractor_contract_drafts: Default::default(),
        activity_night_results: Default::default(),
        replay_events: Default::default(),
        next_replay_sequence: 1,
        night_notify: Arc::new(Notify::new()),
        vote_notify: Arc::new(Notify::new()),
        confirm_notify: Arc::new(Notify::new()),
        day_notify: Arc::new(Notify::new()),
        stats_recorded: false,
    }
}

fn selection_test_config() -> config::BotConfig {
    config::BotConfig {
        game_enabled: true,
        participant_role: "참가자".to_string(),
        manager_role: "관리자".to_string(),
        default_mafia_count: 1,
        default_doctor_count: 0,
        default_police_count: 1,
        default_joker_count: 0,
        max_player_count: 0,
        recruitment_seconds: 60,
        night_seconds: 30,
        discussion_seconds: 30,
        vote_seconds: 30,
        chat_slowmode_seconds: 0,
        reveal_death_roles: false,
        reveal_public_police_status: true,
        reveal_morning_mafia_count: true,
        show_confirmation_vote_counts: true,
        citizen_special_count: 1,
        mafia_special_count: 0,
        neutral_special_count: 0,
        enable_detective: true,
        enable_inspector: true,
        enable_graverobber: false,
        enable_spy: false,
        enable_contractor: false,
        enable_fraudster: false,
        enable_witch: false,
        enable_scientist: false,
        enable_madam: false,
        enable_godfather: false,
        enable_joker: false,
        enable_politician: false,
        enable_judge: false,
        enable_reporter: false,
        enable_hacker: false,
        enable_terrorist: false,
        enable_lover: false,
        enable_civil_servant: false,
        enable_paparazzi: false,
        enable_shaman: false,
        enable_priest: false,
        enable_soldier: false,
        enable_nurse: false,
        enable_gangster: false,
        enable_prophet: false,
        enable_psychologist: false,
        enable_hypnotist: false,
        enable_mercenary: false,
        enable_thief: false,
        enable_cult_team: false,
        use_agent: true,
        use_vigilante: true,
        anonymous_mode: false,
        anonymous_name_mode: "animal".to_string(),
        blacklist_user_ids: Vec::new(),
    }
}

#[test]
fn base_investigation_role_filters_investigation_specials() {
    let config = selection_test_config();
    let mut role_history = HashMap::new();
    role_history.insert(Role::Inspector, 0);
    role_history.insert(Role::Detective, 100);

    let special_roles = choose_special_roles_balanced(&config, &role_history).unwrap();
    let role_counts =
        selected_role_counts_balanced(&config, &special_roles, &role_history).unwrap();
    let investigation_count = role_counts
        .iter()
        .filter(|(role, _)| role.is_investigation_role())
        .map(|(_, count)| *count)
        .sum::<usize>();

    assert_eq!(special_roles, vec![Role::Detective]);
    assert_eq!(investigation_count, config.default_police_count as usize);
    assert_eq!(
        role_counts
            .keys()
            .filter(|role| role.is_investigation_role())
            .count(),
        1
    );
}

#[test]
fn enabled_inspector_can_be_selected_as_base_investigation_role() {
    let mut config = selection_test_config();
    config.citizen_special_count = 0;
    config.use_agent = false;
    config.use_vigilante = false;
    let role_history = HashMap::from([(Role::Police, 10), (Role::Inspector, 0)]);

    let role_counts = selected_role_counts_balanced(&config, &[], &role_history).unwrap();

    assert_eq!(
        role_counts.get(&Role::Inspector),
        Some(&(config.default_police_count as usize))
    );
    assert!(!role_counts.contains_key(&Role::Police));
}

#[test]
fn enabled_base_jokers_are_included_in_role_counts() {
    let mut config = selection_test_config();
    config.default_joker_count = 2;
    config.enable_joker = true;

    let role_counts = selected_role_counts(&config, &[]).unwrap();

    assert_eq!(role_counts.get(&Role::Joker), Some(&2));

    config.enable_joker = false;
    let role_counts = selected_role_counts(&config, &[]).unwrap();
    assert!(!role_counts.contains_key(&Role::Joker));
}

#[test]
fn public_role_count_does_not_count_inspector_as_two_people() {
    let role_counts = HashMap::from([(Role::Mafia, 1), (Role::Inspector, 1), (Role::Citizen, 3)]);

    let text = public_role_count_text_from_counts(&role_counts, Some(5));

    assert!(text.contains("수사직 1명"));
    assert!(text.contains("시민 3명"));
}

#[test]
fn pending_dead_chat_unlocks_when_night_starts() {
    let mut running = dead_chat_test_running();
    let user_id = running.game.players[0].user_id;
    running.game.players[0].alive = false;
    running.pending_dead_chat_user_ids.insert(user_id);

    running.game.phase = Phase::Night;
    let candidates = dead_chat_unlock_candidates(&running);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].user_id, user_id);

    running.game.phase = Phase::Day;
    let candidates = dead_chat_unlock_candidates(&running);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].user_id, user_id);
}

#[test]
fn night_victim_cannot_receive_role_chat_from_death_night() {
    let mut running = dead_chat_test_running();
    running.game.day_number = 1;
    running.game.phase = Phase::Day;
    running.game.players[0].alive = false;
    let dead_player = running.game.players[0].clone();

    record_dead_chat_deaths(&mut running, std::slice::from_ref(&dead_player));

    assert!(can_use_anonymous_dead_chat(&running, &dead_player));
    assert!(!can_receive_role_chat_as_dead(&running, &dead_player));

    running.game.phase = Phase::Night;
    assert!(!can_receive_role_chat_as_dead(&running, &dead_player));

    running.game.day_number = 2;
    assert!(can_receive_role_chat_as_dead(&running, &dead_player));
}

#[test]
fn vote_victim_skips_first_role_chat_night() {
    let mut running = dead_chat_test_running();
    running.game.day_number = 2;
    running.game.phase = Phase::Night;
    running.game.players[0].alive = false;
    let dead_player = running.game.players[0].clone();

    record_dead_chat_deaths(&mut running, std::slice::from_ref(&dead_player));
    assert!(
        running
            .pending_dead_chat_user_ids
            .remove(&dead_player.user_id)
    );
    running.dead_chat_unlocked_ids.insert(dead_player.user_id);

    assert!(can_use_anonymous_dead_chat(&running, &dead_player));
    assert!(!can_receive_role_chat_as_dead(&running, &dead_player));

    running.game.day_number = 3;
    assert!(can_receive_role_chat_as_dead(&running, &dead_player));
}

#[test]
fn unlocked_dead_without_channel_is_retried_on_day() {
    let mut running = dead_chat_test_running();
    let user_id = running.game.players[0].user_id;
    running.game.players[0].alive = false;
    running.game.phase = Phase::Day;
    running.dead_chat_unlocked_ids.insert(user_id);

    let candidates = dead_chat_unlock_candidates(&running);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].user_id, user_id);

    running
        .anonymous_dead_input_channel_ids
        .insert(user_id, serenity::ChannelId::new(123));
    assert!(dead_chat_unlock_candidates(&running).is_empty());
}

#[test]
fn balanced_special_selection_prefers_less_seen_roles() {
    let mut history = std::collections::HashMap::new();
    history.insert(Role::Spy, 10);
    history.insert(Role::Contractor, 5);

    let selected = select_balanced_special_roles_for_slots(
        &[Role::Spy, Role::Contractor, Role::Thief],
        1,
        &history,
    )
    .unwrap();

    assert_eq!(selected, vec![Role::Thief]);
}

#[test]
fn balanced_special_selection_counts_lover_slots() {
    let mut history = std::collections::HashMap::new();
    history.insert(Role::Lover, 100);

    let selected = select_balanced_special_roles_for_slots(
        &[Role::Lover, Role::Detective, Role::Shaman],
        2,
        &history,
    )
    .unwrap();

    assert_eq!(selected, vec![Role::Detective, Role::Shaman]);
}

#[test]
fn frog_cannot_use_any_game_chat() {
    let mut running = dead_chat_test_running();
    let player = running.game.players[0].clone();
    running.game.frog_user_ids.insert(player.user_id);

    assert!(is_player_chat_silenced(&running, &player));

    running.game.phase = Phase::Night;
    assert!(!can_use_anonymous_role_chat(&running, &player, player.role));
    assert!(!private_role_member_can_chat(
        &running.game,
        player.role,
        &player
    ));
    let mut shaman = player.clone();
    shaman.role = Role::Shaman;
    assert!(!can_use_anonymous_shaman_chat(&running, &shaman));

    running.game.phase = Phase::Day;
    running.day_chat_open = true;
    assert!(!can_use_anonymous_general_chat(&running, &player));
}

/// 마담에게 유혹당해도 자신의 최후변론은 할 수 있어야 한다. 개구리는 예외이고,
/// 대상자가 아닌 유혹 상태 플레이어는 여전히 말할 수 없다.
#[test]
fn seduced_nominee_can_speak_during_their_final_defense() {
    let mut running = dead_chat_test_running();
    running.anonymous_enabled = true;
    let player = running.game.players[0].clone();
    running.game.madam_seduced_ids.insert(player.user_id);

    running.game.phase = Phase::FinalDefense;
    running.final_defense_user_id = Some(player.user_id);
    assert!(can_use_anonymous_general_chat(&running, &player));

    // 대상자가 아니면 유혹 상태라 말할 수 없다.
    let other = running.game.players[1].clone();
    running.game.madam_seduced_ids.insert(other.user_id);
    assert!(!can_use_anonymous_general_chat(&running, &other));

    // 개구리 저주는 최후변론에서도 말이 막힌다.
    running.game.frog_user_ids.insert(player.user_id);
    assert!(!can_use_anonymous_general_chat(&running, &player));
}

#[test]
fn anonymous_shared_shaman_channel_is_hidden_from_members() {
    assert_eq!(
        shared_shaman_member_access(true, true, true),
        (false, false)
    );
    assert_eq!(
        shared_shaman_member_access(true, true, false),
        (false, false)
    );
    assert_eq!(shared_shaman_member_access(false, true, true), (true, true));
}

#[test]
fn forced_cleanup_removes_only_game_managed_permission_bits() {
    let kind = serenity::PermissionOverwriteType::Role(serenity::RoleId::new(7));
    let overwrite = serenity::PermissionOverwrite {
        allow: serenity::Permissions::MANAGE_MESSAGES | serenity::Permissions::SEND_MESSAGES,
        deny: serenity::Permissions::VIEW_CHANNEL | serenity::Permissions::CREATE_PUBLIC_THREADS,
        kind,
    };

    let cleaned = stripped_game_permission_overwrite(overwrite, true).unwrap();
    assert_eq!(cleaned.allow, serenity::Permissions::MANAGE_MESSAGES);
    assert!(cleaned.deny.is_empty());
}

#[test]
fn living_shaman_cannot_receive_other_role_night_chat() {
    let mut running = dead_chat_test_running();
    running.anonymous_enabled = true;
    running.game.phase = Phase::Night;
    running.game.players[0].role = Role::Shaman;
    let shaman = running.game.players[0].clone();

    assert!(can_use_anonymous_shaman_chat(&running, &shaman));
    assert!(!can_receive_role_chat_as_dead(&running, &shaman));
}

#[test]
fn recent_role_history_prevents_shaman_starvation() {
    let mut config = selection_test_config();
    config.default_police_count = 0;
    config.enable_inspector = false;
    config.enable_shaman = true;

    let mut stats_file = stats::StatsFile::default();
    stats_file.users.insert(
        "1".to_string(),
        stats::PlayerStats {
            roles: HashMap::from([
                (Role::Shaman.value().to_string(), 100),
                (Role::Detective.value().to_string(), 1),
            ]),
            rating_history: vec![stats::RatingHistoryItem {
                ended_at: "2026-07-14T00:00:00+09:00".to_string(),
                before: 1000,
                after: 1000,
                delta: 0,
                team_delta: 0,
                role_delta: 0,
                streak_delta: 0,
                role: Role::Detective.value().to_string(),
                team: "citizen".to_string(),
                winner: "시민".to_string(),
                players: 5,
                rating_reasons: Vec::new(),
            }],
            ..Default::default()
        },
    );

    let history = stats::role_appearance_counts(&stats_file);
    let selected = choose_special_roles_balanced(&config, &history).unwrap();

    assert_eq!(selected, vec![Role::Shaman]);
}

#[test]
fn started_role_history_rotates_shaman_into_selection() {
    let mut config = selection_test_config();
    config.default_police_count = 0;
    config.enable_inspector = false;
    config.enable_shaman = true;

    let mut stats_file = stats::StatsFile::default();
    stats::record_role_selection(&mut stats_file, [Role::Detective]);
    let history = stats::role_appearance_counts(&stats_file);
    let selected = choose_special_roles_balanced(&config, &history).unwrap();

    assert_eq!(selected, vec![Role::Shaman]);
}

#[test]
fn private_role_chat_closed_during_day() {
    let mut game = role_chat_test_game();
    let doctor = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .cloned()
        .unwrap();

    game.phase = Phase::Day;

    assert!(!private_role_member_can_chat(&game, Role::Doctor, &doctor));
}

#[test]
fn private_role_chat_open_during_night() {
    let game = role_chat_test_game();
    let doctor = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .cloned()
        .unwrap();

    assert!(private_role_member_can_chat(&game, Role::Doctor, &doctor));
}

#[test]
fn mafia_room_visibility_does_not_follow_chat_phase() {
    let mut game = role_chat_test_game();
    let mafia = game
        .players
        .iter()
        .find(|player| player.role == Role::Mafia)
        .cloned()
        .unwrap();

    game.phase = Phase::Day;
    assert!(private_role_member_can_view(&game, Role::Mafia, &mafia));
    assert!(!private_role_member_can_chat(&game, Role::Mafia, &mafia));

    game.phase = Phase::Night;
    assert!(private_role_member_can_view(&game, Role::Mafia, &mafia));
    assert!(private_role_member_can_chat(&game, Role::Mafia, &mafia));
}

#[test]
fn anonymous_mafia_status_targets_every_personal_room_and_shows_choices() {
    let mut running = dead_chat_test_running();
    running.anonymous_enabled = true;
    running.game.get_player_mut(1).unwrap().role = Role::Mafia;
    running.game.get_player_mut(2).unwrap().role = Role::Thief;
    running.game.thief_stolen_roles.insert(2, Role::Mafia);
    running.game.thief_contacted.insert(2);
    running.anonymous_aliases = HashMap::from([
        (1, "마피아A".to_string()),
        (2, "도둑B".to_string()),
        (3, "대상A".to_string()),
        (4, "대상B".to_string()),
    ]);
    running.anonymous_role_input_channel_ids = HashMap::from([
        ((1, Role::Mafia), serenity::ChannelId::new(101)),
        ((2, Role::Mafia), serenity::ChannelId::new(102)),
    ]);
    running.game.mafia_display_targets = HashMap::from([(1, 3), (2, 4)]);

    let targets = anonymous_role_status_targets(&running, Role::Mafia).unwrap();
    let status = mafia_night_target_status_text(&running);

    assert_eq!(
        targets,
        vec![
            ((1, Role::Mafia), serenity::ChannelId::new(101)),
            ((2, Role::Mafia), serenity::ChannelId::new(102)),
        ]
    );
    assert!(status.contains("- 마피아A → 대상A"));
    assert!(status.contains("- 도둑B → 대상B"));
}

#[test]
fn restored_mafia_frog_gets_mafia_private_role_back() {
    let mut running = dead_chat_test_running();
    let mafia = running
        .game
        .players
        .iter()
        .find(|player| player.role == Role::Mafia)
        .cloned()
        .unwrap();
    running.game.frog_user_ids.insert(mafia.user_id);

    let restored = running.game.restore_frogs();
    let restored_mafia = restored
        .iter()
        .find(|player| player.user_id == mafia.user_id)
        .unwrap();

    assert_eq!(
        private_roles_to_restore(&running, restored_mafia),
        vec![Role::Mafia]
    );
}

#[test]
fn ended_game_blocks_late_temporary_chat_creation() {
    assert!(!is_game_channel_creation_allowed(Phase::Ended));
    assert!(is_game_channel_creation_allowed(Phase::Night));
}

#[test]
fn permission_cache_distinguishes_member_and_role_targets() {
    let channel_id = serenity::ChannelId::new(10);
    let member_key = permission_cache_key(
        channel_id,
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(20)),
    );
    let role_key = permission_cache_key(
        channel_id,
        serenity::PermissionOverwriteType::Role(serenity::RoleId::new(20)),
    );

    assert_ne!(member_key, role_key);
}

#[test]
fn created_channel_permissions_are_remembered() {
    let mut running = dead_chat_test_running();
    let channel_id = serenity::ChannelId::new(30);
    let overwrite = anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(1)),
        true,
        false,
    );

    remember_channel_permissions(&mut running, channel_id, std::slice::from_ref(&overwrite));

    assert_eq!(
        remembered_permission(&running, channel_id, overwrite.kind),
        Some(overwrite)
    );
}

#[test]
fn member_role_swap_preserves_unrelated_roles() {
    let participant = serenity::RoleId::new(1);
    let unrelated = serenity::RoleId::new(2);
    let dead = serenity::RoleId::new(3);

    assert_eq!(
        swapped_member_roles(&[participant, unrelated], Some(participant), Some(dead)),
        Some(vec![unrelated, dead])
    );
    assert_eq!(
        swapped_member_roles(&[unrelated, dead], Some(participant), Some(dead)),
        None
    );
}

fn recruitment_fixture() -> Recruitment {
    Recruitment {
        host_user_id: serenity::UserId::new(1),
        participant_role_id: serenity::RoleId::new(10),
        spectator_role_id: Some(serenity::RoleId::new(11)),
        role_counts: HashMap::new(),
        special_roles: Vec::new(),
        max_players: 8,
        minimum_players: 4,
        joined_ids: HashSet::from([1, 2, 3]),
        joined_names: HashMap::new(),
        spectator_ids: HashSet::from([7]),
        spectator_names: HashMap::new(),
        accepting: true,
        cancelled: false,
        auto_start_players: None,
        recruitment_seconds: 60,
        done: Arc::new(tokio::sync::Notify::new()),
    }
}

/// Discord는 텍스트 입력의 `value`가 있으면 1자 이상이어야 한다. 빈 문자열을
/// 보내면 모달 응답이 400으로 실패하고, 인터랙션이 확인되지 않아 사용자에게는
/// "봇이 적시에 응답하지 않았어요"로 보인다.
#[test]
fn auto_start_modal_omits_the_value_when_nothing_is_set_yet() {
    let recruitment = recruitment_fixture();
    assert_eq!(recruitment.auto_start_players, None);

    let json =
        serde_json::to_value(auto_start_modal(serenity::GuildId::new(1), &recruitment)).unwrap();
    let input = &json["components"][0]["components"][0];

    assert!(
        input.get("value").is_none_or(|value| value.is_null()),
        "빈 value가 전송되면 안 된다: {input}"
    );
    assert_eq!(input["custom_id"], "auto_start_players");
    assert!(input["label"].as_str().unwrap().chars().count() <= 45);
    assert!(input["placeholder"].as_str().unwrap().chars().count() <= 100);
    assert_eq!(json["custom_id"], "autostart:1");
    assert!(json["title"].as_str().unwrap().chars().count() <= 45);
}

#[test]
fn auto_start_modal_prefills_the_current_setting() {
    let mut recruitment = recruitment_fixture();
    recruitment.auto_start_players = Some(6);

    let json =
        serde_json::to_value(auto_start_modal(serenity::GuildId::new(1), &recruitment)).unwrap();

    assert_eq!(json["components"][0]["components"][0]["value"], "6");
}

#[test]
fn auto_start_triggers_only_once_the_target_headcount_is_reached() {
    let mut recruitment = recruitment_fixture();
    assert!(!auto_start_reached(&recruitment));

    recruitment.auto_start_players = Some(4);
    assert!(!auto_start_reached(&recruitment));

    recruitment.joined_ids.insert(4);
    assert!(auto_start_reached(&recruitment));

    // 인원을 넘겨 들어와도 여전히 시작 조건이다.
    recruitment.joined_ids.insert(5);
    assert!(auto_start_reached(&recruitment));
}

#[test]
fn cancelled_recruitment_gives_back_participant_and_spectator_roles() {
    let recruitment = recruitment_fixture();

    assert_eq!(
        recruitment_role_removals(&recruitment),
        vec![
            (1, serenity::RoleId::new(10)),
            (2, serenity::RoleId::new(10)),
            (3, serenity::RoleId::new(10)),
            (7, serenity::RoleId::new(11)),
        ]
    );
}

#[test]
fn recruitment_cleanup_skips_spectators_when_the_role_is_missing() {
    let mut recruitment = recruitment_fixture();
    recruitment.spectator_role_id = None;

    let removals = recruitment_role_removals(&recruitment);

    assert_eq!(removals.len(), 3);
    assert!(removals.iter().all(|(user_id, _)| *user_id != 7));
}
