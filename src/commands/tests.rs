// commands 테스트 모듈 (src/commands.rs에서 분리)

use super::*;

#[test]
fn contractor_draft_replaces_role_when_target_changes_and_resolves_duplicates() {
    let mut draft = ContractorContractDraft {
        target_ids: [Some(10), Some(20)],
        guessed_roles: [Some(Role::Citizen), Some(Role::Mafia)],
        ..Default::default()
    };

    set_contractor_draft_target(&mut draft, 0, 30).unwrap();

    // 대상을 바꿔도 이미 고른 직업 추측은 유지된다.
    assert_eq!(draft.target_ids, [Some(30), Some(20)]);
    assert_eq!(
        draft.guessed_roles,
        [Some(Role::Citizen), Some(Role::Mafia)]
    );

    // 반대 슬롯과 같은 대상을 고르면 최근 선택이 이기고 반대 슬롯이 비워진다
    // (에러로 끊기면 확정이 안 되는 막다른 상태가 된다).
    set_contractor_draft_target(&mut draft, 1, 30).unwrap();
    assert_eq!(draft.target_ids, [None, Some(30)]);
    assert_eq!(draft.guessed_roles, [None, Some(Role::Mafia)]);
}

#[test]
fn contractor_draft_submission_requires_both_targets_and_roles() {
    let mut draft = ContractorContractDraft::default();
    assert_eq!(contractor_draft_submission(&draft), None);

    draft.target_ids = [Some(10), Some(20)];
    draft.guessed_roles = [Some(Role::Citizen), Some(Role::Mafia)];

    assert_eq!(
        contractor_draft_submission(&draft),
        Some((10, 20, Role::Citizen, Role::Mafia))
    );
}

/// 사망자/영매 채팅과 역할 채팅 미러링이 모두 이 라벨을 쓴다. 실명은 절대 나오지
/// 않아야 하고, 역할이나 생사에 따라 다른 라벨이 붙지도 않아야 한다.
#[test]
fn anonymous_sender_labels_only_use_the_configured_alias() {
    let mut running = crate::channel::tests::dead_chat_test_running();
    running.anonymous_enabled = true;
    let shaman = Player::new(7, "영매 실제 이름".to_string(), Role::Shaman);
    let mut dead = Player::new(8, "사망자 실제 이름".to_string(), Role::Citizen);
    dead.alive = false;
    running
        .anonymous_aliases
        .insert(shaman.user_id, "3번".to_string());
    running
        .anonymous_aliases
        .insert(dead.user_id, "너구리".to_string());

    assert_eq!(anonymous_sender_label(&running, &shaman), "3번");
    assert_eq!(anonymous_sender_label(&running, &dead), "너구리");
}

#[test]
fn non_anonymous_sender_labels_keep_the_real_name() {
    let mut running = crate::channel::tests::dead_chat_test_running();
    running.anonymous_enabled = false;
    let sender = Player::new(7, "마피아 실제 이름".to_string(), Role::Mafia);
    running
        .anonymous_aliases
        .insert(sender.user_id, "3번".to_string());

    assert_eq!(
        anonymous_sender_label(&running, &sender),
        "마피아 실제 이름"
    );
}

fn memo_test_running(anonymous_enabled: bool) -> RunningGame {
    let mut running = crate::channel::tests::dead_chat_test_running();
    running.anonymous_enabled = anonymous_enabled;
    running.anonymous_aliases = HashMap::from([
        (1, "너구리".to_string()),
        (2, "고양이".to_string()),
        (3, "다람쥐".to_string()),
        (4, "고라니".to_string()),
    ]);
    running
}

/// 익명 게임에서 실제 유저로 메모 대상을 고르면 참가자든 아니든 같은 문구로 거절해야 한다.
/// 익명 이름을 되돌려주면 누구나 실제 유저 ↔ 익명 이름을 맞춰볼 수 있다.
#[test]
fn anonymous_memo_rejects_real_user_targets_without_revealing_anything() {
    let running = memo_test_running(true);

    let participant = memo_target(&running, Some(2), None).unwrap_err();
    let outsider = memo_target(&running, Some(999), None).unwrap_err();
    let with_alias = memo_target(&running, Some(2), Some("고양이")).unwrap_err();

    assert_eq!(participant, MEMO_ANONYMOUS_TARGET_GUIDE);
    assert_eq!(outsider, participant);
    assert_eq!(with_alias, participant);
    for alias in running.anonymous_aliases.values() {
        assert!(!participant.contains(alias.as_str()));
    }
}

#[test]
fn anonymous_memo_targets_are_chosen_by_alias() {
    let running = memo_test_running(true);

    assert_eq!(
        memo_target(&running, None, Some(" 다람쥐 "))
            .unwrap()
            .user_id,
        3
    );
    assert_eq!(
        memo_target(&running, None, None).unwrap_err(),
        MEMO_ANONYMOUS_TARGET_GUIDE
    );
    assert_eq!(
        memo_target(&running, None, Some("  ")).unwrap_err(),
        MEMO_ANONYMOUS_TARGET_GUIDE
    );
    // 실명이나 없는 익명 이름으로는 대상을 찾지 않는다.
    assert!(memo_target(&running, None, Some("p3")).is_err());
    assert!(memo_target(&running, None, Some("호랑이")).is_err());
}

#[test]
fn non_anonymous_memo_still_targets_the_selected_user() {
    let running = memo_test_running(false);

    assert_eq!(memo_target(&running, Some(2), None).unwrap().user_id, 2);
    // 익명 이름 항목은 일반 게임에서 쓰이지 않는다.
    assert_eq!(
        memo_target(&running, Some(2), Some("다람쥐"))
            .unwrap()
            .user_id,
        2
    );
    assert_eq!(
        memo_target(&running, Some(999), None).unwrap_err(),
        "메모 대상은 현재 게임 참가자여야 합니다."
    );
    assert_eq!(
        memo_target(&running, None, Some("다람쥐")).unwrap_err(),
        "메모 대상 참가자를 선택하세요."
    );
}

#[test]
fn memo_alias_choices_only_list_anonymous_aliases_by_name() {
    assert!(memo_alias_choices(&memo_test_running(false), "").is_empty());

    let running = memo_test_running(true);
    assert_eq!(
        memo_alias_choices(&running, ""),
        vec!["고라니", "고양이", "너구리", "다람쥐"]
    );
    assert_eq!(
        memo_alias_choices(&running, " 고 "),
        vec!["고라니", "고양이"]
    );
    assert!(memo_alias_choices(&running, "p1").is_empty());
}
