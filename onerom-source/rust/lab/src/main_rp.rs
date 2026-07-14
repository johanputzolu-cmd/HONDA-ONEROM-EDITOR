//! One ROM Lab firmware - RP2350

// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use embassy_executor::Spawner;
use embassy_executor::main as embassy_main;
use embassy_rp::gpio::{Flex, Level, Output};
#[cfg(feature = "repeat")]
use embassy_time::Timer;

use embedded_alloc::LlffHeap as Heap;
use panic_rtt_target as _;

mod error;
mod info;
mod logs;
mod rom;
mod types;

pub use error::Error;
pub use rom::{Id as RomId, Rom};

use info::{LAB_RAM_INFO, PKG_VERSION};

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[embassy_main]
async fn main(_spawner: Spawner) {
    // Initialize the heap allocator
    {
        use core::mem::MaybeUninit;
        const HEAP_SIZE: usize = 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(&raw mut HEAP_MEM as usize, HEAP_SIZE) }
    }

    // Initialize peripherals with default config
    let p = embassy_rp::init(Default::default());

    // Set up the LED
    let mut led = Output::new(p.PIN_29, Level::Low);

    // Flash LED to show we're alive
    for _ in 0..3 {
        led.set_high();
        embassy_time::Timer::after_millis(200).await;
        led.set_low();
        embassy_time::Timer::after_millis(200).await;
    }

    // Init logging
    logs::init_rtt();

    info!("-----");
    info!("One ROM Lab v{}", PKG_VERSION);
    info!("Copyright (c) 2026 Piers Finlayson");

    info!("-----");
    debug!("RP2350 target");

    debug!(
        "One ROM Lab Flash Info address: {:#010X}",
        &info::LAB_FLASH_INFO as *const _ as usize
    );
    #[allow(static_mut_refs)]
    unsafe {
        debug!(
            "One ROM Lab RAM Info address:   {:#010X}",
            &LAB_RAM_INFO as *const _ as usize
        );
    }

    // fire-28-a
    let addr_pins = [
        Flex::new(p.PIN_25),
        Flex::new(p.PIN_24),
        Flex::new(p.PIN_23),
        Flex::new(p.PIN_22),
        Flex::new(p.PIN_21),
        Flex::new(p.PIN_19),
        Flex::new(p.PIN_20),
        Flex::new(p.PIN_18),
        Flex::new(p.PIN_14),
        Flex::new(p.PIN_12),
        Flex::new(p.PIN_11),
        Flex::new(p.PIN_13),
        Flex::new(p.PIN_17),
        Flex::new(p.PIN_15),
        Flex::new(p.PIN_10),
        Flex::new(p.PIN_16),
        Flex::new(p.PIN_8),
        Flex::new(p.PIN_9),
    ];
    let data_pins = [
        Flex::new(p.PIN_7),
        Flex::new(p.PIN_6),
        Flex::new(p.PIN_5),
        Flex::new(p.PIN_0),
        Flex::new(p.PIN_1),
        Flex::new(p.PIN_2),
        Flex::new(p.PIN_3),
        Flex::new(p.PIN_4),
    ];

    // Create the ROM object
    let mut rom = Rom::new(addr_pins, data_pins);
    unsafe {
        LAB_RAM_INFO.rom_data = rom.buf.as_ptr() as *const core::ffi::c_void;
    }
    rom.init();

    #[cfg(feature = "control")]
    {
        let mut control = control::Control::new(rom);
        control.run().await;
    }

    #[cfg(feature = "qa")]
    {
        info!("QA mode");
        loop {
            match rom.read_rom().await {
                Some(_) => info!("ROM read successfully"),
                None => info!("Failed to read ROM"),
            }
            //embassy_time::Timer::after_secs(5).await;
        }
    }

    #[cfg(not(any(feature = "control", feature = "qa")))]
    {
        loop {
            match rom.read_rom().await {
                Some(_) => break,
                None => info!("Failed to read ROM"),
            }

            #[cfg(feature = "oneshot")]
            {
                info!("Done");
                return;
            }
            #[cfg(feature = "repeat")]
            {
                info!("Waiting 5 seconds before reading again");
                Timer::after_secs(5).await;
            }
        }
    }
}

#[cfg(all(feature = "control", feature = "oneshot"))]
compile_error!("Features 'control' and 'oneshot' are mutually exclusive");
