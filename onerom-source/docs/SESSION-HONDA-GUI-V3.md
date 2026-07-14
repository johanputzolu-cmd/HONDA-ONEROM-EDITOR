# Session Save - Honda GUI V3

Date: 2026-07-11

## What Was Implemented

File modified:
- rust/honda-gui/src/main.rs

Datalog updates in live polling:
- Added numeric values: RPM, MAP (mbar), Battery (V), Injector duration (ms), Ignition advance (deg)
- Kept existing numeric values: ECT, IAT, TPS
- Added digital state booleans with LED intent:
  - VTS = packet[23] bit7
  - MIL = packet[23] bit5
  - FAN = packet[39] bit6

UI updates in LIVE SENSORS:
- Numeric lines now include: ECT, IAT, TPS, MAP, RPM, BATT, INJ, IGN
- Added DIGITAL STATES section:
  - VTS / MIL / FAN
  - Green = active, Red = inactive

## Build Outputs (V3)

Generated binaries:
- builds/gui-v3/onerom-honda-gui-linux-v3
- builds/gui-v3/onerom-honda-gui-windows-v3.exe

## Reopen / Relaunch Later

Open folder in VS Code:
- /home/vboxuser/Desktop/OSTRICH UART ONEROM V20.9 LED/one-rom-main (2)

Run Linux v3 manually:
- ./builds/gui-v3/onerom-honda-gui-linux-v3

If OpenGL/EGL fails in VM, use software rendering:
- LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe ./builds/gui-v3/onerom-honda-gui-linux-v3

Detached launch (keep running after terminal closes):
- nohup env LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe ./builds/gui-v3/onerom-honda-gui-linux-v3 > /tmp/onerom-honda-gui-v3.log 2>&1 &

Check if running:
- pgrep -af onerom-honda-gui-linux-v3

Stop process:
- pkill -f onerom-honda-gui-linux-v3

Read runtime log:
- tail -n 200 /tmp/onerom-honda-gui-v3.log

## Notes

- This workspace path is not a git repository, so no commit-based snapshot was possible.
- Session state was saved to this markdown file and helper script below.

---

## Session Update - V5 (2026-07-11)

File modified:
- rust/honda-gui/src/main.rs

### Confirmed Working

- ROM read now works from base address 0x8000.
- Ostrich read path in GUI aligned with plugin protocol:
  - Supports R and ZR commands
  - Command checksum verified
  - Response checksum verified
  - Uses selected Ostrich serial port (VV probe + type 'O')
- APPLY + AUTO BURN for MIL blink now works with persistent write base 0x8000.

### Functional Changes

- READ HONDA DATA:
  - First tries Ostrich serial read at 0x8000
  - Falls back to USB read_memory on failure
- READ ROM 32KB / SAVE:
  - First tries Ostrich serial read at 0x8000
  - Falls back to USB read_memory on failure
- Upload flow simplified:
  - Removed buttons from UI:
    - UPLOAD LIVE
    - BURN PERSISTENT (OSTRICH)
  - Replaced BURN LIVE+REBOOT with single button:
    - UPLOAD PERSISTENT
  - UPLOAD PERSISTENT now writes using Ostrich persistent path at base 0x8000
  - Requires Port to be set (no live temporary upload path)

### Build Outputs (V5)

Generated binaries at project root:
- onerom-honda-gui-linux-v5
- onerom-honda-gui-windows-v5.exe

### Quick Runtime Validation Sequence

1. Set correct Ostrich port in Honda Datalog panel.
2. Click READ HONDA DATA.
3. Set New MIL value and click APPLY + AUTO BURN.
4. Verify activity log shows post-write verification at address 0xE020.
5. Click READ HONDA DATA again to confirm persisted value.
