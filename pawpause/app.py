from . import config as config_mod
from .notify import notify
from .tray import TrayApp


def main():
    config, created = config_mod.load_or_create()
    if created:
        message = f"Created default config at {config_mod.CONFIG_PATH}"
        print(message)
        notify("PawPause", message)

    TrayApp(config).run()
