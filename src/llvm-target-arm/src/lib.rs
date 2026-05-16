//! AArch64 target backend: register definitions, instruction set, ABI, and IR lowering.

pub mod abi;
pub mod encode;
pub mod instructions;
pub mod lower;
pub mod regs;

pub use encode::AArch64Emitter;
pub use lower::AArch64Backend;
