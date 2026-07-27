import json
from dataclasses import dataclass
from pathlib import Path

CONFIG_DIR = Path.home() / ".config" / "pawpause"
CONFIG_PATH = CONFIG_DIR / "config"

DEFAULTS = {
    "work_minutes": 25,
    "short_break_minutes": 5,
    "long_break_minutes": 20,
    "sessions_before_long_break": 4,
    "video_path": str(Path.home() / "Videos" / "pawpause-break.mp4"),
    "wayland_output": "eDP-1",
}


@dataclass
class Config:
    work_minutes: int
    short_break_minutes: int
    long_break_minutes: int
    sessions_before_long_break: int
    video_path: str
    wayland_output: str


def load_or_create():
    """Load ~/.config/pawpause/config, creating it with defaults if missing.

    Unknown/missing keys fall back to defaults so a hand-edited config never
    crashes the app. Returns (Config, created) where created is True the
    first time the file is written.
    """
    created = False
    if not CONFIG_PATH.exists():
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        CONFIG_PATH.write_text(json.dumps(DEFAULTS, indent=2) + "\n")
        created = True
        data = dict(DEFAULTS)
    else:
        data = dict(DEFAULTS)
        data.update(json.loads(CONFIG_PATH.read_text()))

    return Config(**{key: data[key] for key in DEFAULTS}), created
