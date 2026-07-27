import subprocess


def notify(title, body):
    """Fire a desktop notification. Silently no-ops if notify-send is missing."""
    try:
        subprocess.Popen(["notify-send", title, body])
    except FileNotFoundError:
        pass
