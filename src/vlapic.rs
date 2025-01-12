use core::ptr::NonNull;

use axerrno::{AxError, AxResult};
use tock_registers::interfaces::Readable;

use axaddrspace::device::AccessWidth;
use axaddrspace::{AxMmHal, HostPhysAddr, PhysFrame};
use axdevice_base::DeviceRWContext;

use crate::consts::{ApicRegOffset, RESET_SPURIOUS_INTERRUPT_VECTOR};
use crate::lvt::LocalVectorTable;
use crate::regs::lvt::LVT_TIMER;
use crate::regs::{
    APIC_BASE, ApicBaseRegisterMsr, LocalAPICRegs, SpuriousInterruptVectorRegisterLocal,
};
use crate::timer::TimerMode;

/// Virtual-APIC Registers.
pub struct VirtualApicRegs<H: AxMmHal> {
    /// The virtual-APIC page is a 4-KByte region of memory
    /// that the processor uses to virtualize certain accesses to APIC registers and to manage virtual interrupts.
    /// The physical address of the virtual-APIC page is the virtual-APIC address,
    /// a 64-bit VM-execution control field in the VMCS (see Section 25.6.8).
    virtual_lapic: NonNull<LocalAPICRegs>,
    /// Vector number for the highest priority bit that is set in the ISR
    isrv: u32,

    apic_base: ApicBaseRegisterMsr,

    /// Copies of some registers in the virtual APIC page,
    /// to be able to detect what changed (e.g. svr_last)
    svr_last: SpuriousInterruptVectorRegisterLocal,
    /// Copies of some registers in the virtual APIC page,
    /// to maintain a coherent snapshot of the register (e.g. lvt_last)
    lvt_last: LocalVectorTable,
    apic_page: PhysFrame<H>,
}

impl<H: AxMmHal> VirtualApicRegs<H> {
    /// Create new virtual-APIC registers by allocating a 4-KByte page for the virtual-APIC page.
    pub fn new() -> Self {
        let apic_frame = PhysFrame::alloc_zero().expect("allocate virtual-APIC page failed");
        Self {
            virtual_lapic: NonNull::new(apic_frame.as_mut_ptr().cast()).unwrap(),
            apic_page: apic_frame,
            svr_last: SpuriousInterruptVectorRegisterLocal::new(RESET_SPURIOUS_INTERRUPT_VECTOR),
            lvt_last: LocalVectorTable::default(),
            isrv: 0,
            apic_base: ApicBaseRegisterMsr::new(0),
        }
    }

    const fn regs(&self) -> &LocalAPICRegs {
        unsafe { self.virtual_lapic.as_ref() }
    }

    /// Virtual-APIC address (64 bits).
    /// This field contains the physical address of the 4-KByte virtual-APIC page.
    /// The processor uses the virtual-APIC page to virtualize certain accesses to APIC registers and to manage virtual interrupts;
    /// see Chapter 30.
    pub fn virtual_apic_page_addr(&self) -> HostPhysAddr {
        self.apic_page.start_paddr()
    }

    /// Gets the APIC base MSR value.
    pub fn apic_base(&self) -> u64 {
        self.apic_base.get()
    }

    /// Returns whether the x2APIC mode is enabled.
    pub fn is_x2apic_enabled(&self) -> bool {
        self.apic_base.is_set(APIC_BASE::XAPIC_ENABLED)
            && self.apic_base.is_set(APIC_BASE::X2APIC_Enabled)
    }

    /// Returns whether the xAPIC mode is enabled.
    pub fn is_xapic_enabled(&self) -> bool {
        self.apic_base.is_set(APIC_BASE::XAPIC_ENABLED)
            && !self.apic_base.is_set(APIC_BASE::X2APIC_Enabled)
    }

    pub fn timer_mode(&self) -> AxResult<TimerMode> {
        match self.regs().LVT_TIMER.read_as_enum(LVT_TIMER::TimerMode) {
            Some(LVT_TIMER::TimerMode::Value::OneShot) => Ok(TimerMode::OneShot),
            Some(LVT_TIMER::TimerMode::Value::Periodic) => Ok(TimerMode::Periodic),
            Some(LVT_TIMER::TimerMode::Value::TSCDeadline) => Ok(TimerMode::TscDeadline),
            Some(LVT_TIMER::TimerMode::Value::Reserved) | None => Err(AxError::InvalidData),
        }
    }
}

impl<H: AxMmHal> Drop for VirtualApicRegs<H> {
    fn drop(&mut self) {
        H::dealloc_frame(self.apic_page.start_paddr());
    }
}

impl<H: AxMmHal> VirtualApicRegs<H> {
    pub fn handle_read(
        &self,
        offset: ApicRegOffset,
        width: AccessWidth,
        context: DeviceRWContext,
    ) -> AxResult<usize> {
        let mut value: usize = 0;
        match offset {
            ApicRegOffset::ID => {
                value = self.regs().ID.get() as _;
            }
            ApicRegOffset::Version => {
                value = self.regs().VERSION.get() as _;
            }
            ApicRegOffset::TPR => {
                value = self.regs().TPR.get() as _;
            }
            ApicRegOffset::PPR => {
                value = self.regs().PPR.get() as _;
            }
            ApicRegOffset::EOI => {
                // value = self.regs().EOI.get() as _;
                warn!("[VLAPIC] read EOI register: {:#010X}", value);
            }
            ApicRegOffset::LDR => {
                value = self.regs().LDR.get() as _;
            }
            ApicRegOffset::DFR => {
                value = self.regs().DFR.get() as _;
            }
            ApicRegOffset::SIVR => {
                value = self.regs().SVR.get() as _;
            }
            ApicRegOffset::ISR(index) => {
                value = self.regs().ISR[index.as_usize()].get() as _;
            }
            ApicRegOffset::TMR(index) => {
                value = self.regs().TMR[index.as_usize()].get() as _;
            }
            ApicRegOffset::IRR(index) => {
                value = self.regs().IRR[index.as_usize()].get() as _;
            }
            ApicRegOffset::ESR => {
                value = self.regs().ESR.get() as _;
            }
            ApicRegOffset::ICRLow => {
                value = self.regs().ICR_LO.get() as _;
                if self.is_x2apic_enabled() && width == AccessWidth::Qword {
                    let icr_hi = self.regs().ICR_HI.get() as usize;
                    value |= icr_hi << 32;
                    debug!("[VLAPIC] read ICR register: {:#018X}", value);
                } else {
                    warn!(
                        "[VLAPIC] Illegal read attempt of ICR register at width {:?} with X2APIC {}",
                        width,
                        if self.is_x2apic_enabled() {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    return Err(AxError::InvalidInput);
                }
            }
            ApicRegOffset::ICRHi => {
                value = self.regs().ICR_HI.get() as _;
            }
            // Local Vector Table registers.
            ApicRegOffset::LvtCMCI => {
                value = self.lvt_last.lvt_cmci.get() as _;
            }
            ApicRegOffset::LvtTimer => {
                value = self.lvt_last.lvt_timer.get() as _;
            }
            ApicRegOffset::LvtThermal => {
                value = self.lvt_last.lvt_thermal.get() as _;
            }
            ApicRegOffset::LvtPmc => {
                value = self.lvt_last.lvt_perf_count.get() as _;
            }
            ApicRegOffset::LvtLint0 => {
                value = self.lvt_last.lvt_lint0.get() as _;
            }
            ApicRegOffset::LvtLint1 => {
                value = self.lvt_last.lvt_lint1.get() as _;
            }
            ApicRegOffset::LvtErr => {
                value = self.lvt_last.lvt_err.get() as _;
            }
            // Timer registers.
            ApicRegOffset::TimerInitCount => {
                match self.timer_mode() {
                    Ok(TimerMode::OneShot) | Ok(TimerMode::Periodic) => {
                        value = self.regs().ICR_TIMER.get() as _;
                    }
                    Ok(TimerMode::TscDeadline) => {
                        /* if TSCDEADLINE mode always return 0*/
                        value = 0;
                    }
                    Err(_) => {
                        warn!("[VLAPIC] read TimerInitCount register: invalid timer mode");
                    }
                }
                debug!("[VLAPIC] read TimerInitCount register: {:#010X}", value);
            }
            ApicRegOffset::TimerCurCount => {
                value = self.regs().CCR_TIMER.get() as _;
            }
            ApicRegOffset::TimerDivConf => {
                value = self.regs().DCR_TIMER.get() as _;
            }
            _ => {
                warn!("[VLAPIC] read unknown APIC register: {:?}", offset);
            }
        }
        debug!("[VLAPIC] read {} register: {:#010X}", offset, value);
        Ok(value)
    }

    pub fn handle_write(
        &self,
        offset: ApicRegOffset,
        width: AccessWidth,
        context: DeviceRWContext,
    ) -> AxResult {
        Ok(())
    }
}
