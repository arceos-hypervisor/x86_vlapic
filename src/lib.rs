#![no_std]

extern crate alloc;

#[macro_use]
extern crate log;

mod consts;
mod vlapic;

use alloc::boxed::Box;

use axerrno::AxResult;
use memory_addr::AddrRange;

use axaddrspace::device::{AccessWidth, SysRegAddr, SysRegAddrRange};
use axaddrspace::{AxMmHal, GuestPhysAddr};
use axdevice_base::{BaseDeviceOps, DeviceRWContext, EmuDeviceType, InterruptInjector};

use crate::vlapic::VirtualApicRegs;

pub struct EmulatedLocalApic<H: AxMmHal> {
    vlapic_regs: VirtualApicRegs<H>,
}

impl<H: AxMmHal> EmulatedLocalApic<H> {
    pub fn new() -> Self {
        EmulatedLocalApic {
            vlapic_regs: VirtualApicRegs::new(),
        }
    }
}

impl<H: AxMmHal> BaseDeviceOps<AddrRange<GuestPhysAddr>> for EmulatedLocalApic<H> {
    fn emu_type(&self) -> EmuDeviceType {
        EmuDeviceType::EmuDeviceTInterruptController
    }

    fn address_range(&self) -> AddrRange<GuestPhysAddr> {
        use crate::consts::xapic::{APIC_MMIO_SIZE, DEFAULT_APIC_BASE};
        AddrRange::new(
            GuestPhysAddr::from_usize(DEFAULT_APIC_BASE),
            GuestPhysAddr::from_usize(DEFAULT_APIC_BASE + APIC_MMIO_SIZE),
        )
    }

    fn handle_read(
        &self,
        addr: GuestPhysAddr,
        width: AccessWidth,
        context: DeviceRWContext,
    ) -> AxResult<usize> {
        debug!(
            "EmulatedLocalApic::handle_read: addr={:?}, width={:?}, context={:?}",
            addr, width, context.vcpu_id
        );
        todo!()
    }

    fn handle_write(
        &self,
        addr: GuestPhysAddr,
        width: AccessWidth,
        val: usize,
        context: DeviceRWContext,
    ) -> AxResult {
        debug!(
            "EmulatedLocalApic::handle_write: addr={:?}, width={:?}, val={:#x}, context={:?}",
            addr, width, val, context.vcpu_id
        );
        todo!()
    }

    fn set_interrupt_injector(&mut self, _injector: Box<InterruptInjector>) {
        todo!()
    }
}

impl<H: AxMmHal> BaseDeviceOps<SysRegAddrRange> for EmulatedLocalApic<H> {
    fn emu_type(&self) -> EmuDeviceType {
        EmuDeviceType::EmuDeviceTInterruptController
    }

    fn address_range(&self) -> SysRegAddrRange {
        use crate::consts::x2apic::{X2APIC_MSE_REG_BASE, X2APIC_MSE_REG_SIZE};
        SysRegAddrRange::new(
            SysRegAddr(X2APIC_MSE_REG_BASE),
            SysRegAddr(X2APIC_MSE_REG_BASE + X2APIC_MSE_REG_SIZE),
        )
    }

    fn handle_read(
        &self,
        addr: SysRegAddr,
        width: AccessWidth,
        context: DeviceRWContext,
    ) -> AxResult<usize> {
        debug!(
            "EmulatedLocalApic::handle_read: addr={:?}, width={:?}, context={:?}",
            addr, width, context.vcpu_id
        );
        todo!()
    }

    fn handle_write(
        &self,
        addr: SysRegAddr,
        width: AccessWidth,
        val: usize,
        context: DeviceRWContext,
    ) -> AxResult {
        debug!(
            "EmulatedLocalApic::handle_write: addr={:?}, width={:?}, val={:#x}, context={:?}",
            addr, width, val, context.vcpu_id
        );
        todo!()
    }

    fn set_interrupt_injector(&mut self, _injector: Box<InterruptInjector>) {
        todo!()
    }
}
