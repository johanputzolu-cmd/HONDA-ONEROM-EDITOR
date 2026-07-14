set -e

cargo objcopy --release -- -O binary firmware.bin
dfu-util -a 0 -s 0x08000000:leave -D firmware.bin
