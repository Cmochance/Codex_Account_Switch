from __future__ import annotations

from windows import common


class FakeRegistry:
    HKEY_CURRENT_USER = object()
    KEY_READ = 1
    KEY_WRITE = 2
    REG_EXPAND_SZ = 3

    def __init__(self, path_value: str | None):
        self.path_value = path_value

    def OpenKey(self, *_args, **_kwargs):
        return self

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def QueryValueEx(self, _key, _name):
        if self.path_value is None:
            raise FileNotFoundError
        return self.path_value, self.REG_EXPAND_SZ

    def SetValueEx(self, _key, _name, _reserved, _kind, value):
        self.path_value = value


def test_ensure_dir_on_user_path_prepends_new_entry():
    registry = FakeRegistry(r"C:\Program Files\Codex;C:\Other")

    changed = common.ensure_dir_on_user_path(r"C:\Users\Alice\.codex\bin", registry_module=registry)

    assert changed is True
    assert registry.path_value == r"C:\Users\Alice\.codex\bin;C:\Program Files\Codex;C:\Other"


def test_ensure_dir_on_user_path_moves_existing_entry_to_front():
    registry = FakeRegistry(r"C:\Program Files\Codex;C:\Users\Alice\.codex\bin;C:\Other")

    changed = common.ensure_dir_on_user_path(r"C:\Users\Alice\.codex\bin", registry_module=registry)

    assert changed is True
    assert registry.path_value == r"C:\Users\Alice\.codex\bin;C:\Program Files\Codex;C:\Other"


def test_ensure_dir_on_user_path_noops_when_entry_is_already_first():
    registry = FakeRegistry(r"C:\Users\Alice\.codex\bin;C:\Program Files\Codex;C:\Other")

    changed = common.ensure_dir_on_user_path(r"C:\Users\Alice\.codex\bin", registry_module=registry)

    assert changed is False
    assert registry.path_value == r"C:\Users\Alice\.codex\bin;C:\Program Files\Codex;C:\Other"
