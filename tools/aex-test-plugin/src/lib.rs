//! Milan's AE-format test plug-in. A1: its PiPLs only (`build.rs`); every entry point refuses every command until the
//! host can run them (A2).
#![allow(clippy::missing_safety_doc)]
mod effects;

macro_rules! entries {
    ($($name:ident),*) => {$(
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(_cmd: i32, _in: *mut u8, _out: *mut u8, _params: *mut u8, _output: *mut u8, _extra: *mut u8) -> i32 {
            515
        }
    )*};
}

entries!(MilanTestInvert, MilanTestGain, MilanTestCrash, MilanTestCrashSetup, MilanTestHang, MilanTestParams, MilanTestMarker, MilanTestCounter, MilanTestCallbacks, MilanTestPrevious, MilanTestSupervisor, MilanTestLayer, MilanTestEcho);
