// channel/roles_setup.rs — 특수 역할 선정·인원 계산·설정 텍스트

use super::*;

pub fn is_blacklisted(config: &config::BotConfig, user_id: u64) -> bool {
    config.blacklist_user_ids.contains(&user_id)
}

pub fn enabled_special_roles(config: &config::BotConfig, pool: &[Role]) -> Vec<Role> {
    pool.iter()
        .copied()
        .filter(|role| match role {
            Role::Inspector => config.enable_inspector,
            Role::Detective => config.enable_detective,
            Role::Shaman => config.enable_shaman,
            Role::Graverobber => config.enable_graverobber,
            Role::Spy => config.enable_spy,
            Role::Contractor => config.enable_contractor,
            Role::Fraudster => config.enable_fraudster,
            Role::Witch => config.enable_witch,
            Role::Scientist => config.enable_scientist,
            Role::Madam => config.enable_madam,
            Role::Godfather => config.enable_godfather,
            Role::Joker => config.enable_joker,
            Role::Politician => config.enable_politician,
            Role::Judge => config.enable_judge,
            Role::Reporter => config.enable_reporter,
            Role::Hacker => config.enable_hacker,
            Role::Terrorist => config.enable_terrorist,
            Role::Lover => config.enable_lover,
            Role::CivilServant => config.enable_civil_servant,
            Role::Paparazzi => config.enable_paparazzi,
            Role::Priest => config.enable_priest,
            Role::Soldier => config.enable_soldier,
            Role::Nurse => config.enable_nurse,
            Role::Gangster => config.enable_gangster,
            Role::Prophet => config.enable_prophet,
            Role::Psychologist => config.enable_psychologist,
            Role::Hypnotist => config.enable_hypnotist,
            Role::Mercenary => config.enable_mercenary,
            Role::Thief => config.enable_thief,
            _ => true,
        })
        .collect()
}

pub fn choose_special_roles(config: &config::BotConfig) -> Result<Vec<Role>> {
    let mut rng = system_random::rng();
    let mut selected = Vec::new();
    let mut citizen_candidates = assignable_special_roles(config, CITIZEN_SPECIAL_ROLES);
    citizen_candidates.shuffle(&mut rng);
    let mut citizen_selected = Vec::new();
    if !select_special_roles_for_slots(
        &citizen_candidates,
        config.citizen_special_count as usize,
        &mut citizen_selected,
    ) {
        bail!(
            "활성화된 시민 특수 역할로 설정한 인원 수를 구성할 수 없습니다. 연인은 2명으로 계산됩니다."
        );
    }
    selected.extend(citizen_selected);

    for (pool, count) in [
        (MAFIA_SPECIAL_ROLES, config.mafia_special_count as usize),
        (NEUTRAL_SPECIAL_ROLES, config.neutral_special_count as usize),
    ] {
        let candidates = assignable_special_roles(config, pool);
        if count > candidates.len() {
            bail!(
                "{} 중 활성화된 역할보다 선택할 특수룰 수가 많습니다.",
                pool.iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        selected.extend(candidates.choose_multiple(&mut rng, count).copied());
    }
    Ok(selected)
}

pub fn choose_special_roles_balanced(
    config: &config::BotConfig,
    role_history: &HashMap<Role, i64>,
) -> Result<Vec<Role>> {
    let mut selected = Vec::new();
    let citizen_candidates =
        balanced_special_candidates(config, CITIZEN_SPECIAL_ROLES, role_history);
    let Some(citizen_selected) = select_balanced_special_roles_for_slots(
        &citizen_candidates,
        config.citizen_special_count as usize,
        role_history,
    ) else {
        bail!(
            "활성화된 시민 특수 역할로 설정된 인원 수를 구성할 수 없습니다. 연인은 2명으로 계산합니다."
        );
    };
    selected.extend(citizen_selected);

    for (pool, count) in [
        (MAFIA_SPECIAL_ROLES, config.mafia_special_count as usize),
        (NEUTRAL_SPECIAL_ROLES, config.neutral_special_count as usize),
    ] {
        let candidates = balanced_special_candidates(config, pool, role_history);
        if count > candidates.len() {
            bail!(
                "{} 중 활성화된 역할보다 선택할 특수룰 수가 많습니다.",
                pool.iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        selected.extend(candidates.into_iter().take(count));
    }
    Ok(selected)
}

pub(crate) fn balanced_special_candidates(
    config: &config::BotConfig,
    pool: &[Role],
    role_history: &HashMap<Role, i64>,
) -> Vec<Role> {
    let mut candidates = assignable_special_roles(config, pool);
    candidates.shuffle(&mut system_random::rng());
    candidates.sort_by_key(|role| role_history.get(role).copied().unwrap_or(0));
    candidates
}

pub(crate) fn assignable_special_roles(config: &config::BotConfig, pool: &[Role]) -> Vec<Role> {
    let mut candidates = enabled_special_roles(config, pool);
    if config.default_police_count > 0 {
        candidates.retain(|role| !role.is_investigation_role());
    }
    candidates
}

pub(crate) fn select_balanced_special_roles_for_slots(
    candidates: &[Role],
    remaining_slots: usize,
    role_history: &HashMap<Role, i64>,
) -> Option<Vec<Role>> {
    fn search(
        candidates: &[Role],
        index: usize,
        remaining_slots: usize,
        role_history: &HashMap<Role, i64>,
        selected: &mut Vec<Role>,
        score: i64,
        best: &mut Option<(i64, Vec<Role>)>,
    ) {
        if remaining_slots == 0 {
            if best
                .as_ref()
                .is_none_or(|(best_score, _)| score < *best_score)
            {
                *best = Some((score, selected.clone()));
            }
            return;
        }
        if index >= candidates.len() {
            return;
        }

        let role = candidates[index];
        let slots = special_role_player_count(role);
        if slots <= remaining_slots {
            selected.push(role);
            search(
                candidates,
                index + 1,
                remaining_slots - slots,
                role_history,
                selected,
                score + role_history.get(&role).copied().unwrap_or(0),
                best,
            );
            selected.pop();
        }
        search(
            candidates,
            index + 1,
            remaining_slots,
            role_history,
            selected,
            score,
            best,
        );
    }

    let mut selected = Vec::new();
    let mut best = None;
    search(
        candidates,
        0,
        remaining_slots,
        role_history,
        &mut selected,
        0,
        &mut best,
    );
    best.map(|(_, roles)| roles)
}

pub(crate) fn select_special_roles_for_slots(
    candidates: &[Role],
    remaining_slots: usize,
    selected: &mut Vec<Role>,
) -> bool {
    if remaining_slots == 0 {
        return true;
    }
    for (index, role) in candidates.iter().enumerate() {
        let slots = special_role_player_count(*role);
        if slots > remaining_slots {
            continue;
        }
        selected.push(*role);
        if select_special_roles_for_slots(
            &candidates[index + 1..],
            remaining_slots - slots,
            selected,
        ) {
            return true;
        }
        selected.pop();
    }
    false
}

pub(crate) fn special_role_player_count(role: Role) -> usize {
    if role == Role::Lover { 2 } else { 1 }
}

pub fn expand_special_roles(roles: &[Role]) -> Vec<Role> {
    let mut expanded = Vec::new();
    for role in roles {
        if *role == Role::Lover {
            expanded.extend([Role::Lover, Role::Lover]);
        } else {
            expanded.push(*role);
        }
    }
    expanded
}

pub fn selected_role_counts(
    config: &config::BotConfig,
    special_roles: &[Role],
) -> Result<HashMap<Role, usize>> {
    selected_role_counts_with_history(config, special_roles, None)
}

pub fn selected_role_counts_balanced(
    config: &config::BotConfig,
    special_roles: &[Role],
    role_history: &HashMap<Role, i64>,
) -> Result<HashMap<Role, usize>> {
    selected_role_counts_with_history(config, special_roles, Some(role_history))
}

pub(crate) fn selected_role_counts_with_history(
    config: &config::BotConfig,
    special_roles: &[Role],
    role_history: Option<&HashMap<Role, i64>>,
) -> Result<HashMap<Role, usize>> {
    let mafia_special_count = special_roles
        .iter()
        .filter(|role| role.is_mafia_team())
        .count();
    if mafia_special_count > config.default_mafia_count as usize {
        bail!(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. 현재 마피아 {}명, 마피아 특수 {}명입니다.",
            config.default_mafia_count,
            mafia_special_count
        );
    }
    if config.default_mafia_count as usize - mafia_special_count < 1 {
        bail!(
            "접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. 일반 마피아가 최소 1명 필요합니다."
        );
    }
    let mut counts = HashMap::new();
    counts.insert(
        Role::Mafia,
        config.default_mafia_count as usize - mafia_special_count,
    );
    counts.insert(Role::Doctor, config.default_doctor_count as usize);
    if config.enable_joker && config.default_joker_count > 0 {
        counts.insert(Role::Joker, config.default_joker_count as usize);
    }
    if config.default_police_count > 0 {
        let investigation = role_history
            .map(|history| balanced_investigation_role(config, history))
            .unwrap_or_else(|| random_investigation_role(config));
        counts.insert(investigation, config.default_police_count as usize);
    }
    for role in special_roles {
        *counts.entry(*role).or_default() += if *role == Role::Lover { 2 } else { 1 };
    }
    if config.enable_cult_team {
        *counts.entry(Role::CultLeader).or_default() += 1;
        *counts.entry(Role::Fanatic).or_default() += 1;
    }
    Ok(counts)
}

pub(crate) fn balanced_investigation_role(
    config: &config::BotConfig,
    role_history: &HashMap<Role, i64>,
) -> Role {
    let mut candidates = investigation_role_candidates(config);
    candidates.shuffle(&mut system_random::rng());
    candidates.sort_by_key(|role| role_history.get(role).copied().unwrap_or(0));
    *candidates.first().unwrap_or(&Role::Police)
}

pub fn random_investigation_role(config: &config::BotConfig) -> Role {
    let candidates = investigation_role_candidates(config);
    let mut rng = system_random::rng();
    *candidates.choose(&mut rng).unwrap_or(&Role::Police)
}

pub(crate) fn investigation_role_candidates(config: &config::BotConfig) -> Vec<Role> {
    let mut candidates = vec![Role::Police];
    if config.use_agent {
        candidates.push(Role::Agent);
    }
    if config.use_vigilante {
        candidates.push(Role::Vigilante);
    }
    if config.enable_inspector {
        candidates.push(Role::Inspector);
    }
    candidates
}

pub fn minimum_player_count(role_counts: &HashMap<Role, usize>) -> usize {
    let special_count = role_counts.values().sum::<usize>();
    let mafia_count = role_counts
        .iter()
        .filter(|(role, _)| role.is_mafia_team())
        .map(|(_, count)| *count)
        .sum::<usize>();
    3.max(special_count).max(mafia_count * 2 + 1)
}

pub fn effective_max_player_count(config: &config::BotConfig) -> usize {
    if config.max_player_count == 0 {
        MAX_GAME_PLAYERS
    } else {
        (config.max_player_count as usize).min(MAX_GAME_PLAYERS)
    }
}

pub fn count_group(role_counts: &HashMap<Role, usize>, roles: &[Role]) -> usize {
    roles
        .iter()
        .map(|role| role_counts.get(role).copied().unwrap_or(0))
        .sum()
}

pub fn public_role_count_text_from_counts(
    role_counts: &HashMap<Role, usize>,
    total_players: Option<usize>,
) -> String {
    let mafia_special = count_group(role_counts, PUBLIC_MAFIA_SPECIAL_ROLES);
    let mafia_total = role_counts.get(&Role::Mafia).copied().unwrap_or(0) + mafia_special;
    let doctor_total = role_counts.get(&Role::Doctor).copied().unwrap_or(0);
    let police_total = role_counts.get(&Role::Police).copied().unwrap_or(0);
    let agent_total = role_counts.get(&Role::Agent).copied().unwrap_or(0);
    let vigilante_total = role_counts.get(&Role::Vigilante).copied().unwrap_or(0);
    let inspector_total = role_counts.get(&Role::Inspector).copied().unwrap_or(0);
    let citizen_special = count_group(role_counts, PUBLIC_CITIZEN_SPECIAL_ROLES);
    let neutral_special = count_group(role_counts, PUBLIC_NEUTRAL_SPECIAL_ROLES);
    let cult_total = count_group(role_counts, PUBLIC_CULT_SPECIAL_ROLES);
    let citizen_text = if let Some(total_players) = total_players {
        let citizen_total = total_players.saturating_sub(
            mafia_total
                + doctor_total
                + police_total
                + agent_total
                + vigilante_total
                + inspector_total
                + neutral_special
                + cult_total,
        );
        format!("시민 {citizen_total}명(중 특수 {citizen_special}명)")
    } else {
        format!("시민 변동(중 특수 {citizen_special}명)")
    };
    let mut parts = vec![
        format!("마피아 {mafia_total}명(중 특수 {mafia_special}명)"),
        format!("의사 {doctor_total}명"),
        format!(
            "수사직 {}명",
            police_total + agent_total + vigilante_total + inspector_total
        ),
        citizen_text,
    ];
    if neutral_special > 0 {
        parts.push(format!("중립 특수 {neutral_special}명"));
    }
    if cult_total > 0 {
        parts.push(format!("교주팀 {cult_total}명"));
    }
    parts.join(", ")
}

pub fn public_role_count_text(game: &MafiaGame) -> String {
    let mut counts = HashMap::new();
    for player in &game.players {
        *counts.entry(player.role).or_default() += 1;
    }
    format!(
        "역할 구성: {}",
        public_role_count_text_from_counts(&counts, Some(game.players.len()))
    )
}

pub fn public_game_settings_text(
    game: &MafiaGame,
    config: &config::BotConfig,
    prefix: &str,
) -> String {
    format!(
        "{prefix}\n{}\n최대 참가 인원: {}\n교주팀: {}\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n채팅 슬로우모드: {}초\n익명 채팅: {}{}",
        public_role_count_text(game),
        max_player_setting_text(config),
        if config.enable_cult_team {
            "켜짐 - 교주 1명, 광신도 1명 필수 배정"
        } else {
            "꺼짐"
        },
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
        config.chat_slowmode_seconds,
        if config.anonymous_mode {
            "켜짐"
        } else {
            "꺼짐"
        },
        if config.anonymous_mode {
            format!(" ({})", anonymous_name_mode_text(config))
        } else {
            String::new()
        }
    )
}

pub fn game_rule_text(
    game: &MafiaGame,
    config: &config::BotConfig,
    reveal_death_roles: bool,
) -> String {
    let death_rule = if reveal_death_roles {
        "사망자의 직업은 즉시 공개됩니다."
    } else {
        "사망자의 직업은 즉시 공개되지 않습니다."
    };
    format!(
        "{}\n\n게임은 밤과 낮을 반복합니다.\n- 역할 설명: 전체 역할 설명은 `/역할설명`, 본인 역할 설명은 `/마피아능력`으로 확인할 수 있습니다.
- 개인 티어: 게임마다 각자 2티어(40%)/3티어(30%)/4티어(15%)/5티어(10%)/6티어(5%)가 비공개로 배정됩니다. 3티어는 3티어 능력 1개, 4티어부터는 소속 풀의 4티어 이상 능력을 4티어 1개/5티어 2개/6티어 3개 받고(풀이 모자라면 3티어 능력으로 채움), 같은 능력이 여러 명에게 겹칠 수 있습니다. 내 티어는 역할 DM에서 확인합니다.\n- 밤: 게임 채널 채팅과 반응이 비활성화되고, 밤 행동이 있는 역할은 DM으로 행동합니다.\n- 낮: 생존자는 자유롭게 토론합니다. 생존자 과반이 `바로 투표`를 누르면 토론을 끝내고 지목 투표로 넘어갑니다. 시간이 끝나면 생존자 과반으로 1분 연장을 정할 수 있고, 연장은 낮마다 1번만 가능합니다.\n- 마피아 수 공개: 아침 생존 마피아 수는 {}.\n- 투표: 생존자는 최후변론에 세울 사람 또는 스킵을 선택합니다. 지목자는 20초 동안 혼자 최후변론을 하고, 이후 찬반투표 과반 결과를 따릅니다.\n- 경찰 공개: 조사 성공 여부는 {}. 실제 조사 결과는 경찰에게만 전달됩니다.\n- 채팅: 낮 토론 슬로우모드는 {}초이며 최후변론 중에는 해제됩니다.\n- 사망자: {death_rule} 게임 채널 채팅/반응 권한은 제거되고 '{DEAD_PLAYER_ROLE}' 역할이 부여됩니다.\n\n승리 조건\n- 시민 진영: 모든 마피아를 제거하면 승리합니다.\n- 마피아 진영: 생존 마피아 수가 나머지 생존자 수 이상이면 승리합니다.\n- 교주팀: 교주팀 생존자가 비교주팀 생존자 이상이면 승리합니다.\n- 조커: 낮 투표로 처형되면 즉시 단독 승리합니다.",
        public_role_count_text(game),
        if config.reveal_morning_mafia_count {
            "공개됩니다"
        } else {
            "공개되지 않습니다"
        },
        if config.reveal_public_police_status {
            "공개됩니다"
        } else {
            "공개되지 않습니다"
        },
        config.chat_slowmode_seconds
    )
}

pub fn enabled_special_role_names(config: &config::BotConfig) -> String {
    let roles = [
        Role::Detective,
        Role::Shaman,
        Role::Graverobber,
        Role::Spy,
        Role::Contractor,
        Role::Fraudster,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Godfather,
        Role::Joker,
        Role::Politician,
        Role::Judge,
        Role::Reporter,
        Role::Hacker,
        Role::Inspector,
        Role::Terrorist,
        Role::Lover,
        Role::CivilServant,
        Role::Paparazzi,
        Role::Priest,
        Role::Soldier,
        Role::Nurse,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Hypnotist,
        Role::Mercenary,
        Role::CultLeader,
        Role::Fanatic,
    ]
    .into_iter()
    .filter(|role| match role {
        Role::Inspector => config.enable_inspector,
        Role::Detective => config.enable_detective,
        Role::Shaman => config.enable_shaman,
        Role::Graverobber => config.enable_graverobber,
        Role::Spy => config.enable_spy,
        Role::Contractor => config.enable_contractor,
        Role::Fraudster => config.enable_fraudster,
        Role::Thief => config.enable_thief,
        Role::Witch => config.enable_witch,
        Role::Scientist => config.enable_scientist,
        Role::Madam => config.enable_madam,
        Role::Godfather => config.enable_godfather,
        Role::Joker => config.enable_joker,
        Role::Politician => config.enable_politician,
        Role::Judge => config.enable_judge,
        Role::Reporter => config.enable_reporter,
        Role::Hacker => config.enable_hacker,
        Role::Terrorist => config.enable_terrorist,
        Role::Lover => config.enable_lover,
        Role::CivilServant => config.enable_civil_servant,
        Role::Paparazzi => config.enable_paparazzi,
        Role::Priest => config.enable_priest,
        Role::Soldier => config.enable_soldier,
        Role::Nurse => config.enable_nurse,
        Role::Gangster => config.enable_gangster,
        Role::Prophet => config.enable_prophet,
        Role::Psychologist => config.enable_psychologist,
        Role::Hypnotist => config.enable_hypnotist,
        Role::Mercenary => config.enable_mercenary,
        Role::CultLeader | Role::Fanatic => config.enable_cult_team,
        _ => false,
    })
    .map(|role| role.value())
    .collect::<Vec<_>>();
    if roles.is_empty() {
        "없음".to_string()
    } else {
        roles.join(", ")
    }
}

pub fn investigation_candidates_text(config: &config::BotConfig) -> String {
    let mut candidates = vec!["경찰"];
    if config.use_agent {
        candidates.push("요원");
    }
    if config.use_vigilante {
        candidates.push("자경단원");
    }
    if config.enable_inspector {
        candidates.push("형사");
    }
    candidates.join(", ")
}

pub fn current_settings_text(config: &config::BotConfig, prefix: &str) -> String {
    format!(
        "{prefix}\n게임 상태: {}\n기본 직업: 마피아 {}명, 의사 {}, 수사직 {}\n최대 참가 인원: {}\n참가자 모집 시간: {}초\n특수룰 수: 시민 {}개, 마피아 {}개, 중립 {}개\n활성 특수룰: {}\n수사직 후보: {}\n교주팀: {}\n채팅 슬로우모드: {}초\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n익명 채팅: {}\n익명 이름 방식: {}",
        if config.game_enabled {
            "활성화"
        } else {
            "비활성화"
        },
        config.default_mafia_count,
        if config.default_doctor_count > 0 {
            "활성화"
        } else {
            "비활성화"
        },
        if config.default_police_count > 0 {
            "활성화"
        } else {
            "비활성화"
        },
        max_player_setting_text(config),
        config.effective_recruitment_seconds(),
        config.citizen_special_count,
        config.mafia_special_count,
        config.neutral_special_count,
        enabled_special_role_names(config),
        investigation_candidates_text(config),
        if config.enable_cult_team {
            "켜짐 - 교주 1명, 광신도 1명 필수 배정"
        } else {
            "꺼짐"
        },
        config.chat_slowmode_seconds,
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
        if config.anonymous_mode {
            "켜짐"
        } else {
            "꺼짐"
        },
        anonymous_name_mode_text(config),
    )
}
