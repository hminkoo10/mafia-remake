// activity 테스트 모듈 (src/activity.rs에서 분리)

use super::*;

fn activity_test_running(mut game: MafiaGame) -> RunningGame {
    let initial_roles = game
        .players
        .iter()
        .map(|player| (player.user_id, player.role))
        .collect();
    let participant_user_ids = game.players.iter().map(|player| player.user_id).collect();
    game.phase = Phase::Day;
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
        night_notify: Arc::new(tokio::sync::Notify::new()),
        vote_notify: Arc::new(tokio::sync::Notify::new()),
        confirm_notify: Arc::new(tokio::sync::Notify::new()),
        day_notify: Arc::new(tokio::sync::Notify::new()),
        stats_recorded: false,
    }
}

fn activity_test_game() -> MafiaGame {
    MafiaGame::new(
        vec![
            (1, "p1".to_string()),
            (2, "p2".to_string()),
            (3, "p3".to_string()),
            (4, "p4".to_string()),
            (5, "p5".to_string()),
        ],
        1,
        0,
        0,
        vec![],
    )
    .unwrap()
}

#[test]
fn active_phase_deadline_is_unix_ms() {
    let deadline = Instant::now() + Duration::from_secs(30);
    let millis = phase_deadline_unix_ms(Some(deadline), Phase::Night).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    assert!(millis >= now);
}

#[test]
fn ended_phase_has_no_deadline() {
    let deadline = Instant::now() + Duration::from_secs(30);
    assert_eq!(phase_deadline_unix_ms(Some(deadline), Phase::Ended), None);
}

#[test]
fn activity_state_preserves_dead_players() {
    let mut game = activity_test_game();
    game.get_player_mut(2).unwrap().alive = false;
    let mut running = activity_test_running(game);

    let state = build_game_state(&mut running, 1, true);

    assert!(
        !state
            .players
            .iter()
            .find(|player| player.id == "2")
            .unwrap()
            .alive
    );
    assert!(
        state
            .players
            .iter()
            .find(|player| player.id == "1")
            .unwrap()
            .alive
    );
}

#[test]
fn dead_chat_requires_unlock_after_death() {
    let mut game = activity_test_game();
    game.get_player_mut(2).unwrap().alive = false;
    let mut running = activity_test_running(game);
    let player = running.game.get_player(2).unwrap().clone();

    assert!(!crate::channel::can_use_anonymous_dead_chat(
        &running, &player
    ));
    assert!(!crate::channel::can_use_anonymous_shaman_chat(
        &running, &player
    ));

    running.dead_chat_unlocked_ids.insert(player.user_id);

    assert!(crate::channel::can_use_anonymous_dead_chat(
        &running, &player
    ));
    assert!(crate::channel::can_use_anonymous_shaman_chat(
        &running, &player
    ));
}

#[test]
fn activity_vote_state_is_phase_scoped_and_counts_skip() {
    let mut running = activity_test_running(activity_test_game());
    running.game.phase = Phase::Vote;
    running.game.day_votes.insert(1, Some(2));
    running.game.day_votes.insert(2, None);

    let vote_state = build_game_state(&mut running, 1, true);
    assert_eq!(vote_state.vote_targets.get("2"), Some(&1));
    assert_eq!(vote_state.vote_skip_count, 1);

    running.game.phase = Phase::Night;
    let night_state = build_game_state(&mut running, 1, true);
    assert!(night_state.vote_targets.is_empty());
    assert_eq!(night_state.vote_skip_count, 0);
    assert_eq!(night_state.nominee, None);
}

#[test]
fn activity_role_names_round_trip() {
    let roles = [
        Role::Mafia,
        Role::Doctor,
        Role::Nurse,
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Inspector,
        Role::Reporter,
        Role::Hacker,
        Role::Detective,
        Role::Shaman,
        Role::Priest,
        Role::Soldier,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Hypnotist,
        Role::Mercenary,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Graverobber,
        Role::Godfather,
        Role::Joker,
        Role::Politician,
        Role::Judge,
        Role::Terrorist,
        Role::Lover,
        Role::CultLeader,
        Role::Fanatic,
        Role::Frog,
        Role::Villain,
        Role::Citizen,
    ];

    for role in roles {
        assert_eq!(role_from_str(&role_name(role)), Some(role));
    }
}

#[test]
fn activity_team_uses_game_team_rules() {
    let mut game = MafiaGame::new(
        vec![
            (1, "p1".to_string()),
            (2, "p2".to_string()),
            (3, "p3".to_string()),
        ],
        1,
        0,
        0,
        vec![],
    )
    .unwrap();

    for role in [
        Role::Mafia,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Madam,
        Role::Godfather,
        Role::Villain,
    ] {
        assert_eq!(player_team(&game, &Player::new(99, "test", role)), "Mafia");
    }
    for role in [
        Role::Gangster,
        Role::Fanatic,
        Role::Hypnotist,
        Role::Mercenary,
        Role::Citizen,
    ] {
        assert_eq!(
            player_team(&game, &Player::new(99, "test", role)),
            "Citizen"
        );
    }
    assert_eq!(
        player_team(&game, &Player::new(99, "test", Role::CultLeader)),
        "Cult"
    );
    assert_eq!(
        player_team(&game, &Player::new(99, "test", Role::Joker)),
        "Neutral"
    );
    let scientist = Player::new(99, "test", Role::Scientist);
    assert_eq!(player_team(&game, &scientist), "Mafia");
    game.scientist_contacted.insert(99);
    assert_eq!(player_team(&game, &scientist), "Mafia");

    game.culted_ids.insert(99);
    assert_eq!(
        player_team(&game, &Player::new(99, "test", Role::Thief)),
        "Mafia"
    );
    assert_eq!(
        player_team(&game, &Player::new(99, "test", Role::Fanatic)),
        "Cult"
    );
}
