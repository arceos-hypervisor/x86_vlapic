use tock_registers::register_bitfields;
use tock_registers::registers::ReadWrite;

register_bitfields! {
    u32,
    pub LVT_ERROR [
        /// Reserved2
        Reserved2 OFFSET(17) NUMBITS(15) [],
        /// Mask
        Mask OFFSET(16) NUMBITS(1) [
            /// Not masked.
            NotMasked = 0,
            /// Masked.
            Masked = 1
        ],
        Reserved1 OFFSET(13) NUMBITS(3) [],
        /// Delivery Status (Read Only): Indicates the interrupt delivery status
        DeliveryStatus OFFSET(12) NUMBITS(1) [
            /// 0 (Idle)
            /// There is currently no activity for this interrupt source,
            /// or the previous interrupt from this source was delivered to the processor core and accepted.
            Idle = 0,
            /// 1 (Send Pending)
            /// Indicates that an interrupt from this source has been delivered to the processor core
            /// but has not yet been accepted (see Section 11.5.5, “Local Interrupt Acceptance”).
            SendPending = 1
        ],
        Reserved0 OFFSET(8) NUMBITS(4) [],
        /// Vector: Interrupt vector number.
        Vector OFFSET(0) NUMBITS(8) [],
    ]
}

/// LVT Error Register (FEE0 0370H)
/// Specifies interrupt delivery when the APIC detects an internal error (see Section 11.5.3, “Error Handling”).
pub type LvtErrorRegister = ReadWrite<u32, LVT_ERROR::Register>;
