# fcitx5-osk on Fedora 44 + KDE Plasma 6 (Wayland)

Tested on: Fedora 44, KDE Plasma 6 (Wayland), fcitx5 5.1.22 (patched), fcitx5-osk 0.2.1, 2-in-1 laptop with tablet mode switch.

---

## 1. Prerequisites

    sudo dnf install cmake gcc rust cargo \
      dbus-devel fcitx5-devel fcitx5-libs-devel \
      layer-shell-qt-devel qt6-qtbase-devel \
      libevdev-devel fcitx5 fcitx5-autostart

---

## 2. Build and Install fcitx5-osk

    mkdir -p ~/src && cd ~/src
    git clone https://github.com/fortime/fcitx5-osk.git
    cd fcitx5-osk
    git checkout 0.2.1

    cmake -B build -S . -DCMAKE_BUILD_TYPE=Release
    cmake --build build --parallel
    sudo cmake --install build --component Fcitx5Osk
    sudo cmake --install build --component Fcitx5OskKwinLauncher

---

## 3. Permissions and udev Rules

Two things need access:

- **fcitx5-osk-key-helper** (runs as root as a system service) needs write access to `/dev/uinput` to emit modifier key events.
- **libinput debug-events** (used for tablet mode detection) needs read access to `/dev/input/*`, which requires the user to be in the `input` group.

### uinput udev rule

By default `/dev/uinput` is `root:root 0600`. The key helper's systemd service installs a udev rule, but on Fedora you may need to trigger it manually:

    sudo modprobe uinput
    echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | \
      sudo tee /etc/udev/rules.d/99-fcitx5-osk-uinput.rules
    sudo udevadm control --reload-rules
    sudo udevadm trigger --subsystem-match=misc

To make the uinput module load on boot:

    echo uinput | sudo tee /etc/modules-load.d/uinput.conf

### User groups

Add your user to the `input` group (covers both libinput and uinput access via the rule above):

    groups $USER  # check current groups
    sudo usermod -aG input $USER
    # log out and back in for group membership to take effect

### Enable the Key Helper Service

On Fedora the service file installs to `/usr/lib64/systemd/system/` but systemd looks in `/usr/lib/` -- fix with a symlink:

    sudo ln -s /usr/lib64/systemd/system/fcitx5-osk-key-helper.service \
               /usr/lib/systemd/system/fcitx5-osk-key-helper.service
    sudo systemctl daemon-reload
    sudo systemctl enable --now fcitx5-osk-key-helper

---

## 4. Set fcitx5-osk as the Virtual Keyboard in KDE

System Settings -> Keyboard -> Virtual Keyboard -> Fcitx 5 Osk Kwin Launcher

Do NOT set GTK_IM_MODULE or QT_IM_MODULE globally. Only set in ~/.config/environment.d/fcitx5.conf:

    XMODIFIERS=@im=fcitx

Log out and back in.

---

## 5. Fix Latin Input (keyboard-us) -- Patch fcitx5

keyboard-us produces no input through fcitx5-osk on Wayland out of the box. This is a known upstream issue (PR fcitx/fcitx5#1292, unmerged as of this writing). Rebuild fcitx5 with the patch applied:

    sudo dnf install -y rpm-build rpmdevtools
    rpmdev-setuptree
    sudo dnf builddep -y fcitx5

    cd /tmp
    dnf download --source fcitx5
    rpm -ivh fcitx5-*.src.rpm

    curl -L https://github.com/fcitx/fcitx5/pull/1292.patch \
      -o ~/rpmbuild/SOURCES/fcitx5-osk-modifier.patch

    sed -i '/^Source0:/a Patch100: fcitx5-osk-modifier.patch' \
      ~/rpmbuild/SPECS/fcitx5.spec

    rpmbuild -ba ~/rpmbuild/SPECS/fcitx5.spec

    sudo rpm -Uvh --force \
      ~/rpmbuild/RPMS/x86_64/fcitx5-5.*.rpm \
      ~/rpmbuild/RPMS/x86_64/fcitx5-libs-5.*.rpm

    fcitx5 -r -d

The patch stays in ~/rpmbuild/SOURCES/ and the spec stays patched -- future rebuilds only need rpmbuild -ba + rpm -Uvh.

---

## 6. Tablet Mode Auto-Switch (2-in-1 devices)

Automatically switch between fcitx5-osk (tablet mode) and the standard fcitx5 Wayland launcher (laptop mode) using libinput debug-events to detect the hardware tablet mode switch.

KDE applies the change live when written with kwriteconfig6 --notify. Do NOT use qdbus reconfigure or kwin_wayland --replace -- they crash the session.

Switcher script (~/.local/bin/tablet-mode-vkbd.sh):

    #!/bin/bash
    TABLET='/usr/share/applications/fyi.fortime.Fcitx5Osk.KwinLauncher.desktop'
    LAPTOP='/usr/share/applications/fcitx5-wayland-launcher.desktop'

    libinput debug-events | while read -r line; do
      if echo "$line" | grep -q "SWITCH_TOGGLE.*tablet-mode state 1"; then
        kwriteconfig6 --notify --file kwinrc --group Wayland --key InputMethod "$TABLET"
      elif echo "$line" | grep -q "SWITCH_TOGGLE.*tablet-mode state 0"; then
        kwriteconfig6 --notify --file kwinrc --group Wayland --key InputMethod "$LAPTOP"
      fi
    done

    chmod +x ~/.local/bin/tablet-mode-vkbd.sh

Systemd user service (~/.config/systemd/user/tablet-mode-vkbd.service):

    [Unit]
    Description=Tablet mode virtual keyboard switcher
    After=graphical-session.target

    [Service]
    ExecStart=%h/.local/bin/tablet-mode-vkbd.sh
    Restart=always
    RestartSec=3

    [Install]
    WantedBy=graphical-session.target

    systemctl --user daemon-reload
    systemctl --user enable --now tablet-mode-vkbd

Note: verify your device exposes a hardware tablet mode switch first:
    libinput debug-events | grep SWITCH   # fold/unfold while running

---

## 7. Keeping Everything Updated (Topgrade)

### Prevent dnf from overwriting the patched fcitx5

A regular system update will replace your patched RPM with the official one since they share the same version string. Pin it with versionlock:

    sudo dnf install -y python3-dnf-plugin-versionlock
    sudo dnf versionlock add fcitx5 fcitx5-libs

The update script below unlocks, rebuilds, installs, then relocks automatically.

### ~/.local/bin/update-fcitx5.sh

    #!/bin/bash
    set -e
    # Unlock so we can replace with freshly rebuilt patched version
    sudo dnf versionlock delete fcitx5 fcitx5-libs || true
    cd /tmp
    rm -f fcitx5-*.src.rpm
    dnf download --source fcitx5
    rpm -Uvh --force fcitx5-*.src.rpm
    rpmbuild -ba ~/rpmbuild/SPECS/fcitx5.spec
    sudo rpm -Uvh --force \
      ~/rpmbuild/RPMS/x86_64/fcitx5-5.*.rpm \
      ~/rpmbuild/RPMS/x86_64/fcitx5-libs-5.*.rpm
    sudo dnf versionlock add fcitx5 fcitx5-libs
    fcitx5 -r -d
    rm -f /tmp/fcitx5-*.src.rpm

### ~/.local/bin/update-fcitx5-osk.sh

    #!/bin/bash
    set -e
    REPO=~/src/fcitx5-osk
    if [ ! -d "$REPO" ]; then
      mkdir -p ~/src
      git clone https://github.com/fortime/fcitx5-osk.git "$REPO"
    fi
    cd "$REPO"
    git fetch --tags
    LATEST=$(git tag --sort=-v:refname | head -1)
    CURRENT=$(git describe --tags)
    if [ "$LATEST" = "$CURRENT" ]; then
      echo "fcitx5-osk already at $CURRENT"
      exit 0
    fi
    git checkout "$LATEST"
    cmake -B build -S . -DCMAKE_BUILD_TYPE=Release
    cmake --build build --parallel
    sudo cmake --install build --component Fcitx5Osk
    sudo cmake --install build --component Fcitx5OskKwinLauncher
    sudo systemctl restart fcitx5-osk-key-helper
    echo "Updated fcitx5-osk to $LATEST"

    chmod +x ~/.local/bin/update-fcitx5.sh ~/.local/bin/update-fcitx5-osk.sh

Add to ~/.config/topgrade.toml:

    [commands]
    "fcitx5 (patched)" = "~/.local/bin/update-fcitx5.sh"
    "fcitx5-osk" = "~/.local/bin/update-fcitx5-osk.sh"

---

## Troubleshooting

**keyboard-us produces no input:** fcitx5 is not patched. Verify PR #1292 patch is in the spec and the rebuilt RPMs were force-installed.

**Modifier keys don't work:** Key helper isn't running or lacks uinput access. Check `systemctl status fcitx5-osk-key-helper` and `groups $USER`.

**Service file not found after install:** Symlink from /usr/lib64/ to /usr/lib/ as shown in step 3.

**Tablet mode switch not detected:** Run `libinput debug-events | grep SWITCH` while folding -- if nothing appears, your device doesn't expose a hardware switch.

**KWin crashes:** Only use `kwriteconfig6 --notify` to switch virtual keyboards. Never use `qdbus reconfigure` or `kwin_wayland --replace` for this.
