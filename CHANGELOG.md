# Changelog

## Changes Since 0.1.0

The current development version is `0.2.0`. Notable changes since `0.1.0` include:

Thanks to @MorsMortium for giving a lot of feedback.

* Build and packaging now use CMake, with separate install components for `fcitx5-osk` and `fcitx5-osk-kwin-launcher`; Arch packages are available in the AUR.
* The Rust workspace now uses edition 2024, `iced` 0.14, and `iced_layershell` 0.16.
* Manual mode was added, along with the `fcitx5-osk force-show` command for opening the keyboard manually.
* The keyboard UI now supports a quick action bar, reload action, combo mode, repeat key action, custom actions, and popup key feedback while pressing keys.
* Layout and appearance configuration is more flexible: custom themes, auto/light/dark theme selection, XDG config lookup, `.d` asset overrides, global default layout overrides, fractional sizing, and a wider landscape layout are supported.
* Improved lock/login screen behavior through the KWin launcher.
* Reliability fixes include handling `SIGPIPE` without quitting, corrected secondary symbol rendering, better redraw/hover handling, scaling fixes for toggles and drop-downs, and clearer error notifications.
