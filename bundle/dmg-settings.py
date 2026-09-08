"""Finder layout for the release DMG, built with dmgbuild via uvx."""

from pathlib import Path

# dmgbuild provides command-line -D values in `defines`.
app = Path(defines["app"]).resolve()

format = "UDZO"
filesystem = "HFS+"
files = [(str(app), "ProxyBear.app")]
symlinks = {"Applications": "/Applications"}
icon = str(Path("bundle/ProxyBear.icns").resolve())
background = "builtin-arrow"

window_rect = ((200, 200), (640, 280))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 96
text_size = 14
icon_locations = {"ProxyBear.app": (160, 120), "Applications": (480, 120)}
