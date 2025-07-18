# x86_vlapic

[![CI](https://github.com/arceos-hypervisor/x86_vlapic/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/arceos-hypervisor/x86_vlapic/actions/workflows/ci.yml)

A Rust library for virtualizing x86 Local Advanced Programmable Interrupt Controller (LAPIC) functionality.

## Overview

This library provides a software implementation of the x86 Local APIC (Advanced Programmable Interrupt Controller) for hypervisor use cases. It virtualizes the LAPIC registers and functionality according to the Intel Software Developer's Manual (SDM) specifications.

## Features

- **Complete LAPIC Register Support**: Implements all major LAPIC registers including:
  - Local Vector Table (LVT) registers (Timer, Thermal, Performance Counter, LINT0/1, Error, CMCI)
  - Interrupt Command Register (ICR)
  - Error Status Register (ESR)
  - Spurious Interrupt Vector Register (SVR)
  - Task Priority Register (TPR)
  - And more...

- **Timer Virtualization**: Full support for LAPIC timer modes:
  - One-shot mode
  - Periodic mode
  - TSC-Deadline mode

- **Memory-Mapped I/O Simulation**: Provides MMIO-like interface for register access

- **Local Register Copies**: Efficient local copies of register state for performance

## Architecture

The library is structured into several key modules:

### Core Modules

- [`src/vlapic.rs`](src/vlapic.rs) - Main virtual LAPIC implementation
- [`src/timer.rs`](src/timer.rs) - LAPIC timer virtualization
- [`src/consts.rs`](src/consts.rs) - Constants and register offset definitions
- [`src/utils.rs`](src/utils.rs) - Utility functions

### Register Definitions

- [`src/regs/mod.rs`](src/regs/mod.rs) - Main register structure definitions
- [`src/regs/lvt/`](src/regs/lvt/) - Local Vector Table register implementations
  - [`timer.rs`](src/regs/lvt/timer.rs) - LVT Timer Register
  - [`lint0.rs`](src/regs/lvt/lint0.rs) - LVT LINT0 Register
  - [`lint1.rs`](src/regs/lvt/lint1.rs) - LVT LINT1 Register
  - [`error.rs`](src/regs/lvt/error.rs) - LVT Error Register
  - [`thermal.rs`](src/regs/lvt/thermal.rs) - LVT Thermal Monitor Register
  - [`perfmon.rs`](src/regs/lvt/perfmon.rs) - LVT Performance Counter Register
  - [`cmci.rs`](src/regs/lvt/cmci.rs) - LVT CMCI Register
- [`src/regs/timer/`](src/regs/timer/) - Timer-related register definitions
  - [`dcr.rs`](src/regs/timer/dcr.rs) - Divide Configuration Register
- Other register modules for ICR, ESR, SVR, etc.

## Basic Example

``` rust,ignore
use x86_vlapic::EmulatedLocalApic;
use axvisor_api::vmm::{VMId, VCpuId};

// Create a new emulated Local APIC for VM 1, VCPU 0
let vm_id = VMId::from(1 as usize);
let vcpu_id = VCpuId::from(0 as usize);
let apic = EmulatedLocalApic::new(vm_id, vcpu_id);

// Get the shared virtual APIC access page address (static for all instances)
let access_addr = EmulatedLocalApic::virtual_apic_access_addr();
assert!(access_addr.is_aligned(PAGE_SIZE_4K));

// Get the per-VCPU virtual APIC page address
let page_addr = apic.virtual_apic_page_addr();
assert!(page_addr.is_aligned(PAGE_SIZE_4K));
```

## Target Platform

This library is designed for x86_64 architecture and targets `x86_64-unknown-none` for no-std environments, making it suitable for hypervisor and kernel development.

---

**Note**: This is a virtualization library and does not interact with actual hardware LAPIC. It's designed for use in hypervisors and virtual machine monitors.