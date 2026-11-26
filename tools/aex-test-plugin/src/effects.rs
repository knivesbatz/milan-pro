//! The effects of Milan's AE-format test plug-in: one PiPL each (shared by `build.rs`, which writes them, and the
//! plug-in, which exports their entry points). Match names start "MLNT " (Milan test), never Adobe's prefix.
#![allow(dead_code)]

pub struct Fx {
    pub name: &'static str,
    pub match_name: &'static str,
    pub entry: &'static str,
    pub out_flags: u32,
    pub out_flags2: u32,
}

const NEEDS_FLATTENING: u32 = 1 << 4;
const WIDE_TIME_INPUT: u32 = 1 << 1;
const SEND_UPDATE_PARAMS_UI: u32 = 1 << 26;
const CUSTOM_UI: u32 = 1 << 15;

pub const INVERT: usize = 0;
pub const GAIN: usize = 1;
pub const CRASH: usize = 2;
pub const CRASH_SETUP: usize = 3;
pub const HANG: usize = 4;
pub const PARAMS: usize = 5;
pub const MARKER: usize = 6;
pub const COUNTER: usize = 7;
pub const CALLBACKS: usize = 8;
pub const PREVIOUS: usize = 9;
pub const SUPERVISOR: usize = 10;
pub const LAYER: usize = 11;
pub const ECHO: usize = 12;

pub const ALL: [Fx; 13] = [
    Fx { name: "Test Invert", match_name: "MLNT Test Invert", entry: "MilanTestInvert", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Gain", match_name: "MLNT Test Gain", entry: "MilanTestGain", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Crash", match_name: "MLNT Test Crash", entry: "MilanTestCrash", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Crash Setup", match_name: "MLNT Test Crash Setup", entry: "MilanTestCrashSetup", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Hang", match_name: "MLNT Test Hang", entry: "MilanTestHang", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Params", match_name: "MLNT Test Params", entry: "MilanTestParams", out_flags: CUSTOM_UI, out_flags2: 0 },
    Fx { name: "Test Marker", match_name: "MLNT Test Marker", entry: "MilanTestMarker", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Counter", match_name: "MLNT Test Counter", entry: "MilanTestCounter", out_flags: NEEDS_FLATTENING, out_flags2: 0 },
    Fx { name: "Test Callbacks", match_name: "MLNT Test Callbacks", entry: "MilanTestCallbacks", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Previous Frame", match_name: "MLNT Test Previous Frame", entry: "MilanTestPrevious", out_flags: WIDE_TIME_INPUT, out_flags2: 0 },
    Fx { name: "Test Supervisor", match_name: "MLNT Test Supervisor", entry: "MilanTestSupervisor", out_flags: SEND_UPDATE_PARAMS_UI, out_flags2: 0 },
    Fx { name: "Test Layer", match_name: "MLNT Test Layer", entry: "MilanTestLayer", out_flags: 0, out_flags2: 0 },
    Fx { name: "Test Echo", match_name: "MLNT Test Echo", entry: "MilanTestEcho", out_flags: WIDE_TIME_INPUT, out_flags2: 0 },
];
