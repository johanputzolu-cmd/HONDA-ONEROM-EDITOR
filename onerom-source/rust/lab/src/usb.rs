// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! USB support

#![allow(static_mut_refs)]

use alloc::string::String;
use embassy_executor::Spawner;
use embassy_stm32::peripherals::{self, USB_OTG_FS};
use embassy_stm32::usb::{Config, DmPin, DpPin, Driver};
use embassy_stm32::{Peri, bind_interrupts, usb};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender, State};
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use log::{LevelFilter, Log, Metadata, Record};

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

// Static buffers for USB
static mut EP_OUT_BUFFER: [u8; 256] = [0; 256];
static mut CONFIG_DESCRIPTOR: [u8; 256] = [0; 256];
static mut BOS_DESCRIPTOR: [u8; 256] = [0; 256];
static mut CONTROL_BUF: [u8; 64] = [0; 64];
static mut USB_STATE: State = State::new();

// Channel for log messages
static LOG_CHANNEL: Channel<CriticalSectionRawMutex, String, 8> = Channel::new();

// Channel for key-presses from USB
static KEY_CHANNEL: Channel<CriticalSectionRawMutex, u8, 8> = Channel::new();

pub struct Usb {
    cdc_acm: CdcAcmClass<'static, Driver<'static, USB_OTG_FS>>,
    usb_device: UsbDevice<'static, Driver<'static, USB_OTG_FS>>,
}

impl Usb {
    pub fn new(
        peri: Peri<'static, USB_OTG_FS>,
        dp: Peri<'static, impl DpPin<USB_OTG_FS>>,
        dm: Peri<'static, impl DmPin<USB_OTG_FS>>,
    ) -> Self {
        let config = Config::default();

        // Create the USB driver
        let driver = Driver::new_fs(peri, Irqs, dp, dm, unsafe { &mut EP_OUT_BUFFER }, config);
        // Create embassy-usb Config
        let mut config = UsbConfig::new(0x1234, 0x5678);
        config.manufacturer = Some("piers.rocks");
        config.product = Some("One ROM Lab");
        config.serial_number = Some("n/a");

        // Required for Windows to bind CDC driver
        config.device_class = 0xEF;
        config.device_sub_class = 0x02;
        config.device_protocol = 0x01;
        config.composite_with_iads = true;

        // Create embassy-usb DeviceBuilder
        let mut builder = Builder::new(
            driver,
            config,
            unsafe { &mut CONFIG_DESCRIPTOR },
            unsafe { &mut BOS_DESCRIPTOR },
            &mut [],
            unsafe { &mut CONTROL_BUF },
        );

        // Create CDC ACM class
        let class = CdcAcmClass::new(&mut builder, unsafe { &mut USB_STATE }, 64);

        // Build
        let usb = builder.build();

        Self {
            cdc_acm: class,
            usb_device: usb,
        }
    }
}

pub fn run(spawner: Spawner, usb: Usb) {
    let (sender, receiver) = usb.cdc_acm.split();
    spawner.must_spawn(logger(sender));
    spawner.must_spawn(key_reader(receiver));
    spawner.must_spawn(usb_task(usb.usb_device));
}

#[embassy_executor::task]
async fn usb_task(mut usb_device: UsbDevice<'static, Driver<'static, USB_OTG_FS>>) -> ! {
    usb_device.run().await;
}

#[embassy_executor::task]
async fn logger(mut sender: Sender<'static, Driver<'static, USB_OTG_FS>>) -> ! {
    loop {
        sender.wait_connection().await;
        loop {
            let msg = recv_log().await;
            if sender.write_packet(msg.as_bytes()).await.is_err() {
                break;
            }
        }
    }
}

#[embassy_executor::task]
async fn key_reader(mut receiver: Receiver<'static, Driver<'static, USB_OTG_FS>>) -> ! {
    loop {
        receiver.wait_connection().await;
        let mut buf = [0u8; 64];
        loop {
            if let Ok(n) = receiver.read_packet(&mut buf).await {
                for &b in &buf[..n] {
                    let _ = KEY_CHANNEL.try_send(b);
                }
            } else {
                break;
            }
        }
    }
}

struct UsbLogger;

impl Log for UsbLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        use core::fmt::Write;
        let mut s: String = String::new();
        let _ = core::writeln!(s, "{} - {}", record.level(), record.args());

        if !LOG_CHANNEL.is_full() {
            let _ = LOG_CHANNEL.try_send(s);
        }
    }

    fn flush(&self) {}
}

static USB_LOGGER: UsbLogger = UsbLogger;

pub fn init_logger() {
    log::set_logger(&USB_LOGGER).unwrap();
    log::set_max_level(LevelFilter::Info);
}

pub async fn recv_log() -> String {
    LOG_CHANNEL.receive().await
}

pub async fn recv_key() -> u8 {
    KEY_CHANNEL.receive().await
}

const BOOTLOADER_MAGIC: u32 = 0x1234567F;
const MAGIC_ADDR: u32 = 0x2001_0000;

pub fn check_bootloader_flag() {
    let magic_ptr = MAGIC_ADDR as *mut u32;
    unsafe {
        if magic_ptr.read_volatile() == BOOTLOADER_MAGIC {
            magic_ptr.write_volatile(0);
            cortex_m::asm::bootload(0x1FFF_0000 as *const u32);
        }
    }
}

pub async fn enter_dfu_mode() -> ! {
    log::info!("Setting bootloader flag and resetting to enter DFU mode");
    embassy_time::Timer::after_secs(1).await;
    unsafe {
        (MAGIC_ADDR as *mut u32).write_volatile(BOOTLOADER_MAGIC);
    }
    cortex_m::peripheral::SCB::sys_reset();
}
