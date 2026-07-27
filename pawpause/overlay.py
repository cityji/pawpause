import os
import shutil
import subprocess

from .notify import notify


class Overlay:
    """Starts/stops the mpvpaper break video as a layer-shell overlay.

    Uses `mpvpaper -l overlay` (zwlr_layer_shell_v1's "overlay" layer, above
    normal windows) rather than any custom Wayland protocol code.
    """

    def __init__(self):
        self.proc = None

    def start(self, video_path, output_name):
        self.stop()

        video_path = os.path.expanduser(video_path)
        if not os.path.isfile(video_path):
            notify("PawPause", f"Break video not found: {video_path}")
            return

        if shutil.which("mpvpaper") is None:
            notify("PawPause", "mpvpaper is not installed — skipping video overlay.")
            return

        try:
            self.proc = subprocess.Popen(
                [
                    "mpvpaper",
                    "-l", "overlay",
                    "-o", "loop-file=inf no-audio-display",
                    output_name,
                    video_path,
                ],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        except Exception as exc:
            notify("PawPause", f"Failed to launch mpvpaper: {exc}")
            self.proc = None

    def stop(self):
        if self.proc is None:
            return
        proc, self.proc = self.proc, None
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)
