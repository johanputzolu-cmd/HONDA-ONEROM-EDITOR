// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! RCC (clock) configuration for One ROM Lab

use embassy_stm32::Config;
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Pll, PllMul, PllPDiv, PllPreDiv, PllSource, Sysclk,
};
#[cfg(feature = "usb")]
use embassy_stm32::rcc::{Hse, HseMode, PllQDiv};
#[cfg(feature = "usb")]
use embassy_stm32::time::Hertz;

// Configure max clock using HSI
#[cfg(not(feature = "usb"))]
pub fn configure_hsi(config: &mut Config) {
    config.rcc.hsi = true;
    config.rcc.pll_src = PllSource::HSI;
    config.rcc.sys = Sysclk::PLL1_P;

    #[cfg(feature = "f401re")]
    {
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV16,
            mul: PllMul::MUL336,
            divp: Some(PllPDiv::DIV4),
            divq: None,
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV1;
    }

    #[cfg(feature = "f411re")]
    {
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV16,
            mul: PllMul::MUL400,
            divp: Some(PllPDiv::DIV4),
            divq: None,
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV1;
    }

    #[cfg(feature = "f405rg")]
    {
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV16,
            mul: PllMul::MUL336,
            divp: Some(PllPDiv::DIV2),
            divq: None,
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }

    #[cfg(feature = "f446re")]
    {
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV16,
            mul: PllMul::MUL360,
            divp: Some(PllPDiv::DIV2),
            divq: None,
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
}

// Configure max clock using HSE and enable USB
#[cfg(feature = "usb")]
pub fn configure_hse_usb(config: &mut Config) {
    config.rcc.hse = Some(Hse {
        freq: Hertz(12_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.sys = Sysclk::PLL1_P;

    #[cfg(feature = "f401re")]
    {
        // 84MHz sysclk, 48MHz USB
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV6,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV4),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV1;
    }

    #[cfg(feature = "f411re")]
    {
        // 96MHz sysclk (can't hit 100MHz with 48MHz USB), 48MHz USB
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV6,
            mul: PllMul::MUL192,
            divp: Some(PllPDiv::DIV4),
            divq: Some(PllQDiv::DIV8),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV1;
    }

    #[cfg(feature = "f405rg")]
    {
        // 168MHz sysclk, 48MHz USB
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV6,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }

    #[cfg(feature = "f446re")]
    {
        // 168MHz sysclk (can't hit 180MHz with 48MHz USB), 48MHz USB
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV6,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
}
