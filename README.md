# Fcitx 5 Osk

## Description

Fcitx 5 Osk is an on-screen keyboard designed to work with Fcitx 5. It provides a virtual keyboard for inputting text across various environments.

## Features

* Works under various Wayland compositors and X11 (tested only in XWayland).
* Communicates with Fcitx 5 via `dbus` for key press events.
* Can be used on the KDE lock screen and login screen (SDDM) with the help of Fcitx 5 Osk Kwin Launcher.

## Fcitx 5 Osk Kwin Launcher

Fcitx 5 Osk Kwin Launcher is a critical component that enables Fcitx 5 Osk to function properly on the KDE lock screen and login screen. It initializes Fcitx 5 Osk based on the current context:

* **After login or unlock:** Starts Fcitx 5 with `WAYLAND_SOCKET` and launches Fcitx 5 Osk in normal mode.
* **On the unlock screen:** Starts Fcitx 5 Osk with `WAYLAND_SOCKET` and toggles it based on the KWin virtual keyboard visibility signal. In both the unlock and login screens, only surfaces created with `zwp_input_panel_v1` can be shown. Therefore, Fcitx 5 Osk must be launched with `WAYLAND_SOCKET` and communicate directly with KWin using `zwp_input_method_v1`.
* **On the login screen (SDDM):** Starts with the `--sddm` option, skipping communication with the FDO service (which is not yet available). Fcitx 5 Osk behaves similarly to how it does on the unlock screen.

## Build and Installation

### Arch Linux

```sh
# Build
cd pkg/archlinux
makepkg

# Installation
sudo pacman -U fcitx5-osk*git*.pkg.tar.zst
```

### Manual

* build
```sh
cargo build --frozen --release
```

* install

```sh
sudo mkdir -p /usr/local/share/applications
sudo mkdir -p /usr/local/share/dbus-1/services
sudo cp target/release/fcitx5-osk /usr/local/bin/
sudo cp target/release/fcitx5-osk-kwin-launcher /usr/local/bin/
sed 's/\/usr\/bin/\/usr\/local\/bin/g' pkg/share/applications/fcitx5-osk-kwin-launcher.desktop > /usr/local/share/applications/fcitx5-osk-kwin-launcher.desktop
sed 's/\/usr\/bin/\/usr\/local\/bin/g' pkg/share/dbus-1/services/fyi.fortime.Fcitx5Osk.service > /usr/local/share/dbus-1/services/fyi.fortime.Fcitx5Osk.service
```

## Usage

### Kwin (Wayland)

To enable Fcitx 5 Osk Kwin Launcher:
Go to **System Settings** → **Keyboard** → **Virtual Keyboard**, and select **"Fcitx 5 Osk Kwin Launcher"**.

### SDDM

Add the input method option with the value `"fcitx5-osk-kwin-launcher --sddm"` to `kwin_wayland`.
Here is an example configuration file:

```ini
# /etc/sddm.conf.d/rootless.conf
[General]
DisplayServer=wayland
GreeterEnvironment=QT_WAYLAND_SHELL_INTEGRATION=layer-shell

[Wayland]
CompositorCommand=kwin_wayland --drm --no-lockscreen --no-global-shortcuts --locale1 --inputmethod "fcitx5-osk-kwin-launcher --sddm"
```

## Issues

* [PR fcitx/fcitx5#1292](https://github.com/fcitx/fcitx5/pull/1292) is needed to input correctly in latin mode of Fcitx 5.
* [iced\_layershell](https://github.com/waycrate/exwlshelleventloop) is patched. All patches has been upstreamed. But it is waiting for the release of iced 14.

## TODO

* integrated with xcbcommon.
* use Fcitx 5 Osk Kwin Launcher to monitor the change of orientation and then toggle Fcitx 5 Osk.
* iced 14 and add `RefreshRequest` for our widgets.
* use `RefreshRequest` to implement long press state change?
