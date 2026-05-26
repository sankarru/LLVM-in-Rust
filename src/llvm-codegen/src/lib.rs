//! Target-independent code generation: legalization, instruction selection, register allocation, scheduling, and emission.

pub mod assembler;
/// DWARF CFI (.eh_frame) emission for stack unwinding.
pub mod cfi;
pub mod dwarf_vars;
/// Public API for `emit`.
pub mod emit;
/// Public API for `isel`.
pub mod isel;
/// Public API for `legalize`.
pub mod legalize;
/// Public API for `regalloc`.
pub mod regalloc;
/// Public API for `regalloc_gc`.
pub mod regalloc_gc;
/// LLVM intrinsic recognition and lowering.
pub mod intrinsics;
/// Public API for `schedule`.
pub mod schedule;

/// Public API for `re-export`.
pub use assembler::{
    assemble_bytes, assemble_object, assemble_with_report, AssembledObject, IntegratedAssembler,
    McAssembler, McAssemblyReport,
};
/// Public API for `re-export`.
pub use emit::{
    emit_globals, emit_object, sizeof_ty, Emitter, ObjectFile, ObjectFormat, Reloc, RelocKind,
    Section, Symbol,
};
/// Public API for `re-export`.
pub use isel::{IselBackend, MInstr, MOpcode, MOperand, MachineBlock, MachineFunction, PReg, RegClass, VReg};
/// Public API for `re-export`.
pub use cfi::{x86_fp_prologue_cfi, CfiInstr, CfiWriter};
/// Public API for `re-export`.
pub use dwarf_vars::{
    build_variable_die, encode_breg6, encode_location, encode_reg, encode_sleb128,
    encode_uleb128, VarLocation, X86_DWARF_REG,
};
/// Public API for `re-export`.
pub use regalloc::{
    allocate_registers, apply_allocation, compute_live_intervals, graph_color,
    insert_spill_reloads, linear_scan, RegAllocStrategy,
};
/// Public API for `intrinsics` re-exports.
pub use intrinsics::{is_llvm_intrinsic, lower_intrinsic};
/// Public API for `schedule` re-exports.
pub use schedule::{
    apply_schedule, build_dep_dag, compute_critical_paths, list_schedule, x86_latency, DepEdge,
    DepKind,
};

/// Target Transform Info cost model trait and generic fallback.
pub mod tti;
/// Public API for `tti` re-exports.
pub use tti::{GenericTti, TargetTransformInfo};

/// LSDA (.gcc_except_table) builder for exception-handling metadata.
pub mod lsda;
/// Public API for `lsda` re-exports.
pub use lsda::{
    get_or_create_section_data, write_sleb128, write_uleb128, CallSiteRecord, LsdaBuilder,
    XdataBuilder,
};
