# eizo-switch

Show or switch the input of an EIZO FlexScan EV3895 over USB HID, from macOS or Linux.

The EV3895 exposes no DDC/CI, so `m1ddc` and `ddcutil` cannot switch its input. EIZO's own tool (Screen InStyle) supports only Windows and macOS. This CLI talks to the monitor's `EIZO USB HID Monitor` interface (USB `056d:4065`) instead.

## Usage

```
eizo-switch          # print the current input: dp, usb-c, hdmi1, hdmi2
eizo-switch dp       # switch to DisplayPort
eizo-switch usb-c    # switch to USB-C
```

The monitor's built-in KVM moves its USB hub to whichever PC owns the active input. The monitor is therefore reachable only from the PC that currently has the input. After switching away, the command may print a "not ready" error. This is expected. Switch back from the other PC.

## Waking the target display

If the target PC's display is asleep, the screen stays dark until it wakes. `--before` runs a shell command first, so you can wake the target as you switch:

```
# macOS -> Linux (Hyprland)
eizo-switch dp --before 'ssh linux-host "HYPRLAND_INSTANCE_SIGNATURE=\$(ls /run/user/\$(id -u)/hypr | head -1) hyprctl dispatch \"hl.dsp.dpms({ action = \\\"on\\\" })\""'

# Linux -> macOS
eizo-switch usb-c --before 'ssh mac-host caffeinate -u -t 2'
```

A failing `--before` command prints a warning and the switch still happens.

## Install

```
cargo install --path .
```

On Linux, allow your user to open the hidraw node without root, by installing the bundled udev rule as root:

```
install -m 644 udev/70-eizo-switch.rules /etc/udev/rules.d/
udevadm control --reload && udevadm trigger
```

hidapi needs the `libudev` development files on Linux.

## Scope

Only the EV3895 in single-window mode is supported. Picture-by-picture (PbyP) mode, which addresses two inputs, is not implemented. Verified on macOS (Apple silicon) and Arch-based Linux, switching in both directions.

## References

The report format (feature reports, property id `0x48` for the input source, and the EV3895 input codes) comes from these projects. No code was copied. This is a separate Rust implementation of the same protocol.

- [elfmimi/hid-monitor-control](https://github.com/elfmimi/hid-monitor-control): EIZO USB HID control tool (Ruby, Python, Node.js).
- [markbolhuis/hideizo](https://github.com/markbolhuis/hideizo): Linux HID driver for EIZO EV FlexScan monitors.
- [markbolhuis/libeizo](https://github.com/markbolhuis/libeizo): userspace library, including input port switching.

## License

MIT
