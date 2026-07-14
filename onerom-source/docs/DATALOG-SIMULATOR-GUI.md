Honda Datalog Simulator GUI (Linux)

Purpose
- Visual datalog simulator with live sliders for RPM, TPS, MAP, AFR, IAT, ECT.
- Compatible with SupraRom Studio DATALOG protocol.

Protocol
- Handshake: 0x10 -> 0xCD
- Poll: 0x20 -> 52-byte packet

Run
1) Open terminal in repo:
   cd "/home/vboxuser/Desktop/OSTRICH UART ONEROM V20.9 LED./one-rom-main (2)"
2) Start GUI simulator:
   python3 scripts/honda_datalog_simulator_gui.py
3) Click Start in the app.
4) Copy/select the shown PTY port in SupraRom Studio DATALOG (example: /dev/pts/7).

Double-click launcher (Linux)
1) Install launcher icons/entries:
   ./scripts/install_honda_simulator_launcher.sh
2) Double-click desktop icon:
   ~/Desktop/Honda-Datalog-Simulator.desktop

Launcher files
- scripts/honda_datalog_simulator_gui_launcher.sh
- scripts/install_honda_simulator_launcher.sh

Stop
- In GUI: click Stop
- Or close the window

Serial mode (real serial device)
- python3 scripts/honda_datalog_simulator_gui.py --mode serial --port /dev/ttyUSB0

Controls
- Sliders: RPM, TPS, MAP, AFR, IAT, ECT, Battery
- Option: VTEC force ON
- Option: Auto variation + variation level
- AUTO profile:
   - Enable AUTO profile
   - RPM ramp: 400 -> 9000
   - Acceleration duration: 5 to 20 seconds (adjustable)
   - AFR start/end sliders (default 11 -> 16)
   - MAP start/end sliders
   - Manual slider influence stays active during AUTO phase

Notes
- If tkinter is missing, install Python Tk package for your distro.
- If using serial mode, pyserial is required.
