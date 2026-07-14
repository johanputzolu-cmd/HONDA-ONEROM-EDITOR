Honda Datalog Simulator

README - Lancer Simulateur

Quick start (exact commands)
1) Go to the repo folder:
   cd "/home/vboxuser/Desktop/OSTRICH UART ONEROM V20.9 LED./one-rom-main (2)"
2) Start simulator:
   python3 scripts/honda_datalog_simulator.py
3) In app DATALOG, select the printed PTY port (example: /dev/pts/5), then CONNECT.

Stop simulator
1) In the same running terminal: press Ctrl+C (keyboard shortcut, not a shell command).
2) From another terminal:
   pgrep -af "python3 scripts/honda_datalog_simulator.py"
   kill <PID>
3) One-shot stop:
   pkill -f "python3 scripts/honda_datalog_simulator.py"

Common mistakes
- If "No such file or directory": you are not inside one-rom-main (2).
- Do not type "Ctrl+C" as text; press the keys.
- Replace <PID> with a real number (for example: kill 13844).

Purpose
- Standalone simulator to test live movement in Honda Studio without touching docs/main.rs.
- Emulates TPS, RPM, MAP, AFR, VTEC transitions over a 24-second cycle.

Windows
- The simulator does not auto-create a COM port on Windows.
- Default `pty` mode is Unix-only and creates `/dev/pts/*` endpoints on Linux.
- On Windows, first create a virtual COM pair with a tool such as `com0com`, or use a real USB/serial adapter.
- Then start the simulator on one side of that existing port:
   `python scripts/honda_datalog_simulator.py --mode serial --port COM5`
- In Honda Studio, select the matching COM port partner and connect.

File
- scripts/honda_datalog_simulator.py

How to run (easy mode)
1) Open terminal in repo root.
2) Run:
   python3 scripts/honda_datalog_simulator.py
3) The script prints a virtual serial port path (PTY mode), e.g. /dev/pts/7.
4) In Honda Studio, select that port for DATALOG if available, then CONNECT.

Alternative (real serial cable / USB adapter)
1) Run simulator on a real serial port:
   python3 scripts/honda_datalog_simulator.py --mode serial --port /dev/ttyUSB0
2) Select the same port in app and CONNECT.

Disable
- Stop simulator with Ctrl+C.

What is simulated
- Idle -> acceleration (10-20s style) -> high-rpm hold -> decel
- RPM: ~1000 to ~9000
- TPS: low to WOT
- MAP: vacuum to boost range
- AFR: cruise leaner to WOT richer
- VTEC bit: ON when RPM and TPS thresholds are crossed

Notes
- If PTY devices are not listed by your serial scanner, use serial mode with a real /dev/ttyUSB* device.
- On Windows, no COM port will appear unless you create one outside the simulator first.
- Protocol emulated: app sends 0x20, simulator returns 52-byte packet.
