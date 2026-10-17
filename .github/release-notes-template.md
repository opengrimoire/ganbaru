Expand the Assets section below and choose the installer for your platform.

## Download

| Platform | Download |
| --- | --- |
| **Ubuntu, Debian, Linux Mint** | `.deb` package |
| **Fedora, RHEL, openSUSE** | `.rpm` package |
| **Arch Linux, Manjaro, EndeavourOS, Garuda** | `ganbaru-bin` from the AUR |
| **Other Linux x64 desktops** | `.AppImage` |
| **Windows 10 or Windows 11 x64** | `.msi` installer, or the `.exe` setup if preferred |
| **macOS** | Not available yet. macOS builds need Apple hardware and signing. |
| **Android 10 or newer** | Signed universal `.apk` for direct installation. Back up important data before uninstalling a development-stage build. |
| **iOS** | Not available yet. iOS depends on the Apple build and signing path. |

Arch-based distribution users can install with `yay -S ganbaru-bin` or another AUR helper.

Android direct installation: Play Protect may show "App blocked to protect your device" because Ganbaru declares an optional Accessibility Service for selected-app blocking. If there is no "Install anyway" option, temporarily turn off **Scan apps with Play Protect** in Play Store > profile > Play Protect > settings, install only the APK from this official release, and turn scanning back on immediately.

Use `SHA256SUMS` to verify downloaded installers. The `.sig` files and `latest.json` are consumed by Ganbaru's built-in desktop updater.
