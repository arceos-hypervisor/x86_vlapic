use core::ptr::NonNull;

use axerrno::{AxError, AxResult};
use bit::BitIndex;
use tock_registers::interfaces::{ReadWriteable, Readable, Writeable};

use axaddrspace::device::AccessWidth;
use axaddrspace::{AxMmHal, HostPhysAddr, PhysFrame};
use axdevice_base::DeviceRWContext;

use crate::consts::{
    APIC_LVT_DS, APIC_LVT_M, APIC_LVT_VECTOR, ApicRegOffset, RESET_SPURIOUS_INTERRUPT_VECTOR,
};
use crate::regs::lvt::{
    LVT_CMCI, LVT_ERROR, LVT_LINT0, LVT_LINT1, LVT_PERFORMANCE_COUNTER, LVT_THERMAL_MONITOR,
    LVT_TIMER, LocalVectorTable,
};
use crate::regs::{
    APIC_BASE, ApicBaseRegisterMsr, LocalAPICRegs, SpuriousInterruptVectorRegisterLocal,
};
use crate::regs::{INTERRUPT_COMMAND_HIGH, INTERRUPT_COMMAND_LOW, SPURIOUS_INTERRUPT_VECTOR};
use crate::timer::{ApicTimer, TimerMode};
use crate::utils::fls32;

/// Virtual-APIC Registers.
pub struct VirtualApicRegs<H: AxMmHal> {
    /// The virtual-APIC page is a 4-KByte region of memory
    /// that the processor uses to virtualize certain accesses to APIC registers and to manage virtual interrupts.
    /// The physical address of the virtual-APIC page is the virtual-APIC address,
    /// a 64-bit VM-execution control field in the VMCS (see Section 25.6.8).
    virtual_lapic: NonNull<LocalAPICRegs>,

    vapic_id: u32,
    esr_pending: u32,
    esr_firing: i32,

    virtual_timer: ApicTimer,

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
    pub fn new(vcpu_id: u32) -> Self {
        let apic_frame = PhysFrame::alloc_zero().expect("allocate virtual-APIC page failed");
        Self {
            // virtual-APIC ID is the same as the VCPU ID.
            vapic_id: vcpu_id,
            esr_pending: 0,
            esr_firing: 0,
            virtual_lapic: NonNull::new(apic_frame.as_mut_ptr().cast()).unwrap(),
            apic_page: apic_frame,
            svr_last: SpuriousInterruptVectorRegisterLocal::new(RESET_SPURIOUS_INTERRUPT_VECTOR),
            lvt_last: LocalVectorTable::default(),
            isrv: 0,
            apic_base: ApicBaseRegisterMsr::new(0),
            virtual_timer: ApicTimer::new(),
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

    /// Returns the current timer mode.
    pub fn timer_mode(&self) -> AxResult<TimerMode> {
        match self.regs().LVT_TIMER.read_as_enum(LVT_TIMER::TimerMode) {
            Some(LVT_TIMER::TimerMode::Value::OneShot) => Ok(TimerMode::OneShot),
            Some(LVT_TIMER::TimerMode::Value::Periodic) => Ok(TimerMode::Periodic),
            Some(LVT_TIMER::TimerMode::Value::TSCDeadline) => Ok(TimerMode::TscDeadline),
            Some(LVT_TIMER::TimerMode::Value::Reserved) | None => Err(AxError::InvalidData),
        }
    }

    /// 30.1.4 EOI Virtualization
    /// IF any bits set in VISR
    ///     THEN SVI := highest index of bit set in VISR
    ///     ELSE SVI := 0;
    /// FI;
    fn find_isrv(&self) -> u32 {
        let mut isrv = 0;
        /* i ranges effectively from 7 to 1 */
        for i in (1..8).rev() {
            let val = self.regs().ISR[i].get() as u32;
            if val != 0 {
                isrv = ((i as u32) << 5) | fls32(val) as u32;
                break;
            }
        }

        isrv
    }

    fn update_ppr(&mut self) {
        let isrv = self.isrv;
        let tpr = self.regs().TPR.get() as u32;
        // IF VTPR[7:4] ≥ SVI[7:4]
        let ppr = if prio(tpr) >= prio(isrv) {
            // THEN VPPR := VTPR & FFH;
            tpr
        } else {
            // ELSE VPPR := SVI & F0H;
            isrv & 0xf0
        };
        self.regs().PPR.set(ppr as _);
    }

    /// Process the EOI operation triggered by a write to the EOI register.
    /// 11.8.5 Signaling Interrupt Servicing Completion
    /// 30.1.4 EOI Virtualization
    fn process_eoi(&mut self) {
        let vector = self.isrv;

        if vector == 0 {
            return;
        }

        let (idx, bitpos) = extract_index_and_bitpos_u32(vector);

        // Upon receiving an EOI, the APIC clears the highest priority bit in the ISR
        // and dispatches the next highest priority interrupt to the processor.

        // VISR[Vector] := 0; (see Section 30.1.1 for definition of VISR)
        let mut isr = self.regs().ISR[idx].get();
        isr &= !(1 << bitpos);
        self.regs().ISR[idx].set(isr);

        // IF any bits set in VISR
        // THEN SVI := highest index of bit set in VISR
        // ELSE SVI := 0;
        self.isrv = self.find_isrv();

        // perform PPR virtualiation (see Section 30.1.3);
        self.update_ppr();

        // The trigger mode register (TMR) indicates the trigger mode of the interrupt (see Figure 11-20).
        // Upon acceptance of an interrupt into the IRR, the corresponding TMR bit is cleared for edge-triggered interrupts and set for leveltriggered interrupts.
        // If a TMR bit is set when an EOI cycle for its corresponding interrupt vector is generated, an EOI message is sent to all I/O APICs.
        // (see 11.8.4 Interrupt Acceptance for Fixed Interrupts)
        if (self.regs().TMR[idx].get() as u32).bit(bitpos) {
            // Send EOI to all I/O APICs
            /*
             * Per Intel SDM 10.8.5, Software can inhibit the broadcast of
             * EOI by setting bit 12 of the Spurious Interrupt Vector
             * Register of the LAPIC.
             * TODO: Check if the bit 12 "Suppress EOI Broadcasts" is set.
             */
            unimplemented!("vioapic_broadcast_eoi(vlapic2vcpu(vlapic)->vm, vector);")
        }

        debug!("Gratuitous EOI vector: {:#010X}", vector);

        unimplemented!("vcpu_make_request(vlapic2vcpu(vlapic), ACRN_REQUEST_EVENT);")
    }

    /// Post an interrupt to the vcpu running on 'hostcpu'.
    /// This will use a hardware assist if available (e.g. Posted Interrupt)
    /// or fall back to sending an 'ipinum' to interrupt the 'hostcpu'.
    fn set_err(&mut self, mask: u32) {
        self.esr_pending |= mask;
        self.esr_firing = 1;
        if self.esr_firing == 0 {
            self.esr_firing = 1;
            let _lvt = self.regs().LVT_ERROR.get();
            //  if ((lvt & APIC_LVT_M) == 0U) {
            //     vec = lvt & APIC_LVT_VECTOR;
            //     if (vec >= 16U) {
            //         vlapic_accept_intr(vlapic, vec, LAPIC_TRIG_EDGE);
            //     }
            // }
            unimplemented!("vlapic_accept_intr(vlapic, vec, LAPIC_TRIG_EDGE)");
            // self.esr_firing = 0;
        }
    }

    fn handle_self_ipi(&mut self) {
        unimplemented!("x2apic handle_self_ipi");
    }

    /// Figure 11-13. Logical Destination Register (LDR)
    fn write_ldr(&mut self) {
        const LDR_RESERVED: u32 = 0x00ffffff;

        let mut ldr = self.regs().LDR.get();
        let apic_id = ldr >> 24;
        ldr &= !LDR_RESERVED;

        self.regs().LDR.set(ldr);
        debug!(
            "[VLAPIC] apic_id={:#010X} write LDR register to {:#010X}",
            apic_id, ldr
        );
    }

    fn write_dfr(&mut self) {
        use crate::regs::DESTINATION_FORMAT;

        const APIC_DFR_RESERVED: u32 = 0x0fff_ffff;
        const APIC_DFR_MODEL_MASK: u32 = 0xf000_0000;

        let mut dfr = self.regs().DFR.get();
        dfr &= APIC_DFR_MODEL_MASK;
        dfr |= APIC_DFR_RESERVED;
        self.regs().DFR.set(dfr);

        debug!("[VLAPIC] write DFR register to {:#010X}", dfr);

        match self.regs().DFR.read_as_enum(DESTINATION_FORMAT::Model) {
            Some(DESTINATION_FORMAT::Model::Value::Flat) => {
                debug!("[VLAPIC] DFR in Flat Model");
            }
            Some(DESTINATION_FORMAT::Model::Value::Cluster) => {
                debug!("[VLAPIC] DFR in Cluster Model");
            }
            None => {
                debug!("[VLAPIC] DFR in Unknown Model {:#010X}", dfr);
            }
        }
    }

    /// Figure 11-14. Spurious-Interrupt Vector Register (SVR)
    /// Handle writes to the SVR register.
    fn write_svr(&mut self) -> AxResult {
        let new = self.regs().SVR.extract();
        let old = self.svr_last;

        self.svr_last = new;

        if old.is_set(SPURIOUS_INTERRUPT_VECTOR::APICSoftwareEnableDisable)
            && !new.is_set(SPURIOUS_INTERRUPT_VECTOR::APICSoftwareEnableDisable)
        {
            debug!("[VLAPIC] vlapic [{}] is software-disabled", self.vapic_id);
            // The apic is now disabled so stop the apic timer
            // and mask all the LVT entries.
            self.virtual_timer.delete_timer()?;
            self.mask_lvts()?;
            warn!("VM wire mode should be changed to INTR here, unimplemented");
        } else if !old.is_set(SPURIOUS_INTERRUPT_VECTOR::APICSoftwareEnableDisable)
            && new.is_set(SPURIOUS_INTERRUPT_VECTOR::APICSoftwareEnableDisable)
        {
            debug!("[VLAPIC] vlapic [{}] is software-enabled", self.vapic_id);

            // The apic is now enabled so restart the apic timer
            // if it is configured in periodic mode.
            if self.virtual_timer.is_periodic() {
                debug!("Restarting the apic timer");
                self.virtual_timer.start_timer()?;
            }
        }

        Ok(())
    }

    fn write_esr(&mut self) {
        let esr = self.regs().ESR.get();
        debug!("[VLAPIC] write ESR register to {:#010X}", esr);
        self.regs().ESR.set(self.esr_pending);
        self.esr_pending = 0;
    }

    fn write_icr(&mut self) {
        self.regs()
            .ICR_LO
            .modify(INTERRUPT_COMMAND_LOW::DeliveryStatus::Idle);

        let icr_low = self.regs().ICR_LO.extract();

        let (dest, is_broadcast) = if self.is_x2apic_enabled() {
            use crate::consts::x2apic::X2APIC_BROADCAST_DEST_ID;
            let dest = self.regs().ICR_HI.get();
            (dest, dest == X2APIC_BROADCAST_DEST_ID)
        } else {
            use crate::consts::xapic::XAPIC_BROADCAST_DEST_ID;
            let dest = self.regs().ICR_HI.read(INTERRUPT_COMMAND_HIGH::Destination);
            (dest, dest == XAPIC_BROADCAST_DEST_ID)
        };

        let vec = icr_low.read(INTERRUPT_COMMAND_LOW::Vector);
        let mode = icr_low.read(INTERRUPT_COMMAND_LOW::DeliveryMode);
        let is_phys = icr_low.is_set(INTERRUPT_COMMAND_LOW::DestinationMode);
        let shorthand = icr_low.read(INTERRUPT_COMMAND_LOW::DestinationShorthand);

        if mode == INTERRUPT_COMMAND_LOW::DeliveryMode::Fixed.value && vec < 16 {
            // self.set_err();
        }

    }

    fn extract_lvt_val(&self, offset: ApicRegOffset) -> u32 {
        match offset {
            ApicRegOffset::LvtCMCI => self.regs().LVT_CMCI.get(),
            ApicRegOffset::LvtTimer => self.regs().LVT_TIMER.get(),
            ApicRegOffset::LvtThermal => self.regs().LVT_THERMAL.get(),
            ApicRegOffset::LvtPmc => self.regs().LVT_PMI.get(),
            ApicRegOffset::LvtLint0 => self.regs().LVT_LINT0.get(),
            ApicRegOffset::LvtLint1 => self.regs().LVT_LINT1.get(),
            ApicRegOffset::LvtErr => self.regs().LVT_ERROR.get(),
            _ => {
                warn!("[VLAPIC] read unsupported APIC register: {:?}", offset);
                0
            }
        }
    }

    fn write_lvt(&mut self, offset: ApicRegOffset) -> AxResult {
        let mut val = self.extract_lvt_val(offset);

        if self
            .regs()
            .SVR
            .is_set(SPURIOUS_INTERRUPT_VECTOR::APICSoftwareEnableDisable)
        {
            val |= APIC_LVT_M;
        }

        // Mask::Masked, Delivery Status:SendPending, Vector::SET(0xff)
        let mut mask = APIC_LVT_M | APIC_LVT_DS | APIC_LVT_VECTOR;

        match offset {
            ApicRegOffset::LvtTimer => {
                mask |= LVT_TIMER::TimerMode::SET.mask();
                val &= mask;
                self.regs().LVT_TIMER.set(val);
                self.lvt_last.lvt_timer.set(val);

                let new_timer_mode = self.timer_mode()?;
                // A write to the LVT Timer Register that changes the timer mode disarms the local APIC timer.
                if new_timer_mode != self.virtual_timer.timer_mode() {
                    self.virtual_timer.update_timer_mode(new_timer_mode)?;
                }
            }
            ApicRegOffset::LvtErr => {
                val &= mask;
                self.regs().LVT_ERROR.set(val);
                self.lvt_last.lvt_err.set(val);
            }
            ApicRegOffset::LvtLint0 => {
                mask |= LVT_LINT0::TriggerMode::SET.mask();
                mask |= LVT_LINT0::RemoteIRR::SET.mask();
                mask |= LVT_LINT0::InterruptInputPinPolarity::SET.mask();
                mask |= LVT_LINT0::DeliveryMode::SET.mask();
                val &= mask;

                // vlapic mask/unmask LINT0 for ExtINT?
                if (val & LVT_LINT0::DeliveryMode::SET.mask())
                    == LVT_LINT0::DeliveryMode::ExtINT.mask()
                {
                    let last = self.lvt_last.lvt_lint0;
                    if last.is_set(LVT_LINT0::Mask) && val & LVT_LINT0::Mask::SET.mask() == 0 {
                        // mask -> unmask: may from every vlapic in the vm
                        warn!("vpic wire mode change to LAPIC, unimplemented");
                    } else if !last.is_set(LVT_LINT0::Mask)
                        && val & LVT_LINT0::Mask::SET.mask() != 0
                    {
                        // unmask -> mask: only from the vlapic LINT0-ExtINT enabled
                        warn!("vpic wire mode change to NULL, unimplemented");
                    } else {
                        // APIC_LVT_M unchanged. No action required.
                    }
                }

                self.regs().LVT_LINT0.set(val);
                self.lvt_last.lvt_lint0.set(val);
            }
            ApicRegOffset::LvtLint1 => {
                mask |= LVT_LINT1::TriggerMode::SET.mask();
                mask |= LVT_LINT1::RemoteIRR::SET.mask();
                mask |= LVT_LINT1::InterruptInputPinPolarity::SET.mask();
                mask |= LVT_LINT1::DeliveryMode::SET.mask();
                val &= mask;

                self.regs().LVT_LINT1.set(val);
                self.lvt_last.lvt_lint1.set(val);
            }
            ApicRegOffset::LvtCMCI => {
                mask |= LVT_CMCI::DeliveryMode::SET.mask();
                val &= mask;
                self.regs().LVT_CMCI.set(val);
                self.lvt_last.lvt_cmci.set(val);
            }
            ApicRegOffset::LvtPmc => {
                mask |= LVT_PERFORMANCE_COUNTER::DeliveryMode::SET.mask();
                val &= mask;
                self.regs().LVT_PMI.set(val);
                self.lvt_last.lvt_perf_count.set(val);
            }
            ApicRegOffset::LvtThermal => {
                mask |= LVT_THERMAL_MONITOR::DeliveryMode::SET.mask();
                val &= mask;
                self.regs().LVT_THERMAL.set(val);
                self.lvt_last.lvt_thermal.set(val);
            }
            _ => {
                warn!("[VLAPIC] write unsupported APIC register: {:?}", offset);
                return Err(AxError::InvalidInput);
            }
        }
        Ok(())
    }

    fn mask_lvts(&mut self) -> AxResult {
        self.regs().LVT_CMCI.modify(LVT_CMCI::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtCMCI)?;

        self.regs().LVT_TIMER.modify(LVT_TIMER::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtTimer)?;

        self.regs()
            .LVT_THERMAL
            .modify(LVT_THERMAL_MONITOR::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtThermal)?;

        self.regs()
            .LVT_PMI
            .modify(LVT_PERFORMANCE_COUNTER::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtPmc)?;

        self.regs().LVT_LINT0.modify(LVT_LINT0::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtLint0)?;

        self.regs().LVT_LINT1.modify(LVT_LINT1::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtLint1)?;

        self.regs().LVT_ERROR.modify(LVT_ERROR::Mask::SET);
        self.write_lvt(ApicRegOffset::LvtErr)?;

        Ok(())
    }

    fn write_icrtmr(&mut self) {}

    fn write_dcr(&mut self) {}
}

fn extract_index_u32(vector: u32) -> usize {
    vector as usize >> 5
}

fn extract_index_and_bitpos_u32(vector: u32) -> (usize, usize) {
    (extract_index_u32(vector), vector as usize & 0x1F)
}

/// Figure 11-18. Task-Priority Register (TPR)
/// [7:4]: Task-Priority Class
/// [3:0]: Task-Priority Sub-Class
fn prio(x: u32) -> u32 {
    (x >> 4) & 0xf
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
                } else if self.is_x2apic_enabled() ^ (width == AccessWidth::Qword) {
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
                value = self.virtual_timer.current_counter() as _;
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
        &mut self,
        offset: ApicRegOffset,
        val: usize,
        width: AccessWidth,
        context: DeviceRWContext,
    ) -> AxResult {
        let data32 = val as u32;

        match offset {
            ApicRegOffset::ID => {
                // Force APIC ID to be read-only.
                // self.regs().ID.set(val as _);
            }
            ApicRegOffset::EOI => {
                self.process_eoi();
            }
            ApicRegOffset::LDR => {
                self.regs().LDR.set(data32);
                self.write_ldr();
            }
            ApicRegOffset::DFR => {
                self.regs().DFR.set(data32);
                self.write_dfr();
            }
            ApicRegOffset::SIVR => {
                self.regs().SVR.set(data32);
                self.write_svr()?;
            }
            ApicRegOffset::ESR => {
                self.regs().ESR.set(data32);
                self.write_esr();
            }
            ApicRegOffset::ICRLow => {
                if self.is_x2apic_enabled() && width == AccessWidth::Qword {
                    debug!("[VLAPIC] write ICR register: {:#018X} in X2APIC mode", val);
                    self.regs().ICR_HI.set((val >> 32) as u32);
                } else if self.is_x2apic_enabled() ^ (width == AccessWidth::Qword) {
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
                self.regs().ICR_LO.set(data32);
                self.write_icr();
            }
            // Local Vector Table registers.
            // Local Vector Table registers.
            ApicRegOffset::LvtCMCI => {
                self.regs().LVT_CMCI.set(data32);
                self.write_lvt(offset)?;
            }
            ApicRegOffset::LvtTimer => {
                self.regs().LVT_TIMER.set(data32);
                self.write_lvt(offset)?;
            }
            ApicRegOffset::LvtThermal => {
                self.regs().LVT_THERMAL.set(data32);
                self.write_lvt(offset)?;
            }
            ApicRegOffset::LvtPmc => {
                self.regs().LVT_PMI.set(data32);
                self.write_lvt(offset)?;
            }
            ApicRegOffset::LvtLint0 => {
                self.regs().LVT_LINT0.set(data32);
                self.write_lvt(offset)?;
            }
            ApicRegOffset::LvtLint1 => {
                self.regs().LVT_LINT1.set(data32);
                self.write_lvt(offset)?;
            }
            ApicRegOffset::LvtErr => {
                self.regs().LVT_ERROR.set(data32);
                self.write_lvt(offset)?;
            }
            // Timer registers.
            ApicRegOffset::TimerInitCount => {
                if self.timer_mode()? == TimerMode::TscDeadline {
                    warn!(
                        "[VLAPIC] write TimerInitCount register: ignore icr_timer in TSCDEADLINE mode"
                    );
                    return Ok(());
                }
                self.regs().ICR_TIMER.set(data32);
                self.write_icrtmr();
            }
            ApicRegOffset::TimerDivConf => {
                self.regs().DCR_TIMER.set(data32);
                self.write_dcr();
            }
            ApicRegOffset::SelfIPI => {
                if self.is_x2apic_enabled() {
                    self.regs().SELF_IPI.set(data32);
                    self.handle_self_ipi();
                } else {
                    warn!("[VLAPIC] write SelfIPI register: unsupported in xAPIC mode");
                    return Err(AxError::InvalidInput);
                }
            }
            _ => {
                warn!("[VLAPIC] write unsupported APIC register: {:?}", offset);
                return Err(AxError::InvalidInput);
            }
        }

        debug!("[VLAPIC] write {} register: {:#010X}", offset, val);

        Ok(())
    }
}
