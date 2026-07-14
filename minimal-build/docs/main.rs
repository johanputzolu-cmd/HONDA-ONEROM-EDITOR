use std::collections::VecDeque;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{Read, Write as IoWrite};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use eframe::egui;
use onerom_cli::{
    LIVE_ROM_BASE,
    usb::{enumerate_devices, read_memory, write_memory},
};
use tokio::runtime::Runtime;

const ROM_READ_SIZE: u32 = 32 * 1024;
const ROM_READ_BASE: u32 = 0x8000;
const OSTRICH_ADDR_BASE: u16 = 0x8000;
const BLINK_OFFSET: u16 = 0x6020;
const IACV_OFFSET: u16 = 0x6116;
const SHIFT_LIGHT_ENABLE_OFFSET: u16 = 0x6142;
const SHIFT_LIGHT_RPM_LO_OFFSET: u16 = 0x6144;
const SHIFT_LIGHT_RPM_HI_OFFSET: u16 = 0x6145;
const FAN_CONTROL_ENABLE_OFFSET: u16 = 0x616D;
const FAN_CONTROL_TEMP_OFFSET: u16 = 0x616E;
const CUT_A_OFFSET: u16 = 0x6125;
const CUT_B_OFFSET: u16 = 0x6126;
const IGN_TIMING_61D8_OFFSET: u16 = 0x61D8;
const IGN_TIMING_61E4_OFFSET: u16 = 0x61E4;
const IGN_TIMING_61E5_OFFSET: u16 = 0x61E5;
const LAUNCH_OFFSET: u16 = 0x6152;
const LAUNCH_TPS_ENABLE_OFFSET: u16 = 0x6155;
const LAUNCH_TPS_VALUE_OFFSET: u16 = 0x6158;
const VTEC_SW_6120_OFFSET: u16 = 0x6120;
const VTEC_SW_61F2_OFFSET: u16 = 0x61F2;
const VTEC_SW_6657_OFFSET: u16 = 0x6657;
const VTEC_VAL_6658_OFFSET: u16 = 0x6658;
const VTEC_SW_6659_OFFSET: u16 = 0x6659;
const VTEC_VAL_665A_OFFSET: u16 = 0x665A;
const TPS_GRAPH_WINDOW_SECS: f64 = 20.0;
const RPM_GRAPH_MAX: f64 = 9000.0;
const AFR_GRAPH_MIN: f64 = 10.0;
const AFR_GRAPH_MAX: f64 = 20.0;
const INJECTOR_SIZE_CC: f64 = 240.0;
const HONDA_MBAR_MIN: f64 = -70.0;
const HONDA_MBAR_MAX: f64 = 1790.0;
const HONDA_PSI_MAX: f64 = 10.6;
const AFR_TABLE_SIZE: usize = 16;
const AFR_TABLE_MAP_MIN: f64 = HONDA_MBAR_MIN;
const AFR_TABLE_MAP_MAX: f64 = HONDA_MBAR_MAX;
const AFR_TABLE_RPM_MIN: f64 = 400.0;
const AFR_TABLE_RPM_MAX: f64 = 9000.0;
const ROM_TRACK_MAP_MIN: f64 = HONDA_MBAR_MIN;
const ROM_TRACK_MAP_MAX: f64 = HONDA_MBAR_MAX;
const ROM_TRACK_RPM_MIN: f64 = 400.0;
const ROM_TRACK_RPM_MAX: f64 = 9000.0;
const ROM_FUEL_TABLE_ROWS: usize = 20;
const ROM_FUEL_TABLE_COLS: usize = 10;
const ROM_FUEL_TABLE_ROW_STRIDE: usize = 0x18;
const LOW_CAM_FUEL_TABLE_OFFSET: usize = 0x6EAD;
const HIGH_CAM_FUEL_TABLE_OFFSET: usize = 0x70A5;
const LOW_CAM_IGN_TABLE_OFFSET: usize = 0x729D;
const HIGH_CAM_IGN_TABLE_OFFSET: usize = 0x747D;
const EMBEDDED_BIN_P28: &[u8] = include_bytes!("base_bins/p28.bin");
const EMBEDDED_BIN_P30: &[u8] = include_bytes!("base_bins/p30.bin");
const EMBEDDED_BIN_P73_B18CR: &[u8] = include_bytes!("base_bins/p73 b18cr.bin");

#[derive(Clone, Copy, PartialEq, Eq)]
enum GraphSensor {
    Tps,
    Rpm,
    Afr,
    Map,
    Batt,
    InjMs,
    Ign,
    Lambda,
    Boost,
    Vss,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableMetric {
    Afr,
    FuelValue,
    InjDuty,
    Ign,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RomFuelTableKind {
    LowCam,
    HighCam,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RomTableViewMode {
    TwoD,
    ThreeD,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RomEmbeddedView {
    None,
    Fuel,
    Ign,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LiveTrackingAfrMapMode {
    LiveAfr,
    TargetAfr,
    DiffPct,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LiveTrackingCellMode {
    Single,
    Quad,
}

impl LiveTrackingCellMode {
    fn label(self) -> &'static str {
        match self {
            LiveTrackingCellMode::Single => "1 CELL",
            LiveTrackingCellMode::Quad => "4 CELLS",
        }
    }
}

impl LiveTrackingAfrMapMode {
    fn label(self) -> &'static str {
        match self {
            LiveTrackingAfrMapMode::LiveAfr => "LIVE AFR MAP",
            LiveTrackingAfrMapMode::TargetAfr => "AFR TARGET MAP",
            LiveTrackingAfrMapMode::DiffPct => "AFR DIFF % MAP",
        }
    }
}

impl RomFuelTableKind {
    fn label(self) -> &'static str {
        match self {
            RomFuelTableKind::LowCam => "LOW CAM",
            RomFuelTableKind::HighCam => "HIGH CAM VTEC",
        }
    }

    fn offset(self) -> usize {
        match self {
            RomFuelTableKind::LowCam => LOW_CAM_FUEL_TABLE_OFFSET,
            RomFuelTableKind::HighCam => HIGH_CAM_FUEL_TABLE_OFFSET,
        }
    }

    fn ign_offset(self) -> usize {
        match self {
            RomFuelTableKind::LowCam => LOW_CAM_IGN_TABLE_OFFSET,
            RomFuelTableKind::HighCam => HIGH_CAM_IGN_TABLE_OFFSET,
        }
    }
}

impl RomTableViewMode {
    fn label(self) -> &'static str {
        match self {
            RomTableViewMode::TwoD => "2D",
            RomTableViewMode::ThreeD => "3D",
        }
    }
}

impl TableMetric {
    const ALL: [TableMetric; 4] = [
        TableMetric::Afr,
        TableMetric::FuelValue,
        TableMetric::InjDuty,
        TableMetric::Ign,
    ];

    fn label(self) -> &'static str {
        match self {
            TableMetric::Afr => "AFR",
            TableMetric::FuelValue => "FUEL VALUE",
            TableMetric::InjDuty => "INJ DUTY",
            TableMetric::Ign => "IGN",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            TableMetric::Afr => "",
            TableMetric::FuelValue => "fv",
            TableMetric::InjDuty => "%",
            TableMetric::Ign => "deg",
        }
    }

    fn range(self) -> (f64, f64) {
        match self {
            TableMetric::Afr => (AFR_GRAPH_MIN, AFR_GRAPH_MAX),
            TableMetric::FuelValue => (0.0, 600.0),
            TableMetric::InjDuty => (0.0, 120.0),
            TableMetric::Ign => (-10.0, 60.0),
        }
    }
}

impl GraphSensor {
    const ALL: [GraphSensor; 10] = [
        GraphSensor::Tps,
        GraphSensor::Rpm,
        GraphSensor::Afr,
        GraphSensor::Map,
        GraphSensor::Batt,
        GraphSensor::InjMs,
        GraphSensor::Ign,
        GraphSensor::Lambda,
        GraphSensor::Boost,
        GraphSensor::Vss,
    ];

    fn label(self) -> &'static str {
        match self {
            GraphSensor::Tps => "TPS",
            GraphSensor::Rpm => "RPM",
            GraphSensor::Afr => "AFR",
            GraphSensor::Map => "MAP",
            GraphSensor::Batt => "BATT",
            GraphSensor::InjMs => "INJ",
            GraphSensor::Ign => "IGN",
            GraphSensor::Lambda => "LAMBDA",
            GraphSensor::Boost => "BOOST",
            GraphSensor::Vss => "VSS",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            GraphSensor::Tps => "%",
            GraphSensor::Rpm => "rpm",
            GraphSensor::Afr => "",
            GraphSensor::Map => "mbar",
            GraphSensor::Batt => "V",
            GraphSensor::InjMs => "ms",
            GraphSensor::Ign => "deg",
            GraphSensor::Lambda => "",
            GraphSensor::Boost => "psi",
            GraphSensor::Vss => "km/h",
        }
    }

    fn range(self) -> (f64, f64) {
        match self {
            GraphSensor::Tps => (0.0, 100.0),
            GraphSensor::Rpm => (0.0, RPM_GRAPH_MAX),
            GraphSensor::Afr => (AFR_GRAPH_MIN, AFR_GRAPH_MAX),
            GraphSensor::Map => (HONDA_MBAR_MIN, HONDA_MBAR_MAX),
            GraphSensor::Batt => (0.0, 18.0),
            GraphSensor::InjMs => (0.0, 20.0),
            GraphSensor::Ign => (-10.0, 60.0),
            GraphSensor::Lambda => (0.5, 1.6),
            GraphSensor::Boost => (0.0, HONDA_PSI_MAX),
            GraphSensor::Vss => (0.0, 260.0),
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            GraphSensor::Tps => egui::Color32::from_rgb(40, 200, 120),
            GraphSensor::Rpm => egui::Color32::from_rgb(220, 45, 45),
            GraphSensor::Afr => egui::Color32::from_rgb(70, 140, 255),
            GraphSensor::Map => egui::Color32::from_rgb(255, 175, 60),
            GraphSensor::Batt => egui::Color32::from_rgb(200, 230, 70),
            GraphSensor::InjMs => egui::Color32::from_rgb(220, 90, 190),
            GraphSensor::Ign => egui::Color32::from_rgb(255, 120, 120),
            GraphSensor::Lambda => egui::Color32::from_rgb(160, 220, 255),
            GraphSensor::Boost => egui::Color32::from_rgb(255, 110, 55),
            GraphSensor::Vss => egui::Color32::from_rgb(130, 255, 170),
        }
    }

    fn value_from_sample(self, sample: &GraphSample) -> f64 {
        match self {
            GraphSensor::Tps => sample.tps,
            GraphSensor::Rpm => sample.rpm,
            GraphSensor::Afr => sample.afr,
            GraphSensor::Map => sample.map,
            GraphSensor::Batt => sample.battery,
            GraphSensor::InjMs => sample.inj_ms,
            GraphSensor::Ign => sample.ign,
            GraphSensor::Lambda => sample.lambda,
            GraphSensor::Boost => sample.boost,
            GraphSensor::Vss => sample.vss,
        }
    }
}

#[derive(Clone, Copy)]
struct GraphSample {
    t: f64,
    tps: f64,
    rpm: f64,
    afr: f64,
    map: f64,
    battery: f64,
    inj_ms: f64,
    ign: f64,
    lambda: f64,
    boost: f64,
    vss: f64,
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_title("One ROM Honda Edition"),
        ..Default::default()
    };

    eframe::run_native(
        "One ROM Honda Edition",
        options,
        Box::new(|_cc| Ok(Box::new(HondaGuiApp::default()))),
    )
}

struct HondaGuiApp {
    log: Vec<String>,
    device_status: String,
    selected_file: String,
    selected_serial: Option<String>,
    rom_com_port: String,
    rom_available_ports: Vec<String>,
    datalog_com_port: String,
    datalog_available_ports: Vec<String>,
    rpm: f64,
    ect: f64,
    iat: f64,
    tps: f64,
    map: f64,
    battery: f64,
    afr: f64,
    afr_offset: f64,
    inj_ms: f64,
    injector_duty: f64,
    ign_advance: f64,
    lambda: f64,
    map_volt: f64,
    tps_volt: f64,
    boost_psi: f64,
    vss_kmh: f64,
    gear: u8,
    iacv_duty: f64,
    ebc_duty: f64,
    ebc_base_duty: f64,
    instant_consumption: f64,
    gear_ic: f64,
    ect_fc: f64,
    o2_short_fc: f64,
    o2_long_fc: f64,
    iat_fc: f64,
    ve_fc: f64,
    iat_ic: f64,
    ect_ic: f64,
    eld_volt: f64,
    inj_fv: f64,
    vts_active: bool,
    vtp_active: bool,
    vts_feedback_active: bool,
    park_n_active: bool,
    bksw_active: bool,
    acc_active: bool,
    start_active: bool,
    scc_active: bool,
    psp_active: bool,
    mil_active: bool,
    fan_active: bool,
    output_2nd_map_active: bool,
    flr_active: bool,
    output_fts_active: bool,
    fuelcut1_active: bool,
    fuelcut2_active: bool,
    igncut_active: bool,
    scc_checker_active: bool,
    vtsm_active: bool,
    post_fuel_active: bool,
    at_shift1_active: bool,
    at_shift2_active: bool,
    leanprotect_active: bool,
    boostcut_active: bool,
    bst_active: bool,
    antilag_active: bool,
    ebc_active: bool,
    ac_active: bool,
    atlctrl_active: bool,
    o2heater_active: bool,
    iab_active: bool,
    purge_active: bool,
    fuelpump_active: bool,
    input_ftl_active: bool,
    input_fts_active: bool,
    input_ebc_active: bool,
    input_ebc_hi_active: bool,
    input_bst_active: bool,
    input_gpo1_active: bool,
    input_gpo2_active: bool,
    input_gpo3_active: bool,
    gpo1_active: bool,
    gpo2_active: bool,
    gpo3_active: bool,
    bst_stage2_active: bool,
    bst_stage3_active: bool,
    bst_stage4_active: bool,
    mil_blinks: u8,
    new_mil_blinks: String,
    iacv_value: u8,
    shift_light_enable_value: u8,
    shift_light_rpm_raw_value: u16,
    new_shift_light_rpm_value: String,
    fan_control_enable_value: u8,
    fan_control_temp_value: u8,
    new_fan_control_temp_value: String,
    new_iacv_value: String,
    cut_a_value: u8,
    cut_b_value: u8,
    ign_timing_61d8: u8,
    ign_timing_61e4: u8,
    ign_timing_61e5: u8,
    launch_value: u8,
    launch_tps_enable_value: u8,
    launch_tps_value: u8,
    vtec_sw_6120: u8,
    vtec_sw_61f2: u8,
    vtec_sw_6657: u8,
    vtec_val_6658: u8,
    vtec_sw_6659: u8,
    vtec_val_665a: u8,
    new_vtec_val_6658: String,
    new_vtec_val_665a: String,
    last_read_rom: Option<Vec<u8>>,
    rom_has_unsaved_changes: bool,
    show_hex_window: bool,
    rom_hex_dump: String,
    datalog_connected: bool,
    last_poll: Instant,
    graph_history: VecDeque<GraphSample>,
    graph_trace_a: GraphSensor,
    graph_trace_b: GraphSensor,
    graph_trace_c: GraphSensor,
    show_datalog_graph: bool,
    show_datalog_table: bool,
    show_datalog_table_3d: bool,
    tps_graph_start: Instant,
    show_table_window: bool,
    show_table_3d_window: bool,
    table_3d_pan: egui::Vec2,
    table_3d_scale: f32,
    table_3d_yaw: f32,
    table_3d_pitch: f32,
    show_rom_table_window: bool,
    show_rom_ign_table_window: bool,
    show_live_tracking_window: bool,
    show_live_tracking_afr_window: bool,
    show_new_bin_window: bool,
    show_about_window: bool,
    rom_embedded_view: RomEmbeddedView,
    live_tracking_map: RomEmbeddedView,
    rom_table_kind: RomFuelTableKind,
    rom_ign_table_kind: RomFuelTableKind,
    live_tracking_kind: RomFuelTableKind,
    rom_table_view_mode: RomTableViewMode,
    rom_ign_table_view_mode: RomTableViewMode,
    live_tracking_view_mode: RomTableViewMode,
    live_tracking_afr_map: RomEmbeddedView,
    live_tracking_afr_kind: RomFuelTableKind,
    live_tracking_afr_view_mode: RomTableViewMode,
    live_tracking_afr_map_mode: LiveTrackingAfrMapMode,
    rom_table_3d_pan: egui::Vec2,
    rom_table_3d_scale: f32,
    rom_table_3d_yaw: f32,
    rom_table_3d_pitch: f32,
    rom_ign_table_3d_pan: egui::Vec2,
    rom_ign_table_3d_scale: f32,
    rom_ign_table_3d_yaw: f32,
    rom_ign_table_3d_pitch: f32,
    live_tracking_3d_pan: egui::Vec2,
    live_tracking_3d_scale: f32,
    live_tracking_3d_yaw: f32,
    live_tracking_3d_pitch: f32,
    rom_table_high_3d_pan: egui::Vec2,
    rom_table_high_3d_scale: f32,
    rom_table_high_3d_yaw: f32,
    rom_table_high_3d_pitch: f32,
    rom_ign_table_high_3d_pan: egui::Vec2,
    rom_ign_table_high_3d_scale: f32,
    rom_ign_table_high_3d_yaw: f32,
    rom_ign_table_high_3d_pitch: f32,
    live_tracking_high_3d_pan: egui::Vec2,
    live_tracking_high_3d_scale: f32,
    live_tracking_high_3d_yaw: f32,
    live_tracking_high_3d_pitch: f32,
    live_tracking_afr_3d_pan: egui::Vec2,
    live_tracking_afr_3d_scale: f32,
    live_tracking_afr_3d_yaw: f32,
    live_tracking_afr_3d_pitch: f32,
    rom_table_zone_row_start: usize,
    rom_table_zone_row_end: usize,
    rom_table_zone_col_start: usize,
    rom_table_zone_col_end: usize,
    rom_table_drag_anchor: Option<(usize, usize)>,
    rom_ign_table_zone_row_start: usize,
    rom_ign_table_zone_row_end: usize,
    rom_ign_table_zone_col_start: usize,
    rom_ign_table_zone_col_end: usize,
    rom_ign_table_drag_anchor: Option<(usize, usize)>,
    live_tracking_zone_row_start: usize,
    live_tracking_zone_row_end: usize,
    live_tracking_zone_col_start: usize,
    live_tracking_zone_col_end: usize,
    live_tracking_drag_anchor: Option<(usize, usize)>,
    live_tracking_afr_zone_row_start: usize,
    live_tracking_afr_zone_row_end: usize,
    live_tracking_afr_zone_col_start: usize,
    live_tracking_afr_zone_col_end: usize,
    live_tracking_afr_drag_anchor: Option<(usize, usize)>,
    live_tracking_capture_enabled: bool,
    follow_vtec_tables: bool,
    live_tracking_cell_mode: LiveTrackingCellMode,
    live_tracking_sample_count: u64,
    live_tracking_afr_value_low: Vec<Vec<Option<f64>>>,
    live_tracking_afr_sum_low: Vec<Vec<f64>>,
    live_tracking_afr_count_low: Vec<Vec<u32>>,
    live_tracking_afr_pending_count_low: Vec<Vec<u32>>,
    live_tracking_afr_value_high: Vec<Vec<Option<f64>>>,
    live_tracking_afr_sum_high: Vec<Vec<f64>>,
    live_tracking_afr_count_high: Vec<Vec<u32>>,
    live_tracking_afr_pending_count_high: Vec<Vec<u32>>,
    live_tracking_afr_target: Vec<Vec<f64>>,
    live_tracking_afr_target_input: String,
    sensor_table_kind: RomFuelTableKind,
    table_metric: TableMetric,
    rom_table_column_multipliers: [f64; ROM_FUEL_TABLE_COLS],
    table_afr_low: Vec<Vec<Option<f64>>>,
    table_fuel_value_low: Vec<Vec<Option<f64>>>,
    table_inj_duty_low: Vec<Vec<Option<f64>>>,
    table_ign_low: Vec<Vec<Option<f64>>>,
    table_afr_high: Vec<Vec<Option<f64>>>,
    table_fuel_value_high: Vec<Vec<Option<f64>>>,
    table_inj_duty_high: Vec<Vec<Option<f64>>>,
    table_ign_high: Vec<Vec<Option<f64>>>,
}

impl Default for HondaGuiApp {
    fn default() -> Self {
        Self {
            log: Vec::new(),
            device_status: "Not connected".to_string(),
            selected_file: "No file selected".to_string(),
            selected_serial: None,
            rom_com_port: String::new(),
            rom_available_ports: Vec::new(),
            datalog_com_port: String::new(),
            datalog_available_ports: Vec::new(),
            rpm: 0.0,
            ect: 0.0,
            iat: 0.0,
            tps: 0.0,
            map: 0.0,
            battery: 0.0,
            afr: 0.0,
            afr_offset: 0.0,
            inj_ms: 0.0,
            injector_duty: 0.0,
            ign_advance: 0.0,
            lambda: 0.0,
            map_volt: 0.0,
            tps_volt: 0.0,
            boost_psi: 0.0,
            vss_kmh: 0.0,
            gear: 0,
            iacv_duty: 0.0,
            ebc_duty: 0.0,
            ebc_base_duty: 0.0,
            instant_consumption: 0.0,
            gear_ic: 0.0,
            ect_fc: 0.0,
            o2_short_fc: 0.0,
            o2_long_fc: 0.0,
            iat_fc: 0.0,
            ve_fc: 0.0,
            iat_ic: 0.0,
            ect_ic: 0.0,
            eld_volt: 0.0,
            inj_fv: 0.0,
            vts_active: false,
            vtp_active: false,
            vts_feedback_active: false,
            park_n_active: false,
            bksw_active: false,
            acc_active: false,
            start_active: false,
            scc_active: false,
            psp_active: false,
            mil_active: false,
            fan_active: false,
            output_2nd_map_active: false,
            flr_active: false,
            output_fts_active: false,
            fuelcut1_active: false,
            fuelcut2_active: false,
            igncut_active: false,
            scc_checker_active: false,
            vtsm_active: false,
            post_fuel_active: false,
            at_shift1_active: false,
            at_shift2_active: false,
            leanprotect_active: false,
            boostcut_active: false,
            bst_active: false,
            antilag_active: false,
            ebc_active: false,
            ac_active: false,
            atlctrl_active: false,
            o2heater_active: false,
            iab_active: false,
            purge_active: false,
            fuelpump_active: false,
            input_ftl_active: false,
            input_fts_active: false,
            input_ebc_active: false,
            input_ebc_hi_active: false,
            input_bst_active: false,
            input_gpo1_active: false,
            input_gpo2_active: false,
            input_gpo3_active: false,
            gpo1_active: false,
            gpo2_active: false,
            gpo3_active: false,
            bst_stage2_active: false,
            bst_stage3_active: false,
            bst_stage4_active: false,
            mil_blinks: 0,
            new_mil_blinks: String::new(),
            iacv_value: 0,
            shift_light_enable_value: 0,
            shift_light_rpm_raw_value: 0,
            new_shift_light_rpm_value: "6500".to_string(),
            fan_control_enable_value: 0,
            fan_control_temp_value: 0,
            new_fan_control_temp_value: "0.0".to_string(),
            new_iacv_value: "00".to_string(),
            cut_a_value: 0,
            cut_b_value: 0,
            ign_timing_61d8: 0,
            ign_timing_61e4: 0,
            ign_timing_61e5: 0,
            launch_value: 0,
            launch_tps_enable_value: 0,
            launch_tps_value: 0,
            vtec_sw_6120: 0,
            vtec_sw_61f2: 0,
            vtec_sw_6657: 0,
            vtec_val_6658: 0,
            vtec_sw_6659: 0,
            vtec_val_665a: 0,
            new_vtec_val_6658: "00".to_string(),
            new_vtec_val_665a: "00".to_string(),
            last_read_rom: None,
            rom_has_unsaved_changes: false,
            show_hex_window: false,
            rom_hex_dump: String::new(),
            datalog_connected: false,
            last_poll: Instant::now(),
            graph_history: VecDeque::new(),
            graph_trace_a: GraphSensor::Tps,
            graph_trace_b: GraphSensor::Rpm,
            graph_trace_c: GraphSensor::Afr,
            show_datalog_graph: false,
            show_datalog_table: false,
            show_datalog_table_3d: false,
            tps_graph_start: Instant::now(),
            show_table_window: false,
            show_table_3d_window: false,
            table_3d_pan: egui::vec2(0.0, 0.0),
            table_3d_scale: 1.0,
            table_3d_yaw: -0.75,
            table_3d_pitch: 0.70,
            show_rom_table_window: false,
            show_rom_ign_table_window: false,
            show_live_tracking_window: false,
            show_live_tracking_afr_window: false,
            show_new_bin_window: false,
            show_about_window: false,
            rom_embedded_view: RomEmbeddedView::None,
            live_tracking_map: RomEmbeddedView::Fuel,
            rom_table_kind: RomFuelTableKind::LowCam,
            rom_ign_table_kind: RomFuelTableKind::LowCam,
            live_tracking_kind: RomFuelTableKind::LowCam,
            rom_table_view_mode: RomTableViewMode::ThreeD,
            rom_ign_table_view_mode: RomTableViewMode::ThreeD,
            live_tracking_view_mode: RomTableViewMode::TwoD,
            live_tracking_afr_map: RomEmbeddedView::Fuel,
            live_tracking_afr_kind: RomFuelTableKind::LowCam,
            live_tracking_afr_view_mode: RomTableViewMode::TwoD,
            live_tracking_afr_map_mode: LiveTrackingAfrMapMode::LiveAfr,
            rom_table_3d_pan: egui::vec2(0.0, 0.0),
            rom_table_3d_scale: 1.0,
            rom_table_3d_yaw: -0.75,
            rom_table_3d_pitch: 0.70,
            rom_ign_table_3d_pan: egui::vec2(0.0, 0.0),
            rom_ign_table_3d_scale: 1.0,
            rom_ign_table_3d_yaw: -0.75,
            rom_ign_table_3d_pitch: 0.70,
            live_tracking_3d_pan: egui::vec2(0.0, 0.0),
            live_tracking_3d_scale: 1.0,
            live_tracking_3d_yaw: -0.75,
            live_tracking_3d_pitch: 0.70,
            rom_table_high_3d_pan: egui::vec2(0.0, 0.0),
            rom_table_high_3d_scale: 1.0,
            rom_table_high_3d_yaw: -0.75,
            rom_table_high_3d_pitch: 0.70,
            rom_ign_table_high_3d_pan: egui::vec2(0.0, 0.0),
            rom_ign_table_high_3d_scale: 1.0,
            rom_ign_table_high_3d_yaw: -0.75,
            rom_ign_table_high_3d_pitch: 0.70,
            live_tracking_high_3d_pan: egui::vec2(0.0, 0.0),
            live_tracking_high_3d_scale: 1.0,
            live_tracking_high_3d_yaw: -0.75,
            live_tracking_high_3d_pitch: 0.70,
            live_tracking_afr_3d_pan: egui::vec2(0.0, 0.0),
            live_tracking_afr_3d_scale: 1.0,
            live_tracking_afr_3d_yaw: -0.75,
            live_tracking_afr_3d_pitch: 0.70,
            rom_table_zone_row_start: 1,
            rom_table_zone_row_end: ROM_FUEL_TABLE_ROWS,
            rom_table_zone_col_start: 1,
            rom_table_zone_col_end: ROM_FUEL_TABLE_COLS,
            rom_table_drag_anchor: None,
            rom_ign_table_zone_row_start: 1,
            rom_ign_table_zone_row_end: ROM_FUEL_TABLE_ROWS,
            rom_ign_table_zone_col_start: 1,
            rom_ign_table_zone_col_end: ROM_FUEL_TABLE_COLS,
            rom_ign_table_drag_anchor: None,
            live_tracking_zone_row_start: 1,
            live_tracking_zone_row_end: ROM_FUEL_TABLE_ROWS,
            live_tracking_zone_col_start: 1,
            live_tracking_zone_col_end: ROM_FUEL_TABLE_COLS,
            live_tracking_drag_anchor: None,
            live_tracking_afr_zone_row_start: 1,
            live_tracking_afr_zone_row_end: ROM_FUEL_TABLE_ROWS,
            live_tracking_afr_zone_col_start: 1,
            live_tracking_afr_zone_col_end: ROM_FUEL_TABLE_COLS,
            live_tracking_afr_drag_anchor: None,
            live_tracking_capture_enabled: true,
            follow_vtec_tables: true,
            live_tracking_cell_mode: LiveTrackingCellMode::Single,
            live_tracking_sample_count: 0,
            live_tracking_afr_value_low: Self::new_empty_rom_tracking_value_table(),
            live_tracking_afr_sum_low: Self::new_empty_rom_tracking_sum_table(),
            live_tracking_afr_count_low: Self::new_empty_rom_tracking_count_table(),
            live_tracking_afr_pending_count_low: Self::new_empty_rom_tracking_count_table(),
            live_tracking_afr_value_high: Self::new_empty_rom_tracking_value_table(),
            live_tracking_afr_sum_high: Self::new_empty_rom_tracking_sum_table(),
            live_tracking_afr_count_high: Self::new_empty_rom_tracking_count_table(),
            live_tracking_afr_pending_count_high: Self::new_empty_rom_tracking_count_table(),
            live_tracking_afr_target: Self::new_default_rom_tracking_target_table(),
            live_tracking_afr_target_input: "14.7".to_string(),
            sensor_table_kind: RomFuelTableKind::LowCam,
            table_metric: TableMetric::Afr,
            rom_table_column_multipliers: [4.0; ROM_FUEL_TABLE_COLS],
            table_afr_low: Self::new_empty_table(),
            table_fuel_value_low: Self::new_empty_table(),
            table_inj_duty_low: Self::new_empty_table(),
            table_ign_low: Self::new_empty_table(),
            table_afr_high: Self::new_empty_table(),
            table_fuel_value_high: Self::new_empty_table(),
            table_inj_duty_high: Self::new_empty_table(),
            table_ign_high: Self::new_empty_table(),
        }
    }
}

impl HondaGuiApp {
    fn new_empty_table() -> Vec<Vec<Option<f64>>> {
        vec![vec![None; AFR_TABLE_SIZE]; AFR_TABLE_SIZE]
    }

    fn new_empty_rom_tracking_sum_table() -> Vec<Vec<f64>> {
        vec![vec![0.0; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS]
    }

    fn new_empty_rom_tracking_value_table() -> Vec<Vec<Option<f64>>> {
        vec![vec![None; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS]
    }

    fn new_empty_rom_tracking_count_table() -> Vec<Vec<u32>> {
        vec![vec![0; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS]
    }

    fn new_default_rom_tracking_target_table() -> Vec<Vec<f64>> {
        vec![vec![14.7; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS]
    }

    fn live_tracking_afr_values(&self, kind: RomFuelTableKind) -> &Vec<Vec<Option<f64>>> {
        match kind {
            RomFuelTableKind::LowCam => &self.live_tracking_afr_value_low,
            RomFuelTableKind::HighCam => &self.live_tracking_afr_value_high,
        }
    }

    fn live_tracking_afr_batch_tables_mut(
        &mut self,
        kind: RomFuelTableKind,
    ) -> (
        &mut Vec<Vec<Option<f64>>>,
        &mut Vec<Vec<f64>>,
        &mut Vec<Vec<u32>>,
        &mut Vec<Vec<u32>>,
    ) {
        match kind {
            RomFuelTableKind::LowCam => (
                &mut self.live_tracking_afr_value_low,
                &mut self.live_tracking_afr_sum_low,
                &mut self.live_tracking_afr_pending_count_low,
                &mut self.live_tracking_afr_count_low,
            ),
            RomFuelTableKind::HighCam => (
                &mut self.live_tracking_afr_value_high,
                &mut self.live_tracking_afr_sum_high,
                &mut self.live_tracking_afr_pending_count_high,
                &mut self.live_tracking_afr_count_high,
            ),
        }
    }

    fn graph_sensor_live_value(&self, sensor: GraphSensor) -> f64 {
        match sensor {
            GraphSensor::Tps => self.tps,
            GraphSensor::Rpm => self.rpm,
            GraphSensor::Afr => self.afr,
            GraphSensor::Map => self.map,
            GraphSensor::Batt => self.battery,
            GraphSensor::InjMs => self.inj_ms,
            GraphSensor::Ign => self.ign_advance,
            GraphSensor::Lambda => self.lambda,
            GraphSensor::Boost => self.boost_psi,
            GraphSensor::Vss => self.vss_kmh,
        }
    }

    fn long2bytes(byte1: u8, byte2: u8) -> u16 {
        ((byte2 as u16) << 8) | byte1 as u16
    }

    fn clamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
        v.max(lo).min(hi)
    }

    fn round2(v: f64) -> f64 {
        (v * 100.0).round() / 100.0
    }

    fn fc_ratio(value: u16, base: f64) -> f64 {
        Self::round2(value as f64 / base)
    }

    fn fc_ratio_u8(value: u8, base: f64) -> f64 {
        Self::round2(value as f64 / base)
    }

    fn ic_value(value: u8) -> f64 {
        let v = value as i32;
        if v == 128 {
            0.0
        } else if v < 128 {
            (128 - v) as f64 * -0.25
        } else {
            (v - 128) as f64 * 0.25
        }
    }

    fn ebc_value(value: u8) -> f64 {
        Self::round2((value as f64 / 2.0).min(100.0) * 10.0) / 10.0
    }

    fn calc_gear(vss_kmh: f64, rpm: f64, rpm_div_raw: u16) -> u8 {
        const TRANNY: [u32; 4] = [70, 103, 142, 184];
        if vss_kmh <= 0.0 || rpm <= 0.0 {
            return 0;
        }

        let num = ((vss_kmh as u32 * 256) * rpm_div_raw as u32) / 65535;
        for (i, t) in TRANNY.iter().enumerate() {
            if num < *t {
                return (i + 1) as u8;
            }
        }
        5
    }

    fn calc_instant_consumption(vss_kmh: f64, injector_duty: f64) -> f64 {
        if vss_kmh <= 0.0 {
            return 0.0;
        }
        let hundred_km = 6000.0 / vss_kmh;
        let fuelc = (hundred_km * ((INJECTOR_SIZE_CC / 100.0) * injector_duty)) / 1000.0;
        Self::clamp_f64(fuelc * 4.0, 0.0, 50.0)
    }

    fn is_vtec_engaged_live(&self) -> bool {
        self.vts_active
    }

    fn live_kind_from_vtec(&self) -> RomFuelTableKind {
        if self.is_vtec_engaged_live() {
            RomFuelTableKind::HighCam
        } else {
            RomFuelTableKind::LowCam
        }
    }

    fn sync_table_kinds_to_vtec(&mut self) {
        let live_kind = self.live_kind_from_vtec();
        self.rom_table_kind = live_kind;
        self.rom_ign_table_kind = live_kind;
        self.live_tracking_kind = live_kind;
        self.live_tracking_afr_kind = live_kind;
        self.sensor_table_kind = live_kind;
    }

    fn checksum8(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
    }

    fn ostrich_read_byte_checked(&mut self, addr: u16) -> Option<u8> {
        if self.rom_com_port.trim().is_empty() {
            self.log
                .push("Ostrich read check: set Live ROM Port first".to_string());
            return None;
        }

        let mut port = match serialport::new(&self.rom_com_port, 38400)
            .timeout(Duration::from_millis(700))
            .open()
        {
            Ok(port) => port,
            Err(e) => {
                self.log.push(format!("Ostrich read open error: {e}"));
                return None;
            }
        };

        if port.write_all(b"VV").is_err() {
            self.log.push("Ostrich read check: VV write failed".to_string());
            return None;
        }

        let mut vv = [0u8; 3];
        if port.read_exact(&mut vv).is_err() || vv[2] != b'O' {
            self.log.push(
                "Ostrich read check: invalid VV reply (wrong interface/port)".to_string(),
            );
            return None;
        }

        let header_wo_sum = [b'R', 1u8, (addr >> 8) as u8, (addr & 0xFF) as u8];
        let sum = Self::checksum8(&header_wo_sum);
        let header = [header_wo_sum[0], header_wo_sum[1], header_wo_sum[2], header_wo_sum[3], sum];

        if let Err(e) = port.write_all(&header) {
            self.log.push(format!("Ostrich read cmd error @0x{addr:04X}: {e}"));
            return None;
        }

        let mut resp = [0u8; 2];
        if let Err(e) = port.read_exact(&mut resp) {
            self.log.push(format!("Ostrich read timeout @0x{addr:04X}: {e}"));
            return None;
        }

        if resp[1] != resp[0] {
            self.log.push(format!(
                "Ostrich read checksum mismatch @0x{addr:04X}: data=0x{:02X} sum=0x{:02X}",
                resp[0], resp[1]
            ));
            return None;
        }

        Some(resp[0])
    }

    fn ostrich_read_block_checked(
        &mut self,
        start_addr: u16,
        size: usize,
        reason: &str,
    ) -> Option<Vec<u8>> {
        if self.rom_com_port.trim().is_empty() {
            self.log
                .push(format!("{reason}: set Live ROM Port first for Ostrich read"));
            return None;
        }

        if size == 0 {
            self.log.push(format!("{reason}: no data requested"));
            return Some(Vec::new());
        }

        if (start_addr as usize + size) > 0x1_0000 {
            self.log.push(format!(
                "{reason}: range exceeds 16-bit Ostrich addressing (start=0x{start_addr:04X}, len={size})"
            ));
            return None;
        }

        let mut port = match serialport::new(&self.rom_com_port, 38400)
            .timeout(Duration::from_millis(1200))
            .open()
        {
            Ok(port) => port,
            Err(e) => {
                self.log.push(format!("{reason}: open error: {e}"));
                return None;
            }
        };

        if let Err(e) = port.write_all(b"VV") {
            self.log.push(format!("{reason}: VV write failed: {e}"));
            return None;
        }

        let mut vv = [0u8; 3];
        if let Err(e) = port.read_exact(&mut vv) {
            self.log.push(format!("{reason}: VV read failed: {e}"));
            return None;
        }
        if vv[2] != b'O' {
            self.log.push(format!(
                "{reason}: invalid VV type 0x{:02X} (wrong interface/port)",
                vv[2]
            ));
            return None;
        }

        self.log.push(format!(
            "{reason}: Ostrich VV OK hw=0x{:02X} fw=0x{:02X} type='{}'",
            vv[0], vv[1], vv[2] as char
        ));

        let mut data = Vec::with_capacity(size);
        let mut offset = 0usize;

        while offset < size {
            let remaining = size - offset;
            let chunk_len = remaining.min(256);
            let addr = start_addr.wrapping_add(offset as u16);

            if chunk_len == 256 && (addr % 256 == 0) {
                let block_addr = addr / 256;
                let header_wo_sum = [b'Z', b'R', 1u8, (block_addr >> 8) as u8, (block_addr & 0xFF) as u8];
                let sum = Self::checksum8(&header_wo_sum);
                let header = [header_wo_sum[0], header_wo_sum[1], header_wo_sum[2], header_wo_sum[3], header_wo_sum[4], sum];

                if let Err(e) = port.write_all(&header) {
                    self.log.push(format!("{reason}: ZR write error @0x{addr:04X}: {e}"));
                    return None;
                }
            } else {
                let size_field = if chunk_len == 256 { 0u8 } else { chunk_len as u8 };
                let header_wo_sum = [b'R', size_field, (addr >> 8) as u8, (addr & 0xFF) as u8];
                let sum = Self::checksum8(&header_wo_sum);
                let header = [header_wo_sum[0], header_wo_sum[1], header_wo_sum[2], header_wo_sum[3], sum];

                if let Err(e) = port.write_all(&header) {
                    self.log.push(format!("{reason}: R write error @0x{addr:04X}: {e}"));
                    return None;
                }
            }

            let mut chunk = vec![0u8; chunk_len];
            if let Err(e) = port.read_exact(&mut chunk) {
                self.log
                    .push(format!("{reason}: data timeout @0x{addr:04X} len={chunk_len}: {e}"));
                return None;
            }

            let mut sum_rx = [0u8; 1];
            if let Err(e) = port.read_exact(&mut sum_rx) {
                self.log
                    .push(format!("{reason}: checksum timeout @0x{addr:04X}: {e}"));
                return None;
            }

            let sum_calc = Self::checksum8(&chunk);
            if sum_rx[0] != sum_calc {
                self.log.push(format!(
                    "{reason}: checksum mismatch @0x{addr:04X}: got=0x{:02X} calc=0x{:02X}",
                    sum_rx[0], sum_calc
                ));
                return None;
            }

            data.extend_from_slice(&chunk);
            offset += chunk_len;
        }

        Some(data)
    }

    fn bit_is_set(value: u8, bit: u8) -> bool {
        (value & (1u8 << bit)) != 0
    }

    fn iacv_state_label(value: u8) -> &'static str {
        match value {
            0x00 => "IACV ON",
            0xFF => "IACV OFF (NO IACV)",
            _ => "IACV CUSTOM",
        }
    }

    fn fan_control_state_label(value: u8) -> &'static str {
        match value {
            0xFF => "FAN CONTROL ON",
            0x00 => "FAN CONTROL OFF",
            _ => "FAN CONTROL CUSTOM",
        }
    }

    fn shift_light_state_label(value: u8) -> &'static str {
        match value {
            0xFF => "SHIFT LIGHT ON",
            0x00 => "SHIFT LIGHT OFF",
            _ => "SHIFT LIGHT CUSTOM",
        }
    }

    fn cut_state_label(a: u8, b: u8) -> &'static str {
        match (a, b) {
            (0x00, 0x00) => "FUEL CUT",
            (0xFF, 0xFF) => "IGN CUT",
            (0xFF, 0x00) => "FUEL+IGN CUT",
            (0x00, 0xFF) => "CUT UNKNOWN (00/FF)",
            _ => "CUT CUSTOM",
        }
    }

    fn launch_state_label(value: u8) -> &'static str {
        match value {
            0x00 => "LAUNCH OFF",
            0x80 => "LAUNCH ON",
            _ => "LAUNCH CUSTOM",
        }
    }

    fn launch_tps_state_label(enable: u8, value: u8) -> &'static str {
        match (enable, value) {
            (0x00, 0x5F) => "LAUNCH TPS OFF",
            (0xFF, 0x5E) => "LAUNCH TPS ON",
            _ => "LAUNCH TPS CUSTOM",
        }
    }

    fn vtec_switch_state_label(sw6120: u8, sw61f2: u8, sw6657: u8, sw6659: u8) -> &'static str {
        match (sw6120, sw61f2, sw6657, sw6659) {
            (0x44, 0xFF, 0x52, 0x37) => "VTEC ON",
            (0x43, 0x00, 0x54, 0x33) => "VTEC OFF",
            _ => "VTEC CUSTOM",
        }
    }

    fn vtec_rpm_preset_values(rpm: u16) -> Option<(u8, u8)> {
        match rpm {
            4000 => Some((0xC0, 0xC0)),
            4500 => Some((0xC0, 0xC8)),
            5000 => Some((0xDB, 0xD0)),
            5500 => Some((0xD8, 0xD8)),
            6000 => Some((0xD0, 0xE0)),
            _ => None,
        }
    }

    fn ign_timing_preset_value(ms: u16) -> Option<u8> {
        match ms {
            50 => Some(0x05),
            70 => Some(0x07),
            100 => Some(0x0A),
            _ => None,
        }
    }

    fn ect_raw_to_celsius(raw: u8) -> f64 {
        let x = raw as f64 / 51.0;
        let f = (0.1423 * x.powi(6))
            - (2.4938 * x.powi(5))
            + (17.837 * x.powi(4))
            - (68.698 * x.powi(3))
            + (154.69 * x.powi(2))
            - (232.75 * x)
            + 284.24;
        ((f - 32.0) * 5.0) / 9.0
    }

    fn ect_celsius_to_raw(target_c: f64) -> u8 {
        let mut best_raw = 0u8;
        let mut best_err = f64::INFINITY;
        for raw in 0u8..=u8::MAX {
            let c = Self::ect_raw_to_celsius(raw);
            let err = (c - target_c).abs();
            if err < best_err {
                best_err = err;
                best_raw = raw;
            }
        }
        best_raw
    }

    fn shift_light_raw_to_rpm(raw: u16) -> f64 {
        if raw == 0 {
            return 0.0;
        }
        1_875_000.0 / raw as f64
    }

    fn shift_light_rpm_to_raw(target_rpm: f64) -> Option<u16> {
        if !target_rpm.is_finite() || target_rpm <= 0.0 {
            return None;
        }
        let raw = (1_875_000.0 / target_rpm).round();
        if raw < 1.0 || raw > u16::MAX as f64 {
            return None;
        }
        Some(raw as u16)
    }

    fn log_rom_settings_snapshot(&mut self, source: &str) {
        self.log.push(format!(
            "{source}: IACV @0x6116 = 0x{:02X} => {}",
            self.iacv_value,
            Self::iacv_state_label(self.iacv_value)
        ));
        self.log.push(format!(
            "{source}: SHIFT LIGHT @0x6142 = 0x{:02X} => {} | RPM @0x6144/0x6145 = 0x{:04X} ({:.0} rpm)",
            self.shift_light_enable_value,
            Self::shift_light_state_label(self.shift_light_enable_value),
            self.shift_light_rpm_raw_value,
            Self::shift_light_raw_to_rpm(self.shift_light_rpm_raw_value)
        ));
        self.log.push(format!(
            "{source}: FAN CONTROL @0x616D = 0x{:02X} => {}",
            self.fan_control_enable_value,
            Self::fan_control_state_label(self.fan_control_enable_value)
        ));
        self.log.push(format!(
            "{source}: FAN TEMP @0x616E = 0x{:02X} ({:.1} C)",
            self.fan_control_temp_value,
            Self::ect_raw_to_celsius(self.fan_control_temp_value)
        ));
        self.log.push(format!(
            "{source}: CUT @0x6125/0x6126 = 0x{:02X}/0x{:02X} => {}",
            self.cut_a_value,
            self.cut_b_value,
            Self::cut_state_label(self.cut_a_value, self.cut_b_value)
        ));
        self.log.push(format!(
            "{source}: IGN TIMING @0x61D8/0x61E4/0x61E5 = 0x{:02X}/0x{:02X}/0x{:02X}",
            self.ign_timing_61d8,
            self.ign_timing_61e4,
            self.ign_timing_61e5
        ));
        self.log.push(format!(
            "{source}: Launch @0x6152 = 0x{:02X} => {} | TPS @0x6155/0x6158 = 0x{:02X}/0x{:02X} => {}",
            self.launch_value,
            Self::launch_state_label(self.launch_value),
            self.launch_tps_enable_value,
            self.launch_tps_value,
            Self::launch_tps_state_label(self.launch_tps_enable_value, self.launch_tps_value)
        ));
        self.log.push(format!(
            "{source}: VTEC SW @0x6120/0x61F2/0x6657/0x6659 = 0x{:02X}/0x{:02X}/0x{:02X}/0x{:02X} => {}",
            self.vtec_sw_6120,
            self.vtec_sw_61f2,
            self.vtec_sw_6657,
            self.vtec_sw_6659,
            Self::vtec_switch_state_label(
                self.vtec_sw_6120,
                self.vtec_sw_61f2,
                self.vtec_sw_6657,
                self.vtec_sw_6659,
            )
        ));
        self.log.push(format!(
            "{source}: VTEC VAL @0x6658/0x665A = 0x{:02X}/0x{:02X}",
            self.vtec_val_6658,
            self.vtec_val_665a
        ));
    }

    fn refresh_tuning_values_from_rom(&mut self, rom: &[u8]) {
        let idx = |off: u16| off as usize;
        let get = |off: u16| rom.get(idx(off)).copied();

        if let Some(v) = get(BLINK_OFFSET) {
            self.mil_blinks = v;
            self.new_mil_blinks = v.to_string();
        }

        if let Some(v) = get(IACV_OFFSET) {
            self.iacv_value = v;
            self.new_iacv_value = format!("{v:02X}");
        }

        if let Some(v) = get(SHIFT_LIGHT_ENABLE_OFFSET) {
            self.shift_light_enable_value = v;
        }
        if let (Some(lo), Some(hi)) = (get(SHIFT_LIGHT_RPM_LO_OFFSET), get(SHIFT_LIGHT_RPM_HI_OFFSET)) {
            self.shift_light_rpm_raw_value = u16::from_le_bytes([lo, hi]);
            self.new_shift_light_rpm_value =
                format!("{:.0}", Self::shift_light_raw_to_rpm(self.shift_light_rpm_raw_value));
        }

        if let Some(v) = get(FAN_CONTROL_ENABLE_OFFSET) {
            self.fan_control_enable_value = v;
        }
        if let Some(v) = get(FAN_CONTROL_TEMP_OFFSET) {
            self.fan_control_temp_value = v;
            self.new_fan_control_temp_value = format!("{:.1}", Self::ect_raw_to_celsius(v));
        }

        if let Some(v) = get(CUT_A_OFFSET) {
            self.cut_a_value = v;
        }
        if let Some(v) = get(CUT_B_OFFSET) {
            self.cut_b_value = v;
        }
        if let Some(v) = get(IGN_TIMING_61D8_OFFSET) {
            self.ign_timing_61d8 = v;
        }
        if let Some(v) = get(IGN_TIMING_61E4_OFFSET) {
            self.ign_timing_61e4 = v;
        }
        if let Some(v) = get(IGN_TIMING_61E5_OFFSET) {
            self.ign_timing_61e5 = v;
        }
        if let Some(v) = get(LAUNCH_OFFSET) {
            self.launch_value = v;
        }
        if let Some(v) = get(LAUNCH_TPS_ENABLE_OFFSET) {
            self.launch_tps_enable_value = v;
        }
        if let Some(v) = get(LAUNCH_TPS_VALUE_OFFSET) {
            self.launch_tps_value = v;
        }
        if let Some(v) = get(VTEC_SW_6120_OFFSET) {
            self.vtec_sw_6120 = v;
        }
        if let Some(v) = get(VTEC_SW_61F2_OFFSET) {
            self.vtec_sw_61f2 = v;
        }
        if let Some(v) = get(VTEC_SW_6657_OFFSET) {
            self.vtec_sw_6657 = v;
        }
        if let Some(v) = get(VTEC_VAL_6658_OFFSET) {
            self.vtec_val_6658 = v;
            self.new_vtec_val_6658 = format!("{v:02X}");
        }
        if let Some(v) = get(VTEC_SW_6659_OFFSET) {
            self.vtec_sw_6659 = v;
        }
        if let Some(v) = get(VTEC_VAL_665A_OFFSET) {
            self.vtec_val_665a = v;
            self.new_vtec_val_665a = format!("{v:02X}");
        }

        self.log_rom_settings_snapshot("ROM SETTINGS");
    }

    fn apply_patchset_to_bin(&mut self, reason: &str, patches: &[(u16, u8)]) {
        let mut rom = if self.selected_file_is_usable() {
            match fs::read(&self.selected_file) {
                Ok(rom) => rom,
                Err(e) => {
                    self.log.push(format!("File error: {e}"));
                    return;
                }
            }
        } else if let Some(rom) = &self.last_read_rom {
            self.log.push(format!("{reason}: applying to last live ROM read"));
            rom.clone()
        } else {
            self.log
                .push("Select a BIN or run READ HONDA DATA first".to_string());
            return;
        };

        for &(offset, _) in patches {
            if rom.len() <= offset as usize {
                self.log.push(format!(
                    "{reason}: ROM too small for offset 0x{offset:04X}"
                ));
                return;
            }
        }

        for &(offset, value) in patches {
            rom[offset as usize] = value;
        }

        self.last_read_rom = Some(rom.clone());
        self.rom_has_unsaved_changes = true;
        self.refresh_tuning_values_from_rom(&rom);
        self.log
            .push(format!("{reason}: staged in memory (not saved yet)"));

        if self.rom_com_port.trim().is_empty() {
            self.log.push(format!(
                "{reason}: Ostrich port not set, applying live without reboot (temporary)"
            ));
            self.burn_data_live_no_reboot(&rom);
            return;
        }

        for &(offset, _) in patches {
            let addr = OSTRICH_ADDR_BASE.wrapping_add(offset);
            if let Some(before) = self.ostrich_read_byte_checked(addr) {
                self.log
                    .push(format!("{reason} pre-write @0x{addr:04X} = 0x{before:02X}"));
            }
        }

        let ok = self.ostrich_write_persistent(OSTRICH_ADDR_BASE, &rom, reason);
        if !ok {
            return;
        }

        for &(offset, expected) in patches {
            let addr = OSTRICH_ADDR_BASE.wrapping_add(offset);
            if let Some(after) = self.ostrich_read_byte_checked(addr) {
                self.log
                    .push(format!("{reason} post-write @0x{addr:04X} = 0x{after:02X}"));
                if after == expected {
                    self.log.push(format!(
                        "{reason} verify OK @0x{addr:04X} (0x{expected:02X})"
                    ));
                } else {
                    self.log.push(format!(
                        "{reason} verify mismatch @0x{addr:04X}: expected 0x{expected:02X}, got 0x{after:02X}"
                    ));
                }
            }
        }
    }

    fn selected_file_is_usable(&self) -> bool {
        let p = self.selected_file.trim();
        !p.is_empty() && p != "No file selected"
    }

    fn format_hex_dump(data: &[u8]) -> String {
        let mut out = String::new();
        for (row, chunk) in data.chunks(16).enumerate() {
            let offset = row * 16;
            let _ = write!(&mut out, "{offset:04X}: ");

            for i in 0..16 {
                if i < chunk.len() {
                    let _ = write!(&mut out, "{:02X} ", chunk[i]);
                } else {
                    out.push_str("   ");
                }
            }

            out.push('|');
            for &b in chunk {
                let ch = if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                };
                out.push(ch);
            }
            out.push_str("|\n");
        }
        out
    }

    fn is_likely_serial_port(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.contains("ttyusb")
            || lower.contains("ttyacm")
            || lower.contains("/dev/pts/")
            || lower.starts_with("com")
            || lower.contains("cu.usb")
            || lower.contains("usbserial")
    }

    #[cfg(target_os = "linux")]
    fn list_linux_pts_ports() -> Vec<String> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir("/dev/pts") else {
            return out;
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };

            // Only numeric PTY nodes are usable serial endpoints (skip ptmx).
            if name.chars().all(|c| c.is_ascii_digit()) {
                out.push(format!("/dev/pts/{name}"));
            }
        }

        out.sort();
        out
    }

    #[cfg(not(target_os = "linux"))]
    fn list_linux_pts_ports() -> Vec<String> {
        Vec::new()
    }

    fn hide_problem_entry(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.contains("3d09") || lower.contains("flag")
    }

    fn detect_device(&mut self) {
        self.log.clear();
        self.log.push("Searching for One ROM...".to_string());

        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                self.device_status = format!("Runtime error: {e}");
                return;
            }
        };

        match rt.block_on(enumerate_devices(false, &[])) {
            Ok(devices) => {
                self.log.push(format!("Found {} device(s)", devices.len()));
                if let Some(dev) = devices.first() {
                    self.selected_serial = dev.serial.clone();
                    self.device_status = format!("{}", dev);
                    self.log.push(format!("Using: {}", dev));
                    self.log.push(format!("Serial: {:?}", dev.serial));
                } else {
                    self.selected_serial = None;
                    self.device_status = "No One ROM found".to_string();
                }
            }
            Err(e) => {
                self.selected_serial = None;
                self.device_status = format!("Error: {e}");
                self.log.push(format!("enumerate_devices error: {e}"));
            }
        }
    }

    fn with_first_device<F>(&mut self, action_name: &str, action: F)
    where
        F: FnOnce(&Runtime, &onerom_cli::device::Device) -> Result<(), String>,
    {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                self.log.push(format!("Runtime error: {e}"));
                return;
            }
        };

        match rt.block_on(enumerate_devices(false, &[])) {
            Ok(devices) => {
                if let Some(dev) = devices.first() {
                    self.selected_serial = dev.serial.clone();
                    if let Err(e) = action(&rt, dev) {
                        self.log.push(format!("{action_name} error: {e}"));
                    }
                } else {
                    self.log.push("No One ROM found".to_string());
                }
            }
            Err(e) => self.log.push(format!("USB error: {e}")),
        }
    }

    fn upload_selected_bin(&mut self) {
        let data = if self.rom_has_unsaved_changes {
            if let Some(rom) = &self.last_read_rom {
                self.log
                    .push("UPLOAD using unsaved staged ROM buffer".to_string());
                rom.clone()
            } else if self.selected_file_is_usable() {
                match fs::read(&self.selected_file) {
                    Ok(data) => data,
                    Err(e) => {
                        self.log.push(format!("File error: {e}"));
                        return;
                    }
                }
            } else {
                self.log.push("Select a BIN or run READ HONDA DATA first".to_string());
                return;
            }
        } else if self.selected_file_is_usable() {
            match fs::read(&self.selected_file) {
                Ok(data) => data,
                Err(e) => {
                    self.log.push(format!("File error: {e}"));
                    return;
                }
            }
        } else if let Some(rom) = &self.last_read_rom {
            self.log
                .push("UPLOAD using last ROM read buffer".to_string());
            rom.clone()
        } else {
            self.log.push("Select a BIN or run READ HONDA DATA first".to_string());
            return;
        };

        self.log
            .push(format!("UPLOAD(PERSISTENT): loaded data {} bytes", data.len()));

        if self.rom_com_port.trim().is_empty() {
            self.log.push(
                "UPLOAD(PERSISTENT): set Live ROM Port first (Ostrich interface required)"
                    .to_string(),
            );
            return;
        }

        let _ = self.ostrich_write_persistent(OSTRICH_ADDR_BASE, &data, "UPLOAD(PERSISTENT)");
    }

    fn save_staged_rom_to_selected_bin(&mut self) {
        let rom = match &self.last_read_rom {
            Some(rom) => rom,
            None => {
                self.log
                    .push("SAVE BIN: no staged ROM buffer (run READ ROM DATA first)".to_string());
                return;
            }
        };

        if !self.selected_file_is_usable() {
            let save_path = match rfd::FileDialog::new()
                .add_filter("BIN", &["bin"])
                .set_file_name("new_bin.bin")
                .save_file()
            {
                Some(path) => path,
                None => {
                    self.log.push("SAVE BIN: canceled".to_string());
                    return;
                }
            };
            self.selected_file = save_path.display().to_string();
        }

        match fs::write(&self.selected_file, rom) {
            Ok(_) => {
                self.rom_has_unsaved_changes = false;
                self.log.push("SAVE BIN: file updated".to_string());
            }
            Err(e) => self.log.push(format!("SAVE BIN error: {e}")),
        }
    }

    fn burn_data_live_no_reboot(&mut self, data: &[u8]) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                self.log.push(format!("Runtime error: {e}"));
                return;
            }
        };

        let devices = match rt.block_on(enumerate_devices(false, &[])) {
            Ok(devices) => devices,
            Err(e) => {
                self.log.push(format!("USB error: {e}"));
                return;
            }
        };

        let dev = match devices.first() {
            Some(dev) => dev,
            None => {
                self.log.push("No One ROM found".to_string());
                return;
            }
        };

        match rt.block_on(write_memory(dev, LIVE_ROM_BASE, data)) {
            Ok(_) => self
                .log
                .push("BURN(LIVE NO REBOOT): write OK (temporary until reboot)".to_string()),
            Err(e) => self.log.push(format!("BURN(LIVE NO REBOOT) write error: {e}")),
        }
    }

    fn ostrich_write_persistent(&mut self, start_addr: u16, data: &[u8], reason: &str) -> bool {
        if self.rom_com_port.trim().is_empty() {
            self.log
                .push("Set Live ROM Port first (Ostrich COM/TTY)".to_string());
            return false;
        }

        if data.is_empty() {
            self.log.push(format!("{reason}: no data"));
            return false;
        }

        if (start_addr as usize + data.len()) > 0x1_0000 {
            self.log.push(format!(
                "{reason}: range exceeds 16-bit Ostrich addressing (start=0x{start_addr:04X}, len={})",
                data.len()
            ));
            return false;
        }

        let was_datalog_connected = self.datalog_connected;
        self.datalog_connected = false;

        self.log.push(format!(
            "{reason}: opening Ostrich channel on {}",
            self.rom_com_port
        ));

        let mut port = match serialport::new(&self.rom_com_port, 38400)
            .timeout(Duration::from_millis(700))
            .open()
        {
            Ok(port) => port,
            Err(e) => {
                self.log.push(format!("Ostrich open error: {e}"));
                self.datalog_connected = was_datalog_connected;
                return false;
            }
        };

        // Strict probe: if VV fails, abort to avoid writing on the wrong CDC port.
        if let Err(e) = port.write_all(b"VV") {
            self.log.push(format!("Ostrich probe write error: {e}"));
            self.datalog_connected = was_datalog_connected;
            return false;
        }

        let mut vv_reply = [0u8; 3];
        if let Err(e) = port.read_exact(&mut vv_reply) {
            self.log.push(format!(
                "Ostrich VV failed (wrong port or no Ostrich channel): {e}"
            ));
            self.datalog_connected = was_datalog_connected;
            return false;
        }

        if vv_reply[2] != b'O' {
            self.log.push(format!(
                "Ostrich VV invalid type byte: 0x{:02X} (expected 'O'). Wrong port/interface.",
                vv_reply[2]
            ));
            self.datalog_connected = was_datalog_connected;
            return false;
        }

        self.log.push(format!(
            "Ostrich VV OK: hw=0x{:02X} fw=0x{:02X} type='{}'",
            vv_reply[0], vv_reply[1], vv_reply[2] as char
        ));

        self.log.push(format!(
            "{reason}: streaming {} byte(s) from 0x{start_addr:04X} (Ostrich ZW/W mode)",
            data.len()
        ));

        let mut offset = 0usize;
        while offset < data.len() {
            let remaining = data.len() - offset;
            let chunk_len = remaining.min(256);
            let addr = start_addr.wrapping_add(offset as u16);

            let mut use_bulk = false;
            let header: [u8; 5];
            if chunk_len == 256 && (addr % 256 == 0) {
                // Match Ostrich block writes: ZW <count_blocks> <addr_hi_blocks> <addr_lo_blocks>
                let block_addr = addr / 256;
                header = [b'Z', b'W', 1u8, (block_addr >> 8) as u8, (block_addr & 0xFF) as u8];
                use_bulk = true;
            } else {
                // Fallback to byte-addressed W for non-256-sized tails.
                let size_field = if chunk_len == 256 { 0u8 } else { chunk_len as u8 };
                header = [b'W', size_field, (addr >> 8) as u8, (addr & 0xFF) as u8, 0u8];
            }

            let header_sum_len = if use_bulk { 5 } else { 4 };
            let mut sum = Self::checksum8(&header[..header_sum_len]);
            sum = sum.wrapping_add(Self::checksum8(&data[offset..offset + chunk_len]));

            if let Err(e) = port.write_all(&header[..header_sum_len]) {
                self.log.push(format!("Ostrich header write error @0x{addr:04X}: {e}"));
                self.datalog_connected = was_datalog_connected;
                return false;
            }

            if let Err(e) = port.write_all(&data[offset..offset + chunk_len]) {
                self.log.push(format!("Ostrich data write error @0x{addr:04X}: {e}"));
                self.datalog_connected = was_datalog_connected;
                return false;
            }

            if let Err(e) = port.write_all(&[sum]) {
                self.log
                    .push(format!("Ostrich checksum write error @0x{addr:04X}: {e}"));
                self.datalog_connected = was_datalog_connected;
                return false;
            }

            let mut ack = [0u8; 1];
            match port.read_exact(&mut ack) {
                Ok(_) if ack[0] == b'O' => {}
                Ok(_) => {
                    self.log.push(format!(
                        "Ostrich NACK @0x{addr:04X}: got 0x{:02X}",
                        ack[0]
                    ));
                    self.datalog_connected = was_datalog_connected;
                    return false;
                }
                Err(e) => {
                    self.log
                        .push(format!("Ostrich ACK timeout @0x{addr:04X}: {e}"));
                    self.datalog_connected = was_datalog_connected;
                    return false;
                }
            }

            offset += chunk_len;
        }

        self.log
            .push(format!("{reason}: data sent, waiting for flash auto-commit..."));

        // Keep datalog paused longer to let commit complete.
        thread::sleep(Duration::from_millis(2600));

        self.log.push(
            "Commit wait complete. Datalog remains paused; reconnect manually after verification."
                .to_string(),
        );

        // Intentionally keep datalog disconnected here: frequent datalog traffic
        // can postpone the plugin's idle-based persistence task.
        self.datalog_connected = false;
        true
    }

    fn read_honda_data_from_bin(&mut self) {
        self.log.push(format!(
            "Reading ROM data from 0x{ROM_READ_BASE:04X} ({} bytes)...",
            ROM_READ_SIZE
        ));

        if !self.rom_com_port.trim().is_empty() {
            if let Some(rom) = self.ostrich_read_block_checked(
                ROM_READ_BASE as u16,
                ROM_READ_SIZE as usize,
                "READ HONDA DATA (OSTRICH)",
            ) {
                self.last_read_rom = Some(rom.clone());
                self.rom_has_unsaved_changes = false;
                self.rom_hex_dump = Self::format_hex_dump(&rom);

                if rom.len() <= BLINK_OFFSET as usize {
                    self.log.push("ROM too small for MIL blink offset".to_string());
                    return;
                }

                self.mil_blinks = rom[BLINK_OFFSET as usize];
                self.new_mil_blinks = self.mil_blinks.to_string();
                self.refresh_tuning_values_from_rom(&rom);
                self.log.push(format!(
                    "MIL Blink Count updated from Ostrich ROM @0x{:04X}: {}",
                    OSTRICH_ADDR_BASE.wrapping_add(BLINK_OFFSET),
                    self.mil_blinks
                ));
                return;
            }

            self.log.push(
                "READ HONDA DATA (OSTRICH) failed, falling back to USB read_memory".to_string(),
            );
        }

        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                self.log.push(format!("Runtime error: {e}"));
                return;
            }
        };

        let devices = match rt.block_on(enumerate_devices(false, &[])) {
            Ok(devices) => devices,
            Err(e) => {
                self.log.push(format!("USB error: {e}"));
                return;
            }
        };

        let dev = match devices.first() {
            Some(dev) => dev,
            None => {
                self.log.push("No One ROM found".to_string());
                return;
            }
        };

        let rom = match rt.block_on(read_memory(dev, ROM_READ_BASE, ROM_READ_SIZE)) {
            Ok(rom) => rom,
            Err(e) => {
                self.log.push(format!("READ HONDA DATA error: {e}"));
                return;
            }
        };

        self.last_read_rom = Some(rom.clone());
        self.rom_has_unsaved_changes = false;

        self.rom_hex_dump = Self::format_hex_dump(&rom);

        if rom.len() <= BLINK_OFFSET as usize {
            self.log.push("ROM too small for MIL blink offset".to_string());
            return;
        }

        self.mil_blinks = rom[BLINK_OFFSET as usize];
        self.new_mil_blinks = self.mil_blinks.to_string();
        self.refresh_tuning_values_from_rom(&rom);
        self.log
            .push(format!("MIL Blink Count updated from live ROM: {}", self.mil_blinks));
    }

    fn read_selected_bin_file(&mut self, open_hex: bool) {
        if !self.selected_file_is_usable() {
            self.log.push("READ BIN FILE: select a BIN first".to_string());
            return;
        }

        let rom = match fs::read(&self.selected_file) {
            Ok(rom) => rom,
            Err(e) => {
                self.log.push(format!("READ BIN FILE error: {e}"));
                return;
            }
        };

        self.last_read_rom = Some(rom.clone());
        self.rom_has_unsaved_changes = false;
        self.rom_hex_dump = Self::format_hex_dump(&rom);
        if open_hex {
            self.show_hex_window = true;
        }

        if rom.len() > BLINK_OFFSET as usize {
            self.mil_blinks = rom[BLINK_OFFSET as usize];
            self.new_mil_blinks = self.mil_blinks.to_string();
            self.refresh_tuning_values_from_rom(&rom);
        } else {
            self.log
                .push("READ BIN FILE: ROM too small for tuning offsets".to_string());
        }

        self.log
            .push(format!("READ BIN FILE: loaded {} bytes", rom.len()));
    }

    fn create_new_bin_from_embedded_template(&mut self, template_name: &str, data: &[u8]) {
        if data.is_empty() {
            self.log
                .push(format!("BASEMAP: embedded template {template_name} is empty"));
            return;
        }

        let rom = data.to_vec();
        self.selected_file = "No file selected".to_string();
        self.last_read_rom = Some(rom.clone());
        self.rom_has_unsaved_changes = true;
        self.rom_hex_dump = Self::format_hex_dump(&rom);

        if rom.len() > BLINK_OFFSET as usize {
            self.mil_blinks = rom[BLINK_OFFSET as usize];
            self.new_mil_blinks = self.mil_blinks.to_string();
            self.refresh_tuning_values_from_rom(&rom);
        }

        self.log.push(format!(
            "BASEMAP: template {} loaded in memory (not saved). Click SAVE BIN to choose file.",
            template_name
        ));
    }

    fn draw_new_bin_window(&mut self, ctx: &egui::Context) {
        if !self.show_new_bin_window {
            return;
        }

        let mut open = self.show_new_bin_window;
        let mut selected_template: Option<(&'static str, &'static [u8])> = None;

        egui::Window::new("BASEMAP SELECTION")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label("Select a basemap");
                ui.separator();

                ui.group(|ui| {
                    ui.strong("P28 D16Z6");

                    ui.label("Stock MAP/240cc/VTEC @5500/Rev Limit @7400");
                    if ui.button("LOAD P28").clicked() {
                        selected_template = Some(("P28", EMBEDDED_BIN_P28));
                    }
                });

                ui.group(|ui| {
                    ui.strong("P30 B16A2");

                    ui.label("Stock MAP/240cc/VTEC @5700/Rev Limit @8400");
                    if ui.button("LOAD P30").clicked() {
                        selected_template = Some(("P30", EMBEDDED_BIN_P30));
                    }
                });

                ui.group(|ui| {
                    ui.strong("P73 B18CR");

                    ui.label("Stock MAP/240cc/VTEC @5800/Rev Limit @8900");
                    if ui.button("LOAD P73 B18CR").clicked() {
                        selected_template = Some(("P73_B18CR", EMBEDDED_BIN_P73_B18CR));
                    }
                });
            });

        if let Some((name, data)) = selected_template {
            self.create_new_bin_from_embedded_template(name, data);
            open = false;
        }

        self.show_new_bin_window = open;
    }

    fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about_window {
            return;
        }

        let mut open = self.show_about_window;
        egui::Window::new("ABOUT")
            .open(&mut open)
            .resizable(true)
            .default_size(egui::vec2(620.0, 360.0))
            .show(ctx, |ui| {
                ui.heading("SUPRAROM HONDA STUDIO");
                ui.label("Version: 3D Map Basemap Edition");
                ui.label("RTP/Datalog software made by Yosupra.");
                ui.separator();

                ui.strong("Copyright");
                ui.label("Based on One ROM project by Piers Finlayson.");
                ui.label("Based on One ROM Fire 28 B hardware, modified by Yosupra.");
                ui.label("Original software components: MIT License (Copyright (c) Piers Finlayson).");
                ui.label("Modifications and UI integration: Yosupra.");
                ui.label("Third-party components remain under their respective licenses.");
                ui.separator();

                ui.strong("Basemap Source");
                ui.label("Basemap from HTS2.22.");
                ui.label("All product names and trademarks remain property of their respective owners.");
                ui.separator();

                ui.strong("Purpose");
                ui.label("Basemap loading, ROM table editing, live tracking, and burn workflow for supported setups.");
                ui.label("Use only on legal and compatible hardware/firmware combinations.");
                ui.separator();

                ui.strong("Disclaimer");
                ui.label("This software is provided as-is, without warranty of any kind.");
                ui.label("User assumes all responsibility for calibration, hardware safety, and legal compliance.");
            });

        self.show_about_window = open;
    }

    fn apply_blink_to_bin(&mut self) {
        let mut rom = if self.selected_file_is_usable() {
            match fs::read(&self.selected_file) {
                Ok(rom) => rom,
                Err(e) => {
                    self.log.push(format!("File error: {e}"));
                    return;
                }
            }
        } else if let Some(rom) = &self.last_read_rom {
            self.log.push("Applying blink to last live ROM read".to_string());
            rom.clone()
        } else {
            self.log
                .push("Select a BIN or run READ HONDA DATA first".to_string());
            return;
        };

        if rom.len() <= 0x6020 {
            self.log.push("ROM too small for MIL blink offset".to_string());
            return;
        }

        let new_mil = self
            .new_mil_blinks
            .parse::<u8>()
            .unwrap_or(self.mil_blinks);
        rom[BLINK_OFFSET as usize] = new_mil;

        self.last_read_rom = Some(rom.clone());
        self.rom_has_unsaved_changes = true;
        self.mil_blinks = new_mil;
        self.new_mil_blinks = new_mil.to_string();
        self.refresh_tuning_values_from_rom(&rom);
        self.log.push(format!("MIL Blinks -> {}", new_mil));
        self.log
            .push("BLINK: staged in memory (not saved yet)".to_string());
        self.log.push(
            "BLINK: applying live temporary flash/ram (use BURN ROM for persistent)".to_string(),
        );
        self.burn_data_live_no_reboot(&rom);
    }

    fn read_rom_32k_and_save(&mut self) {
        self.log.push(format!(
            "Reading {} bytes from ROM @0x{ROM_READ_BASE:04X}...",
            ROM_READ_SIZE
        ));

        if !self.rom_com_port.trim().is_empty() {
            if let Some(rom) = self.ostrich_read_block_checked(
                ROM_READ_BASE as u16,
                ROM_READ_SIZE as usize,
                "READ ROM 32KB (OSTRICH)",
            ) {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("rom-32k-ostrich.bin")
                    .save_file()
                {
                    match fs::write(&path, &rom) {
                        Ok(_) => self.log.push(format!(
                            "READ ROM 32KB (OSTRICH) saved: {} bytes",
                            rom.len()
                        )),
                        Err(e) => self.log.push(format!("Save error: {e}")),
                    }
                }
                return;
            }

            self.log
                .push("READ ROM 32KB (OSTRICH) failed, fallback USB".to_string());
        }

        self.with_first_device("READ", |rt, dev| {
            let rom = rt
                .block_on(read_memory(dev, ROM_READ_BASE, ROM_READ_SIZE))
                .map_err(|e| e.to_string())?;

            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("rom-32k-live.bin")
                .save_file()
            {
                fs::write(&path, &rom).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
    }

    fn list_serial_ports(&self) -> Result<(Vec<String>, Vec<String>), String> {
        match serialport::available_ports() {
            Ok(ports) => {
                let mut all_ports: Vec<String> = ports
                    .into_iter()
                    .map(|p| p.port_name)
                    .filter(|p| !Self::hide_problem_entry(p))
                    .collect();

                all_ports.extend(Self::list_linux_pts_ports());
                all_ports.sort();
                all_ports.dedup();

                let mut preferred: Vec<String> = all_ports
                    .iter()
                    .filter(|p| Self::is_likely_serial_port(p))
                    .cloned()
                    .collect();

                if preferred.is_empty() {
                    preferred = all_ports.clone();
                }

                Ok((all_ports, preferred))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn scan_rom_ports(&mut self) {
        self.rom_available_ports.clear();
        match self.list_serial_ports() {
            Ok((all_ports, preferred)) => {
                self.log.push(format!(
                    "Live ROM scan: found {} port(s), showing {} likely serial port(s)",
                    all_ports.len(),
                    preferred.len()
                ));
                self.rom_available_ports = preferred;
            }
            Err(e) => self.log.push(format!("Live ROM port scan error: {e}")),
        }
    }

    fn scan_datalog_ports(&mut self) {
        self.datalog_available_ports.clear();
        match self.list_serial_ports() {
            Ok((all_ports, preferred)) => {
                self.log.push(format!(
                    "Datalog scan: found {} port(s), showing {} likely serial port(s)",
                    all_ports.len(),
                    preferred.len()
                ));
                self.datalog_available_ports = preferred;
            }
            Err(e) => self.log.push(format!("Datalog port scan error: {e}")),
        }
    }

    fn connect_hts(&mut self) {
        if self.datalog_com_port.trim().is_empty() {
            self.log.push("Set Datalog COM/TTY port first".to_string());
            return;
        }

        match serialport::new(&self.datalog_com_port, 38400)
            .timeout(Duration::from_millis(1000))
            .open()
        {
            Ok(mut port) => {
                if let Err(e) = port.write_all(&[0x10]) {
                    self.log.push(format!("HTS write error: {e}"));
                    return;
                }

                let mut reply = [0u8; 1];
                match port.read_exact(&mut reply) {
                    Ok(_) => {
                        self.log.push(format!("HTS reply = 0x{:02X}", reply[0]));
                        if reply[0] == 0xCD {
                            self.datalog_connected = true;
                            self.last_poll = Instant::now();
                            self.log.push("HTS CONNECTED".to_string());
                        } else {
                            self.datalog_connected = false;
                            self.log.push("HTS handshake failed".to_string());
                        }
                    }
                    Err(e) => {
                        self.datalog_connected = false;
                        self.log.push(format!("HTS read error: {e}"));
                    }
                }
            }
            Err(e) => {
                self.datalog_connected = false;
                self.log.push(format!("HTS open error: {e}"));
            }
        }
    }

    fn push_sensor_sample(&mut self) {
        let now_s = self.tps_graph_start.elapsed().as_secs_f64();
        self.graph_history.push_back(GraphSample {
            t: now_s,
            tps: self.tps,
            rpm: self.rpm,
            afr: self.afr,
            map: self.map,
            battery: self.battery,
            inj_ms: self.inj_ms,
            ign: self.ign_advance,
            lambda: self.lambda,
            boost: self.boost_psi,
            vss: self.vss_kmh,
        });

        let cutoff = now_s - TPS_GRAPH_WINDOW_SECS;
        while let Some(sample) = self.graph_history.front() {
            if sample.t < cutoff {
                let _ = self.graph_history.pop_front();
            } else {
                break;
            }
        }
    }

    fn draw_tps_graph(&self, ui: &mut egui::Ui) {
        let trace_item = |ui: &mut egui::Ui, sensor: GraphSensor, value: f64| {
            let unit = sensor.unit();
            let unit_sep = if unit.is_empty() { "" } else { " " };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("●").color(sensor.color()).size(16.0));
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {:.2}{}{}",
                        sensor.label(),
                        value,
                        unit_sep,
                        unit
                    ))
                    .monospace(),
                );
            });
        };

        ui.vertical(|ui| {
            trace_item(ui, self.graph_trace_a, self.graph_sensor_live_value(self.graph_trace_a));
            trace_item(ui, self.graph_trace_b, self.graph_sensor_live_value(self.graph_trace_b));
            trace_item(ui, self.graph_trace_c, self.graph_sensor_live_value(self.graph_trace_c));
        });
        ui.add_space(4.0);

        let desired_size = egui::vec2(ui.available_width().min(520.0), 220.0);
        let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(16, 16, 16));
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(80)),
            egui::StrokeKind::Inside,
        );

        for ratio in [0.25_f32, 0.5_f32, 0.75_f32] {
            let y = egui::lerp(rect.bottom()..=rect.top(), ratio);
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(45)),
            );
        }

        painter.text(
            egui::pos2(rect.left() + 4.0, rect.top() + 4.0),
            egui::Align2::LEFT_TOP,
            "100%",
            egui::TextStyle::Small.resolve(ui.style()),
            egui::Color32::LIGHT_GRAY,
        );
        painter.text(
            egui::pos2(rect.left() + 4.0, rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "0%",
            egui::TextStyle::Small.resolve(ui.style()),
            egui::Color32::LIGHT_GRAY,
        );

        let traces = [self.graph_trace_a, self.graph_trace_b, self.graph_trace_c];
        let last_t = self.graph_history.back().map(|s| s.t).unwrap_or(0.0);
        let min_t = (last_t - TPS_GRAPH_WINDOW_SECS).max(0.0);

        for sensor in traces {
            let (min_v, max_v) = sensor.range();
            let points: Vec<egui::Pos2> = self
                .graph_history
                .iter()
                .filter(|s| s.t >= min_t)
                .map(|s| {
                    let x_ratio = ((s.t - min_t) / TPS_GRAPH_WINDOW_SECS).clamp(0.0, 1.0) as f32;
                    let value = sensor.value_from_sample(s);
                    let y_ratio = ((value - min_v) / (max_v - min_v)).clamp(0.0, 1.0) as f32;
                    let x = egui::lerp(rect.left()..=rect.right(), x_ratio);
                    let y_px = egui::lerp(rect.bottom()..=rect.top(), y_ratio);
                    egui::pos2(x, y_px)
                })
                .collect();

            if points.len() >= 2 {
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(2.0_f32, sensor.color()),
                ));
            }
        }

        painter.text(
            egui::pos2(rect.right() - 4.0, rect.bottom() - 4.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{TPS_GRAPH_WINDOW_SECS:.0}s"),
            egui::TextStyle::Small.resolve(ui.style()),
            egui::Color32::LIGHT_GRAY,
        );
    }

    fn afr_table_indices(&self) -> (usize, usize) {
        let map_ratio = ((self.map - AFR_TABLE_MAP_MIN) / (AFR_TABLE_MAP_MAX - AFR_TABLE_MAP_MIN))
            .clamp(0.0, 1.0);
        let rpm_ratio = ((self.rpm - AFR_TABLE_RPM_MIN) / (AFR_TABLE_RPM_MAX - AFR_TABLE_RPM_MIN))
            .clamp(0.0, 1.0);

        let map_col = (map_ratio * (AFR_TABLE_SIZE as f64 - 1.0)).round() as usize;
        let rpm_row = (rpm_ratio * (AFR_TABLE_SIZE as f64 - 1.0)).round() as usize;
        (rpm_row, map_col)
    }

    fn afr_table_live_cells(&self) -> Vec<(usize, usize)> {
        match self.live_tracking_cell_mode {
            LiveTrackingCellMode::Single => {
                let (row, col) = self.afr_table_indices();
                vec![(row, col)]
            }
            LiveTrackingCellMode::Quad => {
                let map_ratio = ((self.map - AFR_TABLE_MAP_MIN)
                    / (AFR_TABLE_MAP_MAX - AFR_TABLE_MAP_MIN))
                    .clamp(0.0, 1.0);
                let rpm_ratio = ((self.rpm - AFR_TABLE_RPM_MIN)
                    / (AFR_TABLE_RPM_MAX - AFR_TABLE_RPM_MIN))
                    .clamp(0.0, 1.0);

                let map_pos = map_ratio * (AFR_TABLE_SIZE as f64 - 1.0);
                let rpm_pos = rpm_ratio * (AFR_TABLE_SIZE as f64 - 1.0);

                let col_lo = map_pos.floor() as usize;
                let col_hi = map_pos.ceil() as usize;
                let row_lo = rpm_pos.floor() as usize;
                let row_hi = rpm_pos.ceil() as usize;

                let mut cells = vec![
                    (
                        row_lo.clamp(0, AFR_TABLE_SIZE - 1),
                        col_lo.clamp(0, AFR_TABLE_SIZE - 1),
                    ),
                    (
                        row_lo.clamp(0, AFR_TABLE_SIZE - 1),
                        col_hi.clamp(0, AFR_TABLE_SIZE - 1),
                    ),
                    (
                        row_hi.clamp(0, AFR_TABLE_SIZE - 1),
                        col_lo.clamp(0, AFR_TABLE_SIZE - 1),
                    ),
                    (
                        row_hi.clamp(0, AFR_TABLE_SIZE - 1),
                        col_hi.clamp(0, AFR_TABLE_SIZE - 1),
                    ),
                ];

                cells.sort_unstable();
                cells.dedup();
                cells
            }
        }
    }

    fn rom_live_table_indices_1based(&self) -> (usize, usize) {
        let map_ratio = ((self.map - ROM_TRACK_MAP_MIN) / (ROM_TRACK_MAP_MAX - ROM_TRACK_MAP_MIN))
            .clamp(0.0, 1.0);
        let rpm_ratio = ((self.rpm - ROM_TRACK_RPM_MIN) / (ROM_TRACK_RPM_MAX - ROM_TRACK_RPM_MIN))
            .clamp(0.0, 1.0);

        let map_col = (map_ratio * (ROM_FUEL_TABLE_COLS as f64 - 1.0)).round() as usize + 1;
        let rpm_row = (rpm_ratio * (ROM_FUEL_TABLE_ROWS as f64 - 1.0)).round() as usize + 1;
        (
            rpm_row.clamp(1, ROM_FUEL_TABLE_ROWS),
            map_col.clamp(1, ROM_FUEL_TABLE_COLS),
        )
    }

    fn rom_live_table_cells_1based(&self) -> Vec<(usize, usize)> {
        match self.live_tracking_cell_mode {
            LiveTrackingCellMode::Single => {
                let (row, col) = self.rom_live_table_indices_1based();
                vec![(row, col)]
            }
            LiveTrackingCellMode::Quad => {
                let map_ratio = ((self.map - ROM_TRACK_MAP_MIN)
                    / (ROM_TRACK_MAP_MAX - ROM_TRACK_MAP_MIN))
                    .clamp(0.0, 1.0);
                let rpm_ratio = ((self.rpm - ROM_TRACK_RPM_MIN)
                    / (ROM_TRACK_RPM_MAX - ROM_TRACK_RPM_MIN))
                    .clamp(0.0, 1.0);

                let map_pos = map_ratio * (ROM_FUEL_TABLE_COLS as f64 - 1.0);
                let rpm_pos = rpm_ratio * (ROM_FUEL_TABLE_ROWS as f64 - 1.0);

                let col_lo = map_pos.floor() as usize + 1;
                let col_hi = map_pos.ceil() as usize + 1;
                let row_lo = rpm_pos.floor() as usize + 1;
                let row_hi = rpm_pos.ceil() as usize + 1;

                let mut cells = vec![
                    (
                        row_lo.clamp(1, ROM_FUEL_TABLE_ROWS),
                        col_lo.clamp(1, ROM_FUEL_TABLE_COLS),
                    ),
                    (
                        row_lo.clamp(1, ROM_FUEL_TABLE_ROWS),
                        col_hi.clamp(1, ROM_FUEL_TABLE_COLS),
                    ),
                    (
                        row_hi.clamp(1, ROM_FUEL_TABLE_ROWS),
                        col_lo.clamp(1, ROM_FUEL_TABLE_COLS),
                    ),
                    (
                        row_hi.clamp(1, ROM_FUEL_TABLE_ROWS),
                        col_hi.clamp(1, ROM_FUEL_TABLE_COLS),
                    ),
                ];

                cells.sort_unstable();
                cells.dedup();
                cells
            }
        }
    }

    fn reset_live_tracking_afr_map(&mut self) {
        self.live_tracking_sample_count = 0;
        self.live_tracking_afr_value_low = Self::new_empty_rom_tracking_value_table();
        self.live_tracking_afr_sum_low = Self::new_empty_rom_tracking_sum_table();
        self.live_tracking_afr_count_low = Self::new_empty_rom_tracking_count_table();
        self.live_tracking_afr_pending_count_low = Self::new_empty_rom_tracking_count_table();
        self.live_tracking_afr_value_high = Self::new_empty_rom_tracking_value_table();
        self.live_tracking_afr_sum_high = Self::new_empty_rom_tracking_sum_table();
        self.live_tracking_afr_count_high = Self::new_empty_rom_tracking_count_table();
        self.live_tracking_afr_pending_count_high = Self::new_empty_rom_tracking_count_table();
    }

    fn reset_live_tracking_afr_target_map(&mut self) {
        self.live_tracking_afr_target = Self::new_default_rom_tracking_target_table();
    }

    fn apply_live_tracking_afr_target_zone(
        &mut self,
        target: f64,
        row_start_1: usize,
        row_end_1: usize,
        col_start_1: usize,
        col_end_1: usize,
    ) {
        let value = target.clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);
        let row_start = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .min(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let row_end = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .max(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let col_start = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .min(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));
        let col_end = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .max(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));

        for row in row_start..=row_end {
            for col in col_start..=col_end {
                self.live_tracking_afr_target[row - 1][col - 1] = value;
            }
        }
    }

    fn nudge_live_tracking_afr_target_zone(
        &mut self,
        delta: f64,
        row_start_1: usize,
        row_end_1: usize,
        col_start_1: usize,
        col_end_1: usize,
    ) {
        let row_start = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .min(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let row_end = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .max(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let col_start = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .min(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));
        let col_end = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .max(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));

        for row in row_start..=row_end {
            for col in col_start..=col_end {
                let current = self.live_tracking_afr_target[row - 1][col - 1];
                self.live_tracking_afr_target[row - 1][col - 1] =
                    (current + delta).clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);
            }
        }
    }

    fn stack_live_tracking_afr_sample(&mut self) {
        if !self.live_tracking_capture_enabled || !self.afr.is_finite() {
            return;
        }
        let live_cells = self.rom_live_table_cells_1based();
        let capture_kind = self.live_kind_from_vtec();
        let afr = self.afr.clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);
        let (live_tracking_afr_value, live_tracking_afr_sum, live_tracking_afr_pending_count, live_tracking_afr_count) =
            self.live_tracking_afr_batch_tables_mut(capture_kind);

        for (row1, col1) in live_cells {
            let row = row1 - 1;
            let col = col1 - 1;
            live_tracking_afr_sum[row][col] += afr;
            live_tracking_afr_pending_count[row][col] =
                live_tracking_afr_pending_count[row][col].saturating_add(1);
            live_tracking_afr_count[row][col] = live_tracking_afr_count[row][col].saturating_add(1);

            if live_tracking_afr_pending_count[row][col] >= 3 {
                let batch_count = live_tracking_afr_pending_count[row][col] as f64;
                live_tracking_afr_value[row][col] = Some(live_tracking_afr_sum[row][col] / batch_count);
                live_tracking_afr_sum[row][col] = 0.0;
                live_tracking_afr_pending_count[row][col] = 0;
            }
        }
        self.live_tracking_sample_count = self.live_tracking_sample_count.saturating_add(1);
    }

    fn blend_table_cell(cell: &mut Option<f64>, value: f64) {
        *cell = Some(match *cell {
            Some(prev) => (prev * 0.60) + (value * 0.40),
            None => value,
        });
    }

    fn selected_table_value(&self, metric: TableMetric, row: usize, col: usize) -> Option<f64> {
        match (self.sensor_table_kind, metric) {
            (RomFuelTableKind::LowCam, TableMetric::Afr) => self.table_afr_low[row][col],
            (RomFuelTableKind::LowCam, TableMetric::FuelValue) => {
                self.table_fuel_value_low[row][col]
            }
            (RomFuelTableKind::LowCam, TableMetric::InjDuty) => self.table_inj_duty_low[row][col],
            (RomFuelTableKind::LowCam, TableMetric::Ign) => self.table_ign_low[row][col],
            (RomFuelTableKind::HighCam, TableMetric::Afr) => self.table_afr_high[row][col],
            (RomFuelTableKind::HighCam, TableMetric::FuelValue) => {
                self.table_fuel_value_high[row][col]
            }
            (RomFuelTableKind::HighCam, TableMetric::InjDuty) => {
                self.table_inj_duty_high[row][col]
            }
            (RomFuelTableKind::HighCam, TableMetric::Ign) => self.table_ign_high[row][col],
        }
    }

    fn clear_table_metric(&mut self, metric: TableMetric) {
        match (self.sensor_table_kind, metric) {
            (RomFuelTableKind::LowCam, TableMetric::Afr) => self.table_afr_low = Self::new_empty_table(),
            (RomFuelTableKind::LowCam, TableMetric::FuelValue) => {
                self.table_fuel_value_low = Self::new_empty_table()
            }
            (RomFuelTableKind::LowCam, TableMetric::InjDuty) => {
                self.table_inj_duty_low = Self::new_empty_table()
            }
            (RomFuelTableKind::LowCam, TableMetric::Ign) => self.table_ign_low = Self::new_empty_table(),
            (RomFuelTableKind::HighCam, TableMetric::Afr) => {
                self.table_afr_high = Self::new_empty_table()
            }
            (RomFuelTableKind::HighCam, TableMetric::FuelValue) => {
                self.table_fuel_value_high = Self::new_empty_table()
            }
            (RomFuelTableKind::HighCam, TableMetric::InjDuty) => {
                self.table_inj_duty_high = Self::new_empty_table()
            }
            (RomFuelTableKind::HighCam, TableMetric::Ign) => {
                self.table_ign_high = Self::new_empty_table()
            }
        }
    }

    fn table_metric_live_value(&self, metric: TableMetric) -> f64 {
        match metric {
            TableMetric::Afr => self.afr,
            TableMetric::FuelValue => self.inj_fv,
            TableMetric::InjDuty => self.injector_duty,
            TableMetric::Ign => self.ign_advance,
        }
    }

    fn update_table_metrics(&mut self) {
        let (rpm_row, map_col) = self.afr_table_indices();
        let capture_kind = self.live_kind_from_vtec();

        let afr = self.afr.clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);
        let inj_fv = self.inj_fv.clamp(0.0, 600.0);
        let inj_duty = self.injector_duty.clamp(0.0, 120.0);
        let ign = self.ign_advance.clamp(-10.0, 60.0);

        match capture_kind {
            RomFuelTableKind::LowCam => {
                Self::blend_table_cell(&mut self.table_afr_low[rpm_row][map_col], afr);
                Self::blend_table_cell(&mut self.table_fuel_value_low[rpm_row][map_col], inj_fv);
                Self::blend_table_cell(&mut self.table_inj_duty_low[rpm_row][map_col], inj_duty);
                Self::blend_table_cell(&mut self.table_ign_low[rpm_row][map_col], ign);
            }
            RomFuelTableKind::HighCam => {
                Self::blend_table_cell(&mut self.table_afr_high[rpm_row][map_col], afr);
                Self::blend_table_cell(&mut self.table_fuel_value_high[rpm_row][map_col], inj_fv);
                Self::blend_table_cell(&mut self.table_inj_duty_high[rpm_row][map_col], inj_duty);
                Self::blend_table_cell(&mut self.table_ign_high[rpm_row][map_col], ign);
            }
        }
    }

    fn table_value_color(metric: TableMetric, value: Option<f64>) -> egui::Color32 {
        let Some(v) = value else {
            return egui::Color32::from_rgb(26, 26, 26);
        };

        if metric == TableMetric::Afr {
            return Self::afr_value_color(v);
        }

        let (min_v, max_v) = metric.range();
        let t = ((v - min_v) / (max_v - min_v)).clamp(0.0, 1.0);
        let r = (40.0 + 200.0 * t) as u8;
        let g = (190.0 - 120.0 * (t - 0.5).abs() * 2.0).clamp(0.0, 255.0) as u8;
        let b = (220.0 - 170.0 * t) as u8;
        egui::Color32::from_rgb(r, g, b)
    }

    fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
        let tt = t.clamp(0.0, 1.0);
        ((a as f64) + ((b as f64) - (a as f64)) * tt).round() as u8
    }

    fn afr_value_color(value: f64) -> egui::Color32 {
        let afr = value.clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);

        let anchors: &[(f64, (u8, u8, u8))] = &[
            (10.0, (10, 35, 150)),
            (11.0, (10, 35, 150)),
            (12.0, (40, 110, 255)),
            (13.0, (0, 180, 220)),
            (13.5, (0, 210, 90)),
            (13.8, (120, 220, 40)),
            (14.0, (255, 230, 0)),
            (14.7, (255, 145, 0)),
            (15.0, (255, 145, 0)),
            (15.5, (255, 95, 0)),
            (16.0, (235, 20, 20)),
            (20.0, (235, 20, 20)),
        ];

        if afr <= anchors[0].0 {
            let (r, g, b) = anchors[0].1;
            return egui::Color32::from_rgb(r, g, b);
        }

        for window in anchors.windows(2) {
            let (start_v, start_c) = window[0];
            let (end_v, end_c) = window[1];
            if afr <= end_v {
                let span = (end_v - start_v).max(f64::EPSILON);
                let t = (afr - start_v) / span;
                return egui::Color32::from_rgb(
                    Self::lerp_u8(start_c.0, end_c.0, t),
                    Self::lerp_u8(start_c.1, end_c.1, t),
                    Self::lerp_u8(start_c.2, end_c.2, t),
                );
            }
        }

        let (r, g, b) = anchors[anchors.len() - 1].1;
        egui::Color32::from_rgb(r, g, b)
    }

    fn table_value_ratio(metric: TableMetric, value: Option<f64>) -> f32 {
        let Some(v) = value else {
            return 0.0;
        };

        let (min_v, max_v) = metric.range();
        ((v - min_v) / (max_v - min_v)).clamp(0.0, 1.0) as f32
    }

    fn scale_color(color: egui::Color32, factor: f32) -> egui::Color32 {
        let f = factor.clamp(0.0, 2.0);
        let r = ((color.r() as f32) * f).clamp(0.0, 255.0) as u8;
        let g = ((color.g() as f32) * f).clamp(0.0, 255.0) as u8;
        let b = ((color.b() as f32) * f).clamp(0.0, 255.0) as u8;
        egui::Color32::from_rgb(r, g, b)
    }

    fn read_rom_file_for_table_view(&mut self) -> Option<Vec<u8>> {
        if let Some(rom) = &self.last_read_rom {
            return Some(rom.clone());
        }

        if self.selected_file_is_usable() {
            match fs::read(&self.selected_file) {
                Ok(rom) => return Some(rom),
                Err(e) => {
                    self.log.push(format!("FUEL TABLE VIEW error: {e}"));
                    return None;
                }
            }
        }

        self.log
            .push("FUEL TABLE VIEW: load a BIN or read ROM data first".to_string());
        None
    }

    fn rom_fuel_table_values(
        rom: &[u8],
        kind: RomFuelTableKind,
        multipliers: &[f64; ROM_FUEL_TABLE_COLS],
    ) -> Option<Vec<Vec<f64>>> {
        let base = kind.offset();
        let last_index = base + ((ROM_FUEL_TABLE_ROWS - 1) * ROM_FUEL_TABLE_ROW_STRIDE) + (ROM_FUEL_TABLE_COLS - 1);
        if rom.len() <= last_index {
            return None;
        }

        let mut out = vec![vec![0.0; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS];
        for (row_idx, row) in out.iter_mut().enumerate() {
            for (col_idx, value) in row.iter_mut().enumerate() {
                let raw = rom[base + row_idx * ROM_FUEL_TABLE_ROW_STRIDE + col_idx] as f64;
                *value = raw * (multipliers[col_idx] / 4.0);
            }
        }
        Some(out)
    }

    fn table_value_color_from_range(value: f64, min_v: f64, max_v: f64, use_afr_scale: bool) -> egui::Color32 {
        if use_afr_scale {
            return Self::afr_value_color(value);
        }

        let span = (max_v - min_v).max(1.0);
        let t = ((value - min_v) / span).clamp(0.0, 1.0);
        let r = (40.0 + 200.0 * t) as u8;
        let g = (190.0 - 120.0 * (t - 0.5).abs() * 2.0).clamp(0.0, 255.0) as u8;
        let b = (220.0 - 170.0 * t) as u8;
        egui::Color32::from_rgb(r, g, b)
    }

    fn rom_ign_table_values(rom: &[u8], kind: RomFuelTableKind) -> Option<Vec<Vec<f64>>> {
        let base = kind.ign_offset();
        let last_index =
            base + ((ROM_FUEL_TABLE_ROWS - 1) * ROM_FUEL_TABLE_ROW_STRIDE) + (ROM_FUEL_TABLE_COLS - 1);
        if rom.len() <= last_index {
            return None;
        }

        let mut out = vec![vec![0.0; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS];
        for (row_idx, row) in out.iter_mut().enumerate() {
            for (col_idx, value) in row.iter_mut().enumerate() {
                let raw = rom[base + row_idx * ROM_FUEL_TABLE_ROW_STRIDE + col_idx] as f64;
                *value = (raw - 24.0) / 4.0;
            }
        }
        Some(out)
    }

    fn apply_rom_fuel_table_percent_zone(
        &mut self,
        kind: RomFuelTableKind,
        pct: i32,
        row_start_1: usize,
        row_end_1: usize,
        col_start_1: usize,
        col_end_1: usize,
    ) {
        if self.last_read_rom.is_none() {
            if self.selected_file_is_usable() {
                self.read_selected_bin_file(false);
            }
        }

        let Some(rom) = self.last_read_rom.as_mut() else {
            self.log.push("FUEL TABLE %: load BIN or read ROM first".to_string());
            return;
        };

        let row_start = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .min(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let row_end = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .max(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let col_start = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .min(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));
        let col_end = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .max(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));

        let base = kind.offset();
        let last_index =
            base + ((ROM_FUEL_TABLE_ROWS - 1) * ROM_FUEL_TABLE_ROW_STRIDE) + (ROM_FUEL_TABLE_COLS - 1);
        if rom.len() <= last_index {
            self.log.push("FUEL TABLE %: ROM too small for table".to_string());
            return;
        }

        let factor = 1.0 + (pct as f64 / 100.0);
        let mut changed = 0usize;
        for row in row_start..=row_end {
            for col in col_start..=col_end {
                let idx = base + (row - 1) * ROM_FUEL_TABLE_ROW_STRIDE + (col - 1);
                let old = rom[idx];
                let new_val = ((old as f64) * factor).round().clamp(0.0, 255.0) as u8;
                if new_val != old {
                    rom[idx] = new_val;
                    changed += 1;
                }
            }
        }

        self.rom_has_unsaved_changes = true;
        let rom_snapshot = rom.clone();
        self.rom_hex_dump = Self::format_hex_dump(&rom_snapshot);
        self.refresh_tuning_values_from_rom(&rom_snapshot);
        self.log.push(format!(
            "FUEL TABLE {} {:+}% | R{}-R{} C{}-C{} | changed {} cell(s)",
            kind.label(),
            pct,
            row_start,
            row_end,
            col_start,
            col_end,
            changed
        ));
    }

    fn draw_rom_table_window(&mut self, ctx: &egui::Context) {
        if !self.show_rom_table_window {
            return;
        }

        let kind = self.rom_table_kind;
        let is_low_cam = kind == RomFuelTableKind::LowCam;

        let rom = match self.read_rom_file_for_table_view() {
            Some(rom) => rom,
            None => {
                self.show_rom_table_window = false;
                return;
            }
        };

        let values = match Self::rom_fuel_table_values(&rom, kind, &self.rom_table_column_multipliers) {
            Some(values) => values,
            None => {
                self.log.push("FUEL TABLE VIEW: ROM too small for selected table".to_string());
                self.show_rom_table_window = false;
                return;
            }
        };

        let mut open = self.show_rom_table_window;
        let (mut pan, mut scale, mut yaw, mut pitch) = if is_low_cam {
            (
                self.rom_table_3d_pan,
                self.rom_table_3d_scale,
                self.rom_table_3d_yaw,
                self.rom_table_3d_pitch,
            )
        } else {
            (
                self.rom_table_high_3d_pan,
                self.rom_table_high_3d_scale,
                self.rom_table_high_3d_yaw,
                self.rom_table_high_3d_pitch,
            )
        };
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for row in &values {
            for &v in row {
                min_v = min_v.min(v);
                max_v = max_v.max(v);
            }
        }

        egui::Window::new("FUEL TABLE")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .default_size(egui::vec2(1220.0, 860.0))
            .default_pos(egui::pos2(30.0, 110.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                        if ui
                            .selectable_label(self.rom_table_kind == table_kind, table_kind.label())
                            .clicked()
                        {
                            self.rom_table_kind = table_kind;
                        }
                    }

                    ui.separator();

                    for view_mode in [RomTableViewMode::TwoD, RomTableViewMode::ThreeD] {
                        if ui
                            .selectable_label(
                                self.rom_table_view_mode == view_mode,
                                view_mode.label(),
                            )
                            .clicked()
                        {
                            self.rom_table_view_mode = view_mode;
                        }
                    }

                    ui.separator();

                    if ui.button("RESET VIEW").clicked() {
                        pan = egui::vec2(0.0, 0.0);
                        scale = 1.0;
                        yaw = -0.75;
                        pitch = 0.70;
                    }

                    ui.label("ZOOM");
                    ui.add(
                        egui::Slider::new(&mut scale, 0.6..=1.8)
                            .show_value(false)
                            .min_decimals(2),
                    );
                });

                // ui.label("Column multipliers");
                // ui.horizontal_wrapped(|ui| {
                //     for (idx, mult) in self.rom_table_column_multipliers.iter_mut().enumerate() {
                //         ui.vertical(|ui| {
                //             ui.label(format!("C{}", idx + 1));
                //             ui.add(
                //                 egui::DragValue::new(mult)
                //                     .range(0.0..=64.0)
                //                     .speed(0.1),
                //             );
                //         });
                //     }
                // });

                ui.separator();
                // ui.label("Fuel zone adjustment (%) on raw ROM bytes");
                // ui.horizontal_wrapped(|ui| {
                //     ui.label("Rows");
                //     ui.add(
                //         egui::DragValue::new(&mut self.rom_table_zone_row_start)
                //             .range(1..=ROM_FUEL_TABLE_ROWS)
                //             .speed(1),
                //     );
                //     ui.label("to");
                //     ui.add(
                //         egui::DragValue::new(&mut self.rom_table_zone_row_end)
                //             .range(1..=ROM_FUEL_TABLE_ROWS)
                //             .speed(1),
                //     );
                //     ui.separator();
                //     ui.label("Cols");
                //     ui.add(
                //         egui::DragValue::new(&mut self.rom_table_zone_col_start)
                //             .range(1..=ROM_FUEL_TABLE_COLS)
                //             .speed(1),
                //     );
                //     ui.label("to");
                //     ui.add(
                //         egui::DragValue::new(&mut self.rom_table_zone_col_end)
                //             .range(1..=ROM_FUEL_TABLE_COLS)
                //             .speed(1),
                //     );
                //     if ui.button("FULL TABLE").clicked() {
                //         self.rom_table_zone_row_start = 1;
                //         self.rom_table_zone_row_end = ROM_FUEL_TABLE_ROWS;
                //         self.rom_table_zone_col_start = 1;
                //         self.rom_table_zone_col_end = ROM_FUEL_TABLE_COLS;
                //     }
                // });

                ui.horizontal_wrapped(|ui| {
                    if ui.button("FULL TABLE SELECT").clicked() {
                        self.rom_table_zone_row_start = 1;
                        self.rom_table_zone_row_end = ROM_FUEL_TABLE_ROWS;
                        self.rom_table_zone_col_start = 1;
                        self.rom_table_zone_col_end = ROM_FUEL_TABLE_COLS;
                    }
                    for pct in [-10, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 10] {
                        if ui.button(format!("{:+}%", pct)).clicked() {
                            self.apply_rom_fuel_table_percent_zone(
                                self.rom_table_kind,
                                pct,
                                self.rom_table_zone_row_start,
                                self.rom_table_zone_row_end,
                                self.rom_table_zone_col_start,
                                self.rom_table_zone_col_end,
                            );
                        }
                    }
                });

                ui.add_space(6.0);

                match self.rom_table_view_mode {
                    RomTableViewMode::ThreeD => {
                        let row_lo = self
                            .rom_table_zone_row_start
                            .min(self.rom_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let row_hi = self
                            .rom_table_zone_row_start
                            .max(self.rom_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let col_lo = self
                            .rom_table_zone_col_start
                            .min(self.rom_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let col_hi = self
                            .rom_table_zone_col_start
                            .max(self.rom_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let draw_scale = scale;
                        let desired = egui::vec2(1100.0, 620.0);
                        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::drag());

                        if response.hovered() && ui.input(|i| i.pointer.secondary_down()) {
                            let delta = ui.input(|i| i.pointer.delta());
                            yaw += delta.x * 0.01;
                            pitch = (pitch - delta.y * 0.01).clamp(0.2, 1.3);
                        }

                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 14, 14));
                        painter.rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
                            egui::StrokeKind::Inside,
                        );

                        let rows = ROM_FUEL_TABLE_ROWS;
                        let cols = ROM_FUEL_TABLE_COLS;
                        let half_x = cols as f32 * 0.5;
                        let half_y = rows as f32 * 0.5;
                        let unit = 1.0_f32;
                        let gap = 0.08_f32;
                        let z_height_max = 4.6_f32;
                        let fov = 420.0_f32 * draw_scale;
                        let cam_dist = 24.0_f32;
                        let center = egui::pos2(
                            rect.left() + rect.width() * 0.5 + pan.x,
                            rect.top() + rect.height() * 0.57 + pan.y,
                        );

                        let cy = yaw.cos();
                        let sy = yaw.sin();
                        let cp = pitch.cos();
                        let sp = pitch.sin();

                        let rotate = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
                            let xr = x * cy - y * sy;
                            let yr = x * sy + y * cy;
                            let yr2 = yr * cp - z * sp;
                            let zr2 = yr * sp + z * cp;
                            (xr, yr2, zr2)
                        };

                        let project = |x: f32, y: f32, z: f32| -> (egui::Pos2, f32) {
                            let (xr, yr, zr) = rotate(x, y, z);
                            let depth = (cam_dist + yr).max(1.0);
                            let px = center.x + (xr * fov / depth);
                            let py = center.y - (zr * fov / depth);
                            (egui::pos2(px, py), depth)
                        };

                        struct Face {
                            points: Vec<egui::Pos2>,
                            color: egui::Color32,
                            stroke: egui::Stroke,
                            depth: f32,
                        }

                        let mut faces: Vec<Face> = Vec::with_capacity(rows * cols * 3);
                        for row in (0..rows).rev() {
                            for col in 0..cols {
                                let r1 = row + 1;
                                let c1 = col + 1;
                                let in_selected_zone =
                                    r1 >= row_lo && r1 <= row_hi && c1 >= col_lo && c1 <= col_hi;
                                let value = values[row][col];
                                let ratio = ((value - min_v) / (max_v - min_v).max(1.0)).clamp(0.0, 1.0) as f32;
                                let z = ratio * z_height_max;

                                let x0 = (col as f32 - half_x) * (unit + gap);
                                let x1 = x0 + unit;
                                let y0 = (half_y - row as f32) * (unit + gap);
                                let y1 = y0 - unit;

                                let (b0, d0) = project(x0, y0, 0.0);
                                let (b1, d1) = project(x1, y0, 0.0);
                                let (b2, d2) = project(x1, y1, 0.0);

                                let (t0, dt0) = project(x0, y0, z);
                                let (t1, dt1) = project(x1, y0, z);
                                let (t2, dt2) = project(x1, y1, z);
                                let (t3, dt3) = project(x0, y1, z);

                                let mut top_color = Self::table_value_color_from_range(value, min_v, max_v, false);
                                if in_selected_zone {
                                    top_color = Self::scale_color(top_color, 1.25);
                                }
                                let side_color_a = Self::scale_color(top_color, 0.72);
                                let side_color_b = Self::scale_color(top_color, 0.58);

                                faces.push(Face {
                                    points: vec![b1, b2, t2, t1],
                                    color: side_color_a,
                                    stroke: egui::Stroke::NONE,
                                    depth: (d1 + d2 + dt2 + dt1) * 0.25,
                                });
                                faces.push(Face {
                                    points: vec![b0, b1, t1, t0],
                                    color: side_color_b,
                                    stroke: egui::Stroke::NONE,
                                    depth: (d0 + d1 + dt1 + dt0) * 0.25,
                                });
                                faces.push(Face {
                                    points: vec![t0, t1, t2, t3],
                                    color: top_color,
                                    stroke: if in_selected_zone {
                                        egui::Stroke::new(1.7_f32, egui::Color32::from_rgb(250, 220, 70))
                                    } else {
                                        egui::Stroke::new(0.8_f32, egui::Color32::from_gray(40))
                                    },
                                    depth: (dt0 + dt1 + dt2 + dt3) * 0.25,
                                });
                            }
                        }

                        faces.sort_by(|a, b| {
                            b.depth
                                .partial_cmp(&a.depth)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });

                        for face in faces {
                            painter.add(egui::Shape::convex_polygon(face.points, face.color, face.stroke));
                        }

                        for idx in [0_usize, 3, 6, 9] {
                            let xw = (idx as f32 - half_x) * (unit + gap);
                            let (x_proj, _) = project(xw, -(half_y + 2.0), 0.0);
                            painter.text(
                                egui::pos2(x_proj.x, rect.bottom() - 18.0),
                                egui::Align2::CENTER_TOP,
                                format!("C{}", idx + 1),
                                egui::TextStyle::Small.resolve(ui.style()),
                                egui::Color32::LIGHT_GRAY,
                            );
                        }

                        for idx in [0_usize, 5, 10, 15, 19] {
                            let yw = (half_y - idx as f32) * (unit + gap);
                            let (y_proj, _) = project(-(half_x + 2.0), yw, 0.0);
                            painter.text(
                                egui::pos2(rect.left() + 20.0, y_proj.y),
                                egui::Align2::RIGHT_CENTER,
                                format!("R{}", idx + 1),
                                egui::TextStyle::Small.resolve(ui.style()),
                                egui::Color32::LIGHT_GRAY,
                            );
                        }

                        painter.text(
                            egui::pos2(rect.center().x, rect.bottom() - 2.0),
                            egui::Align2::CENTER_TOP,
                            "Columns",
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            egui::pos2(rect.left() + 6.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "Rows",
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            egui::pos2(rect.right() - 8.0, rect.top() + 6.0),
                            egui::Align2::LEFT_BOTTOM,
                            "Z: Fuel Value",
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::WHITE,
                        );
                    }
                    RomTableViewMode::TwoD => {
                        let row_lo = self
                            .rom_table_zone_row_start
                            .min(self.rom_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let row_hi = self
                            .rom_table_zone_row_start
                            .max(self.rom_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let col_lo = self
                            .rom_table_zone_col_start
                            .min(self.rom_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let col_hi = self
                            .rom_table_zone_col_start
                            .max(self.rom_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);

                        let mut cell_rects: Vec<(usize, usize, egui::Rect)> =
                            Vec::with_capacity(ROM_FUEL_TABLE_ROWS * ROM_FUEL_TABLE_COLS);

                        egui::Grid::new(format!("rom_table_grid_{}", kind.label()))
                            .num_columns(ROM_FUEL_TABLE_COLS + 1)
                            .spacing([4.0, 4.0])
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [40.0, 22.0],
                                    egui::Label::new(egui::RichText::new("ROW").monospace()),
                                );

                                for col in 0..ROM_FUEL_TABLE_COLS {
                                    ui.add_sized(
                                        [54.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("C{}", col + 1)).monospace(),
                                        ),
                                    );
                                }
                                ui.end_row();

                                for (row_idx, row) in values.iter().enumerate() {
                                    let r1 = row_idx + 1;
                                    ui.add_sized(
                                        [40.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("R{}", r1)).monospace(),
                                        ),
                                    );

                                    for (col_idx, &value) in row.iter().enumerate() {
                                        let c1 = col_idx + 1;
                                        let in_selected_zone =
                                            r1 >= row_lo && r1 <= row_hi && c1 >= col_lo && c1 <= col_hi;
                                        let fill = Self::table_value_color_from_range(value, min_v, max_v, false);
                                        let cell_response = egui::Frame::new()
                                            .fill(fill)
                                            .stroke(if in_selected_zone {
                                                egui::Stroke::new(
                                                    1.5_f32,
                                                    egui::Color32::from_rgb(250, 220, 70),
                                                )
                                            } else {
                                                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(55))
                                            })
                                            .corner_radius(egui::CornerRadius::same(3))
                                            .inner_margin(egui::Margin::symmetric(3, 2))
                                            .show(ui, |ui| {
                                                ui.add_sized(
                                                    [50.0, 18.0],
                                                    egui::Label::new(
                                                        egui::RichText::new(format!("{:>5.1}", value))
                                                            .monospace()
                                                            .strong()
                                                            .color(egui::Color32::BLACK),
                                                    ),
                                                );
                                            })
                                            .response;

                                        let response = ui.interact(
                                            cell_response.rect,
                                            ui.id().with(("rom_cell", kind.label(), r1, c1)),
                                            egui::Sense::click_and_drag(),
                                        );
                                        cell_rects.push((r1, c1, response.rect));

                                        if response.clicked() || response.drag_started() {
                                            self.rom_table_drag_anchor = Some((r1, c1));
                                            self.rom_table_zone_row_start = r1;
                                            self.rom_table_zone_row_end = r1;
                                            self.rom_table_zone_col_start = c1;
                                            self.rom_table_zone_col_end = c1;
                                        }
                                    }
                                    ui.end_row();
                                }
                            });

                        if ui.input(|i| i.pointer.primary_down()) {
                            if let Some((anchor_r, anchor_c)) = self.rom_table_drag_anchor {
                                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                                    if let Some((r, c, _)) = cell_rects.iter().find(|(_, _, rect)| rect.contains(pos)) {
                                        self.rom_table_zone_row_start = anchor_r.min(*r);
                                        self.rom_table_zone_row_end = anchor_r.max(*r);
                                        self.rom_table_zone_col_start = anchor_c.min(*c);
                                        self.rom_table_zone_col_end = anchor_c.max(*c);
                                    }
                                }
                            }
                        }

                        if !ui.input(|i| i.pointer.primary_down()) {
                            self.rom_table_drag_anchor = None;
                        }
                    }
                }
            });

        self.show_rom_table_window = open;
        if is_low_cam {
            self.rom_table_3d_pan = pan;
            self.rom_table_3d_scale = scale;
            self.rom_table_3d_yaw = yaw;
            self.rom_table_3d_pitch = pitch;
        } else {
            self.rom_table_high_3d_pan = pan;
            self.rom_table_high_3d_scale = scale;
            self.rom_table_high_3d_yaw = yaw;
            self.rom_table_high_3d_pitch = pitch;
        }
    }

    fn apply_rom_ign_table_percent_zone(
        &mut self,
        kind: RomFuelTableKind,
        pct: i32,
        row_start_1: usize,
        row_end_1: usize,
        col_start_1: usize,
        col_end_1: usize,
    ) {
        if self.last_read_rom.is_none() {
            if self.selected_file_is_usable() {
                self.read_selected_bin_file(false);
            }
        }

        let Some(rom) = self.last_read_rom.as_mut() else {
            self.log.push("ROM IGN TABLE %: load BIN or read ROM first".to_string());
            return;
        };

        let row_start = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .min(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let row_end = row_start_1
            .clamp(1, ROM_FUEL_TABLE_ROWS)
            .max(row_end_1.clamp(1, ROM_FUEL_TABLE_ROWS));
        let col_start = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .min(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));
        let col_end = col_start_1
            .clamp(1, ROM_FUEL_TABLE_COLS)
            .max(col_end_1.clamp(1, ROM_FUEL_TABLE_COLS));

        let base = kind.ign_offset();
        let last_index =
            base + ((ROM_FUEL_TABLE_ROWS - 1) * ROM_FUEL_TABLE_ROW_STRIDE) + (ROM_FUEL_TABLE_COLS - 1);
        if rom.len() <= last_index {
            self.log.push("ROM IGN TABLE %: ROM too small for table".to_string());
            return;
        }

        let factor = 1.0 + (pct as f64 / 100.0);
        let mut changed = 0usize;
        for row in row_start..=row_end {
            for col in col_start..=col_end {
                let idx = base + (row - 1) * ROM_FUEL_TABLE_ROW_STRIDE + (col - 1);
                let old = rom[idx];
                let new_val = ((old as f64) * factor).round().clamp(0.0, 255.0) as u8;
                if new_val != old {
                    rom[idx] = new_val;
                    changed += 1;
                }
            }
        }

        self.rom_has_unsaved_changes = true;
        let rom_snapshot = rom.clone();
        self.rom_hex_dump = Self::format_hex_dump(&rom_snapshot);
        self.refresh_tuning_values_from_rom(&rom_snapshot);
        self.log.push(format!(
            "ROM IGN TABLE {} {:+}% | R{}-R{} C{}-C{} | changed {} cell(s)",
            kind.label(),
            pct,
            row_start,
            row_end,
            col_start,
            col_end,
            changed
        ));
    }

    fn draw_rom_ign_table_window(&mut self, ctx: &egui::Context) {
        if !self.show_rom_ign_table_window {
            return;
        }

        let kind = self.rom_ign_table_kind;
        let is_low_cam = kind == RomFuelTableKind::LowCam;

        let rom = match self.read_rom_file_for_table_view() {
            Some(rom) => rom,
            None => {
                self.show_rom_ign_table_window = false;
                return;
            }
        };

        let values = match Self::rom_ign_table_values(&rom, kind) {
            Some(values) => values,
            None => {
                self.log
                    .push("ROM IGN TABLE VIEW: ROM too small for selected table".to_string());
                self.show_rom_ign_table_window = false;
                return;
            }
        };

        let mut open = self.show_rom_ign_table_window;
        let (mut pan, mut scale, mut yaw, mut pitch) = if is_low_cam {
            (
                self.rom_ign_table_3d_pan,
                self.rom_ign_table_3d_scale,
                self.rom_ign_table_3d_yaw,
                self.rom_ign_table_3d_pitch,
            )
        } else {
            (
                self.rom_ign_table_high_3d_pan,
                self.rom_ign_table_high_3d_scale,
                self.rom_ign_table_high_3d_yaw,
                self.rom_ign_table_high_3d_pitch,
            )
        };
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for row in &values {
            for &v in row {
                min_v = min_v.min(v);
                max_v = max_v.max(v);
            }
        }

        egui::Window::new("IGNITION TABLE")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .default_size(egui::vec2(1220.0, 860.0))
            .default_pos(egui::pos2(60.0, 130.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                        if ui
                            .selectable_label(self.rom_ign_table_kind == table_kind, table_kind.label())
                            .clicked()
                        {
                            self.rom_ign_table_kind = table_kind;
                        }
                    }

                    ui.separator();

                    for view_mode in [RomTableViewMode::TwoD, RomTableViewMode::ThreeD] {
                        if ui
                            .selectable_label(self.rom_ign_table_view_mode == view_mode, view_mode.label())
                            .clicked()
                        {
                            self.rom_ign_table_view_mode = view_mode;
                        }
                    }

                    ui.separator();

                    if ui.button("RESET VIEW").clicked() {
                        pan = egui::vec2(0.0, 0.0);
                        scale = 1.0;
                        yaw = -0.75;
                        pitch = 0.70;
                    }

                    ui.label("ZOOM");
                    ui.add(
                        egui::Slider::new(&mut scale, 0.6..=1.8)
                            .show_value(false)
                            .min_decimals(2),
                    );
                });

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("FULL TABLE SELECT").clicked() {
                        self.rom_ign_table_zone_row_start = 1;
                        self.rom_ign_table_zone_row_end = ROM_FUEL_TABLE_ROWS;
                        self.rom_ign_table_zone_col_start = 1;
                        self.rom_ign_table_zone_col_end = ROM_FUEL_TABLE_COLS;
                    }
                    for pct in [-10, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 10] {
                        if ui.button(format!("{:+}%", pct)).clicked() {
                            self.apply_rom_ign_table_percent_zone(
                                self.rom_ign_table_kind,
                                pct,
                                self.rom_ign_table_zone_row_start,
                                self.rom_ign_table_zone_row_end,
                                self.rom_ign_table_zone_col_start,
                                self.rom_ign_table_zone_col_end,
                            );
                        }
                    }
                });

                ui.add_space(6.0);

                match self.rom_ign_table_view_mode {
                    RomTableViewMode::ThreeD => {
                        let row_lo = self
                            .rom_ign_table_zone_row_start
                            .min(self.rom_ign_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let row_hi = self
                            .rom_ign_table_zone_row_start
                            .max(self.rom_ign_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let col_lo = self
                            .rom_ign_table_zone_col_start
                            .min(self.rom_ign_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let col_hi = self
                            .rom_ign_table_zone_col_start
                            .max(self.rom_ign_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let draw_scale = scale;
                        let desired = egui::vec2(1100.0, 620.0);
                        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::drag());

                        if response.hovered() && ui.input(|i| i.pointer.secondary_down()) {
                            let delta = ui.input(|i| i.pointer.delta());
                            yaw += delta.x * 0.01;
                            pitch = (pitch - delta.y * 0.01).clamp(0.2, 1.3);
                        }

                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 14, 14));
                        painter.rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
                            egui::StrokeKind::Inside,
                        );

                        let rows = ROM_FUEL_TABLE_ROWS;
                        let cols = ROM_FUEL_TABLE_COLS;
                        let half_x = cols as f32 * 0.5;
                        let half_y = rows as f32 * 0.5;
                        let unit = 1.0_f32;
                        let gap = 0.08_f32;
                        let z_height_max = 4.6_f32;
                        let fov = 420.0_f32 * draw_scale;
                        let cam_dist = 24.0_f32;
                        let center = egui::pos2(
                            rect.left() + rect.width() * 0.5 + pan.x,
                            rect.top() + rect.height() * 0.57 + pan.y,
                        );

                        let cy = yaw.cos();
                        let sy = yaw.sin();
                        let cp = pitch.cos();
                        let sp = pitch.sin();

                        let rotate = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
                            let xr = x * cy - y * sy;
                            let yr = x * sy + y * cy;
                            let yr2 = yr * cp - z * sp;
                            let zr2 = yr * sp + z * cp;
                            (xr, yr2, zr2)
                        };

                        let project = |x: f32, y: f32, z: f32| -> (egui::Pos2, f32) {
                            let (xr, yr, zr) = rotate(x, y, z);
                            let depth = (cam_dist + yr).max(1.0);
                            let px = center.x + (xr * fov / depth);
                            let py = center.y - (zr * fov / depth);
                            (egui::pos2(px, py), depth)
                        };

                        struct Face {
                            points: Vec<egui::Pos2>,
                            color: egui::Color32,
                            stroke: egui::Stroke,
                            depth: f32,
                        }

                        let mut faces: Vec<Face> = Vec::with_capacity(rows * cols * 3);
                        for row in (0..rows).rev() {
                            for col in 0..cols {
                                let r1 = row + 1;
                                let c1 = col + 1;
                                let in_selected_zone =
                                    r1 >= row_lo && r1 <= row_hi && c1 >= col_lo && c1 <= col_hi;
                                let value = values[row][col];
                                let ratio = ((value - min_v) / (max_v - min_v).max(1.0)).clamp(0.0, 1.0) as f32;
                                let z = ratio * z_height_max;

                                let x0 = (col as f32 - half_x) * (unit + gap);
                                let x1 = x0 + unit;
                                let y0 = (half_y - row as f32) * (unit + gap);
                                let y1 = y0 - unit;

                                let (b0, d0) = project(x0, y0, 0.0);
                                let (b1, d1) = project(x1, y0, 0.0);
                                let (b2, d2) = project(x1, y1, 0.0);

                                let (t0, dt0) = project(x0, y0, z);
                                let (t1, dt1) = project(x1, y0, z);
                                let (t2, dt2) = project(x1, y1, z);
                                let (t3, dt3) = project(x0, y1, z);

                                let mut top_color = Self::table_value_color_from_range(value, min_v, max_v, false);
                                if in_selected_zone {
                                    top_color = Self::scale_color(top_color, 1.25);
                                }
                                let side_color_a = Self::scale_color(top_color, 0.72);
                                let side_color_b = Self::scale_color(top_color, 0.58);

                                faces.push(Face {
                                    points: vec![b1, b2, t2, t1],
                                    color: side_color_a,
                                    stroke: egui::Stroke::NONE,
                                    depth: (d1 + d2 + dt2 + dt1) * 0.25,
                                });
                                faces.push(Face {
                                    points: vec![b0, b1, t1, t0],
                                    color: side_color_b,
                                    stroke: egui::Stroke::NONE,
                                    depth: (d0 + d1 + dt1 + dt0) * 0.25,
                                });
                                faces.push(Face {
                                    points: vec![t0, t1, t2, t3],
                                    color: top_color,
                                    stroke: if in_selected_zone {
                                        egui::Stroke::new(1.7_f32, egui::Color32::from_rgb(250, 220, 70))
                                    } else {
                                        egui::Stroke::new(0.8_f32, egui::Color32::from_gray(40))
                                    },
                                    depth: (dt0 + dt1 + dt2 + dt3) * 0.25,
                                });
                            }
                        }

                        faces.sort_by(|a, b| {
                            b.depth
                                .partial_cmp(&a.depth)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });

                        for face in faces {
                            painter.add(egui::Shape::convex_polygon(face.points, face.color, face.stroke));
                        }

                        for idx in [0_usize, 3, 6, 9] {
                            let xw = (idx as f32 - half_x) * (unit + gap);
                            let (x_proj, _) = project(xw, -(half_y + 2.0), 0.0);
                            painter.text(
                                egui::pos2(x_proj.x, rect.bottom() - 18.0),
                                egui::Align2::CENTER_TOP,
                                format!("C{}", idx + 1),
                                egui::TextStyle::Small.resolve(ui.style()),
                                egui::Color32::LIGHT_GRAY,
                            );
                        }

                        for idx in [0_usize, 5, 10, 15, 19] {
                            let yw = (half_y - idx as f32) * (unit + gap);
                            let (y_proj, _) = project(-(half_x + 2.0), yw, 0.0);
                            painter.text(
                                egui::pos2(rect.left() + 20.0, y_proj.y),
                                egui::Align2::RIGHT_CENTER,
                                format!("R{}", idx + 1),
                                egui::TextStyle::Small.resolve(ui.style()),
                                egui::Color32::LIGHT_GRAY,
                            );
                        }

                        painter.text(
                            egui::pos2(rect.center().x, rect.bottom() - 2.0),
                            egui::Align2::CENTER_TOP,
                            "Columns",
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            egui::pos2(rect.left() + 6.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "Rows",
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            egui::pos2(rect.right() - 8.0, rect.top() + 6.0),
                            egui::Align2::LEFT_BOTTOM,
                            "Z: Ignition (deg)",
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::WHITE,
                        );
                    }
                    RomTableViewMode::TwoD => {
                        let row_lo = self
                            .rom_ign_table_zone_row_start
                            .min(self.rom_ign_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let row_hi = self
                            .rom_ign_table_zone_row_start
                            .max(self.rom_ign_table_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let col_lo = self
                            .rom_ign_table_zone_col_start
                            .min(self.rom_ign_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let col_hi = self
                            .rom_ign_table_zone_col_start
                            .max(self.rom_ign_table_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);

                        let mut cell_rects: Vec<(usize, usize, egui::Rect)> =
                            Vec::with_capacity(ROM_FUEL_TABLE_ROWS * ROM_FUEL_TABLE_COLS);

                        egui::Grid::new(format!("rom_ign_table_grid_{}", kind.label()))
                            .num_columns(ROM_FUEL_TABLE_COLS + 1)
                            .spacing([4.0, 4.0])
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [40.0, 22.0],
                                    egui::Label::new(egui::RichText::new("ROW").monospace()),
                                );

                                for col in 0..ROM_FUEL_TABLE_COLS {
                                    ui.add_sized(
                                        [54.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("C{}", col + 1)).monospace(),
                                        ),
                                    );
                                }
                                ui.end_row();

                                for (row_idx, row) in values.iter().enumerate() {
                                    let r1 = row_idx + 1;
                                    ui.add_sized(
                                        [40.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("R{}", r1)).monospace(),
                                        ),
                                    );

                                    for (col_idx, &value) in row.iter().enumerate() {
                                        let c1 = col_idx + 1;
                                        let in_selected_zone =
                                            r1 >= row_lo && r1 <= row_hi && c1 >= col_lo && c1 <= col_hi;
                                        let fill = Self::table_value_color_from_range(value, min_v, max_v, false);
                                        let cell_response = egui::Frame::new()
                                            .fill(fill)
                                            .stroke(if in_selected_zone {
                                                egui::Stroke::new(
                                                    1.5_f32,
                                                    egui::Color32::from_rgb(250, 220, 70),
                                                )
                                            } else {
                                                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(55))
                                            })
                                            .corner_radius(egui::CornerRadius::same(3))
                                            .inner_margin(egui::Margin::symmetric(3, 2))
                                            .show(ui, |ui| {
                                                ui.add_sized(
                                                    [50.0, 18.0],
                                                    egui::Label::new(
                                                        egui::RichText::new(format!("{:>5.1}", value))
                                                            .monospace()
                                                            .strong()
                                                            .color(egui::Color32::BLACK),
                                                    ),
                                                );
                                            })
                                            .response;

                                        let response = ui.interact(
                                            cell_response.rect,
                                            ui.id().with(("rom_ign_cell", kind.label(), r1, c1)),
                                            egui::Sense::click_and_drag(),
                                        );
                                        cell_rects.push((r1, c1, response.rect));

                                        if response.clicked() || response.drag_started() {
                                            self.rom_ign_table_drag_anchor = Some((r1, c1));
                                            self.rom_ign_table_zone_row_start = r1;
                                            self.rom_ign_table_zone_row_end = r1;
                                            self.rom_ign_table_zone_col_start = c1;
                                            self.rom_ign_table_zone_col_end = c1;
                                        }
                                    }
                                    ui.end_row();
                                }
                            });

                        if ui.input(|i| i.pointer.primary_down()) {
                            if let Some((anchor_r, anchor_c)) = self.rom_ign_table_drag_anchor {
                                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                                    if let Some((r, c, _)) =
                                        cell_rects.iter().find(|(_, _, rect)| rect.contains(pos))
                                    {
                                        self.rom_ign_table_zone_row_start = anchor_r.min(*r);
                                        self.rom_ign_table_zone_row_end = anchor_r.max(*r);
                                        self.rom_ign_table_zone_col_start = anchor_c.min(*c);
                                        self.rom_ign_table_zone_col_end = anchor_c.max(*c);
                                    }
                                }
                            }
                        }

                        if !ui.input(|i| i.pointer.primary_down()) {
                            self.rom_ign_table_drag_anchor = None;
                        }
                    }
                }
            });

        self.show_rom_ign_table_window = open;
        if is_low_cam {
            self.rom_ign_table_3d_pan = pan;
            self.rom_ign_table_3d_scale = scale;
            self.rom_ign_table_3d_yaw = yaw;
            self.rom_ign_table_3d_pitch = pitch;
        } else {
            self.rom_ign_table_high_3d_pan = pan;
            self.rom_ign_table_high_3d_scale = scale;
            self.rom_ign_table_high_3d_yaw = yaw;
            self.rom_ign_table_high_3d_pitch = pitch;
        }
    }

    fn draw_live_tracking_window(&mut self, ctx: &egui::Context) {
        if !self.show_live_tracking_window {
            return;
        }

        // Sync selection from DATALOG map zones to LIVE TRACKING when map type is Fuel/Ign.
        match self.live_tracking_map {
            RomEmbeddedView::Fuel => {
                self.live_tracking_zone_row_start = self.rom_table_zone_row_start;
                self.live_tracking_zone_row_end = self.rom_table_zone_row_end;
                self.live_tracking_zone_col_start = self.rom_table_zone_col_start;
                self.live_tracking_zone_col_end = self.rom_table_zone_col_end;
            }
            RomEmbeddedView::Ign => {
                self.live_tracking_zone_row_start = self.rom_ign_table_zone_row_start;
                self.live_tracking_zone_row_end = self.rom_ign_table_zone_row_end;
                self.live_tracking_zone_col_start = self.rom_ign_table_zone_col_start;
                self.live_tracking_zone_col_end = self.rom_ign_table_zone_col_end;
            }
            RomEmbeddedView::None => {
                match self.live_tracking_afr_map {
                    RomEmbeddedView::Fuel => {
                        self.live_tracking_zone_row_start = self.rom_table_zone_row_start;
                        self.live_tracking_zone_row_end = self.rom_table_zone_row_end;
                        self.live_tracking_zone_col_start = self.rom_table_zone_col_start;
                        self.live_tracking_zone_col_end = self.rom_table_zone_col_end;
                    }
                    RomEmbeddedView::Ign => {
                        self.live_tracking_zone_row_start = self.rom_ign_table_zone_row_start;
                        self.live_tracking_zone_row_end = self.rom_ign_table_zone_row_end;
                        self.live_tracking_zone_col_start = self.rom_ign_table_zone_col_start;
                        self.live_tracking_zone_col_end = self.rom_ign_table_zone_col_end;
                    }
                    RomEmbeddedView::None => {
                        self.live_tracking_zone_row_start = self.live_tracking_afr_zone_row_start;
                        self.live_tracking_zone_row_end = self.live_tracking_afr_zone_row_end;
                        self.live_tracking_zone_col_start = self.live_tracking_afr_zone_col_start;
                        self.live_tracking_zone_col_end = self.live_tracking_afr_zone_col_end;
                    }
                }
            }
        }

        let values = match self.live_tracking_map {
            RomEmbeddedView::Fuel => {
                let rom = match self.read_rom_file_for_table_view() {
                    Some(rom) => rom,
                    None => {
                        self.show_live_tracking_window = false;
                        return;
                    }
                };
                match Self::rom_fuel_table_values(&rom, self.live_tracking_kind, &self.rom_table_column_multipliers)
                {
                    Some(values) => values,
                    None => {
                        self.log
                            .push("LIVE TRACKING: ROM too small for selected table".to_string());
                        self.show_live_tracking_window = false;
                        return;
                    }
                }
            }
            RomEmbeddedView::Ign => {
                let rom = match self.read_rom_file_for_table_view() {
                    Some(rom) => rom,
                    None => {
                        self.show_live_tracking_window = false;
                        return;
                    }
                };
                match Self::rom_ign_table_values(&rom, self.live_tracking_kind) {
                    Some(values) => values,
                    None => {
                        self.log
                            .push("LIVE TRACKING: ROM too small for selected table".to_string());
                        self.show_live_tracking_window = false;
                        return;
                    }
                }
            }
            RomEmbeddedView::None => {
                let live_values = self.live_tracking_afr_values(self.live_tracking_kind);
                let mut afr_values = vec![vec![f64::NAN; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS];
                for (r, row) in afr_values.iter_mut().enumerate() {
                    for (c, cell) in row.iter_mut().enumerate() {
                        if let Some(value) = live_values[r][c] {
                            *cell = value;
                        }
                    }
                }
                afr_values
            }
        };

        let _ = self.rom_live_table_indices_1based();
        let live_cells = self.rom_live_table_cells_1based();

        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for row in &values {
            for &v in row {
                if v.is_finite() {
                    min_v = min_v.min(v);
                    max_v = max_v.max(v);
                }
            }
        }
        if !min_v.is_finite() || !max_v.is_finite() {
            min_v = AFR_GRAPH_MIN;
            max_v = AFR_GRAPH_MAX;
        }
        if (max_v - min_v).abs() < 0.001 {
            min_v = (min_v - 0.5).max(AFR_GRAPH_MIN);
            max_v = (max_v + 0.5).min(AFR_GRAPH_MAX);
        }

        let mut open = self.show_live_tracking_window;

        egui::Window::new("LIVE TRACKING")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .default_size(egui::vec2(1120.0, 780.0))
            .default_pos(egui::pos2(90.0, 140.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(self.live_tracking_map == RomEmbeddedView::Fuel, "FUEL TABLE")
                        .clicked()
                    {
                        self.live_tracking_map = RomEmbeddedView::Fuel;
                    }
                    if ui
                        .selectable_label(self.live_tracking_map == RomEmbeddedView::Ign, "IGNITION TABLE")
                        .clicked()
                    {
                        self.live_tracking_map = RomEmbeddedView::Ign;
                    }
                    if ui
                        .selectable_label(self.live_tracking_map == RomEmbeddedView::None, "AFR LIVE MAP")
                        .clicked()
                    {
                        self.live_tracking_map = RomEmbeddedView::None;
                    }
                    ui.separator();
                    for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                        if ui
                            .selectable_label(self.live_tracking_kind == table_kind, table_kind.label())
                            .clicked()
                        {
                            self.live_tracking_kind = table_kind;
                        }
                    }
                    ui.separator();
                    let follow_vtec_btn = egui::Button::new(
                        egui::RichText::new(if self.follow_vtec_tables {
                            "FOLLOW VTEC"
                        } else {
                            "FOLLOW VTEC OFF"
                        })
                        .strong()
                        .color(egui::Color32::WHITE),
                    )
                    .fill(if self.follow_vtec_tables {
                        egui::Color32::from_rgb(35, 130, 255)
                    } else {
                        egui::Color32::from_rgb(45, 45, 45)
                    });
                    if ui.add(follow_vtec_btn).clicked() {
                        self.follow_vtec_tables = !self.follow_vtec_tables;
                        if self.follow_vtec_tables {
                            self.sync_table_kinds_to_vtec();
                        }
                    }
                    ui.separator();
                    for view_mode in [RomTableViewMode::TwoD, RomTableViewMode::ThreeD] {
                        if ui
                            .selectable_label(self.live_tracking_view_mode == view_mode, view_mode.label())
                            .clicked()
                        {
                            self.live_tracking_view_mode = view_mode;
                        }
                    }
                    ui.separator();
                    if ui.button("RESET VIEW").clicked() {
                        if self.live_tracking_kind == RomFuelTableKind::LowCam {
                            self.live_tracking_3d_pan = egui::vec2(0.0, 0.0);
                            self.live_tracking_3d_scale = 1.0;
                            self.live_tracking_3d_yaw = -0.75;
                            self.live_tracking_3d_pitch = 0.70;
                        } else {
                            self.live_tracking_high_3d_pan = egui::vec2(0.0, 0.0);
                            self.live_tracking_high_3d_scale = 1.0;
                            self.live_tracking_high_3d_yaw = -0.75;
                            self.live_tracking_high_3d_pitch = 0.70;
                        }
                    }
                    if ui.button("FULL TABLE SELECT").clicked() {
                        self.live_tracking_zone_row_start = 1;
                        self.live_tracking_zone_row_end = ROM_FUEL_TABLE_ROWS;
                        self.live_tracking_zone_col_start = 1;
                        self.live_tracking_zone_col_end = ROM_FUEL_TABLE_COLS;
                    }
                    if self.live_tracking_map == RomEmbeddedView::None {
                        let clear_btn = egui::Button::new(
                            egui::RichText::new("CLEAR AFR LIVE MAP")
                                .color(egui::Color32::BLACK)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(255, 200, 70));
                        if ui.add(clear_btn).clicked() {
                            self.reset_live_tracking_afr_map();
                        }
                    }
                    for pct in [-10, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 10] {
                        if ui.button(format!("{:+}%", pct)).clicked() {
                            match self.live_tracking_map {
                                RomEmbeddedView::Fuel => {
                                    self.apply_rom_fuel_table_percent_zone(
                                        self.live_tracking_kind,
                                        pct,
                                        self.live_tracking_zone_row_start,
                                        self.live_tracking_zone_row_end,
                                        self.live_tracking_zone_col_start,
                                        self.live_tracking_zone_col_end,
                                    );
                                }
                                RomEmbeddedView::Ign => {
                                    self.apply_rom_ign_table_percent_zone(
                                        self.live_tracking_kind,
                                        pct,
                                        self.live_tracking_zone_row_start,
                                        self.live_tracking_zone_row_end,
                                        self.live_tracking_zone_col_start,
                                        self.live_tracking_zone_col_end,
                                    );
                                }
                                RomEmbeddedView::None => {
                                    self.live_tracking_afr_kind = self.live_tracking_kind;
                                    match self.live_tracking_afr_map {
                                        RomEmbeddedView::Fuel => {
                                            self.apply_rom_fuel_table_percent_zone(
                                                self.live_tracking_kind,
                                                pct,
                                                self.live_tracking_zone_row_start,
                                                self.live_tracking_zone_row_end,
                                                self.live_tracking_zone_col_start,
                                                self.live_tracking_zone_col_end,
                                            );
                                        }
                                        RomEmbeddedView::Ign => {
                                            self.apply_rom_ign_table_percent_zone(
                                                self.live_tracking_kind,
                                                pct,
                                                self.live_tracking_zone_row_start,
                                                self.live_tracking_zone_row_end,
                                                self.live_tracking_zone_col_start,
                                                self.live_tracking_zone_col_end,
                                            );
                                        }
                                        RomEmbeddedView::None => {
                                            self.log.push(
                                                "LIVE TRACKING AFR MAP: select FUEL TABLE or IGN TABLE in LIVE TRACKING AFR".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let burn_btn = egui::Button::new(
                        egui::RichText::new("BURN ROM")
                            .color(egui::Color32::BLACK)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(220, 40, 40));
                    if ui.add(burn_btn).clicked() {
                        self.upload_selected_bin();
                    }
                    ui.separator();
                    for mode in [LiveTrackingCellMode::Single, LiveTrackingCellMode::Quad] {
                        if ui
                            .selectable_label(self.live_tracking_cell_mode == mode, mode.label())
                            .clicked()
                        {
                            self.live_tracking_cell_mode = mode;
                        }
                    }
                    ui.separator();
                    let mut cell_target_sum = 0.0;
                    for (r, c) in &live_cells {
                        cell_target_sum += self.live_tracking_afr_target[r - 1][c - 1];
                    }
                    let live_cells_count = live_cells.len().max(1) as f64;
                    let cell_target = cell_target_sum / live_cells_count;
                    let delta = self.afr - cell_target;
                    let diff_pct = if cell_target.abs() > f64::EPSILON {
                        (delta / cell_target) * 100.0
                    } else {
                        0.0
                    };
                    ui.label(format!(
                        "AFR {:.2} | AFR TARGET {:.2} | DELTA {:+.2} ({:+.1}%)",
                        self.afr,
                        cell_target,
                        delta,
                        diff_pct
                    ));
                });

                ui.separator();

                match self.live_tracking_view_mode {
                    RomTableViewMode::TwoD => {
                        Self::draw_rom_map_grid_inline(
                            ui,
                            "live_tracking_grid",
                            &values,
                            min_v,
                            max_v,
                            &mut self.live_tracking_zone_row_start,
                            &mut self.live_tracking_zone_row_end,
                            &mut self.live_tracking_zone_col_start,
                            &mut self.live_tracking_zone_col_end,
                            &mut self.live_tracking_drag_anchor,
                            &live_cells,
                            self.live_tracking_map == RomEmbeddedView::None,
                        );
                    }
                    RomTableViewMode::ThreeD => {
                        let row_lo = self
                            .live_tracking_zone_row_start
                            .min(self.live_tracking_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let row_hi = self
                            .live_tracking_zone_row_start
                            .max(self.live_tracking_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let col_lo = self
                            .live_tracking_zone_col_start
                            .min(self.live_tracking_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let col_hi = self
                            .live_tracking_zone_col_start
                            .max(self.live_tracking_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);

                        let z_label = if self.live_tracking_map == RomEmbeddedView::Ign {
                            "Z: Ignition (deg)"
                        } else if self.live_tracking_map == RomEmbeddedView::None {
                            "Z: AFR"
                        } else {
                            "Z: Fuel Value"
                        };

                        if self.live_tracking_kind == RomFuelTableKind::LowCam {
                            Self::draw_rom_map_3d_inline(
                                ui,
                                &values,
                                min_v,
                                max_v,
                                &mut self.live_tracking_3d_pan,
                                &mut self.live_tracking_3d_scale,
                                &mut self.live_tracking_3d_yaw,
                                &mut self.live_tracking_3d_pitch,
                                row_lo,
                                row_hi,
                                col_lo,
                                col_hi,
                                &live_cells,
                                z_label,
                                self.live_tracking_map == RomEmbeddedView::None,
                            );
                        } else {
                            Self::draw_rom_map_3d_inline(
                                ui,
                                &values,
                                min_v,
                                max_v,
                                &mut self.live_tracking_high_3d_pan,
                                &mut self.live_tracking_high_3d_scale,
                                &mut self.live_tracking_high_3d_yaw,
                                &mut self.live_tracking_high_3d_pitch,
                                row_lo,
                                row_hi,
                                col_lo,
                                col_hi,
                                &live_cells,
                                z_label,
                                self.live_tracking_map == RomEmbeddedView::None,
                            );
                        }
                    }
                }
            });

        self.show_live_tracking_window = open;

        // Sync selection back to DATALOG map zones so edits and selection stay synchronized.
        match self.live_tracking_map {
            RomEmbeddedView::Fuel => {
                self.rom_table_zone_row_start = self.live_tracking_zone_row_start;
                self.rom_table_zone_row_end = self.live_tracking_zone_row_end;
                self.rom_table_zone_col_start = self.live_tracking_zone_col_start;
                self.rom_table_zone_col_end = self.live_tracking_zone_col_end;
            }
            RomEmbeddedView::Ign => {
                self.rom_ign_table_zone_row_start = self.live_tracking_zone_row_start;
                self.rom_ign_table_zone_row_end = self.live_tracking_zone_row_end;
                self.rom_ign_table_zone_col_start = self.live_tracking_zone_col_start;
                self.rom_ign_table_zone_col_end = self.live_tracking_zone_col_end;
            }
            RomEmbeddedView::None => {
                self.live_tracking_afr_zone_row_start = self.live_tracking_zone_row_start;
                self.live_tracking_afr_zone_row_end = self.live_tracking_zone_row_end;
                self.live_tracking_afr_zone_col_start = self.live_tracking_zone_col_start;
                self.live_tracking_afr_zone_col_end = self.live_tracking_zone_col_end;
                match self.live_tracking_afr_map {
                    RomEmbeddedView::Fuel => {
                        self.rom_table_zone_row_start = self.live_tracking_zone_row_start;
                        self.rom_table_zone_row_end = self.live_tracking_zone_row_end;
                        self.rom_table_zone_col_start = self.live_tracking_zone_col_start;
                        self.rom_table_zone_col_end = self.live_tracking_zone_col_end;
                    }
                    RomEmbeddedView::Ign => {
                        self.rom_ign_table_zone_row_start = self.live_tracking_zone_row_start;
                        self.rom_ign_table_zone_row_end = self.live_tracking_zone_row_end;
                        self.rom_ign_table_zone_col_start = self.live_tracking_zone_col_start;
                        self.rom_ign_table_zone_col_end = self.live_tracking_zone_col_end;
                    }
                    RomEmbeddedView::None => {}
                }
            }
        }
    }

    fn draw_live_tracking_afr_window(&mut self, ctx: &egui::Context) {
        if !self.show_live_tracking_afr_window {
            return;
        }

        // If LIVE TRACKING is currently on AFR LIVE MAP, use that as source of truth.
        if self.live_tracking_map == RomEmbeddedView::None {
            self.live_tracking_afr_zone_row_start = self.live_tracking_zone_row_start;
            self.live_tracking_afr_zone_row_end = self.live_tracking_zone_row_end;
            self.live_tracking_afr_zone_col_start = self.live_tracking_zone_col_start;
            self.live_tracking_afr_zone_col_end = self.live_tracking_zone_col_end;
        } else {
            // Otherwise keep LIVE TRACKING AFR selection in sync with DATALOG map selection.
            match self.live_tracking_afr_map {
                RomEmbeddedView::Fuel => {
                    self.live_tracking_afr_zone_row_start = self.rom_table_zone_row_start;
                    self.live_tracking_afr_zone_row_end = self.rom_table_zone_row_end;
                    self.live_tracking_afr_zone_col_start = self.rom_table_zone_col_start;
                    self.live_tracking_afr_zone_col_end = self.rom_table_zone_col_end;
                }
                RomEmbeddedView::Ign => {
                    self.live_tracking_afr_zone_row_start = self.rom_ign_table_zone_row_start;
                    self.live_tracking_afr_zone_row_end = self.rom_ign_table_zone_row_end;
                    self.live_tracking_afr_zone_col_start = self.rom_ign_table_zone_col_start;
                    self.live_tracking_afr_zone_col_end = self.rom_ign_table_zone_col_end;
                }
                RomEmbeddedView::None => {}
            }
        }

        let (live_row, live_col) = self.rom_live_table_indices_1based();
        let live_cells = self.rom_live_table_cells_1based();
        let live_values = self.live_tracking_afr_values(self.live_tracking_afr_kind).clone();
        let mut values_live = vec![vec![f64::NAN; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS];

        for row in 0..ROM_FUEL_TABLE_ROWS {
            for col in 0..ROM_FUEL_TABLE_COLS {
                if let Some(value) = live_values[row][col] {
                    values_live[row][col] = value;
                }
            }
        }

        let values = match self.live_tracking_afr_map_mode {
            LiveTrackingAfrMapMode::LiveAfr => values_live,
            LiveTrackingAfrMapMode::TargetAfr => self.live_tracking_afr_target.clone(),
            LiveTrackingAfrMapMode::DiffPct => {
                let mut diff_values = vec![vec![f64::NAN; ROM_FUEL_TABLE_COLS]; ROM_FUEL_TABLE_ROWS];
                for row in 0..ROM_FUEL_TABLE_ROWS {
                    for col in 0..ROM_FUEL_TABLE_COLS {
                        if let Some(live) = live_values[row][col] {
                            let target = self.live_tracking_afr_target[row][col];
                            if target.abs() > f64::EPSILON {
                                diff_values[row][col] = ((live - target) / target) * 100.0;
                            }
                        }
                    }
                }
                diff_values
            }
        };

        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for row in &values {
            for &v in row {
                if v.is_finite() {
                    min_v = min_v.min(v);
                    max_v = max_v.max(v);
                }
            }
        }
        if !min_v.is_finite() || !max_v.is_finite() {
            min_v = AFR_GRAPH_MIN;
            max_v = AFR_GRAPH_MAX;
        }
        if self.live_tracking_afr_map_mode == LiveTrackingAfrMapMode::DiffPct {
            let max_abs = min_v.abs().max(max_v.abs()).max(1.0);
            min_v = -max_abs;
            max_v = max_abs;
        }
        if (max_v - min_v).abs() < 0.001 {
            if self.live_tracking_afr_map_mode == LiveTrackingAfrMapMode::DiffPct {
                min_v -= 0.5;
                max_v += 0.5;
            } else {
                min_v = (min_v - 0.5).max(AFR_GRAPH_MIN);
                max_v = (max_v + 0.5).min(AFR_GRAPH_MAX);
            }
        }

        let mut open = self.show_live_tracking_afr_window;

        egui::Window::new("LIVE TRACKING AFR")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .default_size(egui::vec2(1120.0, 780.0))
            .default_pos(egui::pos2(90.0, 140.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(self.live_tracking_afr_map == RomEmbeddedView::Fuel, "FUEL TABLE")
                        .clicked()
                    {
                        self.live_tracking_afr_map = RomEmbeddedView::Fuel;
                    }
                    if ui
                        .selectable_label(self.live_tracking_afr_map == RomEmbeddedView::Ign, "IGNITION TABLE")
                        .clicked()
                    {
                        self.live_tracking_afr_map = RomEmbeddedView::Ign;
                    }
                    ui.separator();
                    for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                        if ui
                            .selectable_label(self.live_tracking_afr_kind == table_kind, table_kind.label())
                            .clicked()
                        {
                            self.live_tracking_afr_kind = table_kind;
                        }
                    }
                    ui.separator();
                    let follow_vtec_btn = egui::Button::new(
                        egui::RichText::new(if self.follow_vtec_tables {
                            "FOLLOW VTEC"
                        } else {
                            "FOLLOW VTEC OFF"
                        })
                        .strong()
                        .color(egui::Color32::WHITE),
                    )
                    .fill(if self.follow_vtec_tables {
                        egui::Color32::from_rgb(35, 130, 255)
                    } else {
                        egui::Color32::from_rgb(45, 45, 45)
                    });
                    if ui.add(follow_vtec_btn).clicked() {
                        self.follow_vtec_tables = !self.follow_vtec_tables;
                        if self.follow_vtec_tables {
                            self.sync_table_kinds_to_vtec();
                        }
                    }
                    ui.separator();
                    for map_mode in [
                        LiveTrackingAfrMapMode::LiveAfr,
                        LiveTrackingAfrMapMode::TargetAfr,
                        LiveTrackingAfrMapMode::DiffPct,
                    ] {
                        if ui
                            .selectable_label(self.live_tracking_afr_map_mode == map_mode, map_mode.label())
                            .clicked()
                        {
                            self.live_tracking_afr_map_mode = map_mode;
                        }
                    }
                    if self.live_tracking_afr_map_mode == LiveTrackingAfrMapMode::LiveAfr {
                        let clear_btn = egui::Button::new(
                            egui::RichText::new("CLEAR AFR LIVE MAP")
                                .color(egui::Color32::BLACK)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(255, 200, 70));
                        if ui.add(clear_btn).clicked() {
                            self.reset_live_tracking_afr_map();
                        }
                    }
                    ui.separator();
                    for view_mode in [RomTableViewMode::TwoD, RomTableViewMode::ThreeD] {
                        if ui
                            .selectable_label(self.live_tracking_afr_view_mode == view_mode, view_mode.label())
                            .clicked()
                        {
                            self.live_tracking_afr_view_mode = view_mode;
                        }
                    }
                    ui.separator();
                    if ui.button("RESET VIEW").clicked() {
                        self.live_tracking_afr_3d_pan = egui::vec2(0.0, 0.0);
                        self.live_tracking_afr_3d_scale = 1.0;
                        self.live_tracking_afr_3d_yaw = -0.75;
                        self.live_tracking_afr_3d_pitch = 0.70;
                    }
                    if ui.button("FULL TABLE SELECT").clicked() {
                        self.live_tracking_afr_zone_row_start = 1;
                        self.live_tracking_afr_zone_row_end = ROM_FUEL_TABLE_ROWS;
                        self.live_tracking_afr_zone_col_start = 1;
                        self.live_tracking_afr_zone_col_end = ROM_FUEL_TABLE_COLS;
                    }
                    ui.label("TARGET");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.live_tracking_afr_target_input)
                            .desired_width(54.0),
                    );
                    if ui.button("--").clicked() {
                        self.nudge_live_tracking_afr_target_zone(
                            -0.1,
                            self.live_tracking_afr_zone_row_start,
                            self.live_tracking_afr_zone_row_end,
                            self.live_tracking_afr_zone_col_start,
                            self.live_tracking_afr_zone_col_end,
                        );
                        let live_target = self.live_tracking_afr_target[live_row - 1][live_col - 1];
                        self.live_tracking_afr_target_input = format!("{:.1}", live_target);
                    }
                    if ui.button("++").clicked() {
                        self.nudge_live_tracking_afr_target_zone(
                            0.1,
                            self.live_tracking_afr_zone_row_start,
                            self.live_tracking_afr_zone_row_end,
                            self.live_tracking_afr_zone_col_start,
                            self.live_tracking_afr_zone_col_end,
                        );
                        let live_target = self.live_tracking_afr_target[live_row - 1][live_col - 1];
                        self.live_tracking_afr_target_input = format!("{:.1}", live_target);
                    }
                    if ui.button("APPLY TARGET").clicked() {
                        if let Ok(v) = self.live_tracking_afr_target_input.trim().parse::<f64>() {
                            self.apply_live_tracking_afr_target_zone(
                                v,
                                self.live_tracking_afr_zone_row_start,
                                self.live_tracking_afr_zone_row_end,
                                self.live_tracking_afr_zone_col_start,
                                self.live_tracking_afr_zone_col_end,
                            );
                        }
                    }
                    if ui.button("RESET TARGET").clicked() {
                        self.reset_live_tracking_afr_target_map();
                    }
                    for pct in [-10, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 10] {
                        if ui.button(format!("{:+}%", pct)).clicked() {
                            match self.live_tracking_afr_map {
                                RomEmbeddedView::Fuel => {
                                    self.apply_rom_fuel_table_percent_zone(
                                        self.live_tracking_afr_kind,
                                        pct,
                                        self.live_tracking_afr_zone_row_start,
                                        self.live_tracking_afr_zone_row_end,
                                        self.live_tracking_afr_zone_col_start,
                                        self.live_tracking_afr_zone_col_end,
                                    );
                                }
                                RomEmbeddedView::Ign => {
                                    self.apply_rom_ign_table_percent_zone(
                                        self.live_tracking_afr_kind,
                                        pct,
                                        self.live_tracking_afr_zone_row_start,
                                        self.live_tracking_afr_zone_row_end,
                                        self.live_tracking_afr_zone_col_start,
                                        self.live_tracking_afr_zone_col_end,
                                    );
                                }
                                RomEmbeddedView::None => {}
                            }
                        }
                    }
                    let burn_btn = egui::Button::new(
                        egui::RichText::new("BURN ROM")
                            .color(egui::Color32::BLACK)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(220, 40, 40));
                    if ui.add(burn_btn).clicked() {
                        self.upload_selected_bin();
                    }
                    if ui
                        .button(if self.live_tracking_capture_enabled {
                            "CAPTURE ON"
                        } else {
                            "CAPTURE OFF"
                        })
                        .clicked()
                    {
                        self.live_tracking_capture_enabled = !self.live_tracking_capture_enabled;
                    }
                    ui.separator();
                    for mode in [LiveTrackingCellMode::Single, LiveTrackingCellMode::Quad] {
                        if ui
                            .selectable_label(self.live_tracking_cell_mode == mode, mode.label())
                            .clicked()
                        {
                            self.live_tracking_cell_mode = mode;
                        }
                    }
                    ui.label(format!(
                        "AVG x3 HOLD"
                    ));
                    if ui.button("CLEAR RECORDING").clicked() {
                        self.reset_live_tracking_afr_map();
                    }
                    ui.separator();
                    let mut cell_target_sum = 0.0;
                    for (r, c) in &live_cells {
                        cell_target_sum += self.live_tracking_afr_target[r - 1][c - 1];
                    }
                    let live_cells_count = live_cells.len().max(1) as f64;
                    let cell_target = cell_target_sum / live_cells_count;
                    let delta = self.afr - cell_target;
                    let diff_pct = if cell_target.abs() > f64::EPSILON {
                        (delta / cell_target) * 100.0
                    } else {
                        0.0
                    };
                    ui.label(format!(
                        "AFR {:.2} | AFR TARGET {:.2} | DELTA {:+.2} ({:+.1}%)",
                        self.afr,
                        cell_target,
                        delta,
                        diff_pct
                    ));
                });

                ui.separator();

                match self.live_tracking_afr_view_mode {
                    RomTableViewMode::TwoD => {
                        Self::draw_rom_map_grid_inline(
                            ui,
                            "live_tracking_afr_grid",
                            &values,
                            min_v,
                            max_v,
                            &mut self.live_tracking_afr_zone_row_start,
                            &mut self.live_tracking_afr_zone_row_end,
                            &mut self.live_tracking_afr_zone_col_start,
                            &mut self.live_tracking_afr_zone_col_end,
                            &mut self.live_tracking_afr_drag_anchor,
                            &live_cells,
                            self.live_tracking_afr_map_mode != LiveTrackingAfrMapMode::DiffPct,
                        );
                    }
                    RomTableViewMode::ThreeD => {
                        let row_lo = self
                            .live_tracking_afr_zone_row_start
                            .min(self.live_tracking_afr_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let row_hi = self
                            .live_tracking_afr_zone_row_start
                            .max(self.live_tracking_afr_zone_row_end)
                            .clamp(1, ROM_FUEL_TABLE_ROWS);
                        let col_lo = self
                            .live_tracking_afr_zone_col_start
                            .min(self.live_tracking_afr_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);
                        let col_hi = self
                            .live_tracking_afr_zone_col_start
                            .max(self.live_tracking_afr_zone_col_end)
                            .clamp(1, ROM_FUEL_TABLE_COLS);

                        Self::draw_rom_map_3d_inline(
                            ui,
                            &values,
                            min_v,
                            max_v,
                            &mut self.live_tracking_afr_3d_pan,
                            &mut self.live_tracking_afr_3d_scale,
                            &mut self.live_tracking_afr_3d_yaw,
                            &mut self.live_tracking_afr_3d_pitch,
                            row_lo,
                            row_hi,
                            col_lo,
                            col_hi,
                            &live_cells,
                            if self.live_tracking_afr_map_mode == LiveTrackingAfrMapMode::DiffPct {
                                "Z: AFR Diff %"
                            } else {
                                "Z: AFR"
                            },
                            self.live_tracking_afr_map_mode != LiveTrackingAfrMapMode::DiffPct,
                        );
                    }
                }
            });

        self.show_live_tracking_afr_window = open;

        match self.live_tracking_afr_map {
            RomEmbeddedView::Fuel => {
                self.rom_table_zone_row_start = self.live_tracking_afr_zone_row_start;
                self.rom_table_zone_row_end = self.live_tracking_afr_zone_row_end;
                self.rom_table_zone_col_start = self.live_tracking_afr_zone_col_start;
                self.rom_table_zone_col_end = self.live_tracking_afr_zone_col_end;
            }
            RomEmbeddedView::Ign => {
                self.rom_ign_table_zone_row_start = self.live_tracking_afr_zone_row_start;
                self.rom_ign_table_zone_row_end = self.live_tracking_afr_zone_row_end;
                self.rom_ign_table_zone_col_start = self.live_tracking_afr_zone_col_start;
                self.rom_ign_table_zone_col_end = self.live_tracking_afr_zone_col_end;
            }
            RomEmbeddedView::None => {}
        }

        if self.live_tracking_map == RomEmbeddedView::None {
            self.live_tracking_zone_row_start = self.live_tracking_afr_zone_row_start;
            self.live_tracking_zone_row_end = self.live_tracking_afr_zone_row_end;
            self.live_tracking_zone_col_start = self.live_tracking_afr_zone_col_start;
            self.live_tracking_zone_col_end = self.live_tracking_afr_zone_col_end;
        }
    }

    fn draw_rom_map_grid_inline(
        ui: &mut egui::Ui,
        id_salt: &str,
        values: &[Vec<f64>],
        min_v: f64,
        max_v: f64,
        zone_row_start: &mut usize,
        zone_row_end: &mut usize,
        zone_col_start: &mut usize,
        zone_col_end: &mut usize,
        drag_anchor: &mut Option<(usize, usize)>,
        live_cells: &[(usize, usize)],
        use_afr_scale: bool,
    ) {
        let row_lo = (*zone_row_start).min(*zone_row_end).clamp(1, ROM_FUEL_TABLE_ROWS);
        let row_hi = (*zone_row_start).max(*zone_row_end).clamp(1, ROM_FUEL_TABLE_ROWS);
        let col_lo = (*zone_col_start).min(*zone_col_end).clamp(1, ROM_FUEL_TABLE_COLS);
        let col_hi = (*zone_col_start).max(*zone_col_end).clamp(1, ROM_FUEL_TABLE_COLS);

        let mut cell_rects: Vec<(usize, usize, egui::Rect)> =
            Vec::with_capacity(ROM_FUEL_TABLE_ROWS * ROM_FUEL_TABLE_COLS);

        egui::Grid::new(id_salt)
            .num_columns(ROM_FUEL_TABLE_COLS + 1)
            .spacing([4.0, 4.0])
            .show(ui, |ui| {
                ui.add_sized(
                    [40.0, 22.0],
                    egui::Label::new(egui::RichText::new("ROW").monospace()),
                );

                for col in 0..ROM_FUEL_TABLE_COLS {
                    ui.add_sized(
                        [54.0, 22.0],
                        egui::Label::new(egui::RichText::new(format!("C{}", col + 1)).monospace()),
                    );
                }
                ui.end_row();

                for (row_idx, row) in values.iter().enumerate() {
                    let r1 = row_idx + 1;
                    ui.add_sized(
                        [40.0, 22.0],
                        egui::Label::new(egui::RichText::new(format!("R{}", r1)).monospace()),
                    );

                    for (col_idx, &value) in row.iter().enumerate() {
                        let c1 = col_idx + 1;
                        let in_selected_zone =
                            r1 >= row_lo && r1 <= row_hi && c1 >= col_lo && c1 <= col_hi;
                        let in_live_cell = live_cells.contains(&(r1, c1));
                        let is_empty = !value.is_finite();
                        let mut fill = if is_empty {
                            egui::Color32::from_gray(24)
                        } else {
                            Self::table_value_color_from_range(value, min_v, max_v, use_afr_scale)
                        };
                        if in_live_cell {
                            fill = Self::scale_color(fill, 1.30);
                        }
                        let cell_response = egui::Frame::new()
                            .fill(fill)
                            .stroke(if in_live_cell {
                                egui::Stroke::new(2.2_f32, egui::Color32::from_rgb(50, 210, 255))
                            } else if in_selected_zone {
                                egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(250, 220, 70))
                            } else {
                                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(55))
                            })
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(3, 2))
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [50.0, 18.0],
                                    egui::Label::new(
                                        egui::RichText::new(if is_empty {
                                            " -- ".to_string()
                                        } else {
                                            format!("{:>5.1}", value)
                                        })
                                            .monospace()
                                            .strong()
                                            .color(if is_empty {
                                                egui::Color32::GRAY
                                            } else {
                                                egui::Color32::BLACK
                                            }),
                                    ),
                                );
                            })
                            .response;

                        let response = ui.interact(
                            cell_response.rect,
                            ui.id().with((id_salt, r1, c1)),
                            egui::Sense::click_and_drag(),
                        );
                        cell_rects.push((r1, c1, response.rect));

                        if response.clicked() || response.drag_started() {
                            *drag_anchor = Some((r1, c1));
                            *zone_row_start = r1;
                            *zone_row_end = r1;
                            *zone_col_start = c1;
                            *zone_col_end = c1;
                        }
                    }
                    ui.end_row();
                }
            });

        if ui.input(|i| i.pointer.primary_down()) {
            if let Some((anchor_r, anchor_c)) = *drag_anchor {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if let Some((r, c, _)) = cell_rects.iter().find(|(_, _, rect)| rect.contains(pos)) {
                        *zone_row_start = anchor_r.min(*r);
                        *zone_row_end = anchor_r.max(*r);
                        *zone_col_start = anchor_c.min(*c);
                        *zone_col_end = anchor_c.max(*c);
                    }
                }
            }
        }

        if !ui.input(|i| i.pointer.primary_down()) {
            *drag_anchor = None;
        }
    }

    fn draw_rom_map_3d_inline(
        ui: &mut egui::Ui,
        values: &[Vec<f64>],
        min_v: f64,
        max_v: f64,
        pan: &mut egui::Vec2,
        scale: &mut f32,
        yaw: &mut f32,
        pitch: &mut f32,
        row_lo: usize,
        row_hi: usize,
        col_lo: usize,
        col_hi: usize,
        live_cells: &[(usize, usize)],
        z_label: &str,
        use_afr_scale: bool,
    ) {
        let draw_scale = *scale;
        let width = ui.available_width().clamp(540.0, 980.0);
        let height = (width * 0.58).clamp(320.0, 540.0);
        let desired = egui::vec2(width, height);
        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::drag());

        if response.hovered() && ui.input(|i| i.pointer.primary_down()) {
            *pan += ui.input(|i| i.pointer.delta());
        }
        if response.hovered() && ui.input(|i| i.pointer.secondary_down()) {
            let delta = ui.input(|i| i.pointer.delta());
            *yaw += delta.x * 0.01;
            *pitch = (*pitch - delta.y * 0.01).clamp(0.2, 1.3);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > f32::EPSILON {
                *scale = (*scale + (scroll * 0.0015)).clamp(0.6, 1.8);
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 14, 14));
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
            egui::StrokeKind::Inside,
        );

        let rows = ROM_FUEL_TABLE_ROWS;
        let cols = ROM_FUEL_TABLE_COLS;
        let half_x = cols as f32 * 0.5;
        let half_y = rows as f32 * 0.5;
        let unit = 1.0_f32;
        let gap = 0.08_f32;
        let z_height_max = 4.6_f32;
        let fov = 420.0_f32 * draw_scale;
        let cam_dist = 24.0_f32;
        let center = egui::pos2(
            rect.left() + rect.width() * 0.5 + pan.x,
            rect.top() + rect.height() * 0.57 + pan.y,
        );

        let cy = yaw.cos();
        let sy = yaw.sin();
        let cp = pitch.cos();
        let sp = pitch.sin();

        let rotate = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
            let xr = x * cy - y * sy;
            let yr = x * sy + y * cy;
            let yr2 = yr * cp - z * sp;
            let zr2 = yr * sp + z * cp;
            (xr, yr2, zr2)
        };

        let project = |x: f32, y: f32, z: f32| -> (egui::Pos2, f32) {
            let (xr, yr, zr) = rotate(x, y, z);
            let depth = (cam_dist + yr).max(1.0);
            let px = center.x + (xr * fov / depth);
            let py = center.y - (zr * fov / depth);
            (egui::pos2(px, py), depth)
        };

        for col in 0..=cols {
            let xw = (col as f32 - half_x) * (unit + gap);
            let (p0, _) = project(xw, half_y * (unit + gap), 0.0);
            let (p1, _) = project(xw, -(half_y + 1.0) * (unit + gap), 0.0);
            painter.line_segment(
                [p0, p1],
                egui::Stroke::new(
                    if col % 3 == 0 { 1.1_f32 } else { 0.8_f32 },
                    egui::Color32::from_gray(if col % 3 == 0 { 54 } else { 38 }),
                ),
            );
        }
        for row in 0..=rows {
            let yw = (half_y - row as f32) * (unit + gap);
            let (p0, _) = project(-half_x * (unit + gap), yw, 0.0);
            let (p1, _) = project((half_x + 1.0) * (unit + gap), yw, 0.0);
            painter.line_segment(
                [p0, p1],
                egui::Stroke::new(
                    if row % 5 == 0 { 1.1_f32 } else { 0.8_f32 },
                    egui::Color32::from_gray(if row % 5 == 0 { 54 } else { 38 }),
                ),
            );
        }

        let (axis_origin, _) = project(-half_x * (unit + gap), half_y * (unit + gap), 0.0);
        let (axis_x, _) = project((half_x + 0.8) * (unit + gap), half_y * (unit + gap), 0.0);
        let (axis_y, _) = project(-half_x * (unit + gap), -(half_y + 0.8) * (unit + gap), 0.0);
        let (axis_z, _) = project(-half_x * (unit + gap), half_y * (unit + gap), z_height_max * 1.08);
        painter.line_segment(
            [axis_origin, axis_x],
            egui::Stroke::new(1.7_f32, egui::Color32::from_rgb(255, 165, 70)),
        );
        painter.line_segment(
            [axis_origin, axis_y],
            egui::Stroke::new(1.7_f32, egui::Color32::from_rgb(90, 210, 255)),
        );
        painter.line_segment(
            [axis_origin, axis_z],
            egui::Stroke::new(1.7_f32, egui::Color32::from_rgb(120, 255, 150)),
        );

        struct Face {
            points: Vec<egui::Pos2>,
            color: egui::Color32,
            stroke: egui::Stroke,
            depth: f32,
        }

        let mut faces: Vec<Face> = Vec::with_capacity(rows * cols * 5);
        for row in (0..rows).rev() {
            for col in 0..cols {
                let r1 = row + 1;
                let c1 = col + 1;
                let in_selected_zone = r1 >= row_lo && r1 <= row_hi && c1 >= col_lo && c1 <= col_hi;
                let in_live_cell = live_cells.contains(&(r1, c1));
                let value = values[row][col];
                let has_value = value.is_finite();
                let ratio = if has_value {
                    ((value - min_v) / (max_v - min_v).max(1.0)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let z = ratio * z_height_max;

                let x0 = (col as f32 - half_x) * (unit + gap);
                let x1 = x0 + unit;
                let y0 = (half_y - row as f32) * (unit + gap);
                let y1 = y0 - unit;

                let (b0, d0) = project(x0, y0, 0.0);
                let (b1, d1) = project(x1, y0, 0.0);
                let (b2, d2) = project(x1, y1, 0.0);
                let (b3, d3) = project(x0, y1, 0.0);

                let (t0, dt0) = project(x0, y0, z);
                let (t1, dt1) = project(x1, y0, z);
                let (t2, dt2) = project(x1, y1, z);
                let (t3, dt3) = project(x0, y1, z);

                let mut top_color = if has_value {
                    Self::table_value_color_from_range(value, min_v, max_v, use_afr_scale)
                } else {
                    egui::Color32::from_gray(24)
                };
                if in_selected_zone {
                    top_color = Self::scale_color(top_color, 1.25);
                }
                if in_live_cell {
                    top_color = Self::scale_color(top_color, 1.35);
                }
                let side_color_a = Self::scale_color(top_color, 0.72);
                let side_color_b = Self::scale_color(top_color, 0.58);

                faces.push(Face {
                    points: vec![b1, b2, t2, t1],
                    color: side_color_a,
                    stroke: egui::Stroke::NONE,
                    depth: (d1 + d2 + dt2 + dt1) * 0.25,
                });
                faces.push(Face {
                    points: vec![b0, b1, t1, t0],
                    color: side_color_b,
                    stroke: egui::Stroke::NONE,
                    depth: (d0 + d1 + dt1 + dt0) * 0.25,
                });
                faces.push(Face {
                    points: vec![b2, b3, t3, t2],
                    color: Self::scale_color(top_color, 0.64),
                    stroke: egui::Stroke::NONE,
                    depth: (d2 + d3 + dt3 + dt2) * 0.25,
                });
                faces.push(Face {
                    points: vec![b3, b0, t0, t3],
                    color: Self::scale_color(top_color, 0.52),
                    stroke: egui::Stroke::NONE,
                    depth: (d3 + d0 + dt0 + dt3) * 0.25,
                });
                faces.push(Face {
                    points: vec![t0, t1, t2, t3],
                    color: top_color,
                    stroke: if in_live_cell {
                        egui::Stroke::new(2.2_f32, egui::Color32::from_rgb(50, 210, 255))
                    } else if in_selected_zone {
                        egui::Stroke::new(1.7_f32, egui::Color32::from_rgb(250, 220, 70))
                    } else {
                        egui::Stroke::new(0.8_f32, egui::Color32::from_gray(40))
                    },
                    depth: (dt0 + dt1 + dt2 + dt3) * 0.25,
                });
            }
        }

        faces.sort_by(|a, b| {
            b.depth
                .partial_cmp(&a.depth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for face in faces {
            painter.add(egui::Shape::convex_polygon(face.points, face.color, face.stroke));
        }

        for &(r1, c1) in live_cells {
            let row = r1 - 1;
            let col = c1 - 1;
            let value = values[row][col];
            let ratio = if value.is_finite() {
                ((value - min_v) / (max_v - min_v).max(1.0)).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            let z = ratio * z_height_max;
            let cx = (col as f32 - half_x) * (unit + gap) + (unit * 0.5);
            let cyw = (half_y - row as f32) * (unit + gap) - (unit * 0.5);
            let (base, _) = project(cx, cyw, 0.0);
            let (top, _) = project(cx, cyw, z + 0.08);
            painter.line_segment(
                [base, top],
                egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(80, 240, 255)),
            );
            painter.circle_filled(top, 4.0, egui::Color32::from_rgb(80, 240, 255));
            painter.circle_stroke(
                top,
                7.0,
                egui::Stroke::new(1.2_f32, egui::Color32::from_rgba_unmultiplied(80, 240, 255, 120)),
            );
        }

        for idx in [0_usize, 3, 6, 9] {
            let xw = (idx as f32 - half_x) * (unit + gap);
            let (x_proj, _) = project(xw, -(half_y + 2.0), 0.0);
            painter.text(
                egui::pos2(x_proj.x, rect.bottom() - 18.0),
                egui::Align2::CENTER_TOP,
                format!("C{}", idx + 1),
                egui::TextStyle::Small.resolve(ui.style()),
                egui::Color32::LIGHT_GRAY,
            );
        }

        for idx in [0_usize, 5, 10, 15, 19] {
            let yw = (half_y - idx as f32) * (unit + gap);
            let (y_proj, _) = project(-(half_x + 2.0), yw, 0.0);
            painter.text(
                egui::pos2(rect.left() + 20.0, y_proj.y),
                egui::Align2::RIGHT_CENTER,
                format!("R{}", idx + 1),
                egui::TextStyle::Small.resolve(ui.style()),
                egui::Color32::LIGHT_GRAY,
            );
        }

        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 2.0),
            egui::Align2::CENTER_TOP,
            "Columns",
            egui::TextStyle::Body.resolve(ui.style()),
            egui::Color32::WHITE,
        );
        painter.text(
            egui::pos2(rect.left() + 6.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Rows",
            egui::TextStyle::Body.resolve(ui.style()),
            egui::Color32::WHITE,
        );
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.top() + 6.0),
            egui::Align2::LEFT_BOTTOM,
            z_label,
            egui::TextStyle::Body.resolve(ui.style()),
            egui::Color32::WHITE,
        );
        painter.text(
            egui::pos2(rect.left() + 10.0, rect.top() + 10.0),
            egui::Align2::LEFT_TOP,
            "LMB pan | RMB rotate",
            egui::TextStyle::Small.resolve(ui.style()),
            egui::Color32::from_gray(190),
        );
    }

    fn draw_afr_table_content(&mut self, ui: &mut egui::Ui, compact: bool) {
        let live_cells = self.afr_table_live_cells();
        let (axis_w, map_w, row_h, cell_w, spacing) = if compact {
            (14.0, 8.0, 8.0, 8.0, [0.0, 0.0])
        } else {
            (52.0, 42.0, 24.0, 38.0, [4.0, 4.0])
        };

        if !compact {
            ui.separator();

            ui.horizontal_wrapped(|ui| {
                let follow_vtec_btn = egui::Button::new(
                    egui::RichText::new(if self.follow_vtec_tables {
                        "FOLLOW VTEC"
                    } else {
                        "FOLLOW VTEC OFF"
                    })
                    .strong()
                    .color(egui::Color32::WHITE),
                )
                .fill(if self.follow_vtec_tables {
                    egui::Color32::from_rgb(35, 130, 255)
                } else {
                    egui::Color32::from_rgb(45, 45, 45)
                });
                if ui.add(follow_vtec_btn).clicked() {
                    self.follow_vtec_tables = !self.follow_vtec_tables;
                    if self.follow_vtec_tables {
                        self.sync_table_kinds_to_vtec();
                    }
                }
                ui.separator();
                for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                    if ui
                        .selectable_label(self.sensor_table_kind == table_kind, table_kind.label())
                        .clicked()
                    {
                        self.sensor_table_kind = table_kind;
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                for metric in TableMetric::ALL {
                    if ui
                        .selectable_label(self.table_metric == metric, metric.label())
                        .clicked()
                    {
                        self.table_metric = metric;
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                let current_metric = self.table_metric;
                let unit = current_metric.unit();
                let unit_sep = if unit.is_empty() { "" } else { " " };
                ui.label(format!(
                    "Live bin: {} | MAP {:.0} mbar | RPM {:.0} | {} {:.2}{}{}",
                    self.sensor_table_kind.label(),
                    self.map,
                    self.rpm,
                    current_metric.label(),
                    self.table_metric_live_value(current_metric),
                    unit_sep,
                    unit
                ));
                if ui.button("CLEAR CURRENT").clicked() {
                    self.clear_table_metric(current_metric);
                }
            });

            ui.add_space(4.0);
        }
        egui::Grid::new("afr_table_grid")
            .num_columns(AFR_TABLE_SIZE + 1)
            .spacing(spacing)
            .show(ui, |ui| {
                if compact {
                    ui.add_sized([axis_w, row_h], egui::Label::new(""));
                } else {
                    ui.add_sized(
                        [axis_w, 22.0],
                        egui::Label::new(egui::RichText::new("RPM\\MAP").monospace()),
                    );
                }

                for col in 0..AFR_TABLE_SIZE {
                    let map_val = AFR_TABLE_MAP_MIN
                        + (col as f64 / (AFR_TABLE_SIZE as f64 - 1.0))
                            * (AFR_TABLE_MAP_MAX - AFR_TABLE_MAP_MIN);
                    if compact {
                        ui.add_sized([map_w, row_h], egui::Label::new(""));
                    } else {
                        ui.add_sized(
                            [map_w, 22.0],
                            egui::Label::new(
                                egui::RichText::new(format!("{:>4.0}", map_val)).monospace(),
                            ),
                        );
                    }
                }
                ui.end_row();

                for row in 0..AFR_TABLE_SIZE {
                    let rpm_val = AFR_TABLE_RPM_MIN
                        + (row as f64 / (AFR_TABLE_SIZE as f64 - 1.0))
                            * (AFR_TABLE_RPM_MAX - AFR_TABLE_RPM_MIN);
                    if compact {
                        ui.add_sized([axis_w, row_h], egui::Label::new(""));
                    } else {
                        ui.add_sized(
                            [axis_w, row_h],
                            egui::Label::new(
                                egui::RichText::new(format!("{:>4.0}", rpm_val)).monospace(),
                            ),
                        );
                    }

                    for col in 0..AFR_TABLE_SIZE {
                        let metric = self.table_metric;
                        let cell_val = self.selected_table_value(metric, row, col);
                        let fill = Self::table_value_color(metric, cell_val);
                        let is_live_cell = live_cells.contains(&(row, col));
                        let stroke = if is_live_cell {
                            egui::Stroke::new(
                                if compact { 1.0_f32 } else { 1.5_f32 },
                                egui::Color32::YELLOW,
                            )
                        } else {
                            egui::Stroke::new(
                                if compact { 0.6_f32 } else { 1.0_f32 },
                                egui::Color32::from_gray(if compact { 35 } else { 55 }),
                            )
                        };

                        egui::Frame::new()
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(if compact {
                                egui::Margin::symmetric(0, 0)
                            } else {
                                egui::Margin::symmetric(3, 2)
                            })
                            .show(ui, |ui| {
                                if compact {
                                    ui.add_sized([cell_w, row_h - 2.0], egui::Label::new(""));
                                } else {
                                    let text_color = if cell_val.is_some() {
                                        egui::Color32::BLACK
                                    } else {
                                        egui::Color32::GRAY
                                    };
                                    let txt = match cell_val {
                                        Some(v) => format!("{:>4.1}", v),
                                        None => " -- ".to_string(),
                                    };
                                    ui.add_sized(
                                        [cell_w, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(txt)
                                                .monospace()
                                                .strong()
                                                .color(text_color),
                                        ),
                                    );
                                }
                            });
                    }

                    ui.end_row();
                }
            });
    }

    fn draw_afr_table_window(&mut self, ctx: &egui::Context) {
        if !self.show_table_window {
            return;
        }

        let mut open = self.show_table_window;

        egui::Window::new("2D MODE")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| {
                self.draw_afr_table_content(ui, false);
            });

        self.show_table_window = open;
    }

    fn draw_table_3d_window(&mut self, ctx: &egui::Context) {
        if !self.show_table_3d_window {
            return;
        }

        let live_cells = self.afr_table_live_cells();
        let mut open = self.show_table_3d_window;

        egui::Window::new("3D MODE")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .default_size(egui::vec2(1180.0, 820.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let follow_vtec_btn = egui::Button::new(
                        egui::RichText::new(if self.follow_vtec_tables {
                            "FOLLOW VTEC"
                        } else {
                            "FOLLOW VTEC OFF"
                        })
                        .strong()
                        .color(egui::Color32::WHITE),
                    )
                    .fill(if self.follow_vtec_tables {
                        egui::Color32::from_rgb(35, 130, 255)
                    } else {
                        egui::Color32::from_rgb(45, 45, 45)
                    });
                    if ui.add(follow_vtec_btn).clicked() {
                        self.follow_vtec_tables = !self.follow_vtec_tables;
                        if self.follow_vtec_tables {
                            self.sync_table_kinds_to_vtec();
                        }
                    }
                    ui.separator();
                    for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                        if ui
                            .selectable_label(self.sensor_table_kind == table_kind, table_kind.label())
                            .clicked()
                        {
                            self.sensor_table_kind = table_kind;
                        }
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    for metric in TableMetric::ALL {
                        if ui
                            .selectable_label(self.table_metric == metric, metric.label())
                            .clicked()
                        {
                            self.table_metric = metric;
                        }
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    let current_metric = self.table_metric;
                    let unit = current_metric.unit();
                    let unit_sep = if unit.is_empty() { "" } else { " " };
                    ui.label(format!(
                        "Live: {} | MAP {:.0} mbar | RPM {:.0} | {} {:.2}{}{}",
                        self.sensor_table_kind.label(),
                        self.map,
                        self.rpm,
                        current_metric.label(),
                        self.table_metric_live_value(current_metric),
                        unit_sep,
                        unit
                    ));

                    if ui.button("CLEAR CURRENT").clicked() {
                        self.clear_table_metric(current_metric);
                    }

                    if ui.button("RESET VIEW").clicked() {
                        self.table_3d_pan = egui::vec2(0.0, 0.0);
                        self.table_3d_scale = 1.0;
                        self.table_3d_yaw = -0.75;
                        self.table_3d_pitch = 0.70;
                    }
                });

                ui.add(
                    egui::Slider::new(&mut self.table_3d_scale, 0.6..=1.8)
                        .show_value(false),
                );

                ui.add_space(6.0);

                let scale = self.table_3d_scale;
                let desired = egui::vec2(1080.0, 720.0);
                let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::drag());

                if response.hovered() && ui.input(|i| i.pointer.primary_down()) {
                    self.table_3d_pan += ui.input(|i| i.pointer.delta());
                }

                if response.hovered() && ui.input(|i| i.pointer.secondary_down()) {
                    let delta = ui.input(|i| i.pointer.delta());
                    self.table_3d_yaw += delta.x * 0.01;
                    self.table_3d_pitch = (self.table_3d_pitch - delta.y * 0.01).clamp(0.2, 1.3);
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    if scroll.abs() > f32::EPSILON {
                        self.table_3d_scale =
                            (self.table_3d_scale + (scroll * 0.0015)).clamp(0.6, 1.8);
                    }
                }

                let painter = ui.painter_at(rect);

                painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 14, 14));
                painter.rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
                    egui::StrokeKind::Inside,
                );

                let metric = self.table_metric;
                let half = AFR_TABLE_SIZE as f32 * 0.5;
                let unit = 1.0_f32;
                let gap = 0.08_f32;
                let z_height_max = 4.2_f32;
                let fov = 420.0_f32 * scale;
                let cam_dist = 22.0_f32;
                let center = egui::pos2(
                    rect.left() + rect.width() * 0.5 + self.table_3d_pan.x,
                    rect.top() + rect.height() * 0.55 + self.table_3d_pan.y,
                );

                let yaw = self.table_3d_yaw;
                let pitch = self.table_3d_pitch;
                let cy = yaw.cos();
                let sy = yaw.sin();
                let cp = pitch.cos();
                let sp = pitch.sin();

                let rotate = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
                    let xr = x * cy - y * sy;
                    let yr = x * sy + y * cy;
                    let yr2 = yr * cp - z * sp;
                    let zr2 = yr * sp + z * cp;
                    (xr, yr2, zr2)
                };

                let project = |x: f32, y: f32, z: f32| -> (egui::Pos2, f32) {
                    let (xr, yr, zr) = rotate(x, y, z);
                    let depth = (cam_dist + yr).max(1.0);
                    let px = center.x + (xr * fov / depth);
                    let py = center.y - (zr * fov / depth);
                    (egui::pos2(px, py), depth)
                };

                for col in 0..=AFR_TABLE_SIZE {
                    let xw = (col as f32 - half) * (unit + gap);
                    let (p0, _) = project(xw, half * (unit + gap), 0.0);
                    let (p1, _) = project(xw, -(half + 1.0) * (unit + gap), 0.0);
                    painter.line_segment(
                        [p0, p1],
                        egui::Stroke::new(
                            if col % 5 == 0 { 1.0_f32 } else { 0.75_f32 },
                            egui::Color32::from_gray(if col % 5 == 0 { 56 } else { 38 }),
                        ),
                    );
                }
                for row in 0..=AFR_TABLE_SIZE {
                    let yw = (half - row as f32) * (unit + gap);
                    let (p0, _) = project(-half * (unit + gap), yw, 0.0);
                    let (p1, _) = project((half + 1.0) * (unit + gap), yw, 0.0);
                    painter.line_segment(
                        [p0, p1],
                        egui::Stroke::new(
                            if row % 5 == 0 { 1.0_f32 } else { 0.75_f32 },
                            egui::Color32::from_gray(if row % 5 == 0 { 56 } else { 38 }),
                        ),
                    );
                }

                struct Face {
                    points: Vec<egui::Pos2>,
                    color: egui::Color32,
                    stroke: egui::Stroke,
                    depth: f32,
                }

                let mut faces: Vec<Face> = Vec::with_capacity(AFR_TABLE_SIZE * AFR_TABLE_SIZE * 5);

                for row in (0..AFR_TABLE_SIZE).rev() {
                    for col in 0..AFR_TABLE_SIZE {
                        let cell_val = self.selected_table_value(metric, row, col);
                        let ratio = Self::table_value_ratio(metric, cell_val);
                        let z = ratio * z_height_max;

                        let x0 = (col as f32 - half) * (unit + gap);
                        let x1 = x0 + unit;
                        let y0 = (half - row as f32) * (unit + gap);
                        let y1 = y0 - unit;

                        let (b0, d0) = project(x0, y0, 0.0);
                        let (b1, d1) = project(x1, y0, 0.0);
                        let (b2, d2) = project(x1, y1, 0.0);
                        let (b3, d3) = project(x0, y1, 0.0);

                        let (t0, dt0) = project(x0, y0, z);
                        let (t1, dt1) = project(x1, y0, z);
                        let (t2, dt2) = project(x1, y1, z);
                        let (t3, dt3) = project(x0, y1, z);

                        let top_color = Self::table_value_color(metric, cell_val);
                        let side_color_a = Self::scale_color(top_color, 0.72);
                        let side_color_b = Self::scale_color(top_color, 0.58);

                        faces.push(Face {
                            points: vec![b1, b2, t2, t1],
                            color: side_color_a,
                            stroke: egui::Stroke::NONE,
                            depth: (d1 + d2 + dt2 + dt1) * 0.25,
                        });
                        faces.push(Face {
                            points: vec![b0, b1, t1, t0],
                            color: side_color_b,
                            stroke: egui::Stroke::NONE,
                            depth: (d0 + d1 + dt1 + dt0) * 0.25,
                        });
                        faces.push(Face {
                            points: vec![b2, b3, t3, t2],
                            color: Self::scale_color(top_color, 0.64),
                            stroke: egui::Stroke::NONE,
                            depth: (d2 + d3 + dt3 + dt2) * 0.25,
                        });
                        faces.push(Face {
                            points: vec![b3, b0, t0, t3],
                            color: Self::scale_color(top_color, 0.52),
                            stroke: egui::Stroke::NONE,
                            depth: (d3 + d0 + dt0 + dt3) * 0.25,
                        });

                        faces.push(Face {
                            points: vec![t0, t1, t2, t3],
                            color: top_color,
                            stroke: if live_cells.contains(&(row, col)) {
                                egui::Stroke::new(1.2_f32, egui::Color32::YELLOW)
                            } else {
                                egui::Stroke::new(0.8_f32, egui::Color32::from_gray(40))
                            },
                            depth: (dt0 + dt1 + dt2 + dt3) * 0.25,
                        });
                    }
                }

                faces.sort_by(|a, b| {
                    b.depth
                        .partial_cmp(&a.depth)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for face in faces {
                    painter.add(egui::Shape::convex_polygon(face.points, face.color, face.stroke));
                }

                for &(live_row, live_col) in &live_cells {
                    let live_cell_val = self.selected_table_value(metric, live_row, live_col);
                    let live_ratio = Self::table_value_ratio(metric, live_cell_val);
                    let live_z = live_ratio * z_height_max;
                    let live_cx = (live_col as f32 - half) * (unit + gap) + (unit * 0.5);
                    let live_cy = (half - live_row as f32) * (unit + gap) - (unit * 0.5);
                    let (live_base, _) = project(live_cx, live_cy, 0.0);
                    let (live_top, _) = project(live_cx, live_cy, live_z + 0.08);
                    painter.line_segment(
                        [live_base, live_top],
                        egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(255, 235, 90)),
                    );
                    painter.circle_filled(live_top, 4.2, egui::Color32::from_rgb(255, 235, 90));
                }

                for idx in [0_usize, 5, 10, 15] {
                    let map_val = AFR_TABLE_MAP_MIN
                        + (idx as f64 / (AFR_TABLE_SIZE as f64 - 1.0))
                            * (AFR_TABLE_MAP_MAX - AFR_TABLE_MAP_MIN);
                    let xw = (idx as f32 - half) * (unit + gap);
                    let (x_proj, _) = project(xw, -(half + 2.0), 0.0);
                    painter.text(
                        egui::pos2(x_proj.x, rect.bottom() - 18.0),
                        egui::Align2::CENTER_TOP,
                        format!("{:.0}", map_val),
                        egui::TextStyle::Small.resolve(ui.style()),
                        egui::Color32::LIGHT_GRAY,
                    );
                }

                for idx in [0_usize, 5, 10, 15] {
                    let rpm_val = AFR_TABLE_RPM_MIN
                        + (idx as f64 / (AFR_TABLE_SIZE as f64 - 1.0))
                            * (AFR_TABLE_RPM_MAX - AFR_TABLE_RPM_MIN);
                    let yw = (half - idx as f32) * (unit + gap);
                    let (y_proj, _) = project(-(half + 2.0), yw, 0.0);
                    painter.text(
                        egui::pos2(rect.left() + 20.0, y_proj.y),
                        egui::Align2::RIGHT_CENTER,
                        format!("{:.0}", rpm_val),
                        egui::TextStyle::Small.resolve(ui.style()),
                        egui::Color32::LIGHT_GRAY,
                    );
                }

                painter.text(
                    egui::pos2(rect.center().x, rect.bottom() - 2.0),
                    egui::Align2::CENTER_TOP,
                    "MAP (mbar)",
                    egui::TextStyle::Body.resolve(ui.style()),
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "RPM",
                    egui::TextStyle::Body.resolve(ui.style()),
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::pos2(rect.right() - 8.0, rect.top() + 6.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("Z: {}", metric.label()),
                    egui::TextStyle::Body.resolve(ui.style()),
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::pos2(rect.left() + 10.0, rect.top() + 10.0),
                    egui::Align2::LEFT_TOP,
                    "LMB pan | RMB rotate | Wheel zoom",
                    egui::TextStyle::Small.resolve(ui.style()),
                    egui::Color32::from_gray(190),
                );
            });

        self.show_table_3d_window = open;
    }

    fn draw_table_3d_inline(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_min_width(440.0);
            ui.set_max_width(440.0);
            ui.set_min_height(320.0);
            ui.set_max_height(320.0);
            ui.style_mut().spacing.item_spacing = egui::vec2(2.0, 2.0);
            ui.style_mut().spacing.button_padding = egui::vec2(4.0, 1.0);

            let tiny_btn = |label: &str| egui::Button::new(egui::RichText::new(label).size(10.0));

            ui.horizontal(|ui| {
                for metric in TableMetric::ALL {
                    if ui
                        .add_sized(
                            [60.0, 14.0],
                            tiny_btn(metric.label()).selected(self.table_metric == metric),
                        )
                        .clicked()
                    {
                        self.table_metric = metric;
                    }
                }
            });

            ui.horizontal(|ui| {
                if ui.add_sized([46.0, 14.0], tiny_btn("CLEAR")).clicked() {
                    self.clear_table_metric(self.table_metric);
                }
                if ui.add_sized([46.0, 14.0], tiny_btn("RESET")).clicked() {
                    self.table_3d_pan = egui::vec2(0.0, 0.0);
                    self.table_3d_scale = 1.0;
                    self.table_3d_yaw = -0.75;
                    self.table_3d_pitch = 0.70;
                }
                ui.label(egui::RichText::new("ZOOM").size(10.0));
                ui.add_sized(
                    [112.0, 14.0],
                    egui::Slider::new(&mut self.table_3d_scale, 0.6..=1.8).show_value(false),
                );
            });

            ui.add_space(2.0);

            let desired = egui::vec2(428.0, 258.0);
            let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::drag());

            if response.hovered() && ui.input(|i| i.pointer.secondary_down()) {
                let delta = ui.input(|i| i.pointer.delta());
                self.table_3d_yaw += delta.x * 0.01;
                self.table_3d_pitch = (self.table_3d_pitch - delta.y * 0.01).clamp(0.2, 1.3);
            }

            let painter = ui.painter_at(rect);

            painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 14, 14));
            painter.rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
                egui::StrokeKind::Inside,
            );

            let metric = self.table_metric;
            let half = AFR_TABLE_SIZE as f32 * 0.5;
            let unit = 1.0_f32;
            let gap = 0.08_f32;
            let z_height_max = 4.0_f32;
            let fov = 200.0_f32 * self.table_3d_scale;
            let cam_dist = 22.0_f32;
            let center = egui::pos2(
                rect.left() + rect.width() * 0.5 + self.table_3d_pan.x * 0.35,
                rect.top() + rect.height() * 0.58 + self.table_3d_pan.y * 0.35,
            );

            let yaw = self.table_3d_yaw;
            let pitch = self.table_3d_pitch;
            let cy = yaw.cos();
            let sy = yaw.sin();
            let cp = pitch.cos();
            let sp = pitch.sin();

            let rotate = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
                let xr = x * cy - y * sy;
                let yr = x * sy + y * cy;
                let yr2 = yr * cp - z * sp;
                let zr2 = yr * sp + z * cp;
                (xr, yr2, zr2)
            };

            let project = |x: f32, y: f32, z: f32| -> (egui::Pos2, f32) {
                let (xr, yr, zr) = rotate(x, y, z);
                let depth = (cam_dist + yr).max(1.0);
                let px = center.x + (xr * fov / depth);
                let py = center.y - (zr * fov / depth);
                (egui::pos2(px, py), depth)
            };

            struct Face {
                points: Vec<egui::Pos2>,
                color: egui::Color32,
                stroke: egui::Stroke,
                depth: f32,
            }

            let mut faces: Vec<Face> = Vec::with_capacity(AFR_TABLE_SIZE * AFR_TABLE_SIZE * 5);

            for row in (0..AFR_TABLE_SIZE).rev() {
                for col in 0..AFR_TABLE_SIZE {
                    let cell_val = self.selected_table_value(metric, row, col);
                    let ratio = Self::table_value_ratio(metric, cell_val);
                    let z = ratio * z_height_max;

                    let x0 = (col as f32 - half) * (unit + gap);
                    let x1 = x0 + unit;
                    let y0 = (half - row as f32) * (unit + gap);
                    let y1 = y0 - unit;

                    let (b0, d0) = project(x0, y0, 0.0);
                    let (b1, d1) = project(x1, y0, 0.0);
                    let (b2, d2) = project(x1, y1, 0.0);
                    let (b3, d3) = project(x0, y1, 0.0);

                    let (t0, dt0) = project(x0, y0, z);
                    let (t1, dt1) = project(x1, y0, z);
                    let (t2, dt2) = project(x1, y1, z);
                    let (t3, dt3) = project(x0, y1, z);

                    let top_color = Self::table_value_color(metric, cell_val);
                    let side_color_a = Self::scale_color(top_color, 0.72);
                    let side_color_b = Self::scale_color(top_color, 0.58);

                    faces.push(Face {
                        points: vec![b1, b2, t2, t1],
                        color: side_color_a,
                        stroke: egui::Stroke::NONE,
                        depth: (d1 + d2 + dt2 + dt1) * 0.25,
                    });
                    faces.push(Face {
                        points: vec![b0, b1, t1, t0],
                        color: side_color_b,
                        stroke: egui::Stroke::NONE,
                        depth: (d0 + d1 + dt1 + dt0) * 0.25,
                    });
                    faces.push(Face {
                        points: vec![b2, b3, t3, t2],
                        color: Self::scale_color(top_color, 0.64),
                        stroke: egui::Stroke::NONE,
                        depth: (d2 + d3 + dt3 + dt2) * 0.25,
                    });
                    faces.push(Face {
                        points: vec![b3, b0, t0, t3],
                        color: Self::scale_color(top_color, 0.52),
                        stroke: egui::Stroke::NONE,
                        depth: (d3 + d0 + dt0 + dt3) * 0.25,
                    });
                    faces.push(Face {
                        points: vec![t0, t1, t2, t3],
                        color: top_color,
                        stroke: egui::Stroke::new(0.7_f32, egui::Color32::from_gray(40)),
                        depth: (dt0 + dt1 + dt2 + dt3) * 0.25,
                    });
                }
            }

            faces.sort_by(|a, b| {
                b.depth
                    .partial_cmp(&a.depth)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            for face in faces {
                painter.add(egui::Shape::convex_polygon(face.points, face.color, face.stroke));
            }
        });
    }

    fn poll_datalog(&mut self) {
        if !self.datalog_connected {
            return;
        }
        if self.last_poll.elapsed().as_millis() < 100 {
            return;
        }
        self.last_poll = Instant::now();

        let mut port = match serialport::new(&self.datalog_com_port, 38400)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(port) => port,
            Err(_) => return,
        };

        if port.write_all(&[0x20]).is_err() {
            return;
        }

        let mut packet = [0u8; 52];
        if port.read_exact(&mut packet).is_err() {
            return;
        }

        let mut ect = packet[0] as f64 / 51.0;
        ect = (0.1423 * ect.powi(6))
            - (2.4938 * ect.powi(5))
            + (17.837 * ect.powi(4))
            - (68.698 * ect.powi(3))
            + (154.69 * ect.powi(2))
            - (232.75 * ect)
            + 284.24;
        self.ect = ((ect - 32.0) * 5.0) / 9.0;

        let mut iat = packet[1] as f64 / 51.0;
        iat = (0.1423 * iat.powi(6))
            - (2.4938 * iat.powi(5))
            + (17.837 * iat.powi(4))
            - (68.698 * iat.powi(3))
            + (154.69 * iat.powi(2))
            - (232.75 * iat)
            + 284.24;
        self.iat = ((iat - 32.0) * 5.0) / 9.0;

        self.tps = Self::clamp_f64((((packet[5] as f64) - 25.0) / 2.04).round(), 0.0, 100.0);

        // PGMFI OBD1 reference: mbar ~= 365.9 * volts - 29.9
        self.map_volt = Self::clamp_f64(packet[4] as f64 * 0.0196078438311815, 0.0, 5.0);
        self.map = Self::clamp_f64((365.9 * self.map_volt) - 29.9, HONDA_MBAR_MIN, HONDA_MBAR_MAX);

        let rpm_div = Self::long2bytes(packet[6], packet[7]);
        self.rpm = if rpm_div == 0 {
            0.0
        } else {
            1_851_562.0 / rpm_div as f64
        };
        self.rpm = Self::clamp_f64(self.rpm, 0.0, 11_000.0);

        self.boost_psi = if self.map <= 1013.0 {
            0.0
        } else {
            ((self.map - 1013.0) * 0.0145037695765495).clamp(0.0, HONDA_PSI_MAX)
        };

        self.tps_volt = Self::clamp_f64(packet[5] as f64 * 0.0196078438311815, 0.0, 5.0);

        let wb_volt = packet[2] as f64 * 0.0196078438311815;
        let wb_lambda = if wb_volt < 0.0 {
            0.71
        } else if wb_volt > 5.0 {
            1.3
        } else {
            0.71 + (((wb_volt - 0.0) * (1.3 - 0.71)) / (5.0 - 0.0))
        };
        let afr_raw = (wb_lambda * 14.7).clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);
        self.afr = (afr_raw + self.afr_offset).clamp(AFR_GRAPH_MIN, AFR_GRAPH_MAX);
        self.lambda = self.afr / 14.7;

        let inj_raw = Self::long2bytes(packet[17], packet[18]);
        self.inj_ms = (inj_raw as f64 * 3.20000004768372) / 1000.0;
        self.injector_duty = (self.rpm * self.inj_ms) / 1200.0;
        self.inj_fv = inj_raw as f64 / 4.0;

        self.ign_advance = (0.25 * packet[19] as f64) - 6.0;
        self.battery = (26.0 * packet[25] as f64) / 270.0;
        self.eld_volt = Self::clamp_f64(packet[24] as f64 * 0.0196078438311815, 0.0, 5.0);

        self.vss_kmh = packet[16] as f64;
        self.gear = Self::calc_gear(self.vss_kmh, self.rpm, rpm_div);

        let iacv_raw = Self::long2bytes(packet[49], packet[50]) as f64;
        self.iacv_duty = Self::clamp_f64((iacv_raw / 327.68) - 100.0, -100.0, 100.0);
        self.gear_ic = {
            let b = packet[36] as i32;
            if b == 128 {
                0.0
            } else if b < 128 {
                ((128 - b) as f64) * -0.25
            } else {
                ((b - 128) as f64) * 0.25
            }
        };
        self.ebc_duty = Self::clamp_f64(packet[41] as f64 / 2.0, 0.0, 100.0);
        self.ebc_base_duty = Self::ebc_value(packet[40]);
        self.instant_consumption = Self::calc_instant_consumption(self.vss_kmh, self.injector_duty);

        self.ect_fc = Self::fc_ratio_u8(packet[26], 128.0);
        self.o2_short_fc = Self::fc_ratio(Self::long2bytes(packet[27], packet[28]), 32768.0);
        self.o2_long_fc = Self::fc_ratio(Self::long2bytes(packet[29], packet[30]), 32768.0);
        self.iat_fc = Self::fc_ratio(Self::long2bytes(packet[31], packet[32]), 32768.0);
        self.ve_fc = Self::fc_ratio_u8(packet[33], 128.0);
        self.iat_ic = Self::ic_value(packet[34]);
        self.ect_ic = Self::ic_value(packet[35]);

        self.vts_active = Self::bit_is_set(packet[23], 7);
        self.vtp_active = Self::bit_is_set(packet[21], 3);
        self.vts_feedback_active = Self::bit_is_set(packet[21], 6);
        self.park_n_active = Self::bit_is_set(packet[21], 0);
        self.bksw_active = Self::bit_is_set(packet[21], 1);
        self.acc_active = Self::bit_is_set(packet[21], 2);
        self.start_active = Self::bit_is_set(packet[21], 4);
        self.scc_active = Self::bit_is_set(packet[21], 5);
        self.psp_active = Self::bit_is_set(packet[21], 7);
        self.mil_active = Self::bit_is_set(packet[23], 5);
        self.fan_active = Self::bit_is_set(packet[39], 6);
        self.output_2nd_map_active = Self::bit_is_set(packet[39], 5);
        self.flr_active = Self::bit_is_set(packet[39], 0);
        self.output_fts_active = Self::bit_is_set(packet[39], 2);
        self.fuelcut1_active = Self::bit_is_set(packet[8], 4);
        self.fuelcut2_active = Self::bit_is_set(packet[8], 5);
        self.igncut_active = Self::bit_is_set(packet[8], 2);
        self.scc_checker_active = Self::bit_is_set(packet[8], 1);
        self.vtsm_active = Self::bit_is_set(packet[8], 3);
        self.post_fuel_active = Self::bit_is_set(packet[8], 0);
        self.at_shift1_active = Self::bit_is_set(packet[8], 6);
        self.at_shift2_active = Self::bit_is_set(packet[8], 7);
        self.leanprotect_active = Self::bit_is_set(packet[43], 7);
        self.boostcut_active = Self::bit_is_set(packet[39], 3);
        self.bst_active = Self::bit_is_set(packet[39], 7);
        self.antilag_active = Self::bit_is_set(packet[39], 1);
        self.ebc_active = Self::bit_is_set(packet[39], 4);
        self.ac_active = Self::bit_is_set(packet[22], 7);
        self.atlctrl_active = Self::bit_is_set(packet[22], 5);
        self.o2heater_active = Self::bit_is_set(packet[23], 6);
        self.iab_active = Self::bit_is_set(packet[22], 2);
        self.purge_active = Self::bit_is_set(packet[22], 6);
        self.fuelpump_active = Self::bit_is_set(packet[22], 0);
        self.input_ftl_active = Self::bit_is_set(packet[38], 0);
        self.input_fts_active = Self::bit_is_set(packet[38], 1);
        self.input_ebc_active = Self::bit_is_set(packet[38], 2);
        self.input_ebc_hi_active = Self::bit_is_set(packet[38], 3);
        self.input_bst_active = Self::bit_is_set(packet[38], 7);
        self.input_gpo1_active = Self::bit_is_set(packet[38], 4);
        self.input_gpo2_active = Self::bit_is_set(packet[38], 5);
        self.input_gpo3_active = Self::bit_is_set(packet[38], 6);
        self.gpo1_active = Self::bit_is_set(packet[43], 0);
        self.gpo2_active = Self::bit_is_set(packet[43], 1);
        self.gpo3_active = Self::bit_is_set(packet[43], 2);
        self.bst_stage2_active = Self::bit_is_set(packet[43], 3);
        self.bst_stage3_active = Self::bit_is_set(packet[43], 4);
        self.bst_stage4_active = Self::bit_is_set(packet[43], 5);

        if self.follow_vtec_tables {
            self.sync_table_kinds_to_vtec();
        }

        self.push_sensor_sample();
        self.stack_live_tracking_afr_sample();
        self.update_table_metrics();
    }
}

impl eframe::App for HondaGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_datalog();

        let mut style = (*ctx.style()).clone();
        style.visuals.window_fill = egui::Color32::from_rgb(10, 10, 10);
        style.visuals.panel_fill = egui::Color32::from_rgb(14, 14, 14);
        style.visuals.override_text_color = Some(egui::Color32::WHITE);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(190, 24, 24);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(155, 30, 30);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 30, 30);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.heading(egui::RichText::new("SUPRAROM HONDA STUDIO").size(30.0).color(
                        egui::Color32::from_rgb(224, 46, 46),
                    ));
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("RTP/Datalog software made by Yosupra")
                                .color(egui::Color32::GRAY),
                        );
                        if ui.button("ABOUT").clicked() {
                            self.show_about_window = true;
                        }
                    });
                });

                let log_panel_width = 1000.0_f32;
                let spacer = (ui.available_width() - log_panel_width - 10.0).max(8.0);
                ui.add_space(spacer);

                ui.group(|ui| {
                    ui.set_min_width(log_panel_width);
                    ui.set_max_width(log_panel_width);
                    ui.heading("LOG");
                    // Keep the log panel footprint stable in the header.
                    ui.set_min_height(66.0);
                    ui.set_max_height(66.0);
                    for line in self.log.iter().rev().take(3) {
                        ui.add(egui::Label::new(line).truncate());
                    }
                });
            });

            ui.add_space(1.0);
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("DETECT SUPRAROM").clicked() {
                        self.detect_device();
                    }

                    let connected_color = if self.selected_serial.is_some() {
                        egui::Color32::from_rgb(30, 170, 90)
                    } else {
                        egui::Color32::from_rgb(170, 40, 40)
                    };
                    ui.label(egui::RichText::new("DEVICE").strong());
                    ui.colored_label(connected_color, &self.device_status);
                    ui.separator();
                    ui.label(
                        self.selected_serial
                            .clone()
                            .unwrap_or_else(|| "Serial: <none>".to_string()),
                    );
                });
            });

            ui.add_space(4.0);

            ui.separator();

            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .id_salt("main_content_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
            ui.columns(2, |columns| {
                columns[0].group(|ui| {
                    let state_button = |ui: &mut egui::Ui, label: &str, active: bool| {
                        let text = if active {
                            egui::RichText::new(label)
                                .color(egui::Color32::from_rgb(30, 190, 90))
                                .strong()
                        } else {
                            egui::RichText::new(label)
                        };
                        ui.add(egui::Button::new(text))
                    };

                    egui::CollapsingHeader::new("SUPRAROM CONTROL")
                        .id_salt("rom_control_section")
                        .default_open(false)
                        .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("BASEMAP").clicked() {
                            self.show_new_bin_window = true;
                        }
                        if ui.button("OPEN BIN").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("BIN", &["bin"])
                                .pick_file()
                            {
                                self.selected_file = path.display().to_string();
                                self.rom_has_unsaved_changes = false;
                                self.log.push(format!("Selected file: {}", self.selected_file));
                            }
                        }
                        ui.label(&self.selected_file);
                    });

                    ui.horizontal(|ui| {
                        let hex_label = if self.show_hex_window {
                            "HEX ON"
                        } else {
                            "HEX"
                        };

                        if ui.button("SAVE BIN").clicked() {
                            self.save_staged_rom_to_selected_bin();
                        }
                        if ui.button("BURN ROM").clicked() {
                            self.upload_selected_bin();
                        }
                        if ui.button("READ ROM / SAVE").clicked() {
                            self.read_rom_32k_and_save();
                        }
                        if ui.button("READ ROM DATA").clicked() {
                            self.read_honda_data_from_bin();
                        }
                        if ui.button("READ BIN FILE").clicked() {
                            self.read_selected_bin_file(false);
                        }
                        if ui.button("FUEL TABLE").clicked() {
                            if self.last_read_rom.is_none() && self.selected_file_is_usable() {
                                self.read_selected_bin_file(false);
                            }
                            self.show_rom_table_window = true;
                        }
                        if ui.button("IGNITION TABLE").clicked() {
                            if self.last_read_rom.is_none() && self.selected_file_is_usable() {
                                self.read_selected_bin_file(false);
                            }
                            self.show_rom_ign_table_window = true;
                        }
                        if ui.button(hex_label).clicked() {
                            if !self.show_hex_window
                                && self.last_read_rom.is_none()
                                && self.selected_file_is_usable()
                            {
                                self.read_selected_bin_file(false);
                            }
                            self.show_hex_window = !self.show_hex_window;
                        }
                    });

                    if self.rom_has_unsaved_changes {
                        ui.colored_label(
                            egui::Color32::from_rgb(210, 120, 40),
                            "PENDING CHANGES: staged in memory (click SAVE BIN)",
                        );
                    }

                    let persistent_target = !self.rom_com_port.trim().is_empty();
                    let target_text = if persistent_target {
                        "WRITE TARGET: FLASH PERSISTENT (OSTRICH via Port)"
                    } else {
                        "WRITE TARGET: LIVE TEMP (set Port for persistent flash)"
                    };
                    let target_color = if persistent_target {
                        egui::Color32::from_rgb(30, 190, 90)
                    } else {
                        egui::Color32::from_rgb(210, 120, 40)
                    };
                    ui.colored_label(target_color, target_text);

                    ui.horizontal(|ui| {
                        let previous_rom_port = self.rom_com_port.clone();
                        let selected_text = if self.rom_com_port.trim().is_empty() {
                            "Select RTP port".to_string()
                        } else {
                            self.rom_com_port.clone()
                        };

                        egui::ComboBox::from_id_salt("rom_port_selector")
                            .width(280.0)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for port in &self.rom_available_ports {
                                    ui.selectable_value(&mut self.rom_com_port, port.clone(), port);
                                }
                            });

                        if self.rom_com_port != previous_rom_port && !self.rom_com_port.is_empty() {
                            self.log
                                .push(format!("Selected Live ROM port: {}", self.rom_com_port));
                        }

                        if ui.button("SCAN").clicked() {
                            self.scan_rom_ports();
                        }
                    });

                    if self.show_hex_window {
                        ui.group(|ui| {
                            ui.heading("ROM HEX DUMP");
                            egui::ScrollArea::vertical()
                                .id_salt("rom_hex_inline_scroll")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.rom_hex_dump)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .interactive(false),
                                    );
                                });
                        });
                    }

                        });

                    ui.separator();
                        egui::CollapsingHeader::new("ROM SETTINGS")
                            .id_salt("rom_settings_section")
                            .default_open(false)
                            .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("BLINK: {}", self.mil_blinks));
                        ui.label("New:");
                        ui.text_edit_singleline(&mut self.new_mil_blinks);
                        if state_button(ui, "BLINK", false).clicked() {
                            self.apply_blink_to_bin();
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if state_button(ui, "IACV ON", self.iacv_value == 0x00).clicked() {
                            self.apply_patchset_to_bin("IACV ON", &[(IACV_OFFSET, 0x00)]);
                        }
                        if state_button(ui, "NO IACV", self.iacv_value == 0xFF).clicked() {
                            self.apply_patchset_to_bin("NO IACV", &[(IACV_OFFSET, 0xFF)]);
                        }
                        if state_button(
                            ui,
                            "SHIFT LIGHT ON",
                            self.shift_light_enable_value == 0xFF,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "SHIFT LIGHT ON",
                                &[(SHIFT_LIGHT_ENABLE_OFFSET, 0xFF)],
                            );
                        }
                        if state_button(
                            ui,
                            "SHIFT LIGHT OFF",
                            self.shift_light_enable_value == 0x00,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "SHIFT LIGHT OFF",
                                &[(SHIFT_LIGHT_ENABLE_OFFSET, 0x00)],
                            );
                        }
                        if state_button(
                            ui,
                            "FAN CONTROL ON",
                            self.fan_control_enable_value == 0xFF,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "FAN CONTROL ON",
                                &[(FAN_CONTROL_ENABLE_OFFSET, 0xFF)],
                            );
                        }
                        if state_button(
                            ui,
                            "FAN CONTROL OFF",
                            self.fan_control_enable_value == 0x00,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "FAN CONTROL OFF",
                                &[(FAN_CONTROL_ENABLE_OFFSET, 0x00)],
                            );
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.new_shift_light_rpm_value);
                        if state_button(ui, "SHIFT RPM", false).clicked() {
                            let input = self.new_shift_light_rpm_value.trim().replace(',', ".");
                            if let Ok(target_rpm) = input.parse::<f64>() {
                                if let Some(raw) = Self::shift_light_rpm_to_raw(target_rpm) {
                                    let [lo, hi] = raw.to_le_bytes();
                                    self.apply_patchset_to_bin(
                                        "SHIFT LIGHT RPM",
                                        &[(SHIFT_LIGHT_RPM_LO_OFFSET, lo), (SHIFT_LIGHT_RPM_HI_OFFSET, hi)],
                                    );
                                    self.log.push(format!(
                                        "SHIFT LIGHT target {:.0} rpm -> raw 0x{:04X} ({:.0} rpm)",
                                        target_rpm,
                                        raw,
                                        Self::shift_light_raw_to_rpm(raw)
                                    ));
                                } else {
                                    self.log.push(
                                        "Invalid SHIFT RPM value. Enter a positive RPM (example: 7000)."
                                            .to_string(),
                                    );
                                }
                            } else {
                                self.log.push(
                                    "Invalid SHIFT RPM value. Enter a positive RPM (example: 7000)."
                                        .to_string(),
                                );
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.new_fan_control_temp_value);
                        if state_button(ui, "FAN TEMP", false).clicked() {
                            let input = self.new_fan_control_temp_value.trim().replace(',', ".");
                            if let Ok(target_c) = input.parse::<f64>() {
                                let raw = Self::ect_celsius_to_raw(target_c);
                                self.apply_patchset_to_bin("FAN TEMP", &[(FAN_CONTROL_TEMP_OFFSET, raw)]);
                                self.log.push(format!(
                                    "FAN TEMP target {:.1} C -> raw 0x{:02X} ({:.1} C)",
                                    target_c,
                                    raw,
                                    Self::ect_raw_to_celsius(raw)
                                ));
                            } else {
                                self.log.push(
                                    "Invalid FAN TEMP value. Enter Celsius (example: 84 or 84.5).".to_string(),
                                );
                            }
                        }
                    });

                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if state_button(
                            ui,
                            "FUEL CUT",
                            self.cut_a_value == 0x00 && self.cut_b_value == 0x00,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "CUT 00/00",
                                &[(CUT_A_OFFSET, 0x00), (CUT_B_OFFSET, 0x00)],
                            );
                        }
                        if state_button(
                            ui,
                            "IGN CUT",
                            self.cut_a_value == 0xFF && self.cut_b_value == 0xFF,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "CUT FF/FF",
                                &[(CUT_A_OFFSET, 0xFF), (CUT_B_OFFSET, 0xFF)],
                            );
                        }
                        if state_button(
                            ui,
                            "FUELCUT + IGNCUT",
                            self.cut_a_value == 0xFF && self.cut_b_value == 0x00,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "CUT FF/00",
                                &[(CUT_A_OFFSET, 0xFF), (CUT_B_OFFSET, 0x00)],
                            );
                        }
                        if state_button(ui, "LAUNCH OFF", self.launch_value == 0x00).clicked() {
                            self.apply_patchset_to_bin("LAUNCH OFF", &[(LAUNCH_OFFSET, 0x00)]);
                        }
                        if state_button(ui, "LAUNCH ON", self.launch_value == 0x80).clicked() {
                            self.apply_patchset_to_bin("LAUNCH ON", &[(LAUNCH_OFFSET, 0x80)]);
                        }
                        if state_button(
                            ui,
                            "LAUNCH TPS OFF",
                            self.launch_tps_enable_value == 0x00 && self.launch_tps_value == 0x5F,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "LAUNCH TPS OFF",
                                &[
                                    (LAUNCH_TPS_ENABLE_OFFSET, 0x00),
                                    (LAUNCH_TPS_VALUE_OFFSET, 0x5F),
                                ],
                            );
                        }
                        if state_button(
                            ui,
                            "LAUNCH TPS ON",
                            self.launch_tps_enable_value == 0xFF && self.launch_tps_value == 0x5E,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "LAUNCH TPS ON",
                                &[
                                    (LAUNCH_TPS_ENABLE_OFFSET, 0xFF),
                                    (LAUNCH_TPS_VALUE_OFFSET, 0x5E),
                                ],
                            );
                        }
                        
                    });

                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("IGN CUT timing:");
                        for ms in [50_u16, 70, 100] {
                            if let Some(v) = Self::ign_timing_preset_value(ms) {
                                let active = self.ign_timing_61d8 == v
                                    && self.ign_timing_61e4 == v
                                    && self.ign_timing_61e5 == v;
                                if state_button(ui, &format!("{ms}ms"), active).clicked() {
                                    self.apply_patchset_to_bin(
                                        &format!("IGN TIMING {ms}ms"),
                                        &[
                                            (IGN_TIMING_61D8_OFFSET, v),
                                            (IGN_TIMING_61E4_OFFSET, v),
                                            (IGN_TIMING_61E5_OFFSET, v),
                                        ],
                                    );
                                }
                            }
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if state_button(
                            ui,
                            "VTEC ON",
                            self.vtec_sw_6120 == 0x44
                                && self.vtec_sw_61f2 == 0xFF
                                && self.vtec_sw_6657 == 0x52
                                && self.vtec_sw_6659 == 0x37,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "VTEC ON",
                                &[
                                    (VTEC_SW_6120_OFFSET, 0x44),
                                    (VTEC_SW_61F2_OFFSET, 0xFF),
                                    (VTEC_SW_6657_OFFSET, 0x52),
                                    (VTEC_SW_6659_OFFSET, 0x37),
                                ],
                            );
                        }
                        if state_button(
                            ui,
                            "VTEC OFF",
                            self.vtec_sw_6120 == 0x43
                                && self.vtec_sw_61f2 == 0x00
                                && self.vtec_sw_6657 == 0x54
                                && self.vtec_sw_6659 == 0x33,
                        )
                        .clicked()
                        {
                            self.apply_patchset_to_bin(
                                "VTEC OFF",
                                &[
                                    (VTEC_SW_6120_OFFSET, 0x43),
                                    (VTEC_SW_61F2_OFFSET, 0x00),
                                    (VTEC_SW_6657_OFFSET, 0x54),
                                    (VTEC_SW_6659_OFFSET, 0x33),
                                ],
                            );
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("VTEC RPM:");
                        for rpm in [4000_u16, 4500, 5000, 5500, 6000] {
                            if let Some((a, b)) = Self::vtec_rpm_preset_values(rpm) {
                                let active = self.vtec_val_6658 == a && self.vtec_val_665a == b;
                                if state_button(ui, &format!("{rpm}"), active).clicked() {
                                    self.apply_patchset_to_bin(
                                        &format!("VTEC RPM {rpm}"),
                                        &[(VTEC_VAL_6658_OFFSET, a), (VTEC_VAL_665A_OFFSET, b)],
                                    );
                                }
                            }
                        }
                    });

                    });

                    ui.separator();
                    egui::CollapsingHeader::new("MAP EDIT/VIEW")
                        .id_salt("rom_map_view_section")
                        .default_open(false)
                        .show(ui, |ui| {
                        ui.set_min_height(620.0);
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    self.rom_embedded_view == RomEmbeddedView::Fuel,
                                    "FUEL TABLE",
                                )
                                .clicked()
                            {
                                self.rom_embedded_view = RomEmbeddedView::Fuel;
                                self.show_rom_table_window = false;
                                self.show_rom_ign_table_window = false;
                            }
                            if ui
                                .selectable_label(
                                    self.rom_embedded_view == RomEmbeddedView::Ign,
                                    "IGNITION TABLE",
                                )
                                .clicked()
                            {
                                self.rom_embedded_view = RomEmbeddedView::Ign;
                                self.show_rom_table_window = false;
                                self.show_rom_ign_table_window = false;
                            }
                            let follow_vtec_btn = egui::Button::new(
                                egui::RichText::new(if self.follow_vtec_tables {
                                    "FOLLOW VTEC"
                                } else {
                                    "FOLLOW VTEC OFF"
                                })
                                .strong()
                                .color(egui::Color32::WHITE),
                            )
                            .fill(if self.follow_vtec_tables {
                                egui::Color32::from_rgb(35, 130, 255)
                            } else {
                                egui::Color32::from_rgb(45, 45, 45)
                            });
                            if ui.add(follow_vtec_btn).clicked() {
                                self.follow_vtec_tables = !self.follow_vtec_tables;
                                if self.follow_vtec_tables {
                                    self.sync_table_kinds_to_vtec();
                                }
                            }
                            let burn_btn = egui::Button::new(
                                egui::RichText::new("BURN ROM")
                                    .color(egui::Color32::BLACK)
                                    .strong(),
                            )
                            .fill(egui::Color32::from_rgb(220, 40, 40));
                            if ui.add(burn_btn).clicked() {
                                self.upload_selected_bin();
                            }
                        });

                        if self.rom_embedded_view == RomEmbeddedView::None {
                            ui.label("Select FUEL TABLE or IGNITION TABLE");
                        } else if let Some(rom) = self.read_rom_file_for_table_view() {
                            match self.rom_embedded_view {
                                RomEmbeddedView::Fuel => {
                                    if let Some(values) = Self::rom_fuel_table_values(
                                        &rom,
                                        self.rom_table_kind,
                                        &self.rom_table_column_multipliers,
                                    ) {
                                        let mut min_v = f64::INFINITY;
                                        let mut max_v = f64::NEG_INFINITY;
                                        for row in &values {
                                            for &v in row {
                                                min_v = min_v.min(v);
                                                max_v = max_v.max(v);
                                            }
                                        }

                                        ui.horizontal_wrapped(|ui| {
                                            for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                                                if ui
                                                    .selectable_label(self.rom_table_kind == table_kind, table_kind.label())
                                                    .clicked()
                                                {
                                                    self.rom_table_kind = table_kind;
                                                }
                                            }
                                            ui.separator();
                                            for view_mode in [RomTableViewMode::TwoD, RomTableViewMode::ThreeD] {
                                                if ui
                                                    .selectable_label(self.rom_table_view_mode == view_mode, view_mode.label())
                                                    .clicked()
                                                {
                                                    self.rom_table_view_mode = view_mode;
                                                }
                                            }
                                            ui.separator();
                                            if ui.button("RESET VIEW").clicked() {
                                                if self.rom_table_kind == RomFuelTableKind::LowCam {
                                                    self.rom_table_3d_pan = egui::vec2(0.0, 0.0);
                                                    self.rom_table_3d_scale = 1.0;
                                                    self.rom_table_3d_yaw = -0.75;
                                                    self.rom_table_3d_pitch = 0.70;
                                                } else {
                                                    self.rom_table_high_3d_pan = egui::vec2(0.0, 0.0);
                                                    self.rom_table_high_3d_scale = 1.0;
                                                    self.rom_table_high_3d_yaw = -0.75;
                                                    self.rom_table_high_3d_pitch = 0.70;
                                                }
                                            }
                                            if ui.button("FULL TABLE SELECT").clicked() {
                                                self.rom_table_zone_row_start = 1;
                                                self.rom_table_zone_row_end = ROM_FUEL_TABLE_ROWS;
                                                self.rom_table_zone_col_start = 1;
                                                self.rom_table_zone_col_end = ROM_FUEL_TABLE_COLS;
                                            }
                                            for pct in [-10, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 10] {
                                                if ui.button(format!("{:+}%", pct)).clicked() {
                                                    self.apply_rom_fuel_table_percent_zone(
                                                        self.rom_table_kind,
                                                        pct,
                                                        self.rom_table_zone_row_start,
                                                        self.rom_table_zone_row_end,
                                                        self.rom_table_zone_col_start,
                                                        self.rom_table_zone_col_end,
                                                    );
                                                }
                                            }
                                        });

                                        match self.rom_table_view_mode {
                                            RomTableViewMode::TwoD => {
                                                Self::draw_rom_map_grid_inline(
                                                    ui,
                                                    "rom_embedded_fuel_grid",
                                                    &values,
                                                    min_v,
                                                    max_v,
                                                    &mut self.rom_table_zone_row_start,
                                                    &mut self.rom_table_zone_row_end,
                                                    &mut self.rom_table_zone_col_start,
                                                    &mut self.rom_table_zone_col_end,
                                                    &mut self.rom_table_drag_anchor,
                                                    &[],
                                                    false,
                                                );
                                            }
                                            RomTableViewMode::ThreeD => {
                                                let (pan, scale, yaw, pitch) = if self.rom_table_kind == RomFuelTableKind::LowCam {
                                                    (
                                                        &mut self.rom_table_3d_pan,
                                                        &mut self.rom_table_3d_scale,
                                                        &mut self.rom_table_3d_yaw,
                                                        &mut self.rom_table_3d_pitch,
                                                    )
                                                } else {
                                                    (
                                                        &mut self.rom_table_high_3d_pan,
                                                        &mut self.rom_table_high_3d_scale,
                                                        &mut self.rom_table_high_3d_yaw,
                                                        &mut self.rom_table_high_3d_pitch,
                                                    )
                                                };
                                                let row_lo = self
                                                    .rom_table_zone_row_start
                                                    .min(self.rom_table_zone_row_end)
                                                    .clamp(1, ROM_FUEL_TABLE_ROWS);
                                                let row_hi = self
                                                    .rom_table_zone_row_start
                                                    .max(self.rom_table_zone_row_end)
                                                    .clamp(1, ROM_FUEL_TABLE_ROWS);
                                                let col_lo = self
                                                    .rom_table_zone_col_start
                                                    .min(self.rom_table_zone_col_end)
                                                    .clamp(1, ROM_FUEL_TABLE_COLS);
                                                let col_hi = self
                                                    .rom_table_zone_col_start
                                                    .max(self.rom_table_zone_col_end)
                                                    .clamp(1, ROM_FUEL_TABLE_COLS);
                                                Self::draw_rom_map_3d_inline(
                                                    ui,
                                                    &values,
                                                    min_v,
                                                    max_v,
                                                    pan,
                                                    scale,
                                                    yaw,
                                                    pitch,
                                                    row_lo,
                                                    row_hi,
                                                    col_lo,
                                                    col_hi,
                                                    &[],
                                                    "Z: Fuel Value",
                                                    false,
                                                );
                                            }
                                        }
                                    } else {
                                        ui.label("ROM too small for fuel table");
                                    }
                                }
                                RomEmbeddedView::Ign => {
                                    if let Some(values) =
                                        Self::rom_ign_table_values(&rom, self.rom_ign_table_kind)
                                    {
                                        let mut min_v = f64::INFINITY;
                                        let mut max_v = f64::NEG_INFINITY;
                                        for row in &values {
                                            for &v in row {
                                                min_v = min_v.min(v);
                                                max_v = max_v.max(v);
                                            }
                                        }

                                        ui.horizontal_wrapped(|ui| {
                                            for table_kind in [RomFuelTableKind::LowCam, RomFuelTableKind::HighCam] {
                                                if ui
                                                    .selectable_label(self.rom_ign_table_kind == table_kind, table_kind.label())
                                                    .clicked()
                                                {
                                                    self.rom_ign_table_kind = table_kind;
                                                }
                                            }
                                            ui.separator();
                                            for view_mode in [RomTableViewMode::TwoD, RomTableViewMode::ThreeD] {
                                                if ui
                                                    .selectable_label(self.rom_ign_table_view_mode == view_mode, view_mode.label())
                                                    .clicked()
                                                {
                                                    self.rom_ign_table_view_mode = view_mode;
                                                }
                                            }
                                            ui.separator();
                                            if ui.button("RESET VIEW").clicked() {
                                                if self.rom_ign_table_kind == RomFuelTableKind::LowCam {
                                                    self.rom_ign_table_3d_pan = egui::vec2(0.0, 0.0);
                                                    self.rom_ign_table_3d_scale = 1.0;
                                                    self.rom_ign_table_3d_yaw = -0.75;
                                                    self.rom_ign_table_3d_pitch = 0.70;
                                                } else {
                                                    self.rom_ign_table_high_3d_pan = egui::vec2(0.0, 0.0);
                                                    self.rom_ign_table_high_3d_scale = 1.0;
                                                    self.rom_ign_table_high_3d_yaw = -0.75;
                                                    self.rom_ign_table_high_3d_pitch = 0.70;
                                                }
                                            }
                                            if ui.button("FULL TABLE SELECT").clicked() {
                                                self.rom_ign_table_zone_row_start = 1;
                                                self.rom_ign_table_zone_row_end = ROM_FUEL_TABLE_ROWS;
                                                self.rom_ign_table_zone_col_start = 1;
                                                self.rom_ign_table_zone_col_end = ROM_FUEL_TABLE_COLS;
                                            }
                                            for pct in [-10, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 10] {
                                                if ui.button(format!("{:+}%", pct)).clicked() {
                                                    self.apply_rom_ign_table_percent_zone(
                                                        self.rom_ign_table_kind,
                                                        pct,
                                                        self.rom_ign_table_zone_row_start,
                                                        self.rom_ign_table_zone_row_end,
                                                        self.rom_ign_table_zone_col_start,
                                                        self.rom_ign_table_zone_col_end,
                                                    );
                                                }
                                            }
                                        });

                                        match self.rom_ign_table_view_mode {
                                            RomTableViewMode::TwoD => {
                                                Self::draw_rom_map_grid_inline(
                                                    ui,
                                                    "rom_embedded_ign_grid",
                                                    &values,
                                                    min_v,
                                                    max_v,
                                                    &mut self.rom_ign_table_zone_row_start,
                                                    &mut self.rom_ign_table_zone_row_end,
                                                    &mut self.rom_ign_table_zone_col_start,
                                                    &mut self.rom_ign_table_zone_col_end,
                                                    &mut self.rom_ign_table_drag_anchor,
                                                    &[],
                                                    false,
                                                );
                                            }
                                            RomTableViewMode::ThreeD => {
                                                let (pan, scale, yaw, pitch) = if self.rom_ign_table_kind == RomFuelTableKind::LowCam {
                                                    (
                                                        &mut self.rom_ign_table_3d_pan,
                                                        &mut self.rom_ign_table_3d_scale,
                                                        &mut self.rom_ign_table_3d_yaw,
                                                        &mut self.rom_ign_table_3d_pitch,
                                                    )
                                                } else {
                                                    (
                                                        &mut self.rom_ign_table_high_3d_pan,
                                                        &mut self.rom_ign_table_high_3d_scale,
                                                        &mut self.rom_ign_table_high_3d_yaw,
                                                        &mut self.rom_ign_table_high_3d_pitch,
                                                    )
                                                };
                                                let row_lo = self
                                                    .rom_ign_table_zone_row_start
                                                    .min(self.rom_ign_table_zone_row_end)
                                                    .clamp(1, ROM_FUEL_TABLE_ROWS);
                                                let row_hi = self
                                                    .rom_ign_table_zone_row_start
                                                    .max(self.rom_ign_table_zone_row_end)
                                                    .clamp(1, ROM_FUEL_TABLE_ROWS);
                                                let col_lo = self
                                                    .rom_ign_table_zone_col_start
                                                    .min(self.rom_ign_table_zone_col_end)
                                                    .clamp(1, ROM_FUEL_TABLE_COLS);
                                                let col_hi = self
                                                    .rom_ign_table_zone_col_start
                                                    .max(self.rom_ign_table_zone_col_end)
                                                    .clamp(1, ROM_FUEL_TABLE_COLS);
                                                Self::draw_rom_map_3d_inline(
                                                    ui,
                                                    &values,
                                                    min_v,
                                                    max_v,
                                                    pan,
                                                    scale,
                                                    yaw,
                                                    pitch,
                                                    row_lo,
                                                    row_hi,
                                                    col_lo,
                                                    col_hi,
                                                    &[],
                                                    "Z: Ignition (deg)",
                                                    false,
                                                );
                                            }
                                        }
                                    } else {
                                        ui.label("ROM too small for ignition table");
                                    }
                                }
                                RomEmbeddedView::None => {}
                            }
                        } else {
                            ui.label("Load a BIN or read ROM first");
                        }
                    });
                });

                columns[1].group(|ui| {
                    egui::CollapsingHeader::new("SUPRAROM DATALOG")
                        .id_salt("honda_datalog_section")
                        .default_open(false)
                        .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let previous_datalog_port = self.datalog_com_port.clone();
                        let selected_text = if self.datalog_com_port.trim().is_empty() {
                            "Select DATALOG port".to_string()
                        } else {
                            self.datalog_com_port.clone()
                        };

                        egui::ComboBox::from_id_salt("datalog_port_selector")
                            .width(280.0)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for port in &self.datalog_available_ports {
                                    ui.selectable_value(
                                        &mut self.datalog_com_port,
                                        port.clone(),
                                        port,
                                    );
                                }
                            });

                        if self.datalog_com_port != previous_datalog_port
                            && !self.datalog_com_port.is_empty()
                        {
                            self.log
                                .push(format!("Selected datalog port: {}", self.datalog_com_port));
                            self.connect_hts();
                        }

                        if ui.button("SCAN").clicked() {
                            self.scan_datalog_ports();
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.button("CONNECT").clicked() {
                            self.connect_hts();
                        }
                        if ui.button("DISCONNECT").clicked() {
                            self.datalog_connected = false;
                            self.log.push("HTS disconnected".to_string());
                        }
                        ui.label(format!("Connected: {}", self.datalog_connected));
                    });
                    });

                    ui.separator();
                    egui::CollapsingHeader::new("SENSORS")
                        .id_salt("sensors_section")
                        .default_open(false)
                        .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let graph_label = if self.show_datalog_graph {
                            "GRAPH ON"
                        } else {
                            "GRAPH"
                        };
                        if ui.button(graph_label).clicked() {
                            self.show_datalog_graph = !self.show_datalog_graph;
                        }
                        let table_label = if self.show_datalog_table {
                            "2D ON"
                        } else {
                            "2D MODE"
                        };
                        if ui.button(table_label).clicked() {
                            self.show_datalog_table = !self.show_datalog_table;
                            self.show_table_window = true;
                        }
                        let table_3d_label = if self.show_datalog_table_3d {
                            "3D ON"
                        } else {
                            "3D MODE"
                        };
                        if ui.button(table_3d_label).clicked() {
                            self.show_datalog_table_3d = !self.show_datalog_table_3d;
                            self.show_table_3d_window = true;
                        }
                        let live_tracking_label = if self.show_live_tracking_window {
                            "LIVE TRACKING ON"
                        } else {
                            "LIVE TRACKING"
                        };
                        if ui.button(live_tracking_label).clicked() {
                            if self.last_read_rom.is_none() && self.selected_file_is_usable() {
                                self.read_selected_bin_file(false);
                            }
                            self.show_live_tracking_window = !self.show_live_tracking_window;
                        }
                        let live_tracking_afr_label = if self.show_live_tracking_afr_window {
                            "LIVE TRACKING AFR ON"
                        } else {
                            "LIVE TRACKING AFR"
                        };
                        if ui.button(live_tracking_afr_label).clicked() {
                            self.show_live_tracking_afr_window = !self.show_live_tracking_afr_window;
                        }

                        ui.separator();
                        ui.label(format!("AFR OFFSET {:+.1}", self.afr_offset));
                        if ui.button("-0.1 AFR").clicked() {
                            self.afr_offset = (self.afr_offset - 0.1).clamp(-5.0, 5.0);
                            self.log.push(format!(
                                "AFR OFFSET updated to {:+.1} (applied globally)",
                                self.afr_offset
                            ));
                        }
                        if ui.button("+0.1 AFR").clicked() {
                            self.afr_offset = (self.afr_offset + 0.1).clamp(-5.0, 5.0);
                            self.log.push(format!(
                                "AFR OFFSET updated to {:+.1} (applied globally)",
                                self.afr_offset
                            ));
                        }
                    });

                    let sensor_line = |ui: &mut egui::Ui, name: &str, val: f64, unit: &str, max_v: f64| {
                        let ratio = (val / max_v).clamp(0.0, 1.0) as f32;
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [54.0, 22.0],
                                egui::Label::new(egui::RichText::new(format!("{name:<6}")).monospace()),
                            );
                            ui.add(
                                egui::ProgressBar::new(ratio)
                                    .desired_width(170.0)
                                    .fill(egui::Color32::from_rgb(220, 44, 44)),
                            );
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_gray(90)))
                                .inner_margin(egui::Margin::symmetric(8, 2))
                                .show(ui, |ui| {
                                    ui.add_sized(
                                        [92.0, 20.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("{val:>6.1} {unit}")).monospace(),
                                        ),
                                    );
                                });
                        });
                        ui.add_space(2.0);
                    };

                    ui.scope(|ui| {
                        // Tighten only the gap between sensor values and graph/3D panel.
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 8.0);

                        ui.columns(2, |sensor_columns| {
                            sensor_columns[0].vertical(|ui| {
                                sensor_line(ui, "ECT", self.ect, "C", 130.0);
                                sensor_line(ui, "IAT", self.iat, "C", 100.0);
                                sensor_line(ui, "TPS", self.tps, "%", 100.0);
                                sensor_line(ui, "MAP", self.map, "mbar", HONDA_MBAR_MAX);
                                sensor_line(ui, "RPM", self.rpm, "rpm", 9000.0);
                                sensor_line(ui, "AFR", self.afr, "", AFR_GRAPH_MAX);
                                sensor_line(ui, "Lambda", self.lambda, "", 2.0);
                                sensor_line(ui, "BATT", self.battery, "V", 18.0);
                                sensor_line(ui, "INJ", self.inj_ms, "ms", 20.0);
                                sensor_line(ui, "IGN", self.ign_advance, "deg", 60.0);
                            });

                            sensor_columns[1].vertical(|ui| {
                                let sensor_picker = |ui: &mut egui::Ui, id: &str, selected: &mut GraphSensor| {
                                    egui::ComboBox::from_id_salt(id)
                                        .width(96.0)
                                        .selected_text(selected.label())
                                        .show_ui(ui, |ui| {
                                            for sensor in GraphSensor::ALL {
                                                ui.selectable_value(selected, sensor, sensor.label());
                                            }
                                        });
                                };

                                if self.show_datalog_table {
                                    let desired = egui::vec2(440.0, 320.0);
                                    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                                    let painter = ui.painter_at(rect);
                                    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 14, 14));
                                    painter.rect_stroke(
                                        rect,
                                        4.0,
                                        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
                                        egui::StrokeKind::Inside,
                                    );

                                    let inner = rect.shrink2(egui::vec2(6.0, 6.0));
                                    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                                        self.draw_afr_table_content(ui, true);
                                    });
                                } else if self.show_datalog_table_3d {
                                    self.draw_table_3d_inline(ui);
                                } else if self.show_datalog_graph {
                                    egui::Grid::new("graph_trace_selectors")
                                        .num_columns(3)
                                        .min_col_width(104.0)
                                        .spacing([8.0, 6.0])
                                        .show(ui, |ui| {
                                            sensor_picker(ui, "graph_trace_a", &mut self.graph_trace_a);
                                            sensor_picker(ui, "graph_trace_b", &mut self.graph_trace_b);
                                            sensor_picker(ui, "graph_trace_c", &mut self.graph_trace_c);
                                            ui.end_row();
                                    });
                                    self.draw_tps_graph(ui);
                                }
                            });
                        });
                    });
                    });

                    ui.separator();
                    egui::CollapsingHeader::new("SENSOR OUTPUT")
                        .id_salt("sensor_output_section")
                        .default_open(false)
                        .show(ui, |ui| {

                    let digital_led = |ui: &mut egui::Ui, name: &str, active: bool| {
                        let border_color = if active {
                            egui::Color32::from_rgb(30, 190, 90)
                        } else {
                            egui::Color32::from_rgb(180, 35, 35)
                        };
                        let text_color = if active {
                            egui::Color32::from_rgb(30, 190, 90)
                        } else {
                            egui::Color32::WHITE
                        };
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(10, 10, 10))
                            .stroke(egui::Stroke::new(1.0_f32, border_color))
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [84.0, 20.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("{name:<9}"))
                                            .monospace()
                                            .color(text_color),
                                    ),
                                );
                            });
                    };

                    let led_items = [
                        ("VTEC", self.vts_active),
                        ("VTP", self.vtp_active),
                        ("MIL", self.mil_active),
                        ("FAN", self.fan_active),
                        ("FLR", self.flr_active),
                        ("FUELCUT1", self.fuelcut1_active),
                        ("FUELCUT2", self.fuelcut2_active),
                        ("IGNCUT", self.igncut_active),
                        ("LEANPROT", self.leanprotect_active),
                        ("BOOSTCUT", self.boostcut_active),
                        ("BST", self.bst_active),
                        ("ANTILAG", self.antilag_active),
                        ("EBC", self.ebc_active),
                        ("AC", self.ac_active),
                        ("ATLCTRL", self.atlctrl_active),
                        ("O2HEAT", self.o2heater_active),
                        ("2ND_MAP", self.output_2nd_map_active),
                        ("VTS_FB", self.vts_feedback_active),
                        ("PARK_N", self.park_n_active),
                        ("BKSW", self.bksw_active),
                        ("ACC", self.acc_active),
                        ("START", self.start_active),
                        ("SCC", self.scc_active),
                        ("PSP", self.psp_active),
                        ("OUT_FTS", self.output_fts_active),
                        ("IAB", self.iab_active),
                        ("PURGE", self.purge_active),
                        ("FUELPUMP", self.fuelpump_active),
                        ("IN_FTL", self.input_ftl_active),
                        ("IN_FTS", self.input_fts_active),
                        ("IN_EBC", self.input_ebc_active),
                        ("IN_EBCHI", self.input_ebc_hi_active),
                        ("IN_BST", self.input_bst_active),
                        ("IN_GPO1", self.input_gpo1_active),
                        ("IN_GPO2", self.input_gpo2_active),
                        ("IN_GPO3", self.input_gpo3_active),
                        ("GPO1", self.gpo1_active),
                        ("GPO2", self.gpo2_active),
                        ("GPO3", self.gpo3_active),
                        ("BST_ST2", self.bst_stage2_active),
                        ("BST_ST3", self.bst_stage3_active),
                        ("BST_ST4", self.bst_stage4_active),
                        ("SCC_CHK", self.scc_checker_active),
                        ("VTSM", self.vtsm_active),
                        ("POST_FUEL", self.post_fuel_active),
                        ("AT_SHIFT1", self.at_shift1_active),
                        ("AT_SHIFT2", self.at_shift2_active),
                    ];
                    let leds_per_column = 6usize;
                    let column_count = (led_items.len() + leds_per_column - 1) / leds_per_column;

                    ui.columns(column_count, |led_columns| {
                        for (col_idx, col_ui) in led_columns.iter_mut().enumerate() {
                            let start = col_idx * leds_per_column;
                            let end = (start + leds_per_column).min(led_items.len());
                            for (name, active) in &led_items[start..end] {
                                digital_led(col_ui, name, *active);
                            }
                        }
                    });
                    });

                    ui.separator();
                    egui::CollapsingHeader::new("LIVE DATA")
                        .id_salt("live_data_section")
                        .default_open(false)
                        .show(ui, |ui| {
                    let text_metric = |ui: &mut egui::Ui, name: &str, value: String| {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [120.0, 22.0],
                                egui::Label::new(egui::RichText::new(name).monospace()),
                            );
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_gray(90)))
                                .inner_margin(egui::Margin::symmetric(8, 3))
                                .show(ui, |ui| {
                                    ui.add_sized(
                                        [140.0, 20.0],
                                        egui::Label::new(egui::RichText::new(value).monospace()),
                                    );
                                });
                        });
                    };

                    egui::Grid::new("datalog_text_grid")
                        .num_columns(3)
                        .min_col_width(240.0)
                        .spacing([14.0, 8.0])
                        .show(ui, |ui| {
                            text_metric(ui, "Boost", format!("{:.2} PSI", self.boost_psi));
                            text_metric(ui, "MAP Volt", format!("{:.2} V", self.map_volt));
                            text_metric(ui, "TPS Volt", format!("{:.2} V", self.tps_volt));
                            ui.end_row();

                            text_metric(ui, "VSS", format!("{:.0} km/h", self.vss_kmh));
                            text_metric(ui, "Gear", format!("{}", self.gear));
                            text_metric(ui, "Injector Duty", format!("{:.2} %", self.injector_duty));
                            ui.end_row();

                            text_metric(ui, "IACV Duty", format!("{:.2} %", self.iacv_duty));
                            text_metric(ui, "EBC Duty", format!("{:.1} %", self.ebc_duty));
                            text_metric(ui, "Gear IC", format!("{:.2}", self.gear_ic));
                            ui.end_row();

                            text_metric(
                                ui,
                                "Consumption",
                                format!("{:.2} L/100km", self.instant_consumption),
                            );
                            text_metric(ui, "EBC Base", format!("{:.1} %", self.ebc_base_duty));
                            text_metric(ui, "ELD Volt", format!("{:.2} V", self.eld_volt));
                            ui.end_row();

                            text_metric(ui, "INJ FV", format!("{:.2}", self.inj_fv));
                            text_metric(ui, "ECT FC", format!("{:.2}", self.ect_fc));
                            text_metric(ui, "O2 Short FC", format!("{:.2}", self.o2_short_fc));
                            ui.end_row();

                            text_metric(ui, "O2 Long FC", format!("{:.2}", self.o2_long_fc));
                            text_metric(ui, "IAT FC", format!("{:.2}", self.iat_fc));
                            text_metric(ui, "VE FC", format!("{:.2}", self.ve_fc));
                            ui.end_row();

                            text_metric(ui, "IAT IC", format!("{:.2}", self.iat_ic));
                            text_metric(ui, "ECT IC", format!("{:.2}", self.ect_ic));
                            ui.end_row();
                        });
                            });
                });
            });

                });

        });

        self.draw_afr_table_window(ctx);
        self.draw_about_window(ctx);
        self.draw_new_bin_window(ctx);
        self.draw_table_3d_window(ctx);
        self.draw_rom_table_window(ctx);
        self.draw_rom_ign_table_window(ctx);
        self.draw_live_tracking_window(ctx);
        self.draw_live_tracking_afr_window(ctx);

        ctx.request_repaint_after(Duration::from_millis(50));
    }
}
