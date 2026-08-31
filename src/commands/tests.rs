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

    // 1번 대상만 바뀌었으므로 2번 대상의 직업은 유지된다.
    assert_eq!(draft.target_ids, [Some(30), Some(20)]);
    assert_eq!(draft.guessed_roles, [None, Some(Role::Mafia)]);

    // 반대 슬롯과 같은 대상을 고르면 최근 선택이 이기고 반대 슬롯이 비워진다
    // (에러로 끊기면 확정이 안 되는 막다른 상태가 된다).
    set_contractor_draft_target(&mut draft, 1, 30).unwrap();
    assert_eq!(draft.target_ids, [None, Some(30)]);
    assert_eq!(draft.guessed_roles, [None, None]);
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
