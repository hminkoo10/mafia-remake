from __future__ import annotations

from bot import *  # noqa: F401,F403


__all__ = (
    'JoinGameView',
    'NightActionSelect',
    'NightActionView',
    'HackerDayActionSelect',
    'HackerDayActionView',
    'VigilanteDayActionSelect',
    'VigilanteDayActionView',
    'ContractorTargetSelect',
    'ContractorRoleSelect',
    'ContractorContractView',
    'DayVoteSelect',
    'DayVoteView',
    'ConfirmVoteView',
    'DayExtensionVoteView',
    'DaySkipToVoteView',
    'has_changeable_mafia_action',
    'should_finish_night_early',
)


class JoinGameView(discord.ui.View):
    def __init__(
        self,
        guild_id: int,
        host_user_id: int,
        participant_role_id: int,
        role_counts: dict[Role, int],
        reveal_death_roles: bool,
        reveal_public_police_status: bool,
        reveal_morning_mafia_count: bool,
        max_players: int,
    ) -> None:
        super().__init__(timeout=RECRUITMENT_SECONDS + 5)
        self.guild_id = guild_id
        self.host_user_id = host_user_id
        self.participant_role_id = participant_role_id
        self.role_counts = role_counts
        self.reveal_death_roles = reveal_death_roles
        self.reveal_public_police_status = reveal_public_police_status
        self.reveal_morning_mafia_count = reveal_morning_mafia_count
        self.max_players = max_players
        self.minimum_players = minimum_player_count(role_counts)
        self.joined_ids: set[int] = set()
        self.joined_names: dict[int, str] = {}
        self.spectator_ids: set[int] = set()
        self.spectator_names: dict[int, str] = {}
        self.accepting = True
        self.started = False
        self.cancelled = False
        self.done = asyncio.Event()
        self.lock = asyncio.Lock()
        self.message: discord.Message | None = None

    def participant_text(self) -> str:
        if not self.joined_names:
            return "아직 참가자가 없습니다."
        names = sorted(self.joined_names.values(), key=str.casefold)
        return "\n".join(f"{index}. {name}" for index, name in enumerate(names, start=1))

    def spectator_text(self) -> str:
        if not self.spectator_names:
            return "아직 관전자가 없습니다."
        names = sorted(self.spectator_names.values(), key=str.casefold)
        return "\n".join(f"{index}. {name}" for index, name in enumerate(names, start=1))

    def role_count_text(self) -> str:
        return public_role_count_text_from_counts(self.role_counts)

    def minimum_status_text(self) -> str:
        shortage = self.minimum_players - len(self.joined_ids)
        if shortage <= 0:
            return f"최소 시작 인원 **{self.minimum_players}명** 충족"
        return f"최소 시작 인원 **{self.minimum_players}명**까지 **{shortage}명** 더 필요"

    def capacity_status_text(self) -> str:
        remaining = self.max_players - len(self.joined_ids)
        if remaining <= 0:
            return f"최대 참가 인원 **{self.max_players}명** 도달"
        return f"최대 참가 인원 **{self.max_players}명**까지 **{remaining}명** 더 참가 가능"

    def joined_count_text(self) -> str:
        return f"현재 참가자 **{len(self.joined_ids)}/{self.max_players}명**"

    def embed(
        self,
        status: str,
        *,
        title: str = "참가자 모집",
        color: discord.Color = SUCCESS_EMBED_COLOR,
    ) -> discord.Embed:
        return make_embed(
            f"최대 {duration_text(RECRUITMENT_SECONDS)} 동안 참가자를 모집합니다.\n"
            "참가 버튼을 누르면 게임 참가자로 등록되고, "
            f"'{config.participant_role}' 역할이 부여됩니다.\n"
            f"관전 버튼을 누르면 '{SPECTATOR_ROLE}' 역할이 부여되고 게임 채널을 읽을 수 있습니다.\n"
            "주최자는 `시작` 버튼으로 즉시 시작하거나 `취소` 버튼으로 모집을 취소할 수 있습니다.\n\n"
            f"역할 구성: {self.role_count_text()}\n"
            f"사망 시 직업 공개: {'공개' if self.reveal_death_roles else '비공개'}\n"
            f"경찰 조사 성공 여부 공개: {'공개' if self.reveal_public_police_status else '비공개'}\n"
            f"아침 생존 마피아 수 공개: {'공개' if self.reveal_morning_mafia_count else '비공개'}\n"
            f"{self.minimum_status_text()}\n\n"
            f"{self.capacity_status_text()}\n\n"
            f"{self.joined_count_text()}\n"
            f"{self.participant_text()}\n\n"
            f"현재 관전자 **{len(self.spectator_ids)}명**\n"
            f"{self.spectator_text()}\n\n"
            f"{status}",
            title=title,
            color=color,
        )

    async def refresh_message(
        self,
        status: str = "모집 중입니다.",
        *,
        title: str = "참가자 모집",
        color: discord.Color = SUCCESS_EMBED_COLOR,
    ) -> None:
        if not self.message:
            return
        try:
            await self.message.edit(embed=self.embed(status, title=title, color=color), view=self)
        except discord.DiscordException:
            return

    async def finish_from_host(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
        *,
        cancelled: bool,
    ) -> None:
        if interaction.user.id != self.host_user_id:
            await send_interaction_reply(
                interaction,
                "게임을 모집한 주최자만 사용할 수 있습니다.",
                private=True,
            )
            return

        async with self.lock:
            if not self.accepting:
                await send_interaction_reply(interaction, "참가자 모집이 이미 종료되었습니다.", private=True)
                return
            if not cancelled and len(self.joined_ids) < self.minimum_players:
                await send_interaction_reply(
                    interaction,
                    f"아직 시작할 수 없습니다. 최소 {self.minimum_players}명이 필요합니다. "
                    f"현재 {len(self.joined_ids)}명입니다.",
                    private=True,
                )
                return

            self.accepting = False
            self.cancelled = cancelled
            self.started = not cancelled
            disable_view_items(self)
            button.label = "취소 완료" if cancelled else "시작 확정"
            status = "주최자가 참가자 모집을 취소했습니다." if cancelled else "주최자가 게임 시작을 눌렀습니다."
            title = "참가자 모집 취소" if cancelled else "참가자 모집 종료"
            color = ERROR_EMBED_COLOR if cancelled else SUCCESS_EMBED_COLOR
            await interaction.response.edit_message(
                content=None,
                embed=self.embed(status, title=title, color=color),
                view=self,
            )
            self.done.set()
            self.stop()

    @discord.ui.button(label="참가", style=discord.ButtonStyle.success)
    async def join_game(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not interaction.response.is_done():
            with suppress(discord.HTTPException, discord.NotFound):
                await interaction.response.defer(ephemeral=True, thinking=True)
        async with self.lock:
            if not self.accepting:
                await send_interaction_reply(interaction, "참가자 모집이 종료되었습니다.", private=True)
                return
            if interaction.guild_id != self.guild_id or not interaction.guild:
                await send_interaction_reply(interaction, "이 모집에는 참가할 수 없습니다.", private=True)
                return
            if not isinstance(interaction.user, discord.Member) or interaction.user.bot:
                await send_interaction_reply(interaction, "서버 멤버만 참가할 수 있습니다.", private=True)
                return
            if is_blacklisted(interaction.user.id):
                await send_interaction_reply(
                    interaction,
                    "블랙리스트에 등록된 유저는 참가할 수 없습니다.",
                    private=True,
                )
                return
            if member_is_confirmed_offline(interaction.guild_id, interaction.user):
                await send_interaction_reply(
                    interaction,
                    "오프라인으로 표시된 유저는 참가할 수 없습니다.",
                    private=True,
                )
                return

            participant_role = interaction.guild.get_role(self.participant_role_id)
            if not participant_role:
                await send_interaction_reply(
                    interaction,
                    f"'{config.participant_role}' 역할을 찾을 수 없습니다.",
                    private=True,
                )
                return
            if interaction.user.id in self.joined_ids:
                await send_interaction_reply(interaction, "이미 참가했습니다.", private=True)
                return
            if interaction.user.id in self.spectator_ids:
                await send_interaction_reply(interaction, "이미 관전자로 등록되어 있습니다.", private=True)
                return
            if len(self.joined_ids) >= self.max_players:
                await send_interaction_reply(
                    interaction,
                    f"최대 참가 인원 {self.max_players}명에 도달해 더 이상 참가할 수 없습니다.",
                    private=True,
                )
                return

            try:
                if participant_role not in interaction.user.roles:
                    await interaction.user.add_roles(
                        participant_role,
                        reason="마피아 게임 참가 신청",
                    )
            except discord.DiscordException:
                await send_interaction_reply(
                    interaction,
                    f"'{config.participant_role}' 역할 부여에 실패했습니다. "
                    "봇에게 역할 관리 권한이 있고, 봇 역할이 참가자 역할보다 위에 있는지 확인하세요.",
                    private=True,
                )
                return

            self.joined_ids.add(interaction.user.id)
            self.joined_names[interaction.user.id] = display_name(interaction.user)
            await send_interaction_reply(
                interaction,
                f"참가 완료! '{config.participant_role}' 역할을 부여했습니다.",
                private=True,
            )
            await self.refresh_message()

    @discord.ui.button(label="관전", style=discord.ButtonStyle.secondary)
    async def spectate_game(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not interaction.response.is_done():
            with suppress(discord.HTTPException, discord.NotFound):
                await interaction.response.defer(ephemeral=True, thinking=True)
        async with self.lock:
            if not self.accepting:
                await send_interaction_reply(interaction, "참가자 모집이 종료되었습니다.", private=True)
                return
            if interaction.guild_id != self.guild_id or not interaction.guild:
                await send_interaction_reply(interaction, "이 모집에는 관전할 수 없습니다.", private=True)
                return
            if not isinstance(interaction.user, discord.Member) or interaction.user.bot:
                await send_interaction_reply(interaction, "서버 멤버만 관전할 수 있습니다.", private=True)
                return
            if is_blacklisted(interaction.user.id):
                await send_interaction_reply(
                    interaction,
                    "블랙리스트에 등록된 유저는 관전할 수 없습니다.",
                    private=True,
                )
                return
            if interaction.user.id in self.joined_ids:
                await send_interaction_reply(interaction, "이미 참가자로 등록되어 있습니다.", private=True)
                return
            if interaction.user.id in self.spectator_ids:
                await send_interaction_reply(interaction, "이미 관전자로 등록되어 있습니다.", private=True)
                return

            spectator_role = await ensure_spectator_role(interaction.guild)
            if not spectator_role:
                await send_interaction_reply(
                    interaction,
                    f"'{SPECTATOR_ROLE}' 역할을 만들거나 찾을 수 없습니다. "
                    "봇에게 역할 관리 권한이 있고, 봇 역할이 관전자 역할보다 위에 있는지 확인하세요.",
                    private=True,
                )
                return

            try:
                if spectator_role not in interaction.user.roles:
                    await interaction.user.add_roles(
                        spectator_role,
                        reason="마피아 게임 관전 신청",
                    )
            except discord.DiscordException:
                await send_interaction_reply(
                    interaction,
                    f"'{SPECTATOR_ROLE}' 역할 부여에 실패했습니다. "
                    "봇에게 역할 관리 권한이 있고, 봇 역할이 관전자 역할보다 위에 있는지 확인하세요.",
                    private=True,
                )
                return

            self.spectator_ids.add(interaction.user.id)
            self.spectator_names[interaction.user.id] = display_name(interaction.user)
            await send_interaction_reply(
                interaction,
                f"관전 등록 완료! '{SPECTATOR_ROLE}' 역할을 부여했습니다.",
                private=True,
            )
            await self.refresh_message()

    @discord.ui.button(label="시작", style=discord.ButtonStyle.primary)
    async def start_now(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.finish_from_host(interaction, button, cancelled=False)

    @discord.ui.button(label="취소", style=discord.ButtonStyle.danger)
    async def cancel_recruitment(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.finish_from_host(interaction, button, cancelled=True)


class NightActionSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, role: Role, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        if role == Role.REPORTER:
            options.append(discord.SelectOption(label="사용 안함", value="skip"))
        placeholder = {
            Role.MAFIA: "공격할 대상을 선택하세요",
            Role.DOCTOR: "보호할 대상을 선택하세요",
            Role.NURSE: "처방/치료 대상을 선택하세요",
            Role.POLICE: "조사할 대상을 선택하세요",
            Role.VIGILANTE: "숙청할 대상을 선택하세요",
            Role.REPORTER: "특종을 낼 대상 또는 사용 안함을 선택하세요",
            Role.DETECTIVE: "추적할 대상을 선택하세요",
            Role.SHAMAN: "성불할 사망자를 선택하세요",
            Role.PRIEST: "소생할 사망자를 선택하세요",
            Role.SPY: "첩보할 대상을 선택하세요",
            Role.WITCH: "저주할 대상을 선택하세요",
            Role.GODFATHER: "확정 처치할 대상을 선택하세요",
            Role.TERRORIST: "지목할 대상을 선택하세요",
            Role.CULT_LEADER: "포교할 대상을 선택하세요",
            Role.FANATIC: "추종할 대상을 선택하세요",
        }[role]
        super().__init__(placeholder=placeholder, min_values=1, max_values=1, options=options)
        self.guild_id = guild_id
        self.actor_id = actor_id

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.actor_id:
            await send_interaction_reply(
                interaction,
                "본인에게 온 선택지만 사용할 수 있습니다.",
                private=True,
            )
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            target_id = None if self.values[0] == "skip" else int(self.values[0])
            result = running.game.submit_night_action(self.actor_id, target_id)
            cult_bell_count = running.game.consume_cult_bells()
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        actor = running.game.get_player(self.actor_id)
        immediate_result: str | None = None
        if actor and actor.role == Role.POLICE:
            immediate_result = running.game.consume_ready_police_result()
            if immediate_result:
                guild = bot.get_guild(running.guild_id)
                if guild:
                    await send_police_result_message(
                        guild,
                        running,
                        immediate_result,
                        exclude_user_ids={actor.user_id},
                    )
            else:
                immediate_result = "다른 경찰의 선택이 남아 있어 조사 결과는 아직 확정되지 않았습니다."
        if actor and actor.role == Role.MAFIA:
            guild = bot.get_guild(running.guild_id)
            if guild:
                await sync_role_status_message(guild, running, Role.MAFIA)
            await interaction.response.edit_message(
                content=None,
                embed=make_embed(
                    f"{result}\n\n{mafia_night_target_status_text(running)}",
                    title="마피아 처치 선택",
                    color=SUCCESS_EMBED_COLOR,
                ),
                view=self.view,
            )
            return

        disable_view_items(self.view)
        if actor and actor.role == Role.SPY and running.game.spy_can_use_bonus_action(self.actor_id):
            await interaction.response.edit_message(
                content=None,
                embed=make_embed(
                    f"{result}\n\n추가 첩보를 한 번 더 사용할 수 있습니다.",
                    title="접선 성공",
                    color=SUCCESS_EMBED_COLOR,
                ),
                view=NightActionView(self.guild_id, actor, night_targets(running.game, actor)),
            )
            return

        if actor and actor.role == Role.WITCH and running.night_timed_events_due:
            guild = bot.get_guild(running.guild_id)
            channel = guild.get_channel(running.channel_id) if guild else None
            if guild and isinstance(channel, discord.abc.Messageable):
                trigger_timed_night_events(guild, channel, running)
        response_message = result if not immediate_result else f"{result}\n\n{immediate_result}"
        result = response_message
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(result, title="밤 행동 완료", color=SUCCESS_EMBED_COLOR),
            view=self.view,
        )
        if cult_bell_count:
            await announce_cult_bells_now(running, cult_bell_count)
        if should_finish_night_early(running):
            running.night_complete_event.set()


class NightActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(NightActionSelect(guild_id, actor.user_id, actor.role, targets))


class HackerDayActionSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        super().__init__(
            placeholder="해킹할 대상을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
        self.guild_id = guild_id
        self.actor_id = actor_id

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.actor_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_hacker_action(self.actor_id, int(self.values[0]))
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self.view)
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"{result}\n밤이 시작될 때 대상의 직업을 확인합니다.",
                title="해킹 완료",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self.view,
        )


class HackerDayActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(HackerDayActionSelect(guild_id, actor.user_id, targets))


class VigilanteDayActionSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        super().__init__(
            placeholder="숙청 조사 대상을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
        self.guild_id = guild_id
        self.actor_id = actor_id

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.actor_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_vigilante_investigation(self.actor_id, int(self.values[0]))
            investigation_result = running.game.consume_vigilante_results().get(self.actor_id)
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self.view)
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"{result}\n\n{investigation_result or '조사 결과를 확인하지 못했습니다.'}",
                title="숙청 조사 완료",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self.view,
        )


class VigilanteDayActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(VigilanteDayActionSelect(guild_id, actor.user_id, targets))


class ContractorTargetSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, parent_view: "ContractorContractView", slot: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(
                label=target_select_label(target, parent_view.actor.user_id),
                value=str(target.user_id),
            )
            for target in targets[:25]
        ]
        super().__init__(
            placeholder=f"{slot + 1}번째 청부 대상",
            min_values=1,
            max_values=1,
            options=options,
            row=slot * 2,
        )
        self.parent_view = parent_view
        self.slot = slot

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.parent_view.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        self.parent_view.target_ids[self.slot] = int(self.values[0])
        await interaction.response.defer()


class ContractorRoleSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, parent_view: "ContractorContractView", slot: int) -> None:
        options = [
            discord.SelectOption(label=role.value, value=role.name)
            for role in CONTRACTOR_GUESS_ROLES
        ]
        super().__init__(
            placeholder=f"{slot + 1}번째 대상 직업 추측",
            min_values=1,
            max_values=1,
            options=options,
            row=slot * 2 + 1,
        )
        self.parent_view = parent_view
        self.slot = slot

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.parent_view.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        self.parent_view.guessed_roles[self.slot] = Role[self.values[0]]
        await interaction.response.defer()


class ContractorContractView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id
        self.actor = actor
        self.target_ids: list[int | None] = [None, None]
        self.guessed_roles: list[Role | None] = [None, None]
        self.add_item(ContractorTargetSelect(self, 0, targets))
        self.add_item(ContractorRoleSelect(self, 0))
        self.add_item(ContractorTargetSelect(self, 1, targets))
        self.add_item(ContractorRoleSelect(self, 1))

    @discord.ui.button(label="청부 확정", style=discord.ButtonStyle.danger, row=4)
    async def submit_contract(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if interaction.user.id != self.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        if any(target_id is None for target_id in self.target_ids) or any(
            role is None for role in self.guessed_roles
        ):
            await send_interaction_reply(interaction, "청부 대상 2명과 각 대상의 직업을 모두 선택하세요.", private=True)
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_contractor_contract(
                self.actor.user_id,
                int(self.target_ids[0]),
                self.guessed_roles[0],
                int(self.target_ids[1]),
                self.guessed_roles[1],
            )
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self)
        if should_finish_night_early(running):
            running.night_complete_event.set()
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(result, title="밤 행동 완료", color=SUCCESS_EMBED_COLOR),
            view=self,
        )


class DayVoteSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target.name[:100], value=str(target.user_id))
            for target in targets[:24]
        ]
        options.append(discord.SelectOption(label="스킵", value="skip"))
        super().__init__(
            placeholder="처형할 대상 또는 스킵을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
        self.guild_id = guild_id

    async def callback(self, interaction: discord.Interaction) -> None:
        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return
        try:
            target_id = None if self.values[0] == "skip" else int(self.values[0])
            result = running.game.submit_day_vote(interaction.user.id, target_id)
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return
        if running.game.all_day_votes_submitted():
            running.vote_complete_event.set()
        await send_interaction_reply(interaction, result, private=True)


class DayVoteView(discord.ui.View):
    def __init__(self, guild_id: int, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(DayVoteSelect(guild_id, targets))


class ConfirmVoteView(discord.ui.View):
    def __init__(self, guild_id: int) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id

    async def vote(self, interaction: discord.Interaction, approve: bool) -> None:
        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return
        try:
            result = running.game.submit_confirmation_vote(interaction.user.id, approve)
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return
        if running.game.all_confirm_votes_submitted():
            running.confirm_complete_event.set()
        await send_interaction_reply(interaction, result, private=True)

    @discord.ui.button(label="찬성", style=discord.ButtonStyle.success)
    async def approve(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.vote(interaction, True)

    @discord.ui.button(label="반대", style=discord.ButtonStyle.danger)
    async def reject(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.vote(interaction, False)


class DayExtensionVoteView(discord.ui.View):
    def __init__(self, guild_id: int, alive_user_ids: set[int]) -> None:
        super().__init__(timeout=DAY_EXTENSION_VOTE_SECONDS)
        self.guild_id = guild_id
        self.alive_user_ids = alive_user_ids
        self.voter_ids: set[int] = set()
        self.required_votes = (len(alive_user_ids) // 2) + 1
        self.extended = False
        self.accepting = True

    @discord.ui.button(label="1분 연장", style=discord.ButtonStyle.secondary)
    async def extend_discussion(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not self.accepting:
            await send_interaction_reply(interaction, "연장 투표가 종료되었습니다.", private=True)
            return
        if interaction.user.id not in self.alive_user_ids:
            await send_interaction_reply(
                interaction,
                "생존 중인 참가자만 연장 투표를 할 수 있습니다.",
                private=True,
            )
            return
        running = games.get(self.guild_id)
        if not running or running.game.phase != Phase.DAY:
            await send_interaction_reply(
                interaction,
                "지금 진행 중인 낮 토론이 없습니다.",
                private=True,
            )
            return
        if interaction.user.id in self.voter_ids:
            await send_interaction_reply(
                interaction,
                f"이미 1분 연장에 투표했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.voter_ids.add(interaction.user.id)
        if len(self.voter_ids) < self.required_votes:
            await send_interaction_reply(
                interaction,
                f"1분 연장에 투표했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.extended = True
        self.accepting = False
        disable_view_items(self)
        button.label = "연장 확정"
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"생존자 과반수가 1분 연장을 선택했습니다. "
                f"({len(self.voter_ids)}/{len(self.alive_user_ids)}명)\n"
                "낮 토론을 1분 연장합니다.",
                title="낮 토론 연장",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self,
        )
        self.stop()


class DaySkipToVoteView(discord.ui.View):
    def __init__(self, guild_id: int, alive_user_ids: set[int]) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id
        self.alive_user_ids = alive_user_ids
        self.voter_ids: set[int] = set()
        self.required_votes = (len(alive_user_ids) // 2) + 1
        self.accepting = True

    @discord.ui.button(label="바로 투표", style=discord.ButtonStyle.primary)
    async def skip_to_vote(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not self.accepting:
            await send_interaction_reply(interaction, "바로 투표 선택이 종료되었습니다.", private=True)
            return
        if interaction.user.id not in self.alive_user_ids:
            await send_interaction_reply(
                interaction,
                "생존 중인 참가자만 바로 투표를 선택할 수 있습니다.",
                private=True,
            )
            return

        running = games.get(self.guild_id)
        if not running or running.game.phase != Phase.DAY:
            await send_interaction_reply(
                interaction,
                "지금 진행 중인 낮 토론이 없습니다.",
                private=True,
            )
            return
        if interaction.user.id in self.voter_ids:
            await send_interaction_reply(
                interaction,
                f"이미 바로 투표에 동의했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.voter_ids.add(interaction.user.id)
        if len(self.voter_ids) < self.required_votes:
            await send_interaction_reply(
                interaction,
                f"바로 투표에 동의했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.accepting = False
        disable_view_items(self)
        button.label = "투표 확정"
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"생존자 과반수가 바로 투표를 선택했습니다. "
                f"({len(self.voter_ids)}/{len(self.alive_user_ids)}명)\n"
                "토론을 끝내고 바로 지목 투표로 넘어갑니다.",
                title="바로 투표",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self,
        )
        running.day_vote_event.set()
        self.stop()


def has_changeable_mafia_action(running: RunningGame) -> bool:
    return any(actor.role == Role.MAFIA for actor in running.game.night_action_actors())


def should_finish_night_early(running: RunningGame) -> bool:
    return running.game.all_night_actions_submitted() and not has_changeable_mafia_action(running)
