from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
import time

import discord

from game import MafiaGame
from game_models import Player, Role


@dataclass
class BotConfig:
    participant_role: str
    manager_role: str
    default_mafia_count: int
    default_doctor_count: int
    default_police_count: int
    default_joker_count: int
    max_player_count: int
    night_seconds: int
    discussion_seconds: int
    vote_seconds: int
    chat_slowmode_seconds: int = 3
    reveal_death_roles: bool = False
    reveal_public_police_status: bool = True
    reveal_morning_mafia_count: bool = True
    citizen_special_count: int = 0
    mafia_special_count: int = 0
    neutral_special_count: int = 1
    enable_detective: bool = True
    enable_graverobber: bool = True
    enable_spy: bool = True
    enable_contractor: bool = True
    enable_witch: bool = True
    enable_scientist: bool = True
    enable_madam: bool = True
    enable_godfather: bool = True
    enable_joker: bool = True
    enable_politician: bool = True
    enable_judge: bool = True
    enable_reporter: bool = True
    enable_hacker: bool = True
    enable_terrorist: bool = True
    enable_lover: bool = True
    enable_shaman: bool = True
    enable_priest: bool = True
    enable_soldier: bool = True
    enable_nurse: bool = True
    enable_gangster: bool = True
    enable_prophet: bool = True
    enable_psychologist: bool = True
    enable_thief: bool = True
    enable_cult_team: bool = False
    use_agent: bool = False
    use_vigilante: bool = False
    anonymous_mode: bool = False
    anonymous_name_mode: str = "animal"
    game_enabled: bool = True
    blacklist_user_ids: list[int] = field(default_factory=list)


@dataclass
class RunningGame:
    guild_id: int
    channel_id: int
    game: MafiaGame
    reveal_death_roles: bool = False
    reveal_public_police_status: bool = True
    reveal_morning_mafia_count: bool = True
    task: asyncio.Task[None] | None = None
    night_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    vote_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    confirm_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    day_vote_event: asyncio.Event = field(default_factory=asyncio.Event)
    game_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    member_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    madam_seduction_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    original_slowmode_delay: int | None = None
    original_slowmode_channel_id: int | None = None
    participant_user_ids: set[int] = field(default_factory=set)
    private_channel_ids: dict[Role, int] = field(default_factory=dict)
    memo_channel_ids: dict[int, int] = field(default_factory=dict)
    memos: dict[int, dict[int, list[str]]] = field(default_factory=dict)
    spectator_user_ids: set[int] = field(default_factory=set)
    game_status_message_id: int | None = None
    game_status_text: str | None = None
    shaman_channel_id: int | None = None
    shaman_status_message_id: int | None = None
    shaman_status_text: str | None = None
    frog_channel_id: int | None = None
    frog_game_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    night_timed_events_due: bool = False
    anonymous_enabled: bool = False
    anonymous_input_channel_ids: dict[int, int] = field(default_factory=dict)
    anonymous_input_channel_owners: dict[int, int] = field(default_factory=dict)
    anonymous_dead_input_channel_ids: dict[int, int] = field(default_factory=dict)
    anonymous_dead_input_channel_owners: dict[int, int] = field(default_factory=dict)
    anonymous_shaman_input_channel_ids: dict[int, int] = field(default_factory=dict)
    anonymous_shaman_input_channel_owners: dict[int, int] = field(default_factory=dict)
    anonymous_role_input_channel_ids: dict[tuple[int, Role], int] = field(default_factory=dict)
    anonymous_role_input_channels: dict[int, tuple[int, Role]] = field(default_factory=dict)
    anonymous_role_status_message_ids: dict[Role, int] = field(default_factory=dict)
    anonymous_role_input_status_message_ids: dict[tuple[int, Role], int] = field(default_factory=dict)
    private_role_status_message_ids: dict[Role, int] = field(default_factory=dict)
    anonymous_role_status_texts: dict[object, str] = field(default_factory=dict)
    private_role_status_texts: dict[Role, str] = field(default_factory=dict)
    anonymous_channel_topics: dict[int, str] = field(default_factory=dict)
    anonymous_aliases: dict[int, str] = field(default_factory=dict)
    anonymous_original_names: dict[int, str] = field(default_factory=dict)
    anonymous_webhook_urls: dict[int, str] = field(default_factory=dict)
    anonymous_original_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    permission_overwrite_cache: dict[tuple[int, int], discord.PermissionOverwrite | None] = field(default_factory=dict)
    final_defense_user_id: int | None = None
    initial_roles: dict[int, Role] = field(default_factory=dict)
    stats_recorded: bool = False
    started_at: float = field(default_factory=time.monotonic)
    day_chat_open: bool = False


@dataclass
class TimedNightEvents:
    cursed_players: list[Player] = field(default_factory=list)
    witch_contacts: list[int] = field(default_factory=list)
    cult_bell_count: int = 0
    revived_players: list[Player] = field(default_factory=list)
