# Fcitx 5 Osk

## Description

Fcitx 5 Osk is an on-screen keyboard designed to work with Fcitx 5. It provides a virtual keyboard for inputting text across various environments.

***I have only tested it on kwin6 wayland/xwayland.***

* landscape view
![landscape view](assets/landscape.png)

* portrait view
![portrait view](assets/portrait.png)

See [CHANGELOG.md](CHANGELOG.md) for changes since `0.1.0`.

## Features

* Works under various Wayland compositors and X11 (tested only in XWayland).
* Communicates with Fcitx 5 via `dbus` for key press events.
* Can be used on the KDE lock screen and login screen (SDDM) with the help of Fcitx 5 Osk Kwin Launcher.
* Support rotation detection, and use different layouts for landscape and portrait orientations.
* Support custom layouts and keys.
* Support theming.
* Support dock and float mode.
* Support manual mode, a quick action bar, combo mode, repeat actions, and custom actions.

## Fcitx 5 Osk Kwin Launcher

Fcitx 5 Osk Kwin Launcher is a critical component that enables Fcitx 5 Osk to function properly on the KDE lock screen and login screen. It initializes Fcitx 5 Osk based on the current context:

* **After login or unlock:** Starts Fcitx 5 with `WAYLAND_SOCKET` and launches Fcitx 5 Osk in normal mode.
* **On the unlock screen:** Starts Fcitx 5 Osk with `WAYLAND_SOCKET` and toggles it based on the KWin virtual keyboard visibility signal. In both the unlock and login screens, only surfaces created with `zwp_input_panel_v1` can be shown. Therefore, Fcitx 5 Osk must be launched with `WAYLAND_SOCKET` and communicate directly with KWin using `zwp_input_method_v1`.
* **On the login screen (SDDM):** Starts with the `--sddm` option, skipping communication with the FDO service (which is not yet available). Fcitx 5 Osk behaves similarly to how it does on the unlock screen.

## Fcitx 5 Osk Key Helper

Since fcitx5 doesn't forward modifier events correctly on Wayland, uppercase letters cannot be input. I have submitted a [PR fcitx/fcitx5#1292](https://github.com/fcitx/fcitx5/pull/1292), but it looks like it won't be merged. So, I came up a workaround: create a keyboard with evdev and use this keyboard to handle modifier events. This is what `fcitx5-osk-key-helper` does.

### Caveat

Creating a keyboard with evdev requires root privileges, and it may not be safe to expose a universal keyboard D-Bus API to user space. Therefore, fcitx5-osk-key-helper will handle modifier keycodes by default.

## Build and Installation

### Arch Linux

Packages are available in the AUR: [fcitx5-osk-git](https://aur.archlinux.org/packages/fcitx5-osk-git) and [fcitx5-osk-kwin-launcher-git](https://aur.archlinux.org/packages/fcitx5-osk-kwin-launcher-git).

### Manual

* build
```bash
cmake -B build -S .
cmake --build build
```

* install

```bash
cmake --install build --component Fcitx5Osk

# install kwin launcher
cmake --install build --component Fcitx5OskKwinLauncher
```

## Usage

### Enable Fcitx5 Osk Key Helper

```bash
# enable and start
sudo systemctl enable --now fcitx5-osk-key-helper
```

### Use without Fcitx5 Osk Key Helper

You can use a patched `fcitx5` and add/set `modifier_workaround_keycodes` in `${HOME}/.config/fcitx5-osk/config.toml` to empty.
```toml
modifier_workaround_keycodes = []
```

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

* For a manual build, `/usr/local/bin` might not be in `$PATH` of the process of `SDDM`, use the absolute path of `fcitx5-osk-kwin-launcher`:

```ini
# /etc/sddm.conf.d/rootless.conf
[General]
DisplayServer=wayland
GreeterEnvironment=QT_WAYLAND_SHELL_INTEGRATION=layer-shell

[Wayland]
CompositorCommand=kwin_wayland --drm --no-lockscreen --no-global-shortcuts --locale1 --inputmethod "/usr/local/bin/fcitx5-osk-kwin-launcher --sddm"
```

### Open Keyboard Manually

A key event from a keyboard of evdev will make `fcitx5` to hide a virtual keyboard. If the indicator is set to be `AlwaysOff`, and you want to open the keyboard. You can use this command.

```bash
fcitx5-osk force-show
```

Or you can click "Fcitx 5 Osk" in the application menu directly. You can add a quick launcher in the panel too.

### Custom Layouts, Keys, Themes and Actions

You can create your own layouts, keys, themes and custom actions, and specify the layout to be used in a specified input method. Keys are organized by a key set.

* By default, `fcitx5-osk` will search toml files in `fcitx5-osk/layouts`, `fcitx5-osk/key_sets`, `fcitx5-osk/themes` and `fcitx5-osk/custom_actions` under `$XDG_CONFIG_DIRS` and `$XDG_CONFIG_HOME`. If `$XDG_CONFIG_HOME` is not set, `$HOME/.config` will be used. If there are multiple configs with the same name, the latter one has higher priority.

* Set the layout to be used 
```toml
# Layouts to be used in landscape view. If no layout is specified for an input method, the builtin one will be used.
[im_layout_mapping.landscape]
# custom-layout-for-rime-landscape is the name of the layout. rime is the input method name. You can find it in the input method drop-down list of `fcitx5-osk`.

rime = "custom-layout-for-rime-landscape"

# Layouts to be used in portrait view. If no layout is specified for an input method, the builtin one will be used.
[im_layout_mapping.portrait]
# custom-layout-for-rime-portrait is the name of the layout. rime is the input method name. You can find it in the input method drop-down list of `fcitx5-osk`.
rime = "custom-layout-for-rime-portrait"
```

#### Layout Toml File

You can copy and edit the [builtin layout](assets/layouts/default-landscape.toml) to create your own. Here are some explanations:

```toml
# The name of the layout, it should be unique globally.
name = "Some Unique Name"

# The space between each row.
spacing = 1

# A row of the keys, the length or the width is a number of unit. The unit will be dynamic calculated.
[[elements]]
# The height of this row.
height = 6
# The space between each element of the row.
spacing = 1
# There are two kinds of element in a row: key and padding. A key is a name starting with 'k' and a padding is 'p'. You can specify the width of the element with ':number'. By default, the width of a key is 8, and the width of a padding is 1.
elements = ["p:3", "k21", "k22", "k23", "k24", "k25", "k26", "k27", "k28", "k29:12", "p:3"]

# Specify which key is to be used
[key_mappings]
# If no key set is specify, it will be found in the builtin one.
k21 = "k_grave_accent"
# k_grave_accent from a key set named "custom_key_set"
k22 = "custom_key_set:k_grave_accent"
```

#### Key Set Toml File

You can copy and edit the [builtin key set](assets/key_sets/default.toml) to create your own. Here are some explanations:
```toml
# The name of the key set, it should be unique globally.
name = "Some Unique Name"

# Specify "k_escape"
[keys.k_escape]
# The primary form: keysym is 0xff1b, symbol to be shown is "Esc", keycode(x11 variant) is 9
p = {ks = 0xff1b, s = "Esc", kc = 9}

# Specify "k_p_four"
[keys.k_p_four]
# The primary form: character is '4' (symbol and keysym will be generated by the character automatically), keycode(x11 variant) is 13
p = {c = "4", kc = 13}
[[keys.k_p_four.s]]
# The first secondary form: character is '$', keycode(x11 variant) is -13 (Negative keycode means shift should be pressed)
c = "$"
kc = -13
[[keys.k_p_four.s]]
# The second secondary form: character is '/', keycode(x11 variant) is 61
c = "/"
kc = 61
[[keys.k_p_four.s]]
# The third secondary form: character is '?', keycode(x11 variant) is -61
c = "?"
kc = -61
```

#### Theme Toml File

Here is an example.

```toml
name = "Breeze Light"

# all fields of `palette` must be provided
[palette]
background = "#ffffff"
text = "#232629"
primary = "#3daee9"
success = "#27ae60"
danger = "#da4453"
warning = "#f67400"

# This is the extended palette for advanced settings
# all fields of `extended_palette` must be provided if you add the `extended_palette` section
[extended_palette]
is_dark = false

[extended_palette.background.base]
color = "#ffffff"
text = "#232629"
[extended_palette.background.weak]
color = "#ffffff"
text = "#232629"
[extended_palette.background.strong]
color = "#ffffff"
text = "#232629"
[extended_palette.background.weakest]
color = "#ffffff"
text = "#232629"
[extended_palette.background.weaker]
color = "#ffffff"
text = "#232629"
[extended_palette.background.neutral]
color = "#ffffff"
text = "#232629"
[extended_palette.background.stronger]
color = "#ffffff"
text = "#232629"
[extended_palette.background.strongest]
color = "#ffffff"
text = "#232629"

[extended_palette.primary.base]
color = "#ffffff"
text = "#232629"
[extended_palette.primary.weak]
color = "#ffffff"
text = "#232629"
[extended_palette.primary.strong]
color = "#ffffff"
text = "#232629"

[extended_palette.secondary.base]
color = "#ffffff"
text = "#232629"
[extended_palette.secondary.weak]
color = "#ffffff"
text = "#232629"
[extended_palette.secondary.strong]
color = "#ffffff"
text = "#232629"

[extended_palette.success.base]
color = "#ffffff"
text = "#232629"
[extended_palette.success.weak]
color = "#ffffff"
text = "#232629"
[extended_palette.success.strong]
color = "#ffffff"
text = "#232629"

[extended_palette.danger.base]
color = "#ffffff"
text = "#232629"
[extended_palette.danger.weak]
color = "#ffffff"
text = "#232629"
[extended_palette.danger.strong]
color = "#ffffff"
text = "#232629"

[extended_palette.warning.base]
color = "#ffffff"
text = "#232629"
[extended_palette.warning.weak]
color = "#ffffff"
text = "#232629"
[extended_palette.warning.strong]
color = "#ffffff"
text = "#232629"
```

#### Custom Action Toml File

Custom actions are shown in the quick action bar. Put action files under `${XDG_CONFIG_HOME:-$HOME/.config}/fcitx5-osk/custom_actions`, then enable them in `${XDG_CONFIG_HOME:-$HOME/.config}/fcitx5-osk/config.toml`:

```toml
quick_action_bar_state = "On"
custom_actions = ["Ctrl+"]
```

`quick_action_bar_state` can be `Off`, `On` or `Toggle`. The action name in `custom_actions` must match the `name` field in the action file.

Here is a static action that provides `Ctrl+C` and `Ctrl+V` candidates:

```toml
# ~/.config/fcitx5-osk/custom_actions/ctrl_plus.toml
name = "Ctrl+"

[action]
type = "Static"

[[action.groups]]
[[action.groups.keys]]
type = "Key"
ks = 0xffe3
s = "Ctrl"
kc = 37

[[action.groups.keys]]
type = "Key"
c = "c"
kc = 54

[[action.groups]]
[[action.groups.keys]]
type = "Key"
ks = 0xffe3
s = "Ctrl"
kc = 37

[[action.groups.keys]]
type = "Key"
c = "v"
kc = 55
```

Each `[[action.groups]]` entry creates one candidate. Each key in a group has `type = "Key"` and uses the same fields as a key set key value:

* `c`: a character, such as `"c"` or `"v"`.
* `ks`: an X keysym value, such as `0xffe3` for Ctrl.
* `s`: the symbol shown in the UI.
* `kc`: the X11 keycode. A negative keycode means Shift should be pressed.
* `f`: optional font name or font id.

You can also use `type = "Release"` to release the last pressed key in combo mode, or `type = "ReleaseAll"` to release all pressed keys.

##### `HttpApi` Custom Actions

`HttpApi` custom actions are enabled by default at build time. They request candidates from an HTTP endpoint, then show the returned prompts or key groups in the candidate area. Use this for integrations such as OTP or password providers. The [otp-push](https://github.com/fortime/otp-push) project provides a server for requesting and returning OTP or secret values for this workflow.

Installed packages may provide template actions, [otp.toml](pkg/share/fcitx5-osk/custom_actions/otp.toml) and [pw.toml](pkg/share/fcitx5-osk/custom_actions/pw.toml), under `/usr/share/fcitx5-osk/custom_actions/`. Link the template into your user config folder and put private values in a drop-in directory next to the linked file:

```bash
mkdir -p ~/.config/fcitx5-osk/custom_actions
ln -s /usr/share/fcitx5-osk/custom_actions/otp.toml ~/.config/fcitx5-osk/custom_actions/otp.toml
mkdir -p ~/.config/fcitx5-osk/custom_actions/otp.toml.d
```

Then create a drop-in file to override user-specific settings:

```toml
# ~/.config/fcitx5-osk/custom_actions/otp.toml.d/user.toml
name = "user-overridden-name"

[action.headers.auth]
X-Api-Token = ["your-token"]
```

Finally, enable the action in `${XDG_CONFIG_HOME:-$HOME/.config}/fcitx5-osk/config.toml`:

```toml
quick_action_bar_state = "On"
custom_actions = ["OTP"]
```

For password-style actions that need encrypted responses, use the same pattern with the `pw.toml` template:

```bash
ln -s /usr/share/fcitx5-osk/custom_actions/pw.toml ~/.config/fcitx5-osk/custom_actions/pw.toml
mkdir -p ~/.config/fcitx5-osk/custom_actions/pw.toml.d
```

```toml
# ~/.config/fcitx5-osk/custom_actions/pw.toml.d/user.toml
name = "user-overridden-name"

[action.headers.auth]
X-Api-Token = ["your-token"]
```

A minimal `HttpApi` action looks like this:

```toml
name = "OTP"

[action]
type = "HttpApi"

[action.headers.auth]
X-Api-Token = ["your-token"]

[action.targets.0]
headers_name = "auth"
url = "https://example.com/api/request"
method = "POST"
need_enter = true
```

The action supports shared `headers`, `queries` and `bodies` tables. Each target can select them with `headers_name`, `query_name` and `body_name`. `need_enter` appends Enter to returned key groups, `need_mask` hides candidate text, and `need_encrypt` requests an encrypted secret flow and always masks the secret.

The HTTP response is JSON with these fields:

```json
{
  "prompts": [[["Label", null]]],
  "groups": [],
  "secret": null,
  "next": null
}
```

`prompts` shows text candidates, `groups` returns key groups using the same `type = "Key"` shape as static custom actions, `secret` returns encrypted text for `need_encrypt = true`, and `next` makes the action continue polling with `GET`.

## Troubleshoot

### `GTK_IM_MODULE` and `QT_IM_MODULE` are set

With `QT_IM_MODULE` set, the virtual keyboard won't be shown in the SDDM login screen and the KDE lock screen. In a distribution with `im-config` installed, like Debian 13, it will set `*_IM_MODULE` by default. You can set `IM_CONFIG_DEFAULT_MODE` to `none` in `/etc/default/im-config`.

## Issues

* [PR fcitx/fcitx5#1292](https://github.com/fcitx/fcitx5/pull/1292) is needed to input correctly in latin mode of Fcitx 5.

## TODO

* [ ] integrated with xcbcommon.
* [x] iced 14.
* [ ] add `RefreshRequest` for our widgets.
* [ ] use `RefreshRequest` to implement long press event?
* [x] support custom theme
* [ ] support custom shape of key
* [x] support repeat key mode
