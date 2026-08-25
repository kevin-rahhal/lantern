# Installing Lantern

## Windows 10 or 11

1. Download the `.exe` from the [latest release](https://github.com/kevin-rahhal/lantern/releases/latest).
2. Open it and follow the short per-user installer. Administrator access is not normally required.
3. If SmartScreen appears, choose **More info**, verify that the app is named **Lantern**, then choose **Run anyway**.

Lantern uses the Windows OpenSSH Client. In PowerShell, `ssh -V` should print a version. If it is missing, open **Settings → System → Optional features → View features**, search for **OpenSSH Client**, and install it.

## macOS 10.15 or newer

1. Choose the Apple silicon or Intel `.dmg` from the [latest release](https://github.com/kevin-rahhal/lantern/releases/latest).
2. Open the disk image and drag Lantern to **Applications**.
3. Because the first release is not notarized, Control-click Lantern, choose **Open**, then confirm **Open**. You normally need to do this only once.

macOS includes an OpenSSH client.

## Linux

- **AppImage:** download it, make it executable, and open it. AppImage works across many distributions without installation.
- **Debian/Ubuntu:** download the `.deb` and open it with your software installer, or use `sudo apt install ./Lantern_*.deb`.

Install your distribution's OpenSSH client package if `ssh -V` is unavailable. Password saving also requires a Secret Service provider, such as GNOME Keyring or KDE Wallet, to be running.

## Verifying a download

Each release includes checksums. Compare the SHA-256 value for your downloaded file before opening it, especially when the platform displays an unsigned-app warning.

## Uninstalling

- **Windows:** Settings → Apps → Installed apps → Lantern → Uninstall.
- **macOS:** move Lantern from Applications to the Trash.
- **Linux:** delete the AppImage, or remove the installed Debian package using your package manager.

Lantern keeps profiles and personal device names in your per-user application-data folder and passwords in the OS credential vault. Uninstallers may intentionally leave this user data in place. Remove it manually only if you no longer need it.
