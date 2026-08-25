// web_settings/api.rs — API 키 인증·게임/리플레이/전적 JSON API

use super::*;

pub(crate) fn request_api_key(request: &HttpRequest) -> Option<&str> {
    request
        .headers
        .get("x-api-key")
        .map(String::as_str)
        .or_else(|| {
            request
                .headers
                .get("authorization")
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) async fn authenticate_api_key(
    state: &WebSettingsState,
    request: &HttpRequest,
) -> std::result::Result<ApiKeyRecord, ApiAuthError> {
    let key = request_api_key(request).ok_or(ApiAuthError::Missing)?;
    let key_hash = api_key_hash(key);
    state
        .api_keys
        .read()
        .await
        .keys
        .iter()
        .find(|record| !record.revoked && record.key_hash == key_hash)
        .cloned()
        .ok_or(ApiAuthError::Invalid)
}

pub(crate) fn require_key_guild(
    record: &ApiKeyRecord,
    guild_id: u64,
) -> std::result::Result<(), ApiAuthError> {
    if record.guild_id == guild_id {
        Ok(())
    } else {
        Err(ApiAuthError::Forbidden)
    }
}

pub(crate) fn api_key_value(record: &ApiKeyRecord) -> Value {
    json!({
        "id": record.id,
        "label": record.label,
        "guild_id": record.guild_id,
        "created_at": record.created_at,
        "revoked": record.revoked,
    })
}

pub(crate) fn parse_api_guild_path<'a>(
    path: &'a str,
    prefix: &str,
) -> Option<(u64, Option<&'a str>)> {
    let rest = path.strip_prefix(prefix)?;
    let (guild_id, suffix) = rest
        .split_once('/')
        .map_or((rest, None), |(id, suffix)| (id, Some(suffix)));
    Some((guild_id.parse().ok()?, suffix))
}

pub(crate) async fn api_game_value(state: &WebSettingsState, guild_id: u64) -> Option<Value> {
    let running = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())?;
    let running = running.read().await;
    let mut players = running
        .game
        .players
        .iter()
        .map(|player| {
            json!({
                "user_id": player.user_id,
                "name": player.name,
                "alive": player.alive,
                "role": player.role.value(),
            })
        })
        .collect::<Vec<_>>();
    players.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    Some(json!({
        "guild_id": guild_id,
        "game_key": running.activity_game_key.clone(),
        "channel_id": running.channel_id.get(),
        "phase": running.game.phase.value(),
        "day_number": running.game.day_number,
        "participant_count": running.game.players.len(),
        "alive_count": running.game.alive_players().len(),
        "dead_count": running.game.dead_players().len(),
        "spectator_count": running.spectator_user_ids.len(),
        "anonymous_enabled": running.anonymous_enabled,
        "phase_remaining_seconds": running.phase_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()).as_secs()),
        "day_skip_votes": running.day_skip_voter_ids.len(),
        "day_skip_confirmed": running.day_skip_confirmed,
        "replay_event_count": running.replay_events.len(),
        "players": players,
    }))
}

pub(crate) async fn api_game_replay_value(
    state: &WebSettingsState,
    guild_id: u64,
) -> Option<Value> {
    if let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    {
        let running = running.read().await;
        let winner = running.game.winner();
        let status = if running.game.phase == Phase::Ended {
            "completed"
        } else {
            "active"
        };
        return Some(running.replay_snapshot(status, winner, &[]));
    }
    latest_completed_replay_for_guild(state, guild_id).await
}

pub(crate) async fn latest_completed_replay_for_guild(
    state: &WebSettingsState,
    guild_id: u64,
) -> Option<Value> {
    let completed_replays = state.completed_replays.read().await;
    completed_replays
        .iter()
        .find(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
}

pub(crate) async fn api_replay_summaries(state: &WebSettingsState, guild_id: u64) -> Value {
    let mut replays = Vec::new();
    if let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    {
        let running = running.read().await;
        replays.push(running.replay_summary("active", running.game.winner()));
    }
    {
        let completed_replays = state.completed_replays.read().await;
        replays.extend(
            completed_replays
                .iter()
                .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
                .map(|replay| {
                    let event_count = replay["events"]
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or_default();
                    let participant_count = replay["participants"]
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or_default();
                    json!({
                        "game_key": replay["game_key"].clone(),
                        "guild_id": replay["guild_id"].clone(),
                        "channel_id": replay["channel_id"].clone(),
                        "status": replay["status"].clone(),
                        "phase": replay["phase"].clone(),
                        "phase_key": replay["phase_key"].clone(),
                        "day_number": replay["day_number"].clone(),
                        "elapsed_seconds": replay["elapsed_seconds"].clone(),
                        "winner": replay["winner"].clone(),
                        "winner_key": replay["winner_key"].clone(),
                        "participant_count": participant_count,
                        "event_count": event_count,
                    })
                }),
        );
    }
    json!({"replays": replays})
}

pub(crate) async fn api_replay_by_key(
    state: &WebSettingsState,
    guild_id: u64,
    game_key: &str,
) -> Option<Value> {
    if let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    {
        let running = running.read().await;
        if running.activity_game_key == game_key {
            let winner = running.game.winner();
            let status = if running.game.phase == Phase::Ended {
                "completed"
            } else {
                "active"
            };
            return Some(running.replay_snapshot(status, winner, &[]));
        }
    }
    let completed_replays = state.completed_replays.read().await;
    completed_replays
        .iter()
        .find(|replay| {
            replay["guild_id"].as_u64() == Some(guild_id)
                && replay["game_key"].as_str() == Some(game_key)
        })
        .cloned()
}

pub(crate) fn json_page_params(
    query: &HashMap<String, String>,
    default_limit: usize,
) -> (usize, usize) {
    let page = query
        .get("page")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let per_page = query
        .get("per_page")
        .or_else(|| query.get("limit"))
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_limit)
        .min(100);
    (page, per_page)
}

pub(crate) fn slug_key(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

pub(crate) fn winner_slug(replay: &Value) -> Option<String> {
    replay["winner_key"].as_str().map(slug_key)
}

pub(crate) fn player_id_string(value: &Value) -> Option<String> {
    value
        .as_u64()
        .map(|id| id.to_string())
        .or_else(|| value.as_str().map(str::to_string))
}

pub(crate) fn participant_id(participant: &Value) -> Option<String> {
    player_id_string(&participant["user_id"])
}

pub(crate) fn participant_for_user<'a>(replay: &'a Value, user_id: &str) -> Option<&'a Value> {
    replay["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|participant| participant_id(participant).as_deref() == Some(user_id))
}

pub(crate) fn participant_nickname(participant: &Value) -> String {
    participant["name"]
        .as_str()
        .or_else(|| participant["nickname"].as_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn participant_role_slug(participant: &Value) -> String {
    participant["final_role_key"]
        .as_str()
        .or_else(|| participant["role_key"].as_str())
        .map(slug_key)
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn participant_survived(participant: &Value) -> bool {
    participant["alive"].as_bool().unwrap_or(false)
}

pub(crate) fn participant_revealed_role(replay: &Value, user_id: &str) -> Option<String> {
    participant_for_user(replay, user_id).map(participant_role_slug)
}

pub(crate) fn participant_won(participant: &Value, replay: &Value) -> bool {
    let Some(winner) = winner_slug(replay) else {
        return false;
    };
    participant["final_team"].as_str() == Some(winner.as_str())
}

pub(crate) fn death_info_for(replay: &Value, user_id: &str) -> (Option<u64>, Option<String>) {
    let Some(events) = replay["events"].as_array() else {
        return (None, None);
    };
    for event in events {
        let round = event["day_number"].as_u64();
        let details = &event["details"];
        match event["kind"].as_str().unwrap_or_default() {
            "confirmation_vote_resolved" => {
                if player_id_string(&details["executed_user_id"]).as_deref() == Some(user_id) {
                    return (round, Some("execution".to_string()));
                }
                if details["extra_killed_user_ids"]
                    .as_array()
                    .is_some_and(|ids| {
                        ids.iter()
                            .any(|id| player_id_string(id).as_deref() == Some(user_id))
                    })
                {
                    return (round, Some("other".to_string()));
                }
            }
            "night_resolved" => {
                let killed = details["killed_user_ids"].as_array().is_some_and(|ids| {
                    ids.iter()
                        .any(|id| player_id_string(id).as_deref() == Some(user_id))
                });
                if !killed {
                    continue;
                }
                let list_has = |name: &str| {
                    details[name].as_array().is_some_and(|ids| {
                        ids.iter()
                            .any(|id| player_id_string(id).as_deref() == Some(user_id))
                    })
                };
                let cause = if player_id_string(&details["mafia_target_user_id"]).as_deref()
                    == Some(user_id)
                {
                    "mafia_kill"
                } else if list_has("contractor_kill_user_ids") {
                    "contractor_kill"
                } else if list_has("vigilante_kill_user_ids") {
                    "vigilante_kill"
                } else if list_has("mercenary_kill_user_ids") {
                    "mercenary_kill"
                } else {
                    "other"
                };
                return (round, Some(cause.to_string()));
            }
            _ => {}
        }
    }
    (None, None)
}

pub(crate) fn compatible_game_summary(replay: &Value) -> Value {
    let participants = replay["participants"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    json!({
        "game_id": replay["game_key"].clone(),
        "started_at": replay["started_at"].clone(),
        "ended_at": replay["ended_at"].clone(),
        "player_count": participants.len(),
        "winner": winner_slug(replay),
        "rounds": replay["day_number"].clone(),
    })
}

pub(crate) fn compatible_game_detail(replay: &Value) -> Value {
    let players = replay["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|participant| {
            json!({
                "user_id": participant_id(participant),
                "nickname": participant_nickname(participant),
            })
        })
        .collect::<Vec<_>>();
    let mut value = compatible_game_summary(replay);
    if let Some(object) = value.as_object_mut() {
        object.insert("players".to_string(), Value::Array(players));
    }
    value
}

pub(crate) fn compatible_game_result(replay: &Value) -> Value {
    let players = replay["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|participant| {
            let user_id = participant_id(participant).unwrap_or_default();
            let (died_at_round, cause_of_death) = death_info_for(replay, &user_id);
            json!({
                "user_id": user_id,
                "nickname": participant_nickname(participant),
                "role": participant_role_slug(participant),
                "role_name": participant["final_role"].clone(),
                "survived": participant_survived(participant),
                "died_at_round": died_at_round,
                "cause_of_death": cause_of_death,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "game_id": replay["game_key"].clone(),
        "winner": winner_slug(replay),
        "ended_at": replay["ended_at"].clone(),
        "total_rounds": replay["day_number"].clone(),
        "players": players,
    })
}

pub(crate) fn compatible_event_type(kind: &str) -> String {
    match kind {
        "game_started" => "game_start".to_string(),
        "phase_started" => "phase_change".to_string(),
        "day_vote" | "confirmation_vote" | "day_skip_vote" | "day_extension_vote" => {
            "vote".to_string()
        }
        "night_action"
        | "contractor_contract"
        | "hacker_action"
        | "vigilante_investigation"
        | "psychologist_observation"
        | "hypnotist_wake" => "role_action".to_string(),
        "game_ended" => "game_end".to_string(),
        _ => kind.to_string(),
    }
}

pub(crate) fn compatible_events(replay: &Value) -> Value {
    let mut events = Vec::new();
    for event in replay["events"].as_array().into_iter().flatten() {
        let kind = event["kind"].as_str().unwrap_or_default();
        let event_id = event["id"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("e_{:06}", event["seq"].as_u64().unwrap_or_default()));
        let actor_id = event["actor"]["user_id"].as_u64().map(|id| id.to_string());
        let target_id = event["target_user_ids"]
            .as_array()
            .and_then(|ids| ids.first())
            .and_then(player_id_string);
        events.push(json!({
            "id": event_id,
            "timestamp": event["timestamp"].clone(),
            "round": event["day_number"].clone(),
            "type": compatible_event_type(kind),
            "actor_id": actor_id,
            "target_id": target_id,
            "payload": event["details"].clone(),
        }));

        if kind == "night_resolved" {
            for target in event["details"]["killed_user_ids"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(target_id) = player_id_string(target) else {
                    continue;
                };
                let (_, cause) = death_info_for(replay, &target_id);
                let role_revealed = participant_revealed_role(replay, &target_id);
                events.push(json!({
                    "id": format!("{event_id}_death_{target_id}"),
                    "timestamp": event["timestamp"].clone(),
                    "round": event["day_number"].clone(),
                    "type": "death",
                    "actor_id": Value::Null,
                    "target_id": target_id,
                    "payload": {
                        "cause": cause.unwrap_or_else(|| "other".to_string()),
                        "role_revealed": role_revealed,
                    },
                }));
            }
        } else if kind == "confirmation_vote_resolved" {
            if let Some(target_id) = player_id_string(&event["details"]["executed_user_id"]) {
                let role_revealed = participant_revealed_role(replay, &target_id);
                let vote_count = event["details"]["vote_counts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|count| count["approve"].as_bool() == Some(true))
                    .and_then(|count| count["count"].as_i64());
                events.push(json!({
                    "id": format!("{event_id}_death_{target_id}"),
                    "timestamp": event["timestamp"].clone(),
                    "round": event["day_number"].clone(),
                    "type": "death",
                    "actor_id": Value::Null,
                    "target_id": target_id,
                    "payload": {
                        "cause": "execution",
                        "role_revealed": role_revealed,
                        "vote_count": vote_count,
                    },
                }));
            }
        }
    }
    json!({
        "game_id": replay["game_key"].clone(),
        "events": events,
    })
}

pub(crate) async fn api_recent_games_value(
    state: &WebSettingsState,
    guild_id: u64,
    query: &HashMap<String, String>,
) -> Value {
    let (page, per_page) = json_page_params(query, 10);
    let completed_replays = state.completed_replays.read().await;
    let all = completed_replays
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .map(compatible_game_summary)
        .collect::<Vec<_>>();
    let total = all.len();
    let start = per_page.saturating_mul(page.saturating_sub(1));
    let data = all
        .into_iter()
        .skip(start)
        .take(per_page)
        .collect::<Vec<_>>();
    json!({
        "data": data,
        "total": total,
        "current_page": page,
        "per_page": per_page,
    })
}

pub(crate) async fn replay_for_compatible_game(
    state: &WebSettingsState,
    guild_id: u64,
    game_key: &str,
) -> Option<Value> {
    api_replay_by_key(state, guild_id, game_key).await
}

pub(crate) async fn api_recruitment_value(
    state: &WebSettingsState,
    guild_id: u64,
) -> Option<Value> {
    let recruitment = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())?;
    let recruitment = recruitment.read().await;
    let mut participants = recruitment
        .joined_ids
        .iter()
        .map(|user_id| {
            json!({
                "user_id": user_id,
                "name": recruitment.joined_names.get(user_id).cloned().unwrap_or_else(|| user_id.to_string()),
            })
        })
        .collect::<Vec<_>>();
    participants.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    let mut spectators = recruitment
        .spectator_ids
        .iter()
        .map(|user_id| {
            json!({
                "user_id": user_id,
                "name": recruitment.spectator_names.get(user_id).cloned().unwrap_or_else(|| user_id.to_string()),
            })
        })
        .collect::<Vec<_>>();
    spectators.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    let mut role_counts = recruitment
        .role_counts
        .iter()
        .map(|(role, count)| json!({"role": role.value(), "count": count}))
        .collect::<Vec<_>>();
    role_counts.sort_by_key(|item| item["role"].as_str().unwrap_or_default().to_string());
    Some(json!({
        "guild_id": guild_id,
        "host_user_id": recruitment.host_user_id.get(),
        "accepting": recruitment.accepting,
        "cancelled": recruitment.cancelled,
        "minimum_players": recruitment.minimum_players,
        "max_players": recruitment.max_players,
        "participant_count": participants.len(),
        "spectator_count": spectators.len(),
        "participants": participants,
        "spectators": spectators,
        "role_counts": role_counts,
        "special_roles": recruitment.special_roles.iter().map(|role| role.value()).collect::<Vec<_>>(),
    }))
}

pub(crate) async fn control_game(
    state: &WebSettingsState,
    guild_id: u64,
    action: &str,
) -> std::result::Result<Value, String> {
    let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("game not found".to_string());
    };
    let notifications = {
        let mut running = running.write().await;
        match action {
            "stop" => {
                if running.game.phase == Phase::Ended {
                    return Err("game is already ending".to_string());
                }
                running.game.phase = Phase::Ended;
                running.phase_deadline = None;
                vec![
                    running.night_notify.clone(),
                    running.vote_notify.clone(),
                    running.confirm_notify.clone(),
                    running.day_notify.clone(),
                ]
            }
            "skip_day" => {
                if running.game.phase != Phase::Day {
                    return Err("skip_day is only available during day discussion".to_string());
                }
                running.day_skip_confirmed = true;
                running.day_extension_active = false;
                vec![running.day_notify.clone()]
            }
            "extend_day" => {
                if running.game.phase != Phase::Day || !running.day_extension_active {
                    return Err(
                        "extend_day is only available during the day extension vote".to_string()
                    );
                }
                running.day_extension_confirmed = true;
                vec![running.day_notify.clone()]
            }
            _ => return Err("unsupported game action".to_string()),
        }
    };
    for notify in notifications {
        notify.notify_waiters();
    }
    Ok(json!({"ok": true, "guild_id": guild_id, "action": action}))
}

pub(crate) async fn cancel_recruitment(
    state: &WebSettingsState,
    guild_id: u64,
) -> std::result::Result<Value, String> {
    let Some(recruitment) = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("recruitment not found".to_string());
    };
    let notify = {
        let mut recruitment = recruitment.write().await;
        if !recruitment.accepting {
            return Err("recruitment is no longer accepting players".to_string());
        }
        recruitment.cancelled = true;
        recruitment.accepting = false;
        recruitment.done.clone()
    };
    notify.notify_waiters();
    Ok(json!({"ok": true, "guild_id": guild_id, "action": "cancel"}))
}

pub(crate) async fn start_recruitment(
    state: &WebSettingsState,
    guild_id: u64,
) -> std::result::Result<Value, String> {
    let Some(recruitment) = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("recruitment not found".to_string());
    };
    let notify = {
        let mut recruitment = recruitment.write().await;
        if !recruitment.accepting {
            return Err("recruitment is no longer accepting players".to_string());
        }
        if recruitment.joined_ids.len() < recruitment.minimum_players {
            return Err("not enough players to start".to_string());
        }
        recruitment.accepting = false;
        recruitment.done.clone()
    };
    notify.notify_waiters();
    Ok(json!({"ok": true, "guild_id": guild_id, "action": "start"}))
}

pub(crate) async fn route_protected_api_request(
    state: &WebSettingsState,
    request: &HttpRequest,
    path: &str,
    query: &str,
) -> Option<String> {
    let compatible_api_path = path == "/games/recent"
        || path == "/stats/leaderboard"
        || path.starts_with("/game/")
        || path.starts_with("/stats/user/");
    if !path.starts_with("/api/v1/") && !compatible_api_path {
        return None;
    }
    let key = match authenticate_api_key(state, request).await {
        Ok(key) => key,
        Err(error) => return Some(error.response()),
    };
    let query = parse_urlencoded(query);
    let response = match (request.method.as_str(), path) {
        ("GET", "/api/v1/me") => json_response(json!({"key": api_key_value(&key)})),
        ("GET", "/api/v1/config") => {
            let status = web_status_values(state).await;
            json_response(json!({"settings": status["settings"].clone()}))
        }
        ("GET", "/api/v1/stats") => json_response(web_stats_summary(state).await),
        ("GET", "/api/v1/stats/leaderboard") | ("GET", "/stats/leaderboard") => {
            json_response(compatible_leaderboard_values(state, key.guild_id, &query).await)
        }
        ("GET", "/api/v1/games") => {
            let games = api_game_value(state, key.guild_id)
                .await
                .into_iter()
                .collect::<Vec<_>>();
            json_response(json!({"games": games}))
        }
        ("GET", "/api/v1/games/recent") | ("GET", "/games/recent") => {
            json_response(api_recent_games_value(state, key.guild_id, &query).await)
        }
        ("GET", "/api/v1/replays") => {
            json_response(api_replay_summaries(state, key.guild_id).await)
        }
        ("GET", "/api/v1/leaderboard") => {
            let limit = query
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10);
            json_response(web_leaderboard_values(state, "rating", limit).await)
        }
        _ => {
            if let Some(user_path) = path
                .strip_prefix("/api/v1/stats/user/")
                .or_else(|| path.strip_prefix("/stats/user/"))
            {
                if let Some(user_id) = user_path.strip_suffix("/games") {
                    if request.method == "GET" {
                        json_response(
                            compatible_user_games_value(state, key.guild_id, user_id, &query).await,
                        )
                    } else {
                        json_error("404 Not Found", "API endpoint not found")
                    }
                } else if request.method == "GET" {
                    compatible_user_stats_value(state, key.guild_id, user_path)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "user not found"))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some(game_path) = path
                .strip_prefix("/api/v1/game/")
                .or_else(|| path.strip_prefix("/game/"))
            {
                let (game_key, suffix) = game_path
                    .split_once('/')
                    .map_or((game_path, None), |(game_key, suffix)| {
                        (game_key, Some(suffix))
                    });
                if request.method != "GET" {
                    json_error("404 Not Found", "API endpoint not found")
                } else if let Some(replay) =
                    replay_for_compatible_game(state, key.guild_id, game_key).await
                {
                    match suffix {
                        None => json_response(compatible_game_detail(&replay)),
                        Some("result") => json_response(compatible_game_result(&replay)),
                        Some("events") => json_response(compatible_events(&replay)),
                        _ => json_error("404 Not Found", "API endpoint not found"),
                    }
                } else {
                    json_error("404 Not Found", "game not found")
                }
            } else if let Some(metric) = path.strip_prefix("/api/v1/leaderboard/") {
                if !WEB_LEADERBOARD_METRICS.contains(&metric) {
                    json_error("400 Bad Request", "unsupported leaderboard metric")
                } else {
                    let limit = query
                        .get("limit")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(10);
                    json_response(web_leaderboard_values(state, metric, limit).await)
                }
            } else if let Some(game_key) = path.strip_prefix("/api/v1/replays/") {
                if request.method == "GET" {
                    api_replay_by_key(state, key.guild_id, game_key)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "replay not found"))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some((guild_id, suffix)) = parse_api_guild_path(path, "/api/v1/games/") {
                if let Err(error) = require_key_guild(&key, guild_id) {
                    error.response()
                } else if suffix.is_none() && request.method == "GET" {
                    api_game_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "game not found"))
                } else if suffix == Some("replay") && request.method == "GET" {
                    api_game_replay_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "replay not found"))
                } else if suffix == Some("actions") && request.method == "POST" {
                    let action =
                        serde_json::from_str::<Value>(&request.body)
                            .ok()
                            .and_then(|body| {
                                body.get("action")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            });
                    let Some(action) = action else {
                        return Some(json_error("400 Bad Request", "JSON body requires action"));
                    };
                    control_game(state, guild_id, &action)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|message| json_error("409 Conflict", &message))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some((guild_id, suffix)) =
                parse_api_guild_path(path, "/api/v1/recruitments/")
            {
                if let Err(error) = require_key_guild(&key, guild_id) {
                    error.response()
                } else if suffix.is_none() && request.method == "GET" {
                    api_recruitment_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "recruitment not found"))
                } else if suffix == Some("actions") && request.method == "POST" {
                    let action =
                        serde_json::from_str::<Value>(&request.body)
                            .ok()
                            .and_then(|body| {
                                body.get("action")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            });
                    match action.as_deref() {
                        Some("cancel") => cancel_recruitment(state, guild_id)
                            .await
                            .map(json_response)
                            .unwrap_or_else(|message| json_error("409 Conflict", &message)),
                        Some("start") => start_recruitment(state, guild_id)
                            .await
                            .map(json_response)
                            .unwrap_or_else(|message| json_error("409 Conflict", &message)),
                        _ => json_error(
                            "400 Bad Request",
                            "supported recruitment actions: start, cancel",
                        ),
                    }
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else {
                json_error("404 Not Found", "API endpoint not found")
            }
        }
    };
    Some(response)
}

pub(crate) async fn route_public_request(
    state: &WebSettingsState,
    path: &str,
    query: &str,
) -> Option<String> {
    let query = parse_urlencoded(query);
    match path {
        "/" => {
            let status = web_status_values(state).await;
            let leaderboard = web_leaderboard_values(state, "rating", 3).await;
            let stats = web_stats_summary(state).await;
            Some(http_response(
                "200 OK",
                &render_home_page(&status, &leaderboard, &stats),
            ))
        }
        "/status" => {
            let status = web_status_values(state).await;
            Some(http_response("200 OK", &render_status_page(&status)))
        }
        "/leaderboard" => {
            let metric = query.get("metric").map(String::as_str).unwrap_or("rating");
            let leaderboard = web_leaderboard_values(state, metric, 20).await;
            let stats = web_stats_summary(state).await;
            Some(http_response(
                "200 OK",
                &render_leaderboard_page(&leaderboard, &stats),
            ))
        }
        "/rating" => Some(http_response("200 OK", &render_rating_page())),
        "/roles" => Some(http_response("200 OK", &render_roles_page())),
        "/tiers" => Some(http_response("200 OK", &render_tiers_page())),
        "/api" | "/api/docs" => Some(http_response(
            "200 OK",
            &render_api_docs_page(&state.base_url),
        )),
        "/health" => Some(json_response(
            json!({"ok": true, "service": "mafia-discord-bot"}),
        )),
        "/api/status" => Some(json_response(web_status_values(state).await)),
        "/api/games" => {
            let status = web_status_values(state).await;
            Some(json_response(json!({"games": status["games"].clone()})))
        }
        "/api/settings" => {
            let status = web_status_values(state).await;
            Some(json_response(
                json!({"settings": status["settings"].clone()}),
            ))
        }
        "/api/stats" => Some(json_response(web_stats_summary(state).await)),
        "/api/leaderboard" => {
            let limit = query
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10);
            Some(json_response(
                web_leaderboard_values(state, "rating", limit).await,
            ))
        }
        _ => {
            if let Some(metric) = path.strip_prefix("/api/leaderboard/") {
                let limit = query
                    .get("limit")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(10);
                Some(json_response(
                    web_leaderboard_values(state, metric, limit).await,
                ))
            } else {
                None
            }
        }
    }
}

pub(crate) fn valid_api_key_label(value: &str) -> std::result::Result<String, String> {
    let label = value.trim();
    if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
        return Err("API 키 이름은 제어 문자 없이 1~64자여야 합니다.".to_string());
    }
    Ok(label.to_string())
}

pub(crate) fn api_key_records_for_guild(store: &ApiKeyStore, guild_id: u64) -> Vec<ApiKeyRecord> {
    let mut records = store
        .keys
        .iter()
        .filter(|record| record.guild_id == guild_id)
        .cloned()
        .collect::<Vec<_>>();
    records.sort_by_key(|record| std::cmp::Reverse(record.created_at.clone()));
    records
}

pub(crate) async fn route_api_key_management(
    state: &WebSettingsState,
    session: &WebSettingsSession,
    token: &str,
    request: &HttpRequest,
) -> String {
    let action = format!("{WEB_SETTINGS_PATH}/{token}/api-keys");
    match request.method.as_str() {
        "GET" => {
            let store = state.api_keys.read().await;
            let records = api_key_records_for_guild(&store, session.guild_id);
            http_response(
                "200 OK",
                &render_api_key_page(session, &action, &records, None, None),
            )
        }
        "POST" => {
            let form = parse_urlencoded(&request.body);
            let result = match form.get("action").map(String::as_str) {
                Some("create") => {
                    let label = form
                        .get("label")
                        .ok_or_else(|| "API 키 이름을 입력하세요.".to_string())
                        .and_then(|value| valid_api_key_label(value));
                    let label = match label {
                        Ok(label) => label,
                        Err(error) => {
                            return api_key_management_error(state, session, &action, error).await;
                        }
                    };
                    let mut store = state.api_keys.write().await;
                    let previous = store.clone();
                    let key = issue_api_key(&mut store, session.guild_id, session.user_id, label);
                    if let Err(error) = save_api_key_store(&*state.api_keys_path, &store) {
                        *store = previous;
                        let error = error.to_string();
                        drop(store);
                        return api_key_management_error(state, session, &action, error).await;
                    }
                    Ok(Some(key))
                }
                Some("revoke") => {
                    let Some(key_id) = form.get("key_id") else {
                        return api_key_management_error(
                            state,
                            session,
                            &action,
                            "폐기할 API 키를 선택하세요.".to_string(),
                        )
                        .await;
                    };
                    let mut store = state.api_keys.write().await;
                    let previous = store.clone();
                    let Some(record) = store
                        .keys
                        .iter_mut()
                        .find(|record| record.id == *key_id && record.guild_id == session.guild_id)
                    else {
                        drop(store);
                        return api_key_management_error(
                            state,
                            session,
                            &action,
                            "API 키를 찾을 수 없습니다.".to_string(),
                        )
                        .await;
                    };
                    record.revoked = true;
                    if let Err(error) = save_api_key_store(&*state.api_keys_path, &store) {
                        *store = previous;
                        let error = error.to_string();
                        drop(store);
                        return api_key_management_error(state, session, &action, error).await;
                    }
                    Ok(None)
                }
                _ => Err("지원하지 않는 API 키 작업입니다.".to_string()),
            };
            match result {
                Ok(issued_key) => {
                    let store = state.api_keys.read().await;
                    let records = api_key_records_for_guild(&store, session.guild_id);
                    http_response(
                        "200 OK",
                        &render_api_key_page(
                            session,
                            &action,
                            &records,
                            issued_key.as_deref(),
                            None,
                        ),
                    )
                }
                Err(error) => api_key_management_error(state, session, &action, error).await,
            }
        }
        _ => json_error("405 Method Not Allowed", "GET or POST is required"),
    }
}

pub(crate) async fn api_key_management_error(
    state: &WebSettingsState,
    session: &WebSettingsSession,
    action: &str,
    error: String,
) -> String {
    let store = state.api_keys.read().await;
    let records = api_key_records_for_guild(&store, session.guild_id);
    http_response(
        "400 Bad Request",
        &render_api_key_page(session, action, &records, None, Some(&error)),
    )
}

pub(crate) async fn web_status_values(state: &WebSettingsState) -> Value {
    let now = Instant::now();
    let config = state.config.read().await.clone();
    let mut games = Vec::new();
    for entry in state.games.iter() {
        let guild_id = entry.key().get();
        let running = entry.value().read().await;
        let alive_count = running.game.alive_players().len();
        let dead_count = running.game.dead_players().len();
        games.push(json!({
            "guild_id": guild_id,
            "guild_name": guild_id.to_string(),
            "channel_id": running.channel_id.get(),
            "channel_name": format!("#{}", running.channel_id.get()),
            "phase": running.game.phase.value(),
            "day": format!("{}일차", running.game.day_number),
            "participant_count": running.game.players.len(),
            "alive_count": alive_count,
            "dead_count": dead_count,
            "spectator_count": running.spectator_user_ids.len(),
            "anonymous_enabled": running.anonymous_enabled,
            "elapsed": stats::play_duration_text(running.started_at.elapsed().as_secs() as i64),
        }));
    }
    games.sort_by_key(|item| {
        item.get("guild_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });
    json!({
        "bot": {
            "ready": true,
            "name": state.bot_name,
            "latency_ms": 0,
            "guild_count": state.guild_count,
            "user_count": 0,
            "uptime": stats::play_duration_text(now.duration_since(state.started_at).as_secs() as i64),
        },
        "api": {
            "base_url": format!("{}/api", state.base_url.trim_end_matches('/')),
        },
        "games": games,
        "recruiting_guild_count": state.recruitments.len(),
        "settings": {
            "game_enabled": config.game_enabled,
            "max_player_count_text": if config.max_player_count == 0 {
                "제한 없음".to_string()
            } else {
                format!("{}명", config.max_player_count)
            },
            "role_summary": format!(
                "마피아 {}명, 의사 {}, 수사직 {}",
                config.default_mafia_count,
                if config.default_doctor_count > 0 { "활성화" } else { "비활성화" },
                if config.default_police_count > 0 { "활성화" } else { "비활성화" }
            ),
            "special_summary": format!(
                "시민 {}개, 마피아 {}개, 중립 {}개",
                config.citizen_special_count, config.mafia_special_count, config.neutral_special_count
            ),
            "anonymous_mode_text": if config.anonymous_mode {
                format!("켜짐 ({})", match config.anonymous_name_mode.as_str() {
                    "number" => "숫자",
                    _ => "동물",
                })
            } else {
                "꺼짐".to_string()
            },
            "slowmode_text": format!("{}초", config.chat_slowmode_seconds),
            "cult_team_text": if config.enable_cult_team { "켜짐" } else { "꺼짐" },
        }
    })
}

pub(crate) async fn web_stats_summary(state: &WebSettingsState) -> Value {
    let entries = {
        let stats_read = state.stats.read().await;
        stats_read.users.values().cloned().collect::<Vec<_>>()
    };
    let played_entries = entries
        .iter()
        .filter(|entry| entry.games > 0)
        .collect::<Vec<_>>();
    let total_player_games = played_entries.iter().map(|entry| entry.games).sum::<i64>();
    let total_wins = played_entries.iter().map(|entry| entry.wins).sum::<i64>();
    let total_play_seconds = played_entries
        .iter()
        .map(|entry| entry.play_seconds)
        .sum::<i64>();
    let average_rating = if played_entries.is_empty() {
        stats::INITIAL_RATING
    } else {
        (played_entries.iter().map(|entry| entry.rating).sum::<i64>() as f64
            / played_entries.len() as f64)
            .round() as i64
    };
    json!({
        "registered_users": entries.len(),
        "recorded_players": played_entries.len(),
        "total_player_games": total_player_games,
        "total_wins": total_wins,
        "total_playtime": stats::play_duration_text(total_play_seconds),
        "total_play_seconds": total_play_seconds,
        "average_rating": average_rating,
    })
}

pub(crate) async fn web_leaderboard_values(
    state: &WebSettingsState,
    metric: &str,
    limit: usize,
) -> Value {
    let metric = if WEB_LEADERBOARD_METRICS.contains(&metric) {
        metric
    } else {
        "rating"
    };
    let safe_limit = limit.clamp(1, 50);
    let stats_read = {
        let stats_read = state.stats.read().await;
        stats_read.clone()
    };
    let entries = stats::leaderboard_entries(&stats_read, metric, safe_limit)
        .into_iter()
        .enumerate()
        .map(|(index, (user_id, entry))| {
            let winrate = if entry.games > 0 {
                ((entry.wins as f64 / entry.games as f64 * 1000.0).round()) / 10.0
            } else {
                0.0
            };
            json!({
                "rank": index + 1,
                "user_id": user_id,
                "name": if entry.name.is_empty() { "알 수 없음".to_string() } else { entry.name.clone() },
                "games": entry.games,
                "wins": entry.wins,
                "losses": entry.losses,
                "win_streak": entry.win_streak,
                "best_win_streak": entry.best_win_streak,
                "streak_text": format!("{}연승", entry.win_streak),
                "best_streak_text": format!("{}연승", entry.best_win_streak),
                "winrate": winrate,
                "winrate_text": stats::win_rate_text(entry.wins, entry.games),
                "mafia_team_games": entry.mafia_team_games,
                "play_seconds": entry.play_seconds,
                "playtime": stats::play_duration_text(entry.play_seconds),
                "rating": entry.rating,
                "rating_rank": stats::rating_rank(&stats_read, entry.rating, entry.rating_games),
                "rating_peak": entry.rating_peak,
                "rating_peak_rank": stats::rating_rank(&stats_read, entry.rating_peak, entry.rating_games),
                "rating_games": entry.rating_games,
                "value": stats::leaderboard_value(&entry, metric),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "metric": metric,
        "metric_name": stats::leaderboard_metric_name(metric),
        "metrics": WEB_LEADERBOARD_METRICS
            .iter()
            .map(|key| json!({"key": key, "name": stats::leaderboard_metric_name(key)}))
            .collect::<Vec<_>>(),
        "limit": safe_limit,
        "entries": entries,
    })
}

pub(crate) fn most_played_role(entry: &stats::PlayerStats) -> Option<String> {
    entry
        .roles
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(role, _)| role.clone())
}

pub(crate) fn compatible_user_game_rows(replays: &[Value], user_id: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    for replay in replays {
        let Some(participant) = replay["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|participant| participant_id(participant).as_deref() == Some(user_id))
        else {
            continue;
        };
        rows.push(json!({
            "game_id": replay["game_key"].clone(),
            "ended_at": replay["ended_at"].clone(),
            "role": participant_role_slug(participant),
            "role_name": participant["final_role"].clone(),
            "result": if participant_won(participant, replay) { "win" } else { "loss" },
            "survived": participant_survived(participant),
        }));
    }
    rows
}

pub(crate) fn compatible_user_recent_counts(
    replays: &[Value],
    user_id: &str,
) -> (
    i64,
    HashMap<String, i64>,
    HashMap<String, f64>,
    i64,
    i64,
    i64,
) {
    let mut survived = 0;
    let mut role_counts: HashMap<String, i64> = HashMap::new();
    let mut role_wins: HashMap<String, i64> = HashMap::new();
    let mut role_games: HashMap<String, i64> = HashMap::new();
    let mut executed = 0;
    let mut killed_by_mafia = 0;
    for replay in replays {
        let Some(participant) = replay["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|participant| participant_id(participant).as_deref() == Some(user_id))
        else {
            continue;
        };
        let role = participant_role_slug(participant);
        *role_counts.entry(role.clone()).or_default() += 1;
        *role_games.entry(role.clone()).or_default() += 1;
        if participant_won(participant, replay) {
            *role_wins.entry(role).or_default() += 1;
        }
        if participant_survived(participant) {
            survived += 1;
        } else {
            let (_, cause) = death_info_for(replay, user_id);
            match cause.as_deref() {
                Some("execution") => executed += 1,
                Some("mafia_kill") => killed_by_mafia += 1,
                _ => {}
            }
        }
    }
    let win_rate_by_role = role_games
        .iter()
        .map(|(role, games)| {
            let wins = role_wins.get(role).copied().unwrap_or(0);
            let rate = if *games > 0 {
                ((wins as f64 / *games as f64) * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            (role.clone(), rate)
        })
        .collect::<HashMap<_, _>>();
    (
        survived,
        role_counts,
        win_rate_by_role,
        executed,
        killed_by_mafia,
        0,
    )
}

pub(crate) async fn compatible_leaderboard_values(
    state: &WebSettingsState,
    guild_id: u64,
    query: &HashMap<String, String>,
) -> Value {
    let sort = query.get("sort").map(String::as_str).unwrap_or("rating");
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 100);
    let metric = match sort {
        "winrate" => "winrate",
        "games" => "games",
        "wins" => "wins",
        "kills" => "wins",
        "rating" => "rating",
        _ => "rating",
    };
    let stats_read = state.stats.read().await.clone();
    let replays = state
        .completed_replays
        .read()
        .await
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
        .collect::<Vec<_>>();
    let data = stats::leaderboard_entries(&stats_read, metric, limit)
        .into_iter()
        .map(|(user_id, entry)| {
            let (_, _, _, executed, killed_by_mafia, kills) =
                compatible_user_recent_counts(&replays, &user_id);
            let win_rate = if entry.games > 0 {
                ((entry.wins as f64 / entry.games as f64) * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            json!({
                "user_id": user_id,
                "nickname": entry.name,
                "games_played": entry.games,
                "wins": entry.wins,
                "losses": entry.losses,
                "win_rate": win_rate,
                "rating": entry.rating,
                "rating_rank": stats::rating_rank(&stats_read, entry.rating, entry.rating_games),
                "win_streak": entry.win_streak,
                "best_win_streak": entry.best_win_streak,
                "most_played_role": most_played_role(&entry),
                "times_executed": executed,
                "times_killed_by_mafia": killed_by_mafia,
                "kills": kills,
                "most_frequent_killer": Value::Null,
            })
        })
        .collect::<Vec<_>>();
    json!({"data": data})
}

pub(crate) async fn compatible_user_stats_value(
    state: &WebSettingsState,
    guild_id: u64,
    user_id: &str,
) -> Option<Value> {
    let stats_read = state.stats.read().await.clone();
    let entry = stats_read.users.get(user_id).cloned()?;
    let replays = state
        .completed_replays
        .read()
        .await
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
        .collect::<Vec<_>>();
    let (survived, recent_role_counts, win_rate_by_role, executed, killed_by_mafia, kills) =
        compatible_user_recent_counts(&replays, user_id);
    let role_play_count = if recent_role_counts.is_empty() {
        entry.roles.clone()
    } else {
        recent_role_counts
    };
    let win_rate = if entry.games > 0 {
        ((entry.wins as f64 / entry.games as f64) * 1000.0).round() / 1000.0
    } else {
        0.0
    };
    Some(json!({
        "user_id": user_id,
        "nickname": entry.name,
        "total_games": entry.games,
        "wins": entry.wins,
        "losses": entry.losses,
        "win_rate": win_rate,
        "rating": entry.rating,
        "rating_rank": stats::rating_rank(&stats_read, entry.rating, entry.rating_games),
        "win_streak": entry.win_streak,
        "best_win_streak": entry.best_win_streak,
        "win_rate_by_role": win_rate_by_role,
        "role_play_count": role_play_count,
        "most_killed_by": Value::Null,
        "most_killed": Value::Null,
        "kills": kills,
        "times_executed": executed,
        "times_killed_by_mafia": killed_by_mafia,
        "times_survived": survived,
    }))
}

pub(crate) async fn compatible_user_games_value(
    state: &WebSettingsState,
    guild_id: u64,
    user_id: &str,
    query: &HashMap<String, String>,
) -> Value {
    let (page, per_page) = json_page_params(query, 20);
    let replays = state
        .completed_replays
        .read()
        .await
        .iter()
        .filter(|replay| replay["guild_id"].as_u64() == Some(guild_id))
        .cloned()
        .collect::<Vec<_>>();
    let rows = compatible_user_game_rows(&replays, user_id);
    let total = rows.len();
    let start = per_page.saturating_mul(page.saturating_sub(1));
    let data = rows
        .into_iter()
        .skip(start)
        .take(per_page)
        .collect::<Vec<_>>();
    json!({
        "data": data,
        "total": total,
        "current_page": page,
        "per_page": per_page,
    })
}
