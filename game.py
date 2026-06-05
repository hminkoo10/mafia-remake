from __future__ import annotations

from collections import Counter
from dataclasses import dataclass, field
from enum import Enum
import random


class Role(str, Enum):
    MAFIA = "마피아"
    DOCTOR = "의사"
    NURSE = "간호사"
    POLICE = "경찰"
    AGENT = "요원"
    VIGILANTE = "자경단원"
    REPORTER = "기자"
    HACKER = "해커"
    DETECTIVE = "사립탐정"
    SHAMAN = "영매"
    PRIEST = "성직자"
    SOLDIER = "군인"
    SPY = "스파이"
    CONTRACTOR = "청부업자"
    WITCH = "마녀"
    SCIENTIST = "과학자"
    MADAM = "마담"
    GRAVEROBBER = "도굴꾼"
    GODFATHER = "대부"
    JOKER = "조커"
    POLITICIAN = "정치인"
    JUDGE = "판사"
    TERRORIST = "테러리스트"
    LOVER = "연인"
    CULT_LEADER = "교주"
    FANATIC = "광신도"
    FROG = "개구리"
    VILLAIN = "악인"
    CITIZEN = "시민"


class Phase(str, Enum):
    NIGHT = "밤"
    DAY = "낮"
    VOTE = "투표"
    FINAL_DEFENSE = "최후변론"
    CONFIRM_VOTE = "찬반투표"
    ENDED = "종료"


class Winner(str, Enum):
    MAFIA = "마피아"
    CITIZEN = "시민"
    JOKER = "조커"
    CULT = "교주"


MAFIA_TEAM_ROLES = {
    Role.MAFIA,
    Role.SPY,
    Role.CONTRACTOR,
    Role.WITCH,
    Role.SCIENTIST,
    Role.MADAM,
    Role.GODFATHER,
    Role.VILLAIN,
}
INVESTIGATION_ROLES = {Role.POLICE, Role.AGENT, Role.VIGILANTE}
CONTRACTOR_GUESSABLE_ROLES = {
    Role.DOCTOR,
    Role.REPORTER,
    Role.HACKER,
    Role.DETECTIVE,
    Role.SHAMAN,
    Role.PRIEST,
    Role.GRAVEROBBER,
    Role.POLITICIAN,
    Role.JUDGE,
    Role.TERRORIST,
    Role.LOVER,
    Role.SOLDIER,
    Role.NURSE,
    Role.CULT_LEADER,
    Role.FANATIC,
    Role.WITCH,
    Role.SCIENTIST,
    Role.MADAM,
    Role.JOKER,
    Role.CITIZEN,
}


@dataclass
class Player:
    user_id: int
    name: str
    role: Role
    alive: bool = True


@dataclass
class NightResult:
    killed: Player | None
    protected: Player | None
    mafia_target: Player | None
    police_target: Player | None
    police_target_is_mafia: bool | None
    killed_players: list[Player] = field(default_factory=list)
    detective_results: dict[int, str] = field(default_factory=dict)
    spy_results: dict[int, str] = field(default_factory=dict)
    spy_contacts: list[int] = field(default_factory=list)
    contractor_results: dict[int, str] = field(default_factory=dict)
    contractor_contacts: list[int] = field(default_factory=list)
    contractor_kills: list[Player] = field(default_factory=list)
    witch_results: dict[int, str] = field(default_factory=dict)
    witch_contacts: list[int] = field(default_factory=list)
    godfather_results: dict[int, str] = field(default_factory=dict)
    godfather_contacts: list[int] = field(default_factory=list)
    graverobber_results: dict[int, Role] = field(default_factory=dict)
    terrorist_retaliations: list[tuple[Player, Player]] = field(default_factory=list)
    soldier_blocks: list[Player] = field(default_factory=list)
    lover_sacrifices: list[tuple[Player, Player]] = field(default_factory=list)
    shaman_results: dict[int, str] = field(default_factory=dict)
    shaman_purifications: list[int] = field(default_factory=list)
    priest_results: dict[int, str] = field(default_factory=dict)
    priest_revives: list[Player] = field(default_factory=list)
    agent_results: dict[int, str] = field(default_factory=dict)
    reporter_results: dict[int, str] = field(default_factory=dict)
    hacker_results: dict[int, str] = field(default_factory=dict)
    vigilante_results: dict[int, str] = field(default_factory=dict)
    vigilante_kills: list[Player] = field(default_factory=list)
    nurse_results: dict[int, str] = field(default_factory=dict)
    nurse_contacts: list[int] = field(default_factory=list)
    cult_results: dict[int, str] = field(default_factory=dict)
    fanatic_results: dict[int, str] = field(default_factory=dict)
    fanatic_inherits: list[int] = field(default_factory=list)
    cult_bells: int = 0


@dataclass
class VoteResult:
    executed: Player | None
    tied: bool
    skipped: bool = False
    vote_counts: dict[int | None, int] = field(default_factory=dict)
    madam_seduced: list[Player] = field(default_factory=list)


@dataclass
class ConfirmVoteResult:
    executed: Player | None
    approved: bool
    tied: bool
    blocked_by_politician: bool = False
    extra_killed: list[Player] = field(default_factory=list)
    vote_counts: dict[bool, int] = field(default_factory=dict)
    judge: Player | None = None
    judge_choice: bool | None = None
    decided_by_judge: bool = False


class MafiaGame:
    def __init__(
        self,
        players: list[tuple[int, str]],
        mafia_count: int,
        doctor_count: int,
        police_count: int,
        rng: random.Random | None = None,
        joker_count: int = 0,
        special_roles: list[Role] | None = None,
        agent_count: int = 0,
        vigilante_count: int = 0,
    ) -> None:
        self._rng = rng or random.SystemRandom()
        special_roles = special_roles or []
        self._validate_counts(
            players,
            mafia_count,
            doctor_count,
            police_count,
            agent_count,
            vigilante_count,
            joker_count,
            special_roles,
        )

        roles = (
            [Role.MAFIA] * mafia_count
            + [Role.DOCTOR] * doctor_count
            + [Role.POLICE] * police_count
            + [Role.AGENT] * agent_count
            + [Role.VIGILANTE] * vigilante_count
            + [Role.JOKER] * joker_count
            + special_roles
        )
        roles.extend([Role.CITIZEN] * (len(players) - len(roles)))
        self._rng.shuffle(roles)

        shuffled_players = players[:]
        self._rng.shuffle(shuffled_players)
        self.players = [
            Player(user_id=user_id, name=name, role=role)
            for (user_id, name), role in zip(shuffled_players, roles, strict=True)
        ]
        self.phase = Phase.NIGHT
        self.day_number = 1
        self.mafia_targets: dict[int, int] = {}
        self.mafia_display_targets: dict[int, int] = {}
        self.doctor_targets: dict[int, int] = {}
        self.nurse_targets: dict[int, int] = {}
        self.nurse_prescription_targets: dict[int, int] = {}
        self.nurse_contacted: set[int] = set()
        self.nurse_contacts_this_night: list[int] = []
        self.police_targets: dict[int, int] = {}
        self.vigilante_targets: dict[int, int] = {}
        self.vigilante_pending_results: dict[int, int] = {}
        self.vigilante_known_enemy_ids: dict[int, set[int]] = {}
        self.vigilante_investigation_used_ids: set[int] = set()
        self.vigilante_execution_used_ids: set[int] = set()
        self.reporter_targets: dict[int, int] = {}
        self.reporter_skip_submitted: set[int] = set()
        self.reporter_used_ids: set[int] = set()
        self.hacker_targets: dict[int, int] = {}
        self.hacker_pending_results: dict[int, int] = {}
        self.hacker_used_ids: set[int] = set()
        self.hacker_proxy_targets: dict[int, int] = {}
        self.detective_targets: dict[int, int] = {}
        self.shaman_targets: dict[int, int] = {}
        self.priest_targets: dict[int, int] = {}
        self.priest_used_ids: set[int] = set()
        self.spy_targets: dict[int, list[int]] = {}
        self.spy_bonus_pending: set[int] = set()
        self.spy_contacts_this_night: list[int] = []
        self.contractor_contact_targets: dict[int, int] = {}
        self.contractor_contracts: dict[int, tuple[tuple[int, Role], tuple[int, Role]]] = {}
        self.contractor_contacts_this_night: list[int] = []
        self.witch_targets: dict[int, int] = {}
        self.witch_contacted: set[int] = set()
        self.witch_contacts_this_night: list[int] = []
        self.witch_curse_applied_actor_ids: set[int] = set()
        self.godfather_targets: dict[int, int] = {}
        self.terrorist_targets: dict[int, int] = {}
        self.terrorist_action_submitted: set[int] = set()
        self.frog_user_ids: set[int] = set()
        self.soldier_bulletproof_used: set[int] = set()
        self.purified_dead_ids: set[int] = set()
        self.publicly_revealed_ids: set[int] = set()
        self.agent_discovered_ids: set[int] = set()
        self.day_votes: dict[int, int | None] = {}
        self.confirm_votes: dict[int, bool] = {}
        self.spy_contacted: set[int] = set()
        self.contractor_contacted: set[int] = set()
        self.scientist_contacted: set[int] = set()
        self.scientist_revive_used_ids: set[int] = set()
        self.scientist_pending_revive_ids: set[int] = set()
        self.madam_contacted: set[int] = set()
        self.madam_seduced_ids: set[int] = set()
        self.madam_seduction_release_days: dict[int, int] = {}
        self.godfather_contacted: set[int] = set()
        self.revealed_judge_ids: set[int] = set()
        self.cult_targets: dict[int, int] = {}
        self.fanatic_targets: dict[int, int] = {}
        self.culted_ids: set[int] = set()
        self.cult_bells_this_night = 0
        self.joker_won = False
        self.joker_winner_id: int | None = None

    @staticmethod
    def _validate_counts(
        players: list[tuple[int, str]],
        mafia_count: int,
        doctor_count: int,
        police_count: int,
        agent_count: int,
        vigilante_count: int,
        joker_count: int,
        special_roles: list[Role],
    ) -> None:
        if len(players) < 3:
            raise ValueError("최소 3명이 필요합니다.")
        if len(players) > 24:
            raise ValueError("투표 스킵 선택지를 포함해야 해서 최대 24명까지 지원합니다.")
        if len({user_id for user_id, _ in players}) != len(players):
            raise ValueError("중복된 참가자가 있습니다.")
        if mafia_count < 0:
            raise ValueError("기본 마피아 수는 0명 이상이어야 합니다.")
        if (
            doctor_count < 0
            or police_count < 0
            or agent_count < 0
            or vigilante_count < 0
            or joker_count < 0
        ):
            raise ValueError("의사, 경찰, 요원, 자경단원, 조커 수는 0명 이상이어야 합니다.")
        special_agent_count = sum(role == Role.AGENT for role in special_roles)
        special_vigilante_count = sum(role == Role.VIGILANTE for role in special_roles)
        investigation_role_count = sum(
            count > 0
            for count in (
                police_count,
                agent_count + special_agent_count,
                vigilante_count + special_vigilante_count,
            )
        )
        if investigation_role_count > 1:
            raise ValueError("경찰, 요원, 자경단원은 한 게임에 함께 배정할 수 없습니다.")
        if agent_count > 0 and special_agent_count:
            raise ValueError("요원 수가 중복 배정되었습니다.")
        if vigilante_count > 0 and special_vigilante_count:
            raise ValueError("자경단원 수가 중복 배정되었습니다.")
        duplicate_roles = [
            role
            for role, count in Counter(special_roles).items()
            if count > 1 and not (role == Role.LOVER and count == 2)
        ]
        if duplicate_roles:
            raise ValueError("같은 특수 역할은 한 게임에 한 번만 선택됩니다.")

        special_count = (
            mafia_count
            + doctor_count
            + police_count
            + agent_count
            + vigilante_count
            + joker_count
            + len(special_roles)
        )
        if special_count > len(players):
            raise ValueError("직업 수의 합계가 참가자 수보다 많습니다.")

        mafia_team_count = mafia_count + sum(role in MAFIA_TEAM_ROLES for role in special_roles)
        if mafia_team_count < 1:
            raise ValueError("마피아 계열은 최소 1명이어야 합니다.")
        if mafia_team_count >= len(players) - mafia_team_count:
            raise ValueError("시작할 때 시민 진영이 마피아 팀보다 많아야 합니다.")

    def alive_players(self) -> list[Player]:
        return [player for player in self.players if player.alive]

    def dead_players(self) -> list[Player]:
        return [player for player in self.players if not player.alive]

    def unpurified_dead_players(self) -> list[Player]:
        return [player for player in self.dead_players() if player.user_id not in self.purified_dead_ids]

    def alive_mafia(self) -> list[Player]:
        return [player for player in self.alive_players() if player.role == Role.MAFIA]

    def alive_mafia_team(self) -> list[Player]:
        return [player for player in self.alive_players() if self.is_mafia_team(player)]

    def alive_known_mafia_team(self) -> list[Player]:
        return [player for player in self.alive_players() if self.is_known_mafia_team(player)]

    def alive_cult_team(self) -> list[Player]:
        return [player for player in self.alive_players() if self.is_cult_team(player)]

    def alive_role_count(self, role: Role) -> int:
        return sum(1 for player in self.alive_players() if player.role == role)

    def get_player(self, user_id: int) -> Player | None:
        return next((player for player in self.players if player.user_id == user_id), None)

    def is_mafia_team(self, player: Player) -> bool:
        return player.role in MAFIA_TEAM_ROLES

    def is_cult_team(self, player: Player) -> bool:
        return player.role == Role.CULT_LEADER or player.user_id in self.culted_ids

    def is_known_mafia_team(self, player: Player) -> bool:
        if player.role in {Role.MAFIA, Role.VILLAIN}:
            return True
        if player.role == Role.SPY:
            return player.user_id in self.spy_contacted
        if player.role == Role.CONTRACTOR:
            return player.user_id in self.contractor_contacted
        if player.role == Role.WITCH:
            return player.user_id in self.witch_contacted
        if player.role == Role.SCIENTIST:
            return player.user_id in self.scientist_contacted
        if player.role == Role.MADAM:
            return player.user_id in self.madam_contacted
        if player.role == Role.GODFATHER:
            return player.user_id in self.godfather_contacted
        return False

    def is_frog(self, player: Player) -> bool:
        return player.alive and player.user_id in self.frog_user_ids

    def is_madam_seduced(self, player: Player) -> bool:
        return player.alive and player.user_id in self.madam_seduced_ids

    def visible_role(self, player: Player) -> Role:
        return Role.FROG if self.is_frog(player) else player.role

    def can_mafia_attack(self, player: Player, attacker_id: int | None = None) -> bool:
        return player.alive

    def is_citizen_team(self, player: Player) -> bool:
        return not self.is_mafia_team(player) and not self.is_cult_team(player) and player.role != Role.JOKER

    def night_action_actors(self) -> list[Player]:
        self.ensure_godfather_auto_contact()
        alive = self.alive_players()
        actors: list[Player] = []
        for player in alive:
            if self.is_frog(player):
                continue
            if self.is_madam_seduced(player) and not self.is_mafia_team(player):
                continue
            if player.role == Role.MAFIA and any(
                self.can_mafia_attack(target, player.user_id) for target in alive
            ):
                actors.append(player)
            elif player.role == Role.DOCTOR:
                actors.append(player)
            elif player.role == Role.NURSE and self._nurse_can_act(player):
                actors.append(player)
            elif player.role in {Role.POLICE, Role.DETECTIVE, Role.SPY, Role.TERRORIST} and any(
                target.user_id != player.user_id for target in alive
            ):
                actors.append(player)
            elif player.role == Role.VIGILANTE and self.vigilante_execution_targets(player):
                actors.append(player)
            elif player.role == Role.REPORTER and self._reporter_can_act(player):
                actors.append(player)
            elif player.role == Role.CONTRACTOR and self._contractor_can_act(player):
                actors.append(player)
            elif player.role == Role.WITCH and any(
                target.user_id != player.user_id for target in alive
            ):
                actors.append(player)
            elif player.role == Role.SHAMAN and self.unpurified_dead_players():
                actors.append(player)
            elif (
                player.role == Role.PRIEST
                and player.user_id not in self.priest_used_ids
                and self.unpurified_dead_players()
            ):
                actors.append(player)
            elif (
                player.role == Role.GODFATHER
                and player.user_id in self.godfather_contacted
                and any(target.user_id != player.user_id for target in alive)
            ):
                actors.append(player)
            elif player.role == Role.CULT_LEADER and self._cult_leader_can_act(player):
                actors.append(player)
            elif player.role == Role.FANATIC and any(target.user_id != player.user_id for target in alive):
                actors.append(player)
        return actors

    def all_night_actions_submitted(self) -> bool:
        if self.phase != Phase.NIGHT:
            return False

        for actor in self.night_action_actors():
            if actor.role == Role.MAFIA and actor.user_id not in self.mafia_targets:
                return False
            if actor.role == Role.DOCTOR and actor.user_id not in self.doctor_targets:
                return False
            if (
                actor.role == Role.NURSE
                and actor.user_id not in self.nurse_targets
                and actor.user_id not in self.nurse_prescription_targets
            ):
                return False
            if actor.role == Role.POLICE and actor.user_id not in self.police_targets:
                return False
            if actor.role == Role.VIGILANTE and actor.user_id not in self.vigilante_targets:
                return False
            if (
                actor.role == Role.REPORTER
                and actor.user_id not in self.reporter_targets
                and actor.user_id not in self.reporter_skip_submitted
            ):
                return False
            if actor.role == Role.DETECTIVE and actor.user_id not in self.detective_targets:
                return False
            if actor.role == Role.SHAMAN and actor.user_id not in self.shaman_targets:
                return False
            if actor.role == Role.PRIEST and actor.user_id not in self.priest_targets:
                return False
            if actor.role == Role.SPY and not self.spy_targets.get(actor.user_id):
                return False
            if actor.role == Role.SPY and actor.user_id in self.spy_bonus_pending:
                return False
            if (
                actor.role == Role.CONTRACTOR
                and actor.user_id not in self.contractor_contact_targets
                and actor.user_id not in self.contractor_contracts
            ):
                return False
            if actor.role == Role.WITCH and actor.user_id not in self.witch_targets:
                return False
            if actor.role == Role.GODFATHER and actor.user_id not in self.godfather_targets:
                return False
            if actor.role == Role.TERRORIST and actor.user_id not in self.terrorist_action_submitted:
                return False
            if actor.role == Role.CULT_LEADER and actor.user_id not in self.cult_targets:
                return False
            if actor.role == Role.FANATIC and actor.user_id not in self.fanatic_targets:
                return False
        return True

    def _nurse_can_act(self, player: Player) -> bool:
        if player.user_id in self.nurse_contacted:
            return not self.alive_role_count(Role.DOCTOR) and bool(self.alive_players())
        return any(target.user_id != player.user_id for target in self.alive_players())

    def _cult_leader_can_act(self, player: Player) -> bool:
        return self.day_number % 2 == 1 and any(
            target.user_id != player.user_id and target.user_id not in self.culted_ids
            for target in self.alive_players()
        )

    def spy_can_use_bonus_action(self, actor_id: int) -> bool:
        return self.phase == Phase.NIGHT and self._is_alive(actor_id) and actor_id in self.spy_bonus_pending

    def contractor_can_use_contract(self, actor_id: int) -> bool:
        actor = self.get_player(actor_id)
        return bool(
            self.phase == Phase.NIGHT
            and actor
            and actor.alive
            and actor.role == Role.CONTRACTOR
            and self.day_number >= 2
            and len(self.contractor_contract_targets(actor)) >= 2
        )

    def contractor_contract_targets(self, actor: Player) -> list[Player]:
        return [
            player
            for player in self.alive_players()
            if player.user_id != actor.user_id
            and player.role not in INVESTIGATION_ROLES
            and not self.is_publicly_revealed(player)
        ]

    def is_publicly_revealed(self, player: Player) -> bool:
        return player.user_id in self.publicly_revealed_ids

    def hacker_day_actors(self) -> list[Player]:
        if self.phase != Phase.DAY:
            return []
        return [
            player
            for player in self.alive_players()
            if player.role == Role.HACKER
            and not self.is_madam_seduced(player)
            and player.user_id not in self.hacker_used_ids
            and any(target.user_id != player.user_id for target in self.alive_players())
        ]

    def submit_hacker_action(self, actor_id: int, target_id: int) -> str:
        if self.phase != Phase.DAY:
            raise ValueError("해킹은 낮에만 사용할 수 있습니다.")
        actor = self._require_alive(actor_id)
        if actor.role != Role.HACKER:
            raise ValueError("해커만 해킹을 사용할 수 있습니다.")
        if self.is_madam_seduced(actor):
            raise ValueError("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.")
        if actor_id in self.hacker_used_ids:
            raise ValueError("해킹은 이미 사용했습니다.")
        target = self._require_alive(target_id)
        if actor_id == target_id:
            raise ValueError("해커는 자기 자신을 해킹할 수 없습니다.")

        self.hacker_targets[actor_id] = target_id
        self.hacker_pending_results[actor_id] = target_id
        self.hacker_proxy_targets[actor_id] = target_id
        self.hacker_used_ids.add(actor_id)
        return f"해킹 대상: {target.name}"

    def vigilante_day_actors(self) -> list[Player]:
        if self.phase != Phase.DAY:
            return []
        return [
            player
            for player in self.alive_players()
            if player.role == Role.VIGILANTE
            and not self.is_madam_seduced(player)
            and player.user_id not in self.vigilante_investigation_used_ids
            and player.user_id not in self.vigilante_execution_used_ids
            and any(target.user_id != player.user_id for target in self.alive_players())
        ]

    def submit_vigilante_investigation(self, actor_id: int, target_id: int) -> str:
        if self.phase != Phase.DAY:
            raise ValueError("자경단원 조사는 낮에만 사용할 수 있습니다.")
        actor = self._require_alive(actor_id)
        if actor.role != Role.VIGILANTE:
            raise ValueError("자경단원만 숙청 조사를 사용할 수 있습니다.")
        if self.is_madam_seduced(actor):
            raise ValueError("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.")
        if actor_id in self.vigilante_investigation_used_ids:
            raise ValueError("자경단원 조사는 이미 사용했습니다.")
        if actor_id in self.vigilante_execution_used_ids:
            raise ValueError("숙청 처형을 이미 시도해 더 이상 조사할 수 없습니다.")
        target = self._require_alive(target_id)
        if actor_id == target_id:
            raise ValueError("자경단원은 자기 자신을 조사할 수 없습니다.")

        self.vigilante_pending_results[actor_id] = target_id
        self.vigilante_investigation_used_ids.add(actor_id)
        return f"숙청 조사 대상: {target.name}"

    def consume_vigilante_results(self) -> dict[int, str]:
        results: dict[int, str] = {}
        for actor_id, target_id in list(self.vigilante_pending_results.items()):
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target:
                continue
            if self.is_known_mafia_team(target):
                self.vigilante_known_enemy_ids.setdefault(actor_id, set()).add(target_id)
                result_text = "마피아팀입니다"
            else:
                result_text = "마피아팀이 아닙니다"
            results[actor_id] = f"[숙청] {target.name} 님은 **{result_text}**."
        self.vigilante_pending_results.clear()
        return results

    def vigilante_execution_targets(self, actor: Player) -> list[Player]:
        if actor.role != Role.VIGILANTE or not actor.alive:
            return []
        known_enemy_ids = self.vigilante_known_enemy_ids.get(actor.user_id, set())
        targets: list[Player] = []
        for player in self.alive_players():
            if player.user_id == actor.user_id:
                continue
            if player.user_id in known_enemy_ids:
                targets.append(player)
                continue
            if self.is_publicly_revealed(player) and self.is_mafia_team(player):
                targets.append(player)
        return targets

    def consume_hacker_results(self) -> dict[int, str]:
        results: dict[int, str] = {}
        for actor_id, target_id in list(self.hacker_pending_results.items()):
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target:
                continue
            results[actor_id] = (
                f"[해킹] {target.name} 님의 직업은 **{self.visible_role(target).value}** 입니다."
            )
        self.hacker_pending_results.clear()
        return results

    def all_day_votes_submitted(self) -> bool:
        if self.phase != Phase.VOTE:
            return False
        return all(player.user_id in self.day_votes for player in self.alive_players())

    def all_confirm_votes_submitted(self) -> bool:
        if self.phase != Phase.CONFIRM_VOTE:
            return False
        return all(player.user_id in self.confirm_votes for player in self.alive_players())

    def submit_night_action(self, actor_id: int, target_id: int | None) -> str:
        if self.phase != Phase.NIGHT:
            raise ValueError("지금은 밤이 아닙니다.")
        actor = self._require_alive(actor_id)
        if self.is_frog(actor):
            raise ValueError("개구리 상태에서는 밤 행동을 사용할 수 없습니다.")
        if self.is_madam_seduced(actor) and not self.is_mafia_team(actor):
            raise ValueError("마담에게 유혹당한 상태에서는 능력을 사용할 수 없습니다.")

        if actor.role == Role.MAFIA:
            if target_id is None:
                raise ValueError("공격 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            previous_target_id = self.mafia_display_targets.get(actor_id)
            target = self._proxy_target(selected_target)
            self.mafia_targets[actor_id] = target.user_id
            self.mafia_display_targets[actor_id] = selected_target.user_id
            if previous_target_id and previous_target_id != selected_target.user_id:
                return f"공격 대상 변경: {selected_target.name}"
            if previous_target_id == selected_target.user_id:
                return f"공격 대상 유지: {selected_target.name}"
            return f"공격 대상: {selected_target.name}"

        if actor.role == Role.DOCTOR:
            if target_id is None:
                raise ValueError("보호 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.doctor_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            target = self._proxy_target(selected_target)
            self.doctor_targets[actor_id] = target.user_id
            return f"보호 대상: {selected_target.name}"

        if actor.role == Role.NURSE:
            if target_id is None:
                raise ValueError("대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.nurse_targets or actor_id in self.nurse_prescription_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor.user_id in self.nurse_contacted:
                if self.alive_role_count(Role.DOCTOR):
                    raise ValueError("의사가 살아있는 동안 간호사는 치료 대상을 직접 선택하지 않습니다.")
                target = self._proxy_target(selected_target)
                self.nurse_targets[actor_id] = target.user_id
                return f"치료 대상: {selected_target.name}"
            if actor_id == target_id:
                raise ValueError("간호사는 자기 자신을 처방할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.nurse_prescription_targets[actor_id] = target.user_id
            if target.role == Role.DOCTOR:
                self.nurse_contacted.add(actor_id)
                self.nurse_contacts_this_night.append(actor_id)
                return "[처방] 의사와 접선했습니다."
            return "[처방] 대상은 의사가 아닙니다."

        if actor.role == Role.POLICE:
            if target_id is None:
                raise ValueError("조사 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.police_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("경찰은 자기 자신을 조사할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.police_targets[actor_id] = target.user_id
            return f"조사 투표 대상: {selected_target.name}"

        if actor.role == Role.VIGILANTE:
            if target_id is None:
                raise ValueError("숙청 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.vigilante_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id in self.vigilante_execution_used_ids:
                raise ValueError("숙청 처형은 이미 사용했습니다.")
            if actor_id == target_id:
                raise ValueError("자경단원은 자기 자신을 숙청할 수 없습니다.")
            available_target_ids = {
                player.user_id for player in self.vigilante_execution_targets(actor)
            }
            if target_id not in available_target_ids:
                raise ValueError("자경단원은 확실하게 알고 있는 마피아팀만 숙청할 수 있습니다.")

            target = self._proxy_target(selected_target)
            self.vigilante_targets[actor_id] = target.user_id
            self.vigilante_execution_used_ids.add(actor_id)
            self.vigilante_investigation_used_ids.add(actor_id)
            return f"숙청 대상: {selected_target.name}"

        if actor.role == Role.REPORTER:
            if self.day_number < 2:
                raise ValueError("엠바고로 첫 번째 낮에는 기사를 낼 수 없습니다.")
            if actor_id in self.reporter_used_ids:
                raise ValueError("기자는 특종을 이미 사용했습니다.")
            if actor_id in self.reporter_targets or actor_id in self.reporter_skip_submitted:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if target_id is None:
                self.reporter_skip_submitted.add(actor_id)
                return "이번 밤에는 특종을 사용하지 않습니다."
            selected_target = self._require_alive(target_id)
            if actor_id == target_id:
                raise ValueError("기자는 자기 자신을 취재할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.reporter_targets[actor_id] = target.user_id
            self.reporter_used_ids.add(actor_id)
            return f"특종 대상: {selected_target.name}"

        if actor.role == Role.DETECTIVE:
            if target_id is None:
                raise ValueError("추적 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.detective_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("사립탐정은 자기 자신을 추적할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.detective_targets[actor_id] = target.user_id
            return f"추적 대상: {selected_target.name}"

        if actor.role == Role.SHAMAN:
            if target_id is None:
                raise ValueError("성불 대상을 선택해야 합니다.")
            target = self._require_player(target_id)
            if actor_id in self.shaman_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if target.alive:
                raise ValueError("영매는 사망한 참가자만 성불할 수 있습니다.")
            if target.user_id in self.purified_dead_ids:
                raise ValueError("이미 성불한 사망자입니다.")
            self.shaman_targets[actor_id] = target_id
            return f"성불 대상: {target.name}"

        if actor.role == Role.PRIEST:
            if target_id is None:
                raise ValueError("소생 대상을 선택해야 합니다.")
            target = self._require_player(target_id)
            if actor_id in self.priest_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id in self.priest_used_ids:
                raise ValueError("소생은 이미 사용했습니다.")
            if target.alive:
                raise ValueError("성직자는 사망한 참가자만 소생시킬 수 있습니다.")
            if target.user_id in self.purified_dead_ids:
                raise ValueError("성불 상태인 사망자는 소생시킬 수 없습니다.")
            self.priest_targets[actor_id] = target_id
            self.priest_used_ids.add(actor_id)
            return f"소생 대상: {target.name}"

        if actor.role == Role.SPY:
            if target_id is None:
                raise ValueError("첩보 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if self._spy_actions_used(actor_id) >= self._spy_action_limit(actor_id):
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("스파이는 자기 자신을 지목할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.spy_targets.setdefault(actor_id, []).append(target.user_id)

            lines = [f"[첩보] {target.name} 님의 직업은 **{self.visible_role(target).value}** 입니다."]
            if actor_id not in self.spy_contacted and target.role == Role.MAFIA:
                self.spy_contacted.add(actor_id)
                self.spy_bonus_pending.add(actor_id)
                self.spy_contacts_this_night.append(actor_id)
                lines.append("[접선] 마피아와 접선했습니다. 이번 밤에 한 번 더 첩보를 사용할 수 있습니다.")

            if actor_id in self.spy_bonus_pending and self._spy_actions_used(actor_id) >= 2:
                self.spy_bonus_pending.discard(actor_id)
            return "\n".join(lines)

        if actor.role == Role.TERRORIST:
            if target_id is None:
                raise ValueError("지목 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.terrorist_action_submitted:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("테러리스트는 자기 자신을 지목할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.terrorist_targets[actor_id] = target.user_id
            self.terrorist_action_submitted.add(actor_id)
            return f"지목 대상: {selected_target.name}"

        if actor.role == Role.WITCH:
            if target_id is None:
                raise ValueError("저주 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.witch_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("마녀는 자기 자신을 저주할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.witch_targets[actor_id] = target.user_id
            return f"저주 대상: {selected_target.name}"

        if actor.role == Role.GODFATHER:
            if target_id is None:
                raise ValueError("확정 처치 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            self.ensure_godfather_auto_contact()
            if actor_id not in self.godfather_contacted:
                raise ValueError("대부는 세 번째 밤부터 마피아 팀과 자동 접선되어 행동할 수 있습니다.")
            if actor_id in self.godfather_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("대부는 자기 자신을 지목할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.godfather_targets[actor_id] = target.user_id
            return f"확정 처치 대상: {selected_target.name}"

        if actor.role == Role.CULT_LEADER:
            if target_id is None:
                raise ValueError("포교 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if self.day_number % 2 != 1:
                raise ValueError("교주는 홀수날 밤에만 포교할 수 있습니다.")
            if actor_id in self.cult_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("교주는 자기 자신을 포교할 수 없습니다.")
            target = self._proxy_target(selected_target)
            if self.is_cult_team(target):
                raise ValueError("이미 교주팀인 대상은 포교할 수 없습니다.")
            self.cult_targets[actor_id] = target.user_id
            if target.role == Role.PRIEST and not self.is_frog(target):
                return f"포교 대상: {selected_target.name}"
            if not self.is_mafia_team(target) and target.role != Role.CULT_LEADER:
                self.culted_ids.add(target.user_id)
                self.cult_bells_this_night += 1
            return f"포교 대상: {selected_target.name}"

        if actor.role == Role.FANATIC:
            if target_id is None:
                raise ValueError("추종 대상을 선택해야 합니다.")
            selected_target = self._require_alive(target_id)
            if actor_id in self.fanatic_targets:
                raise ValueError("이미 이번 밤 행동을 선택했습니다.")
            if actor_id == target_id:
                raise ValueError("광신도는 자기 자신을 추종할 수 없습니다.")
            target = self._proxy_target(selected_target)
            self.fanatic_targets[actor_id] = target.user_id
            if target.role == Role.CULT_LEADER and actor_id not in self.culted_ids:
                self.culted_ids.add(actor_id)
                self.cult_bells_this_night += 1
            return f"추종 대상: {selected_target.name}"

        raise ValueError(f"{actor.role.value}은/는 밤 행동이 없습니다.")

    def submit_contractor_contact(self, actor_id: int, target_id: int) -> str:
        if self.phase != Phase.NIGHT:
            raise ValueError("지금은 밤이 아닙니다.")
        actor = self._require_alive(actor_id)
        if actor.role != Role.CONTRACTOR:
            raise ValueError("청부업자만 동업을 사용할 수 있습니다.")
        if self.is_frog(actor):
            raise ValueError("개구리 상태에서는 밤 행동을 사용할 수 없습니다.")
        if actor_id in self.contractor_contact_targets or actor_id in self.contractor_contracts:
            raise ValueError("이미 이번 밤 행동을 선택했습니다.")
        if actor_id in self.contractor_contacted:
            raise ValueError("이미 마피아와 접선했습니다.")

        selected_target = self._require_alive(target_id)
        if actor_id == target_id:
            raise ValueError("청부업자는 자기 자신을 지목할 수 없습니다.")

        target = self._proxy_target(selected_target)
        self.contractor_contact_targets[actor_id] = target.user_id
        if target.role == Role.MAFIA:
            self.contractor_contacted.add(actor_id)
            self.contractor_contacts_this_night.append(actor_id)
            return "[동업] 마피아와 접선했습니다."
        return "[동업] 접선에 실패했습니다."

    def submit_contractor_contract(
        self,
        actor_id: int,
        first_target_id: int,
        first_role: Role,
        second_target_id: int,
        second_role: Role,
    ) -> str:
        if self.phase != Phase.NIGHT:
            raise ValueError("지금은 밤이 아닙니다.")
        actor = self._require_alive(actor_id)
        if actor.role != Role.CONTRACTOR:
            raise ValueError("청부업자만 청부를 사용할 수 있습니다.")
        if self.is_frog(actor):
            raise ValueError("개구리 상태에서는 밤 행동을 사용할 수 없습니다.")
        if self.day_number < 2:
            raise ValueError("청부는 두 번째 밤부터 사용할 수 있습니다.")
        if actor_id in self.contractor_contact_targets or actor_id in self.contractor_contracts:
            raise ValueError("이미 이번 밤 행동을 선택했습니다.")
        if first_target_id == second_target_id:
            raise ValueError("청부 대상 두 명은 서로 달라야 합니다.")
        if first_role not in CONTRACTOR_GUESSABLE_ROLES or second_role not in CONTRACTOR_GUESSABLE_ROLES:
            raise ValueError("청부로 추측할 수 없는 직업입니다.")

        first_selected_target = self._require_alive(first_target_id)
        second_selected_target = self._require_alive(second_target_id)
        if actor_id in {first_target_id, second_target_id}:
            raise ValueError("청부업자는 자기 자신을 지목할 수 없습니다.")
        first_target = first_selected_target
        second_target = second_selected_target
        if first_target.user_id == second_target.user_id:
            raise ValueError("청부 대상 두 명은 서로 달라야 합니다.")
        if (
            first_selected_target.role in INVESTIGATION_ROLES
            or second_selected_target.role in INVESTIGATION_ROLES
            or first_target.role in INVESTIGATION_ROLES
            or second_target.role in INVESTIGATION_ROLES
        ):
            raise ValueError("경찰, 요원, 자경단원은 청부 대상으로 지목할 수 없습니다.")
        if (
            self.is_publicly_revealed(first_selected_target)
            or self.is_publicly_revealed(second_selected_target)
            or self.is_publicly_revealed(first_target)
            or self.is_publicly_revealed(second_target)
        ):
            raise ValueError("게임 채널에 직업이 공개된 사람은 청부 대상으로 지목할 수 없습니다.")

        self.contractor_contracts[actor_id] = (
            (first_target.user_id, first_role),
            (second_target.user_id, second_role),
        )
        return (
            "[청부] 암살 대상을 선택했습니다.\n"
            f"- {first_selected_target.name}: {first_role.value}\n"
            f"- {second_selected_target.name}: {second_role.value}"
        )

    def apply_witch_curses(self) -> tuple[list[Player], list[int]]:
        cursed_players: list[Player] = []
        contacts: list[int] = []
        for actor_id, target_id in list(self.witch_targets.items()):
            if actor_id in self.witch_curse_applied_actor_ids:
                continue
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            self.witch_curse_applied_actor_ids.add(actor_id)
            if not actor or not actor.alive or self.is_frog(actor) or not target or not target.alive:
                continue

            self.frog_user_ids.add(target.user_id)
            self._clear_night_action(target.user_id)
            cursed_players.append(target)
            self._resolve_priest_cult_after_curse(target)
            if target.role == Role.MAFIA and actor_id not in self.witch_contacted:
                self.witch_contacted.add(actor_id)
                self.witch_contacts_this_night.append(actor_id)
                contacts.append(actor_id)
        return cursed_players, contacts

    def _resolve_priest_cult_after_curse(self, target: Player) -> None:
        if target.role != Role.PRIEST or target.user_id in self.culted_ids:
            return
        for actor_id, target_id in self.cult_targets.items():
            actor = self.get_player(actor_id)
            if (
                target_id == target.user_id
                and actor
                and actor.alive
                and actor.role == Role.CULT_LEADER
            ):
                self.culted_ids.add(target.user_id)
                self.cult_bells_this_night += 1
                return

    def restore_frogs(self) -> list[Player]:
        restored: list[Player] = []
        for user_id in list(self.frog_user_ids):
            player = self.get_player(user_id)
            if player:
                restored.append(player)
            self.frog_user_ids.discard(user_id)
        return restored

    def revive_pending_scientists(self) -> list[Player]:
        revived: list[Player] = []
        for user_id in list(self.scientist_pending_revive_ids):
            player = self.get_player(user_id)
            self.scientist_pending_revive_ids.discard(user_id)
            if not player or player.alive:
                continue
            player.alive = True
            self.scientist_contacted.add(player.user_id)
            self.publicly_revealed_ids.add(player.user_id)
            revived.append(player)
        return revived

    def has_pending_scientist_revive(self) -> bool:
        return any(
            bool(player and not player.alive)
            for player in (self.get_player(user_id) for user_id in self.scientist_pending_revive_ids)
        )

    def _mark_dead(self, player: Player) -> None:
        if not player.alive:
            return
        player.alive = False
        self.frog_user_ids.discard(player.user_id)
        if player.role == Role.SCIENTIST and player.user_id not in self.scientist_revive_used_ids:
            self.scientist_revive_used_ids.add(player.user_id)
            self.scientist_pending_revive_ids.add(player.user_id)
            self.scientist_contacted.add(player.user_id)
        if player.role == Role.MADAM and not any(
            other.alive and other.role == Role.MADAM for other in self.players
        ):
            release_day = self.day_number + 1
            for user_id in self.madam_seduced_ids:
                self.madam_seduction_release_days[user_id] = max(
                    self.madam_seduction_release_days.get(user_id, release_day),
                    release_day,
                )

    def _clear_night_action(self, actor_id: int) -> None:
        self.mafia_targets.pop(actor_id, None)
        self.mafia_display_targets.pop(actor_id, None)
        self.doctor_targets.pop(actor_id, None)
        self.police_targets.pop(actor_id, None)
        self.vigilante_targets.pop(actor_id, None)
        self.detective_targets.pop(actor_id, None)
        self.shaman_targets.pop(actor_id, None)
        if actor_id in self.priest_targets:
            self.priest_targets.pop(actor_id, None)
            self.priest_used_ids.discard(actor_id)
        self.godfather_targets.pop(actor_id, None)
        # Terrorist designation is a persistent "currently marked" target.
        # Curses cancel the current night's action state, but should not erase
        # the target used by vote-death retaliation.
        self.terrorist_action_submitted.discard(actor_id)
        if actor_id in self.reporter_targets:
            self.reporter_targets.pop(actor_id, None)
            self.reporter_used_ids.discard(actor_id)
        self.reporter_skip_submitted.discard(actor_id)
        if actor_id in self.spy_contacts_this_night:
            self.spy_contacted.discard(actor_id)
            self.spy_contacts_this_night = [
                user_id for user_id in self.spy_contacts_this_night if user_id != actor_id
            ]
        self.spy_targets.pop(actor_id, None)
        self.spy_bonus_pending.discard(actor_id)
        if actor_id in self.contractor_contacts_this_night:
            self.contractor_contacted.discard(actor_id)
            self.contractor_contacts_this_night = [
                user_id for user_id in self.contractor_contacts_this_night if user_id != actor_id
            ]
        self.contractor_contact_targets.pop(actor_id, None)
        self.contractor_contracts.pop(actor_id, None)
        if actor_id in self.witch_contacts_this_night:
            self.witch_contacted.discard(actor_id)
            self.witch_contacts_this_night = [
                user_id for user_id in self.witch_contacts_this_night if user_id != actor_id
            ]
        self.witch_targets.pop(actor_id, None)
        self.witch_curse_applied_actor_ids.discard(actor_id)

    def resolve_night(self) -> NightResult:
        if self.phase != Phase.NIGHT:
            raise ValueError("밤 단계만 정산할 수 있습니다.")

        self.ensure_godfather_auto_contact()
        self.apply_witch_curses()
        timed_cult_bells = self.consume_cult_bells()
        witch_contacts = list(self.witch_contacts_this_night)
        godfather_attackers = {
            actor_id: target_id
            for actor_id, target_id in self.godfather_targets.items()
            if actor_id in self.godfather_contacted
        }
        mafia_attackers = {**self.mafia_targets, **godfather_attackers}
        mafia_target_id = self._majority_target(
            mafia_attackers,
        )
        healing_targets = dict(self.doctor_targets)
        if not self.alive_role_count(Role.DOCTOR):
            healing_targets.update(self.nurse_targets)
        protected_id = self._majority_target(healing_targets)
        police_target_id = self._majority_target(
            self.police_targets,
        )
        godfather_target_id = (
            mafia_target_id
            if mafia_target_id and mafia_target_id in godfather_attackers.values()
            else None
        )

        mafia_target = self.get_player(mafia_target_id) if mafia_target_id else None
        protected = self.get_player(protected_id) if protected_id else None
        police_target = self.get_player(police_target_id) if police_target_id else None
        godfather_target = self.get_player(godfather_target_id) if godfather_target_id else None

        detective_results = self._resolve_detective_results(
            mafia_target_id,
            protected_id,
            police_target_id,
            godfather_target_id,
        )
        spy_results, spy_contacts = self._resolve_spy_results()
        contractor_results, contractor_contacts, contractor_kills = self._resolve_contractor_results()
        godfather_results = self._resolve_godfather_results()
        shaman_results, shaman_purifications = self._resolve_shaman_results()
        reporter_results = self._resolve_reporter_results()
        vigilante_results, vigilante_kills = self._resolve_vigilante_results()
        nurse_results, nurse_contacts = self._resolve_nurse_results()
        cult_results, cult_bells = self._resolve_cult_results()
        fanatic_results, fanatic_bells = self._resolve_fanatic_results()
        fanatic_inherits = self.ensure_fanatic_reincarnation()

        killed_players: list[Player] = []
        killed_by_mafia_team_ids: set[int] = set()
        soldier_blocks: list[Player] = []
        lover_sacrifices: list[tuple[Player, Player]] = []
        enhanced_protection = protected_id is not None and self._nurse_enhanced_heal_active()
        if mafia_target and mafia_target.alive:
            lover_savior = self._lover_sacrifice_for(mafia_target)
            if lover_savior:
                self._mark_dead(lover_savior)
                killed_players.append(lover_savior)
                killed_by_mafia_team_ids.add(lover_savior.user_id)
                lover_sacrifices.append((lover_savior, mafia_target))
            elif mafia_target.user_id == protected_id and enhanced_protection:
                pass
            elif godfather_target_id:
                self._mark_dead(mafia_target)
                killed_players.append(mafia_target)
                killed_by_mafia_team_ids.add(mafia_target.user_id)
            elif mafia_target.user_id != protected_id and (
                mafia_target.role == Role.SOLDIER
                and mafia_target.user_id not in self.soldier_bulletproof_used
            ):
                self.soldier_bulletproof_used.add(mafia_target.user_id)
                self.publicly_revealed_ids.add(mafia_target.user_id)
                soldier_blocks.append(mafia_target)
            elif mafia_target.user_id != protected_id:
                self._mark_dead(mafia_target)
                killed_players.append(mafia_target)
                killed_by_mafia_team_ids.add(mafia_target.user_id)

        for contractor_kill in contractor_kills:
            if contractor_kill.alive:
                self._mark_dead(contractor_kill)
            if contractor_kill not in killed_players:
                killed_players.append(contractor_kill)
            killed_by_mafia_team_ids.add(contractor_kill.user_id)

        for vigilante_kill in vigilante_kills:
            if vigilante_kill.alive:
                self._mark_dead(vigilante_kill)
            if vigilante_kill not in killed_players:
                killed_players.append(vigilante_kill)

        terrorist_retaliations = self._resolve_terrorist_night_retaliations(
            killed_by_mafia_team_ids,
            killed_players,
        )
        priest_results, priest_revives = self._resolve_priest_results(killed_players)

        graverobber_results = self._resolve_graverobbers(killed_players)
        agent_results = self._resolve_agent_results()
        for user_id in self.ensure_fanatic_reincarnation():
            if user_id not in fanatic_inherits:
                fanatic_inherits.append(user_id)

        self.mafia_targets.clear()
        self.mafia_display_targets.clear()
        self.doctor_targets.clear()
        self.nurse_targets.clear()
        self.nurse_prescription_targets.clear()
        self.nurse_contacts_this_night.clear()
        self.police_targets.clear()
        self.vigilante_targets.clear()
        self.reporter_targets.clear()
        self.reporter_skip_submitted.clear()
        self.detective_targets.clear()
        self.shaman_targets.clear()
        self.priest_targets.clear()
        self.spy_targets.clear()
        self.spy_bonus_pending.clear()
        self.spy_contacts_this_night.clear()
        self.contractor_contact_targets.clear()
        self.contractor_contracts.clear()
        self.contractor_contacts_this_night.clear()
        self.witch_targets.clear()
        self.witch_contacts_this_night.clear()
        self.witch_curse_applied_actor_ids.clear()
        self.godfather_targets.clear()
        self.terrorist_action_submitted.clear()
        self.cult_targets.clear()
        self.fanatic_targets.clear()
        self.cult_bells_this_night = 0
        self.day_votes.clear()
        self.confirm_votes.clear()
        self.phase = Phase.DAY
        return NightResult(
            killed=killed_players[0] if killed_players else None,
            protected=protected,
            mafia_target=mafia_target,
            police_target=police_target,
            police_target_is_mafia=(
                self.is_known_mafia_team(police_target) if police_target is not None else None
            ),
            killed_players=killed_players,
            detective_results=detective_results,
            spy_results=spy_results,
            spy_contacts=spy_contacts,
            contractor_results=contractor_results,
            contractor_contacts=contractor_contacts,
            contractor_kills=contractor_kills,
            witch_contacts=witch_contacts,
            godfather_results=godfather_results,
            godfather_contacts=[],
            graverobber_results=graverobber_results,
            terrorist_retaliations=terrorist_retaliations,
            soldier_blocks=soldier_blocks,
            lover_sacrifices=lover_sacrifices,
            shaman_results=shaman_results,
            shaman_purifications=shaman_purifications,
            priest_results=priest_results,
            priest_revives=priest_revives,
            agent_results=agent_results,
            reporter_results=reporter_results,
            vigilante_results=vigilante_results,
            vigilante_kills=vigilante_kills,
            nurse_results=nurse_results,
            nurse_contacts=nurse_contacts,
            cult_results=cult_results,
            fanatic_results=fanatic_results,
            fanatic_inherits=fanatic_inherits,
            cult_bells=timed_cult_bells + cult_bells + fanatic_bells,
        )

    def ensure_godfather_auto_contact(self) -> list[int]:
        if self.day_number < 3:
            return []
        contacted: list[int] = []
        for player in self.alive_players():
            if player.role == Role.GODFATHER and player.user_id not in self.godfather_contacted:
                self.godfather_contacted.add(player.user_id)
                contacted.append(player.user_id)
        return contacted

    def _contact_mafia_team_member(self, player: Player) -> None:
        if player.role == Role.SPY:
            self.spy_contacted.add(player.user_id)
        elif player.role == Role.CONTRACTOR:
            self.contractor_contacted.add(player.user_id)
        elif player.role == Role.WITCH:
            self.witch_contacted.add(player.user_id)
        elif player.role == Role.SCIENTIST:
            self.scientist_contacted.add(player.user_id)
        elif player.role == Role.MADAM:
            self.madam_contacted.add(player.user_id)
        elif player.role == Role.GODFATHER:
            self.godfather_contacted.add(player.user_id)

    def _apply_madam_seduction(self, live_votes: dict[int, int | None]) -> list[Player]:
        seduced: list[Player] = []
        for voter_id, target_id in live_votes.items():
            if target_id is None:
                continue
            voter = self.get_player(voter_id)
            target = self.get_player(target_id)
            if (
                not voter
                or not voter.alive
                or not target
                or not target.alive
                or target.role != Role.MADAM
                or voter.user_id == target.user_id
            ):
                continue
            if voter.user_id not in self.madam_seduced_ids:
                seduced.append(voter)
            self.madam_seduced_ids.add(voter.user_id)
            self.madam_seduction_release_days.pop(voter.user_id, None)
            if self.is_mafia_team(voter):
                self._contact_mafia_team_member(voter)
                self.madam_contacted.add(target.user_id)
        return seduced

    def start_vote(self) -> None:
        if self.phase != Phase.DAY:
            raise ValueError("낮 단계에서만 투표를 시작할 수 있습니다.")
        self.phase = Phase.VOTE
        self.day_votes.clear()
        self.confirm_votes.clear()

    def submit_day_vote(self, voter_id: int, target_id: int | None) -> str:
        if self.phase != Phase.VOTE:
            raise ValueError("지금은 투표 시간이 아닙니다.")
        voter = self._require_alive(voter_id)
        if target_id is None:
            self.day_votes[voter.user_id] = None
            return "투표 대상: 스킵"

        target = self._require_alive(target_id)
        self.day_votes[voter.user_id] = target.user_id
        return f"투표 대상: {target.name}"

    def resolve_nomination_vote(self) -> VoteResult:
        if self.phase != Phase.VOTE:
            raise ValueError("투표 단계만 정산할 수 있습니다.")

        live_votes = {
            voter_id: target_id
            for voter_id, target_id in self.day_votes.items()
            if self._is_alive(voter_id) and (target_id is None or self._is_alive(target_id))
        }
        madam_seduced = self._apply_madam_seduction(live_votes)
        if not live_votes:
            self._advance_to_next_night()
            return VoteResult(executed=None, tied=False, skipped=False, madam_seduced=madam_seduced)

        counts: Counter[int | None] = Counter()
        for voter_id, target_id in live_votes.items():
            counts[target_id] += self._vote_weight(voter_id)
        vote_counts = dict(counts)
        highest = max(counts.values())
        top_targets = [target_id for target_id, count in counts.items() if count == highest]
        if len(top_targets) != 1:
            self._advance_to_next_night()
            return VoteResult(
                executed=None,
                tied=True,
                skipped=False,
                vote_counts=vote_counts,
                madam_seduced=madam_seduced,
            )

        if top_targets[0] is None:
            self._advance_to_next_night()
            return VoteResult(
                executed=None,
                tied=False,
                skipped=True,
                vote_counts=vote_counts,
                madam_seduced=madam_seduced,
            )

        nominated = self.get_player(top_targets[0])
        self.phase = Phase.FINAL_DEFENSE
        return VoteResult(
            executed=nominated,
            tied=False,
            skipped=False,
            vote_counts=vote_counts,
            madam_seduced=madam_seduced,
        )

    def resolve_vote(self) -> VoteResult:
        return self.resolve_nomination_vote()

    def start_confirmation_vote(self) -> None:
        if self.phase != Phase.FINAL_DEFENSE:
            raise ValueError("최후변론 뒤에만 찬반투표를 시작할 수 있습니다.")
        self.phase = Phase.CONFIRM_VOTE
        self.confirm_votes.clear()

    def submit_confirmation_vote(self, voter_id: int, approve: bool) -> str:
        if self.phase != Phase.CONFIRM_VOTE:
            raise ValueError("지금은 찬반투표 시간이 아닙니다.")
        voter = self._require_alive(voter_id)
        self.confirm_votes[voter.user_id] = approve
        return "찬성에 투표했습니다." if approve else "반대에 투표했습니다."

    def resolve_confirmation_vote(self, target_id: int) -> ConfirmVoteResult:
        if self.phase != Phase.CONFIRM_VOTE:
            raise ValueError("찬반투표 단계만 정산할 수 있습니다.")

        live_votes = {
            voter_id: approve
            for voter_id, approve in self.confirm_votes.items()
            if self._is_alive(voter_id)
        }
        counts: Counter[bool] = Counter()
        for voter_id, approve in live_votes.items():
            counts[approve] += self._vote_weight(voter_id)
        yes = counts.get(True, 0)
        no = counts.get(False, 0)
        target = self.get_player(target_id)
        normal_approved = target is not None and target.alive and yes > no
        approved = normal_approved
        judge = self._active_judge()
        judge_choice = live_votes.get(judge.user_id) if judge else None
        decided_by_judge = False
        if judge and judge.user_id in self.revealed_judge_ids:
            approved = target is not None and target.alive and bool(judge_choice)
            decided_by_judge = True
        elif judge and judge_choice is not None and judge_choice != normal_approved:
            self.revealed_judge_ids.add(judge.user_id)
            self.publicly_revealed_ids.add(judge.user_id)
            approved = target is not None and target.alive and judge_choice
            decided_by_judge = True
        tied = False if decided_by_judge else yes == no
        blocked_by_politician = bool(approved and target and target.role == Role.POLITICIAN)
        executed = target if approved and not blocked_by_politician else None
        extra_killed: list[Player] = []
        if blocked_by_politician and target:
            self.publicly_revealed_ids.add(target.user_id)
        if executed:
            self._mark_dead(executed)
            if executed.role == Role.JOKER:
                self.joker_won = True
                self.joker_winner_id = executed.user_id
            if executed.role == Role.TERRORIST:
                terrorist_target = self.get_player(self.terrorist_targets.get(executed.user_id, 0))
                if (
                    terrorist_target
                    and terrorist_target.alive
                    and not self.is_citizen_team(terrorist_target)
                ):
                    self._mark_dead(terrorist_target)
                    extra_killed.append(terrorist_target)
        self.ensure_fanatic_reincarnation()
        self._advance_to_next_night()
        return ConfirmVoteResult(
            executed=executed,
            approved=approved,
            tied=tied,
            blocked_by_politician=blocked_by_politician,
            extra_killed=extra_killed,
            vote_counts=dict(counts),
            judge=judge if decided_by_judge else None,
            judge_choice=judge_choice if decided_by_judge else None,
            decided_by_judge=decided_by_judge,
        )

    def winner(self) -> Winner | None:
        if self.joker_won:
            return Winner.JOKER
        mafia_alive = len(self.alive_known_mafia_team())
        cult_alive = len(self.alive_cult_team())
        non_cult_alive = len(self.alive_players()) - cult_alive
        cult_leader_alive = any(player.alive and player.role == Role.CULT_LEADER for player in self.players)
        if cult_leader_alive and cult_alive > 0 and cult_alive >= non_cult_alive:
            return Winner.CULT
        non_mafia_alive = len(
            [
                player
                for player in self.alive_players()
                if not self.is_known_mafia_team(player)
            ]
        )
        if mafia_alive == 0:
            if self.has_pending_scientist_revive():
                return None
            return Winner.CITIZEN
        if mafia_alive >= non_mafia_alive:
            if self._revealed_judge_alive():
                return None
            return Winner.MAFIA
        return None

    def _active_judge(self) -> Player | None:
        judges = [
            player
            for player in self.alive_players()
            if player.role == Role.JUDGE
        ]
        if not judges:
            return None
        revealed = [
            judge for judge in judges if judge.user_id in self.revealed_judge_ids
        ]
        return sorted(revealed or judges, key=lambda player: player.name.casefold())[0]

    def _revealed_judge_alive(self) -> bool:
        return any(
            player.alive and player.role == Role.JUDGE and player.user_id in self.revealed_judge_ids
            for player in self.players
        )

    def reveal_roles(self) -> str:
        return "\n".join(
            f"- {player.name}: {player.role.value}{'' if player.alive else ' (사망)'}"
            for player in sorted(self.players, key=lambda item: item.name.casefold())
        )

    def public_status(self) -> str:
        alive = ", ".join(player.name for player in self.alive_players())
        dead = ", ".join(player.name for player in self.dead_players()) or "없음"
        return (
            f"{self.day_number}일차 / 현재 단계: {self.phase.value}\n"
            f"생존자({len(self.alive_players())}명): {alive}\n"
            f"사망자: {dead}"
        )

    def _resolve_detective_results(
        self,
        mafia_target_id: int | None,
        protected_id: int | None,
        police_target_id: int | None,
        godfather_target_id: int | None,
    ) -> dict[int, str]:
        results: dict[int, str] = {}
        for actor_id, watched_id in self.detective_targets.items():
            actor = self.get_player(actor_id)
            watched = self.get_player(watched_id)
            if not actor or not actor.alive or not watched:
                continue

            action_target_id = self._resolved_action_target(
                watched,
                mafia_target_id,
                protected_id,
                police_target_id,
                godfather_target_id,
            )
            if action_target_id is None:
                results[actor_id] = f"{watched.name} 님은 밤에 능력을 사용하지 않았습니다."
                continue

            action_target = self.get_player(action_target_id)
            target_name = action_target.name if action_target else str(action_target_id)
            results[actor_id] = f"{watched.name} 님은 밤에 {target_name} 님에게 능력을 사용했습니다."
        return results

    def _resolved_action_target(
        self,
        watched: Player,
        mafia_target_id: int | None,
        protected_id: int | None,
        police_target_id: int | None,
        godfather_target_id: int | None,
    ) -> int | None:
        if watched.role == Role.MAFIA:
            return mafia_target_id
        if watched.role == Role.DOCTOR:
            return self.doctor_targets.get(watched.user_id)
        if watched.role == Role.NURSE:
            return self.nurse_targets.get(watched.user_id) or self.nurse_prescription_targets.get(watched.user_id)
        if watched.role == Role.POLICE:
            return police_target_id if watched.user_id in self.police_targets else None
        if watched.role == Role.VIGILANTE:
            return self.vigilante_targets.get(watched.user_id)
        if watched.role == Role.REPORTER:
            return self.reporter_targets.get(watched.user_id)
        if watched.role == Role.DETECTIVE:
            return self.detective_targets.get(watched.user_id)
        if watched.role == Role.SHAMAN:
            return self.shaman_targets.get(watched.user_id)
        if watched.role == Role.PRIEST:
            return self.priest_targets.get(watched.user_id)
        if watched.role == Role.SPY:
            targets = self.spy_targets.get(watched.user_id, [])
            return targets[-1] if targets else None
        if watched.role == Role.CONTRACTOR:
            if watched.user_id in self.contractor_contact_targets:
                return self.contractor_contact_targets.get(watched.user_id)
            contract = self.contractor_contracts.get(watched.user_id)
            return contract[0][0] if contract else None
        if watched.role == Role.WITCH:
            return self.witch_targets.get(watched.user_id)
        if watched.role == Role.TERRORIST:
            return self.terrorist_targets.get(watched.user_id)
        if watched.role == Role.GODFATHER:
            if watched.user_id in self.godfather_contacted:
                return godfather_target_id
            return self.godfather_targets.get(watched.user_id)
        if watched.role == Role.CULT_LEADER:
            return self.cult_targets.get(watched.user_id)
        if watched.role == Role.FANATIC:
            return self.fanatic_targets.get(watched.user_id)
        return None

    def _resolve_terrorist_night_retaliations(
        self,
        killed_by_mafia_team_ids: set[int],
        killed_players: list[Player],
    ) -> list[tuple[Player, Player]]:
        retaliations: list[tuple[Player, Player]] = []
        for terrorist_id in killed_by_mafia_team_ids:
            terrorist = self.get_player(terrorist_id)
            if not terrorist or terrorist.role != Role.TERRORIST:
                continue

            target = self.get_player(self.terrorist_targets.get(terrorist_id, 0))
            if not target or not target.alive or not self.is_mafia_team(target):
                continue

            self._mark_dead(target)
            killed_players.append(target)
            retaliations.append((terrorist, target))
        return retaliations

    def _resolve_spy_results(self) -> tuple[dict[int, str], list[int]]:
        results: dict[int, str] = {}
        for actor_id, target_ids in self.spy_targets.items():
            actor = self.get_player(actor_id)
            if not actor or not actor.alive:
                continue
            lines: list[str] = []
            for target_id in target_ids:
                target = self.get_player(target_id)
                if target:
                    lines.append(
                        f"[첩보] {target.name} 님의 직업은 **{self.visible_role(target).value}** 입니다."
                    )
            if actor_id in self.spy_contacts_this_night:
                lines.append("[접선] 마피아와 접선했습니다.")
            if lines:
                results[actor_id] = "\n".join(lines)
        return results, list(self.spy_contacts_this_night)

    def _resolve_contractor_results(self) -> tuple[dict[int, str], list[int], list[Player]]:
        results: dict[int, str] = {}
        kills: list[Player] = []

        for actor_id, target_id in self.contractor_contact_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target:
                continue
            if target.role == Role.MAFIA and actor_id in self.contractor_contacts_this_night:
                results[actor_id] = "[동업] 마피아와 접선했습니다."
            else:
                results[actor_id] = "[동업] 접선에 실패했습니다."

        for actor_id, contract in self.contractor_contracts.items():
            actor = self.get_player(actor_id)
            if not actor or not actor.alive:
                continue

            targets = [
                (self.get_player(target_id), guessed_role)
                for target_id, guessed_role in contract
            ]
            success = all(
                target
                and target.alive
                and self.is_citizen_team(target)
                and target.role == guessed_role
                and not self.is_publicly_revealed(target)
                for target, guessed_role in targets
            )
            if not success:
                results[actor_id] = "대상의 정보가 정확하지 않아 암살에 실패했습니다."
                continue

            for target, _guessed_role in targets:
                if target and target not in kills:
                    kills.append(target)
            results[actor_id] = "청부가 성공했습니다. 대상 둘이 아침에 암살됩니다."

        return results, list(self.contractor_contacts_this_night), kills

    def _resolve_godfather_results(self) -> dict[int, str]:
        results: dict[int, str] = {}
        for actor_id, target_id in self.godfather_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target or not target.alive:
                continue
            results[actor_id] = f"{target.name} 님을 확정 처치 대상으로 지목했습니다."
        return results

    def _resolve_shaman_results(self) -> tuple[dict[int, str], list[int]]:
        results: dict[int, str] = {}
        purifications: list[int] = []
        for actor_id, target_id in self.shaman_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if (
                not actor
                or not actor.alive
                or not target
                or target.alive
                or target.user_id in self.purified_dead_ids
            ):
                continue

            self.purified_dead_ids.add(target.user_id)
            purifications.append(target.user_id)
            results[actor_id] = (
                f"[성불] {target.name} 님의 직업은 **{self.visible_role(target).value}** 입니다.\n"
                "대상은 사망자 채널에서 채팅할 수 없습니다."
            )
        return results, purifications

    def _resolve_reporter_results(self) -> dict[int, str]:
        results: dict[int, str] = {}
        for actor_id, target_id in self.reporter_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target:
                continue
            visible_role = self.visible_role(target)
            if visible_role != Role.FROG:
                self.publicly_revealed_ids.add(target.user_id)
            results[actor_id] = (
                f"[속보입니다! {target.name}님이 {visible_role.value}이라는 소식입니다!]"
            )
        return results

    def _resolve_vigilante_results(self) -> tuple[dict[int, str], list[Player]]:
        results: dict[int, str] = {}
        kills: list[Player] = []
        for actor_id, target_id in self.vigilante_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target:
                continue
            if target.alive and self.is_mafia_team(target):
                kills.append(target)
                results[actor_id] = f"[숙청] {target.name} 님을 처형했습니다."
            else:
                results[actor_id] = "[숙청] 대상이 마피아팀이 아니거나 이미 사망해 처형에 실패했습니다."
        return results, kills

    def _resolve_nurse_results(self) -> tuple[dict[int, str], list[int]]:
        results: dict[int, str] = {}
        for actor_id, target_id in self.nurse_prescription_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or not target:
                continue
            if target.role == Role.DOCTOR:
                self.nurse_contacted.add(actor_id)
                if actor_id not in self.nurse_contacts_this_night:
                    self.nurse_contacts_this_night.append(actor_id)
                results[actor_id] = f"[처방] {target.name} 님은 의사입니다. 의사와 접선했습니다."
            else:
                results[actor_id] = f"[처방] {target.name} 님은 의사가 아닙니다."
        for actor_id, target_id in self.nurse_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if actor and actor.alive and target:
                results[actor_id] = f"[치료] {target.name} 님을 치료 대상으로 선택했습니다."
        return results, list(self.nurse_contacts_this_night)

    def _nurse_enhanced_heal_active(self) -> bool:
        return any(
            player.alive and player.role == Role.NURSE and player.user_id in self.nurse_contacted
            for player in self.players
        )

    def _resolve_priest_results(self, killed_players: list[Player]) -> tuple[dict[int, str], list[Player]]:
        results: dict[int, str] = {}
        revived: list[Player] = []
        killed_ids = {player.user_id for player in killed_players}
        for actor_id, target_id in self.priest_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor:
                continue
            if actor_id in killed_ids or not actor.alive:
                continue
            if not target or target.alive:
                results[actor_id] = "[소생] 대상이 이미 살아있어 부활에 실패했습니다."
                continue
            if target.user_id in self.purified_dead_ids:
                results[actor_id] = "[소생] 대상이 성불 상태라 부활에 실패했습니다."
                continue
            target.alive = True
            self.scientist_pending_revive_ids.discard(target.user_id)
            revived.append(target)
            results[actor_id] = f"[소생] {target.name} 님을 부활시켰습니다."
        return results, revived

    def _resolve_cult_results(self) -> tuple[dict[int, str], int]:
        results: dict[int, str] = {}
        for actor_id, target_id in self.cult_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or actor.role != Role.CULT_LEADER or not target or not target.alive:
                continue
            if target.user_id in self.culted_ids:
                results[actor_id] = f"[포교] {target.name} 님을 포교했습니다. 직업은 **{target.role.value}** 입니다."
                continue
            if self.is_mafia_team(target) or target.role == Role.CULT_LEADER:
                results[actor_id] = "[포교] 포교에 실패했습니다."
                continue
            if target.role == Role.PRIEST and target.user_id not in self.culted_ids:
                results[actor_id] = "[포교] 포교에 실패했습니다."
                results[target.user_id] = f"[신앙] 교주가 포교를 시도했습니다. 교주는 **{actor.name}** 님입니다."
                continue
            self.culted_ids.add(target.user_id)
            results[actor_id] = f"[포교] {target.name} 님을 포교했습니다. 직업은 **{target.role.value}** 입니다."
        return results, 0

    def _resolve_fanatic_results(self) -> tuple[dict[int, str], int]:
        results: dict[int, str] = {}
        for actor_id, target_id in self.fanatic_targets.items():
            actor = self.get_player(actor_id)
            target = self.get_player(target_id)
            if not actor or not actor.alive or actor.role != Role.FANATIC or not target:
                continue
            is_cult = self.is_cult_team(target)
            if target.role == Role.CULT_LEADER and actor_id not in self.culted_ids:
                self.culted_ids.add(actor_id)
            suffix = "교주팀입니다" if is_cult else "교주팀이 아닙니다"
            results[actor_id] = f"[추종] {target.name} 님은 **{suffix}**."
        return results, 0

    def consume_cult_bells(self) -> int:
        count = self.cult_bells_this_night
        self.cult_bells_this_night = 0
        return count

    def ensure_fanatic_reincarnation(self) -> list[int]:
        if any(player.alive and player.role == Role.CULT_LEADER for player in self.players):
            return []
        inherited: list[int] = []
        for player in self.alive_players():
            if player.role == Role.FANATIC and player.user_id in self.culted_ids:
                player.role = Role.CULT_LEADER
                self.culted_ids.add(player.user_id)
                inherited.append(player.user_id)
                break
        return inherited

    def _resolve_agent_results(self) -> dict[int, str]:
        results: dict[int, str] = {}
        for agent in self.alive_players():
            if agent.role != Role.AGENT:
                continue

            candidates = [
                player
                for player in self.alive_players()
                if player.user_id != agent.user_id
                and self.is_citizen_team(player)
                and player.user_id not in self.agent_discovered_ids
                and not self.is_publicly_revealed(player)
            ]
            if not candidates:
                results[agent.user_id] = "지령이 도착하지 않았습니다."
                continue

            target = self._rng.choice(candidates)
            self.agent_discovered_ids.add(target.user_id)
            results[agent.user_id] = (
                f"[공작] 지령이 도착했습니다.\n"
                f"{target.name} 님의 직업은 **{target.role.value}** 입니다."
            )
        return results

    def _resolve_graverobbers(self, killed_players: list[Player]) -> dict[int, Role]:
        if self.day_number != 1:
            return {}

        robbed_player = killed_players[0] if killed_players else None
        inherited_role = robbed_player.role if robbed_player else Role.CITIZEN
        results: dict[int, Role] = {}
        for player in self.alive_players():
            if player.role != Role.GRAVEROBBER:
                continue
            player.role = inherited_role
            results[player.user_id] = inherited_role
        if results and robbed_player:
            robbed_player.role = Role.VILLAIN if inherited_role in MAFIA_TEAM_ROLES else Role.CITIZEN
        return results

    def _lover_sacrifice_for(self, target: Player) -> Player | None:
        if target.role != Role.LOVER:
            return None
        alive_lovers = [
            player
            for player in self.alive_players()
            if player.role == Role.LOVER
        ]
        if len(alive_lovers) < 2:
            return None
        return next((lover for lover in alive_lovers if lover.user_id != target.user_id), None)

    def _require_alive(self, user_id: int) -> Player:
        player = self._require_player(user_id)
        if not player.alive:
            raise ValueError("사망한 참가자는 행동할 수 없습니다.")
        return player

    def _require_player(self, user_id: int) -> Player:
        player = self.get_player(user_id)
        if not player:
            raise ValueError("게임 참가자가 아닙니다.")
        return player

    def _proxy_target(self, target: Player) -> Player:
        if not target.alive or target.role != Role.HACKER:
            return target
        proxy = self.get_player(self.hacker_proxy_targets.get(target.user_id, 0))
        if proxy and proxy.alive:
            return proxy
        return target

    def _is_alive(self, user_id: int) -> bool:
        player = self.get_player(user_id)
        return bool(player and player.alive)

    def _majority_target(self, targets: dict[int, int]) -> int | None:
        live_targets = [
            target_id
            for actor_id, target_id in targets.items()
            if self._is_alive(actor_id) and self._is_alive(target_id)
        ]
        voter_count = len(live_targets)
        if not live_targets or voter_count <= 0:
            return None
        counts = Counter(live_targets)
        highest = max(counts.values())
        tied = [target_id for target_id, count in counts.items() if count == highest]
        if len(tied) != 1:
            return None
        if highest <= voter_count / 2:
            return None
        return tied[0]

    def _spy_actions_used(self, actor_id: int) -> int:
        return len(self.spy_targets.get(actor_id, []))

    def _spy_action_limit(self, actor_id: int) -> int:
        return 2 if actor_id in self.spy_bonus_pending else 1

    def _contractor_can_act(self, player: Player) -> bool:
        alive_targets = [
            target for target in self.alive_players() if target.user_id != player.user_id
        ]
        if player.user_id not in self.contractor_contacted and alive_targets:
            return True
        return self.day_number >= 2 and len(self.contractor_contract_targets(player)) >= 2

    def _reporter_can_act(self, player: Player) -> bool:
        return (
            self.day_number >= 2
            and player.user_id not in self.reporter_used_ids
            and any(target.user_id != player.user_id for target in self.alive_players())
        )

    def _vote_weight(self, voter_id: int) -> int:
        voter = self.get_player(voter_id)
        if voter and voter.alive and voter.role == Role.POLITICIAN:
            return 2
        return 1

    def _advance_to_next_night(self) -> None:
        self._expire_madam_seductions()
        self.phase = Phase.NIGHT
        self.day_number += 1

    def _expire_madam_seductions(self) -> None:
        if any(player.alive and player.role == Role.MADAM for player in self.players):
            return
        for user_id, release_day in list(self.madam_seduction_release_days.items()):
            if release_day <= self.day_number:
                self.madam_seduced_ids.discard(user_id)
                self.madam_seduction_release_days.pop(user_id, None)
