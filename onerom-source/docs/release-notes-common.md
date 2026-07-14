# One ROM - Release Notes

The most flexible ROM replacement for your retro computer.

## Changes In This Release

[CHANGELOG_PLACEHOLDER]

## Quick Start

Use [One ROM Studio](https://onerom.org/studio) to configure and build custom firmware images for your One ROM.

Alternatively, download pre-built firmware images from the releases below.

## File Types

- **onerom-{hw-rev}-{mcu}-{rom-config}.bin** - Single binary file for a specific hardware revision, MCU variant and ROM configuration, ready to flash
- **onerom-{hw-rev}-{mcu}.zip** - Zip of binaries for a specific hardware revision and MCU variant, ready to flash
- **manifest.json** - JSON manifest file listing all available firmware images, their hardware and ROM configurations, and SHA256 checksums, for use by automated tooling.

## Supported Configurations

Each zip firmware with a selection of ROM images for multiple system configurations, including:

- C64
- VIC-20 PAL  
- PET
- Commodore 1541 disk drive
- IEEE disk drives
- And more...

Firmware for ROM image configurations that require explicit licence acceptance (for example ['c64'](/old-config/c64.mk)) are not included in the release.  You can build them yourself, by following instructions in the [README](/README.md) and [INSTALL](/INSTALL.md).

There are more hardware, MCU and other build configurations options supported by One ROM than are included in the release artifacts.  You can build them yourself, by following instructions in the [README](/README.md) and [INSTALL](/INSTALL.md).

## Installation

1. Download the appropriate **.bin** for your hardware and desired ROM configuration
2. Flash the desired binary to your One ROM
3. Install One ROM into your retro computer

## Support

For documentation, hardware setup guides, and support:

- GitHub: [Project](https://piers.rocks/u/one)
- YouTube: [@piers.rocks](https://www.youtube.com/@piers_rocks)
