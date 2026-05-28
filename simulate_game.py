from __future__ import annotations

import random

from game import MafiaGame, Phase, Role, Winner


def main() -> None:
    players = [(index, f"Player{index}") for index in range(1, 7)]
    game = MafiaGame(players, mafia_count=2, doctor_count=1, police_count=1, rng=random.Random(7))

    mafias = [player for player in game.alive_players() if player.role == Role.MAFIA]
    mafia = mafias[0]
    doctor = next(player for player in game.alive_players() if player.role == Role.DOCTOR)
    police = next(player for player in game.alive_players() if player.role == Role.POLICE)
    citizen_target = next(player for player in game.alive_players() if player.role == Role.CITIZEN)

    assert not game.all_night_actions_submitted()
    game.submit_night_action(mafia.user_id, citizen_target.user_id)
    assert not game.all_night_actions_submitted()
    game.submit_night_action(mafias[1].user_id, citizen_target.user_id)
    game.submit_night_action(doctor.user_id, citizen_target.user_id)
    police_result = game.submit_night_action(police.user_id, mafia.user_id)
    assert game.all_night_actions_submitted()
    try:
        game.submit_night_action(police.user_id, citizen_target.user_id)
    except ValueError as error:
        assert "이미 이번 밤 행동을 선택했습니다" in str(error)
    else:
        raise AssertionError("Police investigated twice in one night")
    night_result = game.resolve_night()

    assert "조사 투표 대상" in police_result
    assert night_result.police_target is not None
    assert night_result.police_target_is_mafia
    assert night_result.killed is None
    assert game.phase == Phase.DAY

    game.start_vote()
    alive = game.alive_players()
    for voter in alive[:3]:
        game.submit_day_vote(voter.user_id, mafia.user_id)
    assert not game.all_day_votes_submitted()
    vote_result = game.resolve_vote()

    assert vote_result.executed is not None
    assert vote_result.executed.user_id == mafia.user_id
    assert vote_result.vote_counts == {mafia.user_id: 3}
    assert game.phase == Phase.FINAL_DEFENSE
    game.start_confirmation_vote()
    for voter in game.alive_players()[:3]:
        game.submit_confirmation_vote(voter.user_id, True)
    confirm_result = game.resolve_confirmation_vote(mafia.user_id)
    assert confirm_result.executed is not None
    assert confirm_result.executed.user_id == mafia.user_id
    assert game.phase == Phase.NIGHT

    game.resolve_night()
    game.start_vote()
    assert not game.all_day_votes_submitted()
    skip_voter_count = len(game.alive_players())
    for voter in game.alive_players():
        game.submit_day_vote(voter.user_id, None)
    assert game.all_day_votes_submitted()
    skip_result = game.resolve_vote()

    assert skip_result.executed is None
    assert skip_result.skipped
    assert skip_result.vote_counts == {None: skip_voter_count}
    assert game.phase == Phase.NIGHT

    joker_game = MafiaGame(
        [(index, f"JokerGame{index}") for index in range(1, 7)],
        mafia_count=1,
        doctor_count=1,
        police_count=1,
        joker_count=1,
        rng=random.Random(11),
    )
    joker = next(player for player in joker_game.players if player.role == Role.JOKER)

    joker_game.resolve_night()
    joker_game.start_vote()
    for voter in joker_game.alive_players()[:3]:
        joker_game.submit_day_vote(voter.user_id, joker.user_id)
    joker_vote = joker_game.resolve_vote()

    assert joker_vote.executed is not None
    assert joker_vote.executed.user_id == joker.user_id
    joker_game.start_confirmation_vote()
    for voter in joker_game.alive_players()[:3]:
        joker_game.submit_confirmation_vote(voter.user_id, True)
    joker_confirm = joker_game.resolve_confirmation_vote(joker.user_id)
    assert joker_confirm.executed is not None
    assert joker_game.winner() == Winner.JOKER

    politician_game = MafiaGame(
        [(index, f"PoliticianGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=0,
        police_count=0,
        special_roles=[Role.POLITICIAN],
        rng=random.Random(41),
    )
    politician = next(player for player in politician_game.players if player.role == Role.POLITICIAN)
    politician_game.resolve_night()
    politician_game.start_vote()
    for voter in politician_game.alive_players()[:3]:
        politician_game.submit_day_vote(voter.user_id, politician.user_id)
    politician_vote = politician_game.resolve_vote()
    assert politician_vote.executed is not None
    assert politician_vote.executed.user_id == politician.user_id
    assert politician_game.phase == Phase.FINAL_DEFENSE
    politician_game.start_confirmation_vote()
    for voter in politician_game.alive_players():
        politician_game.submit_confirmation_vote(voter.user_id, True)
    politician_confirm = politician_game.resolve_confirmation_vote(politician.user_id)
    assert politician_confirm.approved
    assert politician_confirm.blocked_by_politician
    assert politician_confirm.executed is None
    assert politician.alive
    assert politician_game.phase == Phase.NIGHT

    politician_weight_game = MafiaGame(
        [(index, f"PoliticianWeightGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=0,
        police_count=0,
        special_roles=[Role.POLITICIAN],
        rng=random.Random(43),
    )
    weight_mafia = next(player for player in politician_weight_game.players if player.role == Role.MAFIA)
    weight_politician = next(player for player in politician_weight_game.players if player.role == Role.POLITICIAN)
    weight_other = next(
        player
        for player in politician_weight_game.players
        if player.user_id not in {weight_mafia.user_id, weight_politician.user_id}
    )
    politician_weight_game.resolve_night()
    politician_weight_game.start_vote()
    politician_weight_game.submit_day_vote(weight_politician.user_id, weight_mafia.user_id)
    politician_weight_game.submit_day_vote(weight_other.user_id, weight_politician.user_id)
    politician_weight_vote = politician_weight_game.resolve_vote()
    assert politician_weight_vote.executed is not None
    assert politician_weight_vote.executed.user_id == weight_mafia.user_id
    assert politician_weight_vote.vote_counts == {
        weight_mafia.user_id: 2,
        weight_politician.user_id: 1,
    }
    politician_weight_game.start_confirmation_vote()
    politician_weight_game.submit_confirmation_vote(weight_politician.user_id, True)
    politician_weight_game.submit_confirmation_vote(weight_other.user_id, False)
    politician_weight_confirm = politician_weight_game.resolve_confirmation_vote(weight_mafia.user_id)
    assert politician_weight_confirm.executed is not None
    assert politician_weight_confirm.vote_counts == {True: 2, False: 1}

    spy_game = MafiaGame(
        [(index, f"SpyGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=0,
        police_count=0,
        special_roles=[Role.SPY],
        rng=random.Random(45),
    )
    spy = next(player for player in spy_game.players if player.role == Role.SPY)
    spy_mafia = next(player for player in spy_game.players if player.role == Role.MAFIA)
    spy_citizen = next(player for player in spy_game.players if player.role == Role.CITIZEN)
    spy_result = spy_game.submit_night_action(spy.user_id, spy_mafia.user_id)
    assert "[첩보]" in spy_result
    assert "[접선]" in spy_result
    assert spy.user_id in spy_game.spy_contacted
    assert spy_game.spy_can_use_bonus_action(spy.user_id)
    assert not spy_game.all_night_actions_submitted()
    spy_bonus_result = spy_game.submit_night_action(spy.user_id, spy_citizen.user_id)
    assert spy_citizen.role.value in spy_bonus_result
    assert not spy_game.spy_can_use_bonus_action(spy.user_id)
    spy_game.submit_night_action(spy_mafia.user_id, spy_citizen.user_id)
    assert spy_game.all_night_actions_submitted()
    spy_night = spy_game.resolve_night()
    assert spy_night.spy_contacts == [spy.user_id]

    graverobber_game = MafiaGame(
        [(index, f"GraverobberGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=1,
        police_count=0,
        special_roles=[Role.GRAVEROBBER],
        rng=random.Random(47),
    )
    graverobber_mafia = next(player for player in graverobber_game.players if player.role == Role.MAFIA)
    graverobber = next(player for player in graverobber_game.players if player.role == Role.GRAVEROBBER)
    robbed_doctor = next(player for player in graverobber_game.players if player.role == Role.DOCTOR)
    graverobber_game.submit_night_action(graverobber_mafia.user_id, robbed_doctor.user_id)
    graverobber_result = graverobber_game.resolve_night()
    assert graverobber.role == Role.DOCTOR
    assert robbed_doctor.role == Role.CITIZEN
    assert graverobber_result.graverobber_results == {graverobber.user_id: Role.DOCTOR}

    terrorist_night_game = MafiaGame(
        [(index, f"TerroristNightGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=0,
        police_count=0,
        special_roles=[Role.TERRORIST],
        rng=random.Random(51),
    )
    night_mafia = next(player for player in terrorist_night_game.players if player.role == Role.MAFIA)
    night_terrorist = next(player for player in terrorist_night_game.players if player.role == Role.TERRORIST)
    terrorist_night_game.submit_night_action(night_terrorist.user_id, night_mafia.user_id)
    terrorist_night_game.submit_night_action(night_mafia.user_id, night_terrorist.user_id)
    terrorist_night_result = terrorist_night_game.resolve_night()
    assert not night_terrorist.alive
    assert not night_mafia.alive
    assert {player.user_id for player in terrorist_night_result.killed_players} == {
        night_terrorist.user_id,
        night_mafia.user_id,
    }
    assert terrorist_night_result.terrorist_retaliations == [(night_terrorist, night_mafia)]

    terrorist_vote_game = MafiaGame(
        [(index, f"TerroristVoteGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=0,
        police_count=0,
        special_roles=[Role.TERRORIST],
        rng=random.Random(61),
    )
    vote_mafia = next(player for player in terrorist_vote_game.players if player.role == Role.MAFIA)
    vote_terrorist = next(player for player in terrorist_vote_game.players if player.role == Role.TERRORIST)
    terrorist_vote_game.submit_night_action(vote_terrorist.user_id, vote_mafia.user_id)
    terrorist_vote_game.resolve_night()
    terrorist_vote_game.start_vote()
    for voter in terrorist_vote_game.alive_players()[:3]:
        terrorist_vote_game.submit_day_vote(voter.user_id, vote_terrorist.user_id)
    terrorist_vote = terrorist_vote_game.resolve_vote()
    assert terrorist_vote.executed is not None
    assert terrorist_vote.executed.user_id == vote_terrorist.user_id
    terrorist_vote_game.start_confirmation_vote()
    for voter in terrorist_vote_game.alive_players():
        terrorist_vote_game.submit_confirmation_vote(voter.user_id, True)
    terrorist_confirm = terrorist_vote_game.resolve_confirmation_vote(vote_terrorist.user_id)
    assert terrorist_confirm.executed is not None
    assert terrorist_confirm.executed.user_id == vote_terrorist.user_id
    assert terrorist_confirm.extra_killed == [vote_mafia]
    assert not vote_terrorist.alive
    assert not vote_mafia.alive

    godfather_game = MafiaGame(
        [(index, f"GodfatherGame{index}") for index in range(1, 7)],
        mafia_count=1,
        doctor_count=1,
        police_count=0,
        special_roles=[Role.GODFATHER],
        rng=random.Random(31),
    )
    godfather = next(player for player in godfather_game.players if player.role == Role.GODFATHER)
    assert godfather.user_id not in godfather_game.godfather_contacted
    godfather_game.resolve_night()
    godfather_game.start_vote()
    godfather_game.resolve_vote()
    assert godfather_game.phase == Phase.NIGHT
    assert godfather_game.day_number == 2
    godfather_game.resolve_night()
    godfather_game.start_vote()
    godfather_game.resolve_vote()
    assert godfather_game.phase == Phase.NIGHT
    assert godfather_game.day_number == 3
    contacted = godfather_game.ensure_godfather_auto_contact()
    assert godfather.user_id in contacted

    split_doctor_game = MafiaGame(
        [(index, f"DoctorGame{index}") for index in range(1, 6)],
        mafia_count=1,
        doctor_count=2,
        police_count=0,
        joker_count=0,
        rng=random.Random(21),
    )
    split_mafia = next(player for player in split_doctor_game.players if player.role == Role.MAFIA)
    split_doctors = [player for player in split_doctor_game.players if player.role == Role.DOCTOR]
    split_victim = next(player for player in split_doctor_game.players if player.role == Role.CITIZEN)
    split_other = next(
        player
        for player in split_doctor_game.players
        if player.user_id not in {split_mafia.user_id, split_victim.user_id, split_doctors[0].user_id}
    )

    split_doctor_game.submit_night_action(split_mafia.user_id, split_victim.user_id)
    split_doctor_game.submit_night_action(split_doctors[0].user_id, split_victim.user_id)
    split_doctor_game.submit_night_action(split_doctors[1].user_id, split_other.user_id)
    split_result = split_doctor_game.resolve_night()

    assert split_result.protected is None
    assert split_result.killed is not None
    assert split_result.killed.user_id == split_victim.user_id
    print("simulation ok")


if __name__ == "__main__":
    main()
