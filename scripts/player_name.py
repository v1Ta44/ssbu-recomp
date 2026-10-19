"""Explicit local game identity shared by the command-line launchers; never uses an OS account name."""
import configparser
from pathlib import Path
import unicodedata


def game_name(build, explicit=None):
    if explicit is None:
        ini = configparser.ConfigParser(interpolation=None)
        ini.read(Path(build) / 'launcher/launcher.ini', encoding='utf-8-sig')
        explicit = ini.get('run', 'game_name', fallback=None)
        if explicit is None:
            explicit = ini.get('net', 'game_name', fallback='')
    value = explicit.strip()
    if not value:
        raise ValueError('Fill in the in-game name on the Console Start page, or pass --nickname <name>.')
    if len(value.encode('utf-8')) > 32 or any(unicodedata.category(c) == 'Cc' for c in value):
        raise ValueError('The in-game name must use at most 32 UTF-8 bytes and no control characters.')
    return value
