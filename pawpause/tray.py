import shutil
import subprocess
import threading
import time

import pystray
from PIL import Image, ImageDraw

from . import config as config_mod
from .notify import notify
from .overlay import Overlay
from .pomodoro import BREAK_PHASES, Phase, Pomodoro, RunState

PHASE_COLORS = {
    None: (140, 140, 140, 255),
    Phase.WORK: (214, 64, 69, 255),
    Phase.SHORT_BREAK: (72, 168, 105, 255),
    Phase.LONG_BREAK: (66, 133, 214, 255),
}

TRANSITION_MESSAGES = {
    Phase.WORK: ("Back to work", "Focus time started."),
    Phase.SHORT_BREAK: ("Break time", "Step away for a short break."),
    Phase.LONG_BREAK: ("Long break", "Nice work — take a longer break."),
    None: ("PawPause", "Stopped."),
}

SETTINGS_EDITORS = ["cosmic-edit", "gnome-text-editor", "gedit", "kate", "xdg-open"]


def _make_icon_image(phase, paused):
    color = PHASE_COLORS[phase]
    if paused:
        color = tuple(int(c + (255 - c) * 0.5) if i < 3 else c for i, c in enumerate(color))

    img = Image.new("RGBA", (64, 64), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.ellipse((4, 4, 60, 60), fill=color)
    return img


class TrayApp:
    def __init__(self, config):
        self.config = config
        self.overlay = Overlay()
        self.pomodoro = Pomodoro(config, self._on_transition, self._on_tick)
        self._stop_event = threading.Event()

        self.icon = pystray.Icon(
            "pawpause",
            _make_icon_image(None, False),
            "PawPause — Idle",
            menu=self._build_menu(),
        )

    # ---- Pomodoro callbacks -------------------------------------------------

    def _on_transition(self, old_phase, new_phase):
        if old_phase in BREAK_PHASES:
            self.overlay.stop()
        if new_phase in BREAK_PHASES:
            self.overlay.start(self.config.video_path, self.config.wayland_output)

        title, body = TRANSITION_MESSAGES[new_phase]
        notify(title, body)
        self._refresh_ui()

    def _on_tick(self):
        self._refresh_ui()

    # ---- UI -------------------------------------------------------------

    def _refresh_ui(self):
        paused = self.pomodoro.state == RunState.PAUSED
        self.icon.icon = _make_icon_image(self.pomodoro.phase, paused)
        self.icon.title = f"PawPause — {self.pomodoro.status_text()}"
        # Rebuild a fresh Menu object rather than mutating existing MenuItems:
        # COSMIC's tray host does not reliably redraw in-place item mutations.
        self.icon.menu = self._build_menu()

    def _build_menu(self):
        status_item = pystray.MenuItem(self.pomodoro.status_text(), None, enabled=False)
        items = [status_item, pystray.Menu.SEPARATOR]

        if self.pomodoro.state == RunState.IDLE:
            items.append(pystray.MenuItem("Start Pomodoro", self._handle_start))
        else:
            toggle_label = "Pause" if self.pomodoro.state == RunState.RUNNING else "Resume"
            items.append(pystray.MenuItem(toggle_label, self._handle_toggle))
            items.append(pystray.MenuItem("Skip", self._handle_skip))
            items.append(pystray.MenuItem("Stop/Reset", self._handle_stop))

        items.append(pystray.Menu.SEPARATOR)
        items.append(pystray.MenuItem("Settings", self._handle_settings))
        items.append(pystray.MenuItem("Quit", self._handle_quit))
        return pystray.Menu(*items)

    # ---- Menu handlers ----------------------------------------------------

    def _handle_start(self, icon, item):
        self.pomodoro.start()

    def _handle_toggle(self, icon, item):
        if self.pomodoro.state == RunState.RUNNING:
            self.pomodoro.pause()
        else:
            self.pomodoro.resume()
        self._refresh_ui()

    def _handle_skip(self, icon, item):
        self.pomodoro.skip()

    def _handle_stop(self, icon, item):
        self.pomodoro.stop()

    def _handle_settings(self, icon, item):
        for editor in SETTINGS_EDITORS:
            if shutil.which(editor) is None:
                continue
            try:
                subprocess.Popen([editor, str(config_mod.CONFIG_PATH)])
                return
            except OSError:
                continue
        notify("PawPause", f"Open {config_mod.CONFIG_PATH} in your editor to change settings.")

    def _handle_quit(self, icon, item):
        self._stop_event.set()
        self.overlay.stop()
        icon.stop()

    # ---- Lifecycle ----------------------------------------------------

    def _ticker_loop(self):
        while not self._stop_event.is_set():
            time.sleep(1)
            self.pomodoro.tick()

    def run(self):
        def setup(icon):
            icon.visible = True
            threading.Thread(target=self._ticker_loop, daemon=True).start()

        self.icon.run(setup=setup)
