from __future__ import annotations

from functools import lru_cache
from io import BytesIO
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from stats_store import (
    INITIAL_RATING,
    leaderboard_entries,
    leaderboard_metric_name,
    win_rate_text,
)
from time_text import play_duration_text


BASE_DIR = Path(__file__).resolve().parent
IMAGE_WIDTH = 1280
TOP_PADDING = 40
SIDE_PADDING = 48
HEADER_HEIGHT = 150
ROW_HEIGHT = 78
BOTTOM_PADDING = 44

BACKGROUND = "#111318"
PANEL = "#1d2028"
ROW_DARK = "#242832"
ROW_LIGHT = "#292e3a"
TEXT = "#f5f7fb"
MUTED = "#aeb6c8"
ACCENT = "#ffd166"
BLUE = "#60a5fa"
GREEN = "#34d399"
RED = "#fb7185"


FONT_PATHS = {
    "regular": (
        BASE_DIR / "MalangmalangR.ttf",
        Path("C:/Windows/Fonts/malgun.ttf"),
        Path("C:/Windows/Fonts/MalangmalangR.ttf"),
        Path("/usr/share/fonts/truetype/nanum/NanumGothic.ttf"),
        Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        Path("/usr/share/fonts/truetype/noto/NotoSansKR-Regular.ttf"),
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ),
    "bold": (
        BASE_DIR / "MalangmalangB.ttf",
        BASE_DIR / "MalangmalangR.ttf",
        Path("C:/Windows/Fonts/MalangmalangB.ttf"),
        Path("C:/Windows/Fonts/MalangmalangR.ttf"),
        Path("C:/Windows/Fonts/malgunbd.ttf"),
        Path("/usr/share/fonts/truetype/nanum/NanumGothicBold.ttf"),
        Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc"),
        Path("/usr/share/fonts/truetype/noto/NotoSansKR-Bold.ttf"),
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
    ),
}


@lru_cache(maxsize=32)
def load_font(size: int, *, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for path in FONT_PATHS["bold" if bold else "regular"]:
        if path.exists():
            return ImageFont.truetype(str(path), size)
    return ImageFont.load_default()


def text_width(draw: ImageDraw.ImageDraw, text: str, font: ImageFont.ImageFont) -> int:
    box = draw.textbbox((0, 0), text, font=font)
    return box[2] - box[0]


def fit_text(draw: ImageDraw.ImageDraw, text: str, font: ImageFont.ImageFont, max_width: int) -> str:
    if text_width(draw, text, font) <= max_width:
        return text
    ellipsis = "..."
    while text and text_width(draw, text + ellipsis, font) > max_width:
        text = text[:-1]
    return text + ellipsis if text else ellipsis


def draw_pill(
    draw: ImageDraw.ImageDraw,
    x: int,
    y: int,
    text: str,
    font: ImageFont.ImageFont,
    *,
    fill: str,
    text_fill: str = TEXT,
) -> None:
    width = text_width(draw, text, font) + 34
    height = 38
    draw.rounded_rectangle((x, y, x + width, y + height), radius=19, fill=fill)
    draw.text((x + 17, y + 7), text, font=font, fill=text_fill)


def metric_column(metric: str) -> str:
    return {
        "wins": "record",
        "winrate": "winrate",
        "games": "games",
        "mafia": "mafia",
        "playtime": "time",
        "rating": "rating",
    }.get(metric, "record")


def render_leaderboard_image(metric: str) -> BytesIO | None:
    entries = leaderboard_entries(metric)
    if not entries:
        return None

    height = TOP_PADDING + HEADER_HEIGHT + ROW_HEIGHT * len(entries) + BOTTOM_PADDING
    image = Image.new("RGB", (IMAGE_WIDTH, height), BACKGROUND)
    draw = ImageDraw.Draw(image)

    title_font = load_font(44, bold=True)
    subtitle_font = load_font(24)
    header_font = load_font(21, bold=True)
    rank_font = load_font(24, bold=True)
    name_font = load_font(27, bold=True)
    value_font = load_font(23)
    value_bold_font = load_font(23, bold=True)
    small_font = load_font(18)

    draw.text((SIDE_PADDING, TOP_PADDING), "마피아 리더보드", font=title_font, fill=TEXT)
    draw.text(
        (SIDE_PADDING, TOP_PADDING + 58),
        "게임 종료 후 기록된 전적 기준",
        font=subtitle_font,
        fill=MUTED,
    )
    draw_pill(
        draw,
        IMAGE_WIDTH - SIDE_PADDING - 190,
        TOP_PADDING + 10,
        f"기준: {leaderboard_metric_name(metric)}",
        subtitle_font,
        fill="#374151",
    )

    panel_top = TOP_PADDING + 116
    panel_bottom = height - BOTTOM_PADDING + 8
    draw.rounded_rectangle(
        (SIDE_PADDING, panel_top, IMAGE_WIDTH - SIDE_PADDING, panel_bottom),
        radius=18,
        fill=PANEL,
    )

    columns = {
        "rank": SIDE_PADDING + 32,
        "name": SIDE_PADDING + 110,
        "record": SIDE_PADDING + 410,
        "games": SIDE_PADDING + 555,
        "winrate": SIDE_PADDING + 665,
        "mafia": SIDE_PADDING + 800,
        "time": SIDE_PADDING + 930,
        "rating": SIDE_PADDING + 1085,
    }
    selected_column = metric_column(metric)
    header_y = panel_top + 24
    for key, label in (
        ("rank", "#"),
        ("name", "이름"),
        ("record", "승패"),
        ("games", "판수"),
        ("winrate", "승률"),
        ("mafia", "마피아"),
        ("time", "시간"),
        ("rating", "레이팅"),
    ):
        draw.text(
            (columns[key], header_y),
            label,
            font=header_font,
            fill=ACCENT if key == selected_column else MUTED,
        )

    medal_fills = {1: "#f6c945", 2: "#c4ccd8", 3: "#c58b5b"}
    row_start_y = panel_top + 62
    for rank, (_user_id, entry) in enumerate(entries, start=1):
        y = row_start_y + (rank - 1) * ROW_HEIGHT
        row_fill = ROW_DARK if rank % 2 else ROW_LIGHT
        draw.rounded_rectangle(
            (SIDE_PADDING + 18, y, IMAGE_WIDTH - SIDE_PADDING - 18, y + ROW_HEIGHT - 10),
            radius=14,
            fill=row_fill,
        )

        medal_fill = medal_fills.get(rank, "#3b4252")
        draw.ellipse((columns["rank"] - 3, y + 16, columns["rank"] + 37, y + 56), fill=medal_fill)
        draw.text(
            (columns["rank"] + (9 if rank < 10 else 3), y + 22),
            str(rank),
            font=rank_font,
            fill="#111318" if rank <= 3 else TEXT,
        )

        games = int(entry.get("games", 0))
        wins = int(entry.get("wins", 0))
        losses = int(entry.get("losses", 0))
        mafia_games = int(entry.get("mafia_team_games", 0))
        play_seconds = int(entry.get("play_seconds", 0))
        rating = int(entry.get("rating", INITIAL_RATING))

        name = fit_text(draw, str(entry.get("name", "알 수 없음")), name_font, 250)
        draw.text((columns["name"], y + 18), name, font=name_font, fill=TEXT)
        draw.text((columns["record"], y + 21), f"{wins}승 {losses}패", font=value_bold_font if selected_column == "record" else value_font, fill=ACCENT if selected_column == "record" else TEXT)
        draw.text((columns["games"], y + 21), f"{games}판", font=value_bold_font if selected_column == "games" else value_font, fill=ACCENT if selected_column == "games" else TEXT)
        draw.text((columns["winrate"], y + 21), win_rate_text(wins, games), font=value_bold_font if selected_column == "winrate" else value_font, fill=ACCENT if selected_column == "winrate" else TEXT)
        draw.text((columns["mafia"], y + 21), f"{mafia_games}회", font=value_bold_font if selected_column == "mafia" else value_font, fill=ACCENT if selected_column == "mafia" else TEXT)
        draw.text((columns["time"], y + 21), play_duration_text(play_seconds), font=value_bold_font if selected_column == "time" else value_font, fill=ACCENT if selected_column == "time" else TEXT)
        draw.text((columns["rating"], y + 21), f"{rating}점", font=value_bold_font if selected_column == "rating" else value_font, fill=ACCENT if selected_column == "rating" else TEXT)

    draw.text(
        (SIDE_PADDING + 18, height - 30),
        "마피아 게임 진행 메시지",
        font=small_font,
        fill="#7f8798",
    )

    buffer = BytesIO()
    image.save(buffer, format="PNG", optimize=True)
    buffer.seek(0)
    return buffer
