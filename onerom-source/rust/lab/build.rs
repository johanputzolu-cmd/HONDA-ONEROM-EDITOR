// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn main() {
    // Re-run this build script if anything in git changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");

    // Set up STM32 linking
    println!("cargo:rustc-link-arg=-v");
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");

    // Re-run this build script of DEFMT_LOG changes.
    println!("cargo:rerun-if-env-changed=DEFMT_LOG");

    // Re-run if the ROMs change
    println!("cargo:rerun-if-changed=roms");

    // Police features
    let features = [
        cfg!(feature = "oneshot"),
        cfg!(feature = "control"),
        cfg!(feature = "repeat"),
        cfg!(feature = "qa"),
    ];
    let count = features.iter().filter(|&&f| f).count();
    if count != 1 {
        panic!("Exactly one of 'oneshot', 'control', 'repeat', or 'qa' features must be enabled");
    }

    // Set the cargo runner
    set_cargo_runner();

    // Generate memory.x if STM32F4, otherwise us stock RP2350 one
    #[cfg(feature = "stm32f4")]
    generate_stm32f4_memory_x();
    #[cfg(feature = "rp2350")]
    generate_rp2350_memory_x();

    // Generate built information
    built::write_built_file().expect("Failed to acquire build-time information");
}

fn set_cargo_runner() {
    const RUN_CMD_PREFIX: &str = "probe-rs run --no-location --chip ";

    let chip_id = if cfg!(feature = "f401re") {
        "STM32F401RETx"
    } else if cfg!(feature = "f405rg") {
        "STM32F405RGTx"
    } else if cfg!(feature = "f411re") {
        "STM32F411RETx"
    } else if cfg!(feature = "f446re") {
        "STM32F446RETx"
    } else {
        // No known hardware variant selected - do nothing
        eprintln!("One ROM Lab - No hardware variant selected - not setting cargo runner");
        return;
    };

    // Create the script to run the binary using probe-rs
    let runner_cmd = format!("{RUN_CMD_PREFIX}{chip_id}");
    let script = format!(
        r#"#!/bin/bash
echo "-----"
echo Running {runner_cmd} "$@"
echo "-----"
{runner_cmd} "$@"
"#
    );

    let out_dir = env::var("OUT_DIR").unwrap();
    let runner_path = format!("{out_dir}/runner.sh");

    fs::write(&runner_path, script).unwrap();
    fs::set_permissions(&runner_path, fs::Permissions::from_mode(0o755)).unwrap();
}

// Creates a custom memory.x file for this firmware.  We do this so we can
// place LAB_FLASH_INFO at a 0x200 offset from the start of flash, and
// LAB_RAM_INFO at the beginning of RAM.  This allows Airfrog to find it and
// decode the firmware and runtime information.
//
// This works by leveraging cortex_m_rt's link.x flexibility, to jiggle stuff
// around.  We leave the .vector_table in place (it has to be first in flash
// for the STM32), but push .data out from the start of RAM so we can have it.
#[cfg(feature = "stm32f4")]
fn generate_stm32f4_memory_x() {
    const STM32_FLASH_START: usize = 0x08000000;
    const STM32_RAM_START: usize = 0x20000000;
    const AIRFROG_FLASH_LOOKUP_OFFSET: usize = 0x200;

    const FLASH_INFO_SIZE: usize = 256;
    const RAM_INFO_SIZE: usize = 256;

    const FLASH_INFO_START: usize = STM32_FLASH_START + AIRFROG_FLASH_LOOKUP_OFFSET;
    const RAM_FLASH_INFO_START: usize = FLASH_INFO_START + FLASH_INFO_SIZE;
    const RAM_RAM_INFO_START: usize = STM32_RAM_START;
    const POST_FLASH_INFO: usize = RAM_FLASH_INFO_START + RAM_INFO_SIZE;
    const NEW_RAM_START: usize = STM32_RAM_START + RAM_INFO_SIZE;

    const FLASH_INFO_SECTION: &str = ".lab_flash_info";
    const RAM_INFO_SECTION: &str = ".lab_ram_info";

    let out_dir = env::var("OUT_DIR").unwrap();
    let memory_path = Path::new(&out_dir).join("memory.x");

    let memory_x = format!(
        r#"
/* Standard STM32F405RG memory layout */
MEMORY
{{
    FLASH   : ORIGIN = {STM32_FLASH_START:#010X}, LENGTH = 1024K
    PRIVATE : ORIGIN = {STM32_RAM_START:#010X}, LENGTH = {RAM_INFO_SIZE:#05X}
    RAM     : ORIGIN = {NEW_RAM_START:#010X}, LENGTH = 128K - {RAM_INFO_SIZE:#05X}
}}

/* Section to store firmware information to flash */ 
SECTIONS
{{
    {FLASH_INFO_SECTION} {FLASH_INFO_START:#010X} : AT({FLASH_INFO_START:#010X}) {{
        *({FLASH_INFO_SECTION}*)
    }} > FLASH
}}
INSERT AFTER .vector_table

/* Force .text to start after {FLASH_INFO_SECTION} */
PROVIDE(_stext = {POST_FLASH_INFO:#010X});

/* Section to store runtime information in RAM */
/* Needs to be physically located in flash (hence AT) and copied by info.rs at startup */
SECTIONS
{{
    {RAM_INFO_SECTION} {RAM_RAM_INFO_START:#010X} : AT({RAM_FLASH_INFO_START:#010X}) {{
        __lab_ram_info_start = .;
        *({RAM_INFO_SECTION}*)
        __lab_ram_info_end = .;
    }} > PRIVATE
     __lab_ram_info_load = LOADADDR(.lab_ram_info);
     __lab_ram_info_size = __lab_ram_info_end - __lab_ram_info_start;
}}
INSERT AFTER .rodata;

_SEGGER_RTT_ADDRESS = ABSOLUTE(_SEGGER_RTT);
"#
    );

    fs::write(memory_path, memory_x).unwrap();

    println!("cargo:rustc-link-search={out_dir}");
}

#[cfg(feature = "rp2350")]
fn generate_rp2350_memory_x() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let memory_path = Path::new(&out_dir).join("memory.x");

    let memory_x = r#"
MEMORY {
    /*
     * The RP2350 has either external or internal flash.
     *
     * 2 MiB is a safe default here, although a Pico 2 has 4 MiB.
     */
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    /*
     * RAM consists of 8 banks, SRAM0-SRAM7, with a striped mapping.
     * This is usually good for performance, as it distributes load on
     * those banks evenly.
     */
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
    /*
     * RAM banks 8 and 9 use a direct mapping. They can be used to have
     * memory areas dedicated for some specific job, improving predictability
     * of access times.
     * Example: Separate stacks for core0 and core1.
     */
    SRAM8 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM9 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    /* ### Boot ROM info
     *
     * Goes after .vector_table, to keep it in the first 4K of flash
     * where the Boot ROM (and picotool) can find it
     */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH

} INSERT AFTER .vector_table;

/* move .text to start /after/ the boot info */
_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    /* ### Picotool 'Binary Info' Entries
     *
     * Picotool looks through this block (as we have pointers to it in our
     * header) to find interesting information.
     */
    .bi_entries : ALIGN(4)
    {
        /* We put this in the header */
        __bi_entries_start = .;
        /* Here are the entries */
        KEEP(*(.bi_entries));
        /* Keep this block a nice round size */
        . = ALIGN(4);
        /* We put this in the header */
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    /* ### Boot ROM extra info
     *
     * Goes after everything in our program, so it can contain a signature.
     */
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH

} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
    "#;

    fs::write(memory_path, memory_x).unwrap();
    println!("cargo:rustc-link-search={out_dir}");
}
