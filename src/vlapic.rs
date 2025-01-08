use paste::paste;

use memory_addr::PAGE_SIZE_4K;

use axaddrspace::{AxMmHal, HostPhysAddr, HostVirtAddr, PhysFrame};

use crate::consts::ApicRegOffset;

#[repr(align(4096))]
struct APICAccessPage([u8; PAGE_SIZE_4K]);

static VIRTUAL_APIC_ACCESS_PAGE: APICAccessPage = APICAccessPage([0; PAGE_SIZE_4K]);

// Virtual-APIC Registers.
pub struct VirtualApicRegs<H: AxMmHal> {
    /// The virtual-APIC page is a 4-KByte region of memory
    /// that the processor uses to virtualize certain accesses to APIC registers and to manage virtual interrupts.
    /// The physical address of the virtual-APIC page is the virtual-APIC address,
    /// a 64-bit VM-execution control field in the VMCS (see Section 25.6.8).
    virtual_apic_page: PhysFrame<H>,
}

impl<H: AxMmHal> VirtualApicRegs<H> {
    pub fn new() -> Self {
        Self {
            virtual_apic_page: PhysFrame::alloc_zero().expect("allocate virtual-APIC page failed"),
        }
    }

    /// APIC-access address (64 bits).
    /// This field contains the physical address of the 4-KByte APIC-access page.
    /// If the “virtualize APIC accesses” VM-execution control is 1,
    /// access to this page may cause VM exits or be virtualized by the processor.
    /// See Section 30.4.
    pub fn virtual_apic_access_addr() -> HostPhysAddr {
        H::virt_to_phys(HostVirtAddr::from_usize(
            VIRTUAL_APIC_ACCESS_PAGE.0.as_ptr() as usize,
        ))
    }

    /// Virtual-APIC address (64 bits).
    /// This field contains the physical address of the 4-KByte virtual-APIC page.
    /// The processor uses the virtual-APIC page to virtualize certain accesses to APIC registers and to manage virtual interrupts;
    /// see Chapter 30.
    pub fn virtual_apic_page_addr(&self) -> HostPhysAddr {
        self.virtual_apic_page.start_paddr()
    }
}

impl<H: AxMmHal> Drop for VirtualApicRegs<H> {
    fn drop(&mut self) {
        H::dealloc_frame(self.virtual_apic_page.start_paddr());
    }
}

macro_rules! impl_virtual_apic_set_reg {
    ($name:ident, $offset:expr) => {
        paste! {
            #[doc = concat!("Sets the ", stringify!($name), " register.")]
            fn [<set_$name>](&self, value: u32) {
                let ptr = self.virtual_apic_page.as_mut_ptr().wrapping_add($offset) as *mut u32;
                unsafe { ptr.write_volatile(value) }
            }
        }
    };
    ($name:ident, $offset:expr, $length:expr) => {
        paste! {
            #[doc = concat!("Sets the ", stringify!($name), " register at the given index.")]
            fn [<set_$name>](&self, index: usize, value: u32) {
                assert!(index < $length);
                let ptr = self
                    .virtual_apic_page
                    .as_mut_ptr()
                    .wrapping_add($offset + index * 0x10) as *mut u32;
                unsafe { ptr.write_volatile(value) }
            }
        }
    };
    () => {};
}

macro_rules! impl_virtual_apic_get_reg {
    ($name:ident, $offset:expr) => {
        paste! {
            #[doc = concat!("Gets the ", stringify!($name), " register.")]
            fn [<get_$name>](&self) -> u32 {
                let ptr = self.virtual_apic_page.as_mut_ptr().wrapping_add($offset) as *mut u32;
                unsafe { ptr.read_volatile() }
            }
        }
    };
    ($name:ident, $offset:expr, $length:expr) => {
        paste! {
            #[doc = concat!("Gets the ", stringify!($name), " register at the given index.")]
            fn [<get_$name>](&self, index: usize) -> u32 {
                assert!(index < $length);
                let ptr = self
                    .virtual_apic_page
                    .as_mut_ptr()
                    .wrapping_add($offset + index * 0x10) as *mut u32;
                unsafe { ptr.read_volatile() }
            }
        }
    };
    () => {};
}

macro_rules! impl_virtual_apic_regs_get_set {
    ($(($name:ident, $offset:expr)),* $(,)?) => {
        $(
            impl_virtual_apic_get_reg!($name, $offset);
            impl_virtual_apic_set_reg!($name, $offset);
        )*
    };
    ($(($name:ident, $offset:expr, $length:expr)),* $(,)?) => {
        $(
            impl_virtual_apic_get_reg!($name, $offset, $length);
            impl_virtual_apic_set_reg!($name, $offset, $length);
        )*
    };
    () => {};
}

/// 30.1 VIRTUAL APIC STATE
/// 30.1.1 Virtualized APIC Registers
#[allow(unused)]
impl<H: AxMmHal> VirtualApicRegs<H> {
    impl_virtual_apic_regs_get_set!(
        // Local APIC ID register (VID): the 32-bit field located at offset 000H on the virtual-APIC page.
        (id, 0x20),
        // Local APIC Version register (VVER): the 32-bit field located at offset 030H on the virtual-APIC page.
        (version, 0x30),
        // Virtual task-priority register (VTPR): the 32-bit field located at offset 080H on the virtual-APIC page.
        (tpr, 0x80),
        // Virtual APIC-priority register (VAPR): the 32-bit field located at offset 090H on the virtual-APIC page.
        (apr, 0x90),
        // Virtual processor-priority register (VPPR): the 32-bit field located at offset 0A0H on the virtual-APIC page.
        (ppr, 0xA0),
        // Virtual end-of-interrupt register (VEOI): the 32-bit field located at offset 0B0H on the virtual-APIC page.
        (eoi, 0xB0),
        // Virtual Remote Read Register1 (RRD): the 32-bit field located at offset 0C0H on the virtual-APIC page.
        (rrd, 0xC0),
        // Virtual Remote Read Register1 (RRD): the 32-bit field located at offset 0D0H on the virtual-APIC page.
        (ldr, 0xD0),
        // Virtual Destination Format Register (DFR): the 32-bit field located at offset 0E0H on the virtual-APIC page.
        (dfr, 0xE0),
        // Virtual Spurious Interrupt Vector Register (SVR): the 32-bit field located at offset 0F0H on the virtual-APIC page.
        (svr, 0xF0),
        // Virtual Error Status Register (VESR): the 32-bit field located at offset 280H on the virtual-APIC page.
        (esr, 0x280),
        // Virtual LVT Corrected Machine Check Interrupt (CMCI) Register
        (lvt_cmci, 0x2F0),
        // Virtual Interrupt Command Register (ICR): the 64-bit field located at offset 300H on the virtual-APIC page.
        (icr_lo, 0x300),
        (icr_hi, 0x310),
        // Virtual LVT Timer Register: the 32-bit field located at offset 320H on the virtual-APIC page.
        (lvt_timer, 0x320),
        // Virtual LVT Thermal Sensor register: the 32-bit field located at offset 330H on the virtual-APIC page.
        (lvt_thermal, 0x330),
        // Virtual LVT Performance Monitoring Counters register: the 32-bit field located at offset 340H on the virtual-APIC page.
        (lvt_pmi, 0x340),
        // Virtual LVT LINT0 register: the 32-bit field located at offset 350H on the virtual-APIC page.
        (lvt_lint0, 0x350),
        // Virtual LVT LINT1 register: the 32-bit field located at offset 360H on the virtual-APIC page.
        (lvt_lint1, 0x360),
        // Virtual LVT Error register: the 32-bit field located at offset 370H on the virtual-APIC page.
        (lvt_error, 0x370),
        // Virtual Initial Count Register (for Timer): the 32-bit field located at offset 380H on the virtual-APIC page.
        (icr_timer, 0x380),
        // Virtual Current Count Register (for Timer): the 32-bit field located at offset 390H on the virtual-APIC page.
        (ccr_timer, 0x390),
        // Virtual Divide Configuration Register (for Timer): the 32-bit field located at offset 3E0H on the virtual-APIC page.
        (dcr_timer, 0x3E0),
        // Virtual SELF IPI Register: the 32-bit field located at offset 3F0H on the virtual-APIC page.
        (self_ipi, 0x3F0)
    );

    impl_virtual_apic_regs_get_set!(
        // Virtual interrupt-service register (VISR):
        // the 256-bit value comprising eight non-contiguous 32-bit fields at offsets
        // 100H, 110H, 120H, 130H, 140H, 150H, 160H, and 170H on the virtual-APIC page.
        (isr, 0x100, 8),
        // Virtual trigger-mode register (VTMR):
        // the 256-bit value comprising eight non-contiguous 32-bit fields at offsets
        // 180H, 190H, 1A0H, 1B0H, 1C0H, 1D0H, 1E0H, and 1F0H on the virtual-APIC page.
        (tmr, 0x180, 8),
        // Virtual interrupt-request register (VIRR):
        // the 256-bit value comprising eight non-contiguous 32-bit fields at offsets
        // 200H, 210H, 220H, 230H, 240H, 250H, 260H, and 270H on the virtual-APIC page.
        // Bit x of the VIRR is at bit position (x & 1FH) at offset (200H | ((x & E0H) » 1)).
        // The processor uses only the low 4 bytes of each of the 16-Byte fields at offsets 200H, 210H, 220H, 230H, 240H, 250H, 260H, and 270H.
        (irr, 0x200, 8),
    );
}

impl<H: AxMmHal> VirtualApicRegs<H> {
    /// Returns the host physical address of the virtual-APIC page.
    pub fn virtual_apic_page_host_paddr(&self) -> HostPhysAddr {
        self.virtual_apic_page.start_paddr()
    }

    pub fn handle_read(&self, offset: ApicRegOffset) -> u32 {
        let mut value = 0;
        match offset {
            ApicRegOffset::ID => {
                value = self.get_id();
                debug!("[VLAPIC] read APIC ID register: {:#010X}", value);
            }
            ApicRegOffset::Version => {
                value = self.get_version();
                debug!("[VLAPIC] read APIC Version register: {:#010X}", value);
            }
            ApicRegOffset::PPR => {
                value = self.get_ppr();
                debug!("[VLAPIC] read PPR register: {:#010X}", value);
            }
            ApicRegOffset::EOI => {
                value = self.get_eoi();
                debug!("[VLAPIC] read EOI register: {:#010X}", value);
            }
            ApicRegOffset::LDR => {
                value = self.get_ldr();
                debug!("[VLAPIC] read LDR register: {:#010X}", value);
            }
            ApicRegOffset::DFR => {
                value = self.get_dfr();
                debug!("[VLAPIC] read DFR register: {:#010X}", value);
            }
            ApicRegOffset::SIVR => {
                value = self.get_svr();
                debug!("[VLAPIC] read SIVR register: {:#010X}", value);
            }
            ApicRegOffset::ISR(index) => {
                value = self.get_isr(index.as_usize());
                debug!("[VLAPIC] read ISR[{}] register: {:#010X}", index, value);
            }
            ApicRegOffset::TMR(index) => {
                value = self.get_tmr(index.as_usize());
                debug!("[VLAPIC] read TMR[{}] register: {:#010X}", index, value);
            }
            ApicRegOffset::IRR(index) => {
                value = self.get_irr(index.as_usize());
                debug!("[VLAPIC] read IRR[{}] register: {:#010X}", index, value);
            }
            ApicRegOffset::ESR => {
                value = self.get_esr();
                debug!("[VLAPIC] read ESR register: {:#010X}", value);
            }
            _ => {
                warn!("[VLAPIC] read unknown APIC register: {:?}", offset);
            }
        }
        value
    }
}
