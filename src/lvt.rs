//! Local Vector Table

use tock_registers::register_bitfields;
use tock_registers::registers::ReadOnly;

use crate::consts::{RESET_LVT_THERMAL, RESET_LVT_TIMER};
use crate::regs::lvt::LvtCmciRegisterLocal;

pub struct LocalVectorTable {
    /// LVT CMCI Register (FEE0 02F0H)
    lvt_cmci: LvtCmciRegisterLocal,
    /// LVT Timer Register (FEE0 0320H)
    lvt_timer: u32,
    /// LVT Thermal Monitor Register (FEE0 0330H)
    lvt_thermal: u32,
    /// LVT Performance Counter Register (FEE0 0340H)
    lvt_pmi: u32,
    /// LVT LINT0 Register (FEE0 0350H)
    lvt_lint0: u32,
    /// LVT LINT1 Register (FEE0 0360H)
    lvt_lint1: u32,
    /// LVT Error register 0x37.
    lvt_err: u32,
}

impl Default for LocalVectorTable {
    fn default() -> Self {
        LocalVectorTable {
            lvt_cmci: LvtCmciRegisterLocal::new(0),
            // Value after Reset: 0001 0000H
            lvt_timer: RESET_LVT_TIMER,
            lvt_thermal: RESET_LVT_THERMAL,
            lvt_pmi: 0,
            lvt_lint0: 0,
            lvt_lint1: 0,
            lvt_err: 0,
        }
    }
}
