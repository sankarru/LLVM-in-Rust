# Architecture Guide

This document describes the internal architecture of LLVM-in-Rust for new
contributors. It walks through each layer of the compiler pipeline — IR,
analysis, optimisation, code generation, target backends, text-format parsing,
and bitcode serialisation — explaining the key data structures, design
decisions, and how the layers interact.

---

## Table of Contents

1. [IR Layer (`llvm-ir`)](#1-ir-layer-llvm-ir)
2. [Analysis Layer (`llvm-analysis`)](#2-analysis-layer-llvm-analysis)
3. [Optimization Passes (`llvm-transforms`)](#3-optimization-passes-llvm-transforms)
4. [Code Generation (`llvm-codegen`)](#4-code-generation-llvm-codegen)
5. [Target Backends](#5-target-backends)
6. [IR Text Format (`llvm-ir-parser`)](#6-ir-text-format-llvm-ir-parser)
7. [LRIR Bitcode (`llvm-bitcode`)](#7-lrir-bitcode-llvm-bitcode)
8. [Key Design Decisions](#8-key-design-decisions)

---

## 1. IR Layer (`llvm-ir`)

Source: `src/llvm-ir/src/`

The IR crate is the foundation that every other crate depends on. It defines
the in-memory representation of LLVM IR and provides builder and printer APIs.
There are no dependencies on any other crate in the workspace.

### 1.1 Arena allocation model

The IR avoids pointer-based graphs (which would require `Rc<RefCell<...>>` and
create reference cycles) by using two flat arenas backed by `Vec<T>`:

- **`Context`** owns all `TypeId`s and `ConstId`s via `types: Vec<TypeData>`
  and `constants: Vec<ConstantData>` (`src/llvm-ir/src/context.rs`).
- **`Function`** owns all `InstrId`s via a flat `instructions: Vec<Instruction>`
  pool (`src/llvm-ir/src/function.rs:19`).

Every handle into these arenas is a newtype wrapping `u32`:

```
TypeId(u32)   — index into Context::types
ConstId(u32)  — index into Context::constants
InstrId(u32)  — index into Function::instructions
BlockId(u32)  — index into Function::blocks
ArgId(u32)    — index into Function::args
FunctionId(u32) — index into Module::functions
GlobalId(u32)   — index into Module::globals
```

All of these are `Copy` (no heap allocation, no lifetime), so they can be
stored freely inside `InstrKind` variants and other data structures without
borrow-checker complications.

Allocation is O(1) amortised: `alloc_instr` does a `Vec::push` and returns
`InstrId(self.instructions.len() - 1)` (`src/llvm-ir/src/function.rs:114`).
There are no destructors to run and no reference counting. Freeing the
`Function` or `Context` reclaims all memory at once.

```
Context
 ├── types: Vec<TypeData>        ← indexed by TypeId(u32)
 ├── type_map: HashMap<TypeData, TypeId>   (structural interning)
 ├── named_struct_map: HashMap<String, TypeId>  (nominal lookup)
 └── constants: Vec<ConstantData>   ← indexed by ConstId(u32)

Function
 ├── instructions: Vec<Instruction>  ← indexed by InstrId(u32)
 ├── blocks: Vec<BasicBlock>         ← indexed by BlockId(u32)
 └── args: Vec<Argument>             ← indexed by ArgId(u32)
```

### 1.2 SSA invariant and ValueRef

The IR is always in SSA form: every value is defined exactly once. The `ValueRef`
enum (`src/llvm-ir/src/context.rs:45`) is the universal currency for referring
to any SSA value:

```rust
pub enum ValueRef {
    Instruction(InstrId),   // result of an instruction
    Argument(ArgId),        // function parameter
    Constant(ConstId),      // compile-time constant from Context
    Global(GlobalId),       // global variable or function reference
}
```

`ValueRef` is `Copy` so it can be embedded directly in `InstrKind` fields
(e.g. `Add { lhs: ValueRef, rhs: ValueRef, ... }`).

Uses are tracked lazily: `UseDefInfo::compute` in `llvm-analysis` scans all
instructions and builds a `HashMap<ValueRef, Vec<(BlockId, InstrId)>>` mapping
each defined value to its use sites. This is recomputed on demand rather than
maintained incrementally, which keeps the IR mutation API simple.

`mem2reg` establishes true SSA from alloca/load/store patterns emitted by
frontends. After `mem2reg` runs, no `Alloca`/`Load`/`Store` triples remain for
promoted scalars — only phi nodes and pure SSA values.

### 1.3 Type interning

`Context` maintains two interning tables (`src/llvm-ir/src/context.rs:86`):

- **`type_map: HashMap<TypeData, TypeId>`** — structural interning for
  anonymous types (integers, floats, pointers, arrays, vectors, function types,
  anonymous structs). Calling `mk_int(32)` twice returns the same `TypeId`.
- **`named_struct_map: HashMap<String, TypeId>`** — nominal lookup for named
  structs. Two different `%Foo` and `%Bar` structs with identical field lists
  are still distinct types; the name is the identity.

Why keep them separate? Structural interning is correct for types whose meaning
is fully described by their shape — `[4 x i32]` is the same type regardless of
where it appears. Named structs, however, need nominal identity to support
forward declarations and recursive types (a struct can contain a pointer to
itself before its body is known). `mk_struct_named` allocates an opaque (empty)
body immediately; `define_struct_body` fills it in later
(`src/llvm-ir/src/context.rs:239`).

Pre-interned singletons (`void_ty`, `i1_ty`, `i8_ty`, `i32_ty`, `i64_ty`,
`f32_ty`, `f64_ty`, `ptr_ty`, `label_ty`) are created in `Context::new` so
common types can be accessed as fields without a hash-map lookup.

`TypeData` (`src/llvm-ir/src/types.rs`) covers all LLVM type variants:

```
Void | Integer(u32) | Float(FloatKind) | Pointer
Array { element: TypeId, len: u64 }
Vector { element: TypeId, len: u32, scalable: bool }
Struct(StructType)       — name: Option<String>, fields: Vec<TypeId>
Function(FunctionType)   — ret: TypeId, params: Vec<TypeId>, variadic: bool
Label | Metadata
```

### 1.4 Instruction representation

`InstrKind` (`src/llvm-ir/src/instruction.rs:361`) is a large enum covering all
LLVM IR opcodes: integer and FP arithmetic, bitwise ops, comparisons, memory
(Alloca/Load/Store/GEP), casts (Trunc/ZExt/SExt/…), phi, select, call, invoke,
atomics (Fence/CmpXchg/AtomicRmw), and terminators (Ret/Br/CondBr/Switch).

Key field name conventions (important when pattern-matching):

- `Select`: `then_val` / `else_val` (not `true_val` / `false_val`)
- `CondBr`: `then_dest` / `else_dest` (not `true_dest` / `false_dest`)
- `Phi`: `incoming: Vec<(ValueRef, BlockId)>` — value first, block second

Each `Instruction` wraps `InstrKind` with an optional result name and a
`TypeId` for the result type (void for side-effectful instructions and
terminators).

`InstrKind::operands()` returns all `ValueRef` inputs; `InstrKind::successors()`
returns all `BlockId` targets (terminators only). Both methods are exhaustive —
every variant is listed explicitly, so adding a new opcode without updating
these methods is a compile error.

`BasicBlock` (`src/llvm-ir/src/basic_block.rs`) stores:

```rust
pub struct BasicBlock {
    pub name: String,
    pub body: Vec<InstrId>,          // non-terminator instructions in order
    pub terminator: Option<InstrId>, // the block's terminator
}
```

All `InstrId`s index into the owning `Function`'s flat `instructions` pool.

### 1.5 Builder API

`Builder<'a>` (`src/llvm-ir/src/builder.rs`) provides a programmatic API for
constructing IR. It holds `&'a mut Context` and `&'a mut Module` so a single
builder can construct multiple functions and look up shared types.

```rust
pub struct Builder<'a> {
    pub ctx: &'a mut Context,
    pub module: &'a mut Module,
    current_function: Option<FunctionId>,
    current_block: Option<BlockId>,
}
```

Typical usage:

```rust
let mut ctx = Context::new();
let mut module = Module::new("example");
let mut b = Builder::new(&mut ctx, &mut module);

let fid = b.add_function("add", b.ctx.i32_ty, vec![i32, i32],
                          vec!["a".into(), "b".into()], false, Linkage::External);
let entry = b.add_block("entry");
b.position_at_end(entry);

let a = b.arg(0);
let bv = b.arg(1);
let sum = b.build_add("sum", a, bv);
b.build_ret(sum);
```

`build_add` (and all other `build_*` methods) allocates an `Instruction` in the
current function's pool via `Function::alloc_instr`, appends the `InstrId` to
the current block's `body`, and returns a `ValueRef::Instruction(iid)`.

### 1.6 IR printer

`Printer` (`src/llvm-ir/src/printer.rs`) emits valid `.ll` text. Constants are
printed through `write_const_value` (type already known from context); globals
use `write_const_with_type` only for initial values where the surrounding type
context is not available.

---

## 2. Analysis Layer (`llvm-analysis`)

Source: `src/llvm-analysis/src/`

Analyses are computed on demand and not stored in the IR. Each pass that needs
an analysis constructs it from scratch. The four analyses are:

- `Cfg` — control-flow graph
- `DomTree` — dominator tree
- `UseDefInfo` — use-def / def-use chains
- `LoopInfo` — natural loop detection

### 2.1 Control-flow graph (CFG)

`Cfg::compute(func)` (`src/llvm-analysis/src/cfg.rs:41`) walks every block's
terminator and calls `InstrKind::successors()` to build two parallel adjacency
lists:

```rust
pub struct Cfg {
    num_blocks: usize,
    succs: Vec<Vec<BlockId>>,   // successors of each block
    preds: Vec<Vec<BlockId>>,   // predecessors of each block
    reachable: Vec<bool>,       // DFS reachability from entry
    reachable_count: usize,
}
```

After building the successor/predecessor lists, a DFS from `BlockId(0)` (the
entry) marks reachable blocks. Unreachable blocks are retained in the graph
(so `num_blocks()` always equals `func.num_blocks()`) but are excluded from
`rpo()` and `post_order()`.

Key traversal methods:
- `cfg.rpo()` — reverse post-order, entry first; a block always precedes its
  successors (modulo back-edges). Used by dominator computation and RPO-based
  constant propagation.
- `cfg.post_order()` — post-order; a block follows all reachable successors.
- `cfg.is_reachable(bid)` — O(1) reachability query.

### 2.2 Dominator tree

`DomTree::compute(func, cfg)` (`src/llvm-analysis/src/dominators.rs:24`) uses
the iterative dataflow algorithm from Cooper, Harvey & Kennedy (2001), which
converges quickly in practice on reducible CFGs.

The algorithm assigns each block an RPO index, then iterates a dataflow equation
until stable:
- For each block (in RPO order, skipping the entry), its immediate dominator is
  the common dominator of all predecessors that have already been processed.
- The `intersect(a, b, idom)` helper walks up both finger-posts until they meet.

The result is stored as `idom: Vec<Option<BlockId>>` where `idom[i]` is the
immediate dominator of `BlockId(i)`, or `None` for the entry block.

Key methods:
- `dom.idom(bid)` — immediate dominator, `None` for entry.
- `dom.dominates(a, b)` — `true` if `a` dominates `b` (walks the idom chain).
- `dom.dominance_frontier(cfg)` — returns a `HashMap<BlockId, Vec<BlockId>>`
  mapping each block to its dominance frontier. Used by `mem2reg` to determine
  where to insert phi nodes.

### 2.3 Use-def chains

`UseDefInfo::compute(func)` (`src/llvm-analysis/src/use_def.rs:49`) walks every
instruction in a function and builds:

1. `instr_block: HashMap<InstrId, BlockId>` — which block defines each instruction.
2. `uses: HashMap<ValueRef, Vec<(BlockId, InstrId)>>` — all use sites of each
   SSA value. For phi incoming values the block recorded is the phi's own block
   (correct for DCE).
3. `phi_uses: HashMap<ValueRef, Vec<(BlockId, InstrId)>>` — phi incoming values
   recorded at the predecessor block (correct for liveness / mem2reg).

The dual recording of phi uses resolves an SSA subtlety: `[%v, %pred]` in a
phi is semantically a use at the end of `%pred`, not at the phi's own block.
`UseDefInfo::is_dead(vref)` returns `true` if the value has no entries in
`uses`.

### 2.4 Loop detection

`LoopInfo::compute(func, cfg, dom)` (`src/llvm-analysis/src/loops.rs:59`)
identifies natural loops via the back-edge + reverse-CFG BFS algorithm:

1. Walk the CFG with DFS; any edge `(n → h)` where `dom.dominates(h, n)` is a
   back-edge.
2. For each back-edge, collect the natural loop body by BFS in the reverse CFG
   from the tail (`n`) up to and including the header (`h`).
3. Assign parent loops: a loop is a child of the smallest loop whose body
   contains the child's header.

The result is stored as a sorted `Vec<Loop>` (largest body first) plus a
`HashMap<BlockId, usize>` mapping each block to its innermost containing loop
index.

**Note on reducibility**: the algorithm only detects natural loops and requires
a reducible CFG. Irreducible CFGs (where a strongly-connected component has no
single dominating header) are not handled; such cycles will not be reported.
This is acceptable because most frontends and optimisers produce reducible CFGs.

---

## 3. Optimization Passes (`llvm-transforms`)

Source: `src/llvm-transforms/src/`

### 3.1 Pass trait hierarchy

Three traits define the pass interface (`src/llvm-transforms/src/pass.rs`):

```rust
pub trait FunctionPass {
    fn run_on_function(&mut self, ctx: &mut Context, func: &mut Function) -> bool;
    fn name(&self) -> &'static str;
}

pub trait ModulePass {
    fn run_on_module(&mut self, ctx: &mut Context, module: &mut Module) -> bool;
    fn name(&self) -> &'static str;
}
```

Both return `true` if they modified the IR. `FunctionPassAdapter<P>` wraps a
`FunctionPass` into a `ModulePass` by iterating over every non-declaration
function in the module. This means most passes only implement `FunctionPass`.

`PassManager` (`src/llvm-transforms/src/pass.rs:64`) holds a
`Vec<Box<dyn ModulePass>>` and provides:
- `run(ctx, module)` — runs all passes once in order, returns whether any pass
  changed the IR.
- `run_until_fixed_point(ctx, module, max_iter)` — calls `run` repeatedly until
  no pass reports a change or `max_iter` is reached.

### 3.2 Pass pipeline

`build_pipeline(level: OptLevel)` (`src/llvm-transforms/src/pipeline.rs:40`)
returns a pre-configured `PassManager` for a given optimisation level:

| Level | Passes |
|-------|--------|
| O0    | (empty — no optimisations) |
| O1    | SROA → mem2reg → ConstantFold → ConstProp → DCE |
| O2    | SROA → mem2reg → Inliner → GVN → LoopUnroll → ConstantFold → ConstProp → DCE → JumpThreading → TailCallOpt → (GVN → ConstantFold → ConstProp → DCE again) |
| O3    | As O2 plus IPCP, DeadArgElim, larger inliner/unroll budgets, extra cleanup rounds |

### 3.3 mem2reg

`Mem2Reg` (`src/llvm-transforms/src/mem2reg.rs`) implements the Cytron et al.
(1991) SSA construction algorithm. It promotes scalar `alloca`/`load`/`store`
triples to pure SSA values.

**Eligibility**: an alloca is promotable when it is in the entry block, has no
`num_elements` count, and its address is only ever used as a load source or
store target (no escapes through calls, GEP, casts, or phi/select uses of the
pointer itself). Allocas deeper than the entry block are not promotable.

**Algorithm**:

1. Identify promotable allocas and collect the set of store blocks for each.
2. Compute the iterated dominance frontier (IDF) of the store blocks. Insert
   a phi node for each alloca at each IDF block.
3. Rename by DFS over the dominator tree: maintain a stack of current
   definitions per alloca. Each `store` pushes a new definition; each `load` is
   replaced by the top of the stack; phi incoming values are filled from the
   predecessor's stack top.

After the pass, all promoted allocas, their loads, and their stores are removed
from block bodies.

### 3.4 Constant folding and propagation

`ConstantFold` and `ConstProp` are two separate passes that work together.

**`try_fold(ctx, kind)`** (`src/llvm-transforms/src/constant_fold.rs`)
evaluates a single instruction to a constant if all operands are constants.
Handles integer arithmetic (with correct sign-extension for narrow types),
shifts (with `width-1` mask), comparisons, and casts. Returns `None` if
folding is not possible or the instruction has side effects.

**`ConstProp`** (`src/llvm-transforms/src/const_prop.rs`) walks all
instructions in RPO order. For each instruction it first applies pending
substitutions (replacing `ValueRef::Instruction` references with constant
replacements discovered earlier), then calls `try_fold`. When folding succeeds,
it records `InstrId → ConstId` in a substitution map and drops the folded
instruction from the block body. RPO traversal ensures that constants propagate
through straight-line code in a single pass; back-edges require an additional
pass via `run_until_fixed_point`.

### 3.5 Dead-code elimination (DCE)

`DeadCodeElim` (`src/llvm-transforms/src/dce.rs`) uses `UseDefInfo` to identify
dead instructions. A single scan over `Function::instructions` collects all
`InstrId`s for which `is_dce_safe(kind)` is true and `info.is_dead(vref)` is
true. Side-effecting instructions (`Store`, `Call`, `Load`, `Alloca`,
terminators) are never removed even if their results are unused. After
collecting dead instructions, they are removed from each block's `body` vector.

### 3.6 Function inlining

`Inliner` (`src/llvm-transforms/src/inline_pass.rs`) is a `ModulePass`. For
each eligible call site it performs:

1. **Split** the caller block at the call instruction into a pre-block and
   post-block.
2. **Clone** the callee's instructions and blocks into the caller. `InstrId`s
   are offset by `caller.instructions.len()` before cloning; `BlockId`s are
   offset by `caller.blocks.len()`. This avoids aliasing between original and
   cloned IDs.
3. Callee arguments (`ValueRef::Argument(ArgId(i))`) are mapped to the i-th
   call argument.
4. Each `Ret` in the clone is rewritten to an unconditional branch to the
   post-block; return values are collected into a phi at the head of the
   post-block (or forwarded directly if there is only one return site).
5. The original `Call` instruction is removed.

Eligibility: callee is a definition (not a declaration), not variadic, not
self-recursive, body has at most `size_limit` non-terminator instructions. The
`hot_loop_bonus` field increases the effective size limit for call sites inside
loop bodies.

---

## 4. Code Generation (`llvm-codegen`)

Source: `src/llvm-codegen/src/`

Code generation is split into a target-independent crate (`llvm-codegen`) that
defines the machine IR and algorithms, and target-specific crates that implement
the `IselBackend` and `Emitter` traits.

```
LLVM IR  →  [IselBackend::lower_function]  →  MachineFunction (VRegs)
         →  [compute_live_intervals]        →  LiveInterval[]
         →  [allocate_registers]            →  RegAllocResult
         →  [insert_spill_reloads]          →  spill/reload MOVs added
         →  [apply_allocation]              →  VRegs replaced by PRegs
         →  [Emitter::emit_function]        →  Section (bytes + relocs)
         →  [emit_object]                   →  ObjectFile (ELF/Mach-O/COFF)
```

### 4.1 Virtual and physical registers

```rust
pub struct VReg(pub u32);   // unlimited supply; created during isel
pub struct PReg(pub u8);    // physical register (target-specific numbering)
pub struct MOpcode(pub u32); // target-specific opcode constant
```

During instruction selection every IR value that produces a result becomes a
`VReg`. After register allocation, `VReg`s are replaced by `PReg`s (or spilled
to the stack).

### 4.2 Machine IR types

`MInstr` (`src/llvm-codegen/src/isel.rs:53`):

```rust
pub struct MInstr {
    pub opcode: MOpcode,
    pub dst: Option<VReg>,        // output virtual register
    pub operands: Vec<MOperand>,  // inputs: VReg | PReg | Imm | Block | Bytes
    pub phys_uses: Vec<PReg>,     // ABI-fixed inputs (e.g. argument registers)
    pub clobbers: Vec<PReg>,      // registers destroyed (e.g. caller-saved at call)
    pub debug_loc: Option<DebugLoc>,
}
```

`MOperand` captures the variety of machine operands: virtual registers, physical
registers, immediate integers, branch target block indices, and raw byte
sequences for inline assembly. The builder-style methods (`with_dst`,
`with_vreg`, `with_preg`, `with_imm`, `with_block`) make construction readable.

`MachineBlock` is a labelled linear sequence of `MInstr`s. `MachineFunction`
holds `Vec<MachineBlock>` (block 0 is the entry), plus bookkeeping for virtual
register allocation, spill slots, and callee-saved register tracking.

### 4.3 Instruction selection (`IselBackend` trait)

```rust
pub trait IselBackend {
    fn lower_function(&mut self, ctx: &Context, module: &Module,
                      func: &Function) -> MachineFunction;
}
```

Target backends implement this trait. `lower_function` walks IR blocks in
program order and lowers each `InstrKind` to one or more `MInstr`s using
virtual registers. Calling convention handling is done here: `ArgLocation::Reg`
arguments are placed in fixed physical registers (`phys_uses`); `Stack`
arguments are loaded from the appropriate frame offset.

Phi-destruction (converting IR phi nodes to parallel copies) is also performed
during instruction selection, immediately before the block's terminator. The
backends use a two-phase parallel copy to avoid the swap problem: first, all phi
source values are read into fresh temporaries; then, the temporaries are written
to the phi destination registers. This is necessary because after register
allocation, a phi source may share a physical register with a subsequent phi
destination.

### 4.4 Live interval analysis

`compute_live_intervals(mf)` (`src/llvm-codegen/src/regalloc.rs:43`) scans all
instructions of `mf` in a single flat pass (blocks concatenated in order),
assigning each instruction a program-order position. For each `VReg` it
computes a half-open interval `[start, end)`:

- A definition at position `p` sets `start = min(start, p)`, `end = max(end, p+1)`.
- A use at position `p` extends `end = max(end, p+1)`.

The result is a `Vec<LiveInterval>` sorted by `(start, end, vreg)` for
deterministic allocation.

### 4.5 Register allocation

`allocate_registers(intervals, allocatable, strategy)` dispatches to either
linear scan or graph colouring.

**Linear scan** (`linear_scan` in `src/llvm-codegen/src/regalloc.rs:121`) follows
Poletto & Sarkar (1999):

1. Sort intervals by start position.
2. Maintain an "active" set (intervals currently live) sorted by end position.
3. For each new interval:
   a. Expire active intervals whose end ≤ current start; return their physical
      registers to the free pool.
   b. If a free register is available, assign it.
   c. Otherwise, spill: compare the new interval's end with the active interval
      with the largest end. Spill whichever ends later (steal its register if it
      ends later than the new interval, otherwise spill the new interval).

The result is `RegAllocResult { vreg_to_preg: HashMap<VReg, PReg>, spilled: Vec<VReg> }`.

**Spill handling**: `insert_spill_reloads(mf, result, load_op, store_op)`
walks the machine function and inserts a `MOV_STORE_RM` (store to stack slot)
after each definition of a spilled `VReg`, and a `MOV_LOAD_MR` (load from stack
slot) before each use. Spill slots are allocated by `mf.alloc_spill_slot(vreg)`.

**`apply_allocation(mf, result)`** replaces all `MOperand::VReg(vr)` with
`MOperand::PReg(pr)` and sets `mf.used_callee_saved` to the set of callee-saved
registers that were actually assigned. The frame size is computed from the
number of spill slots.

### 4.6 Object file emission

`emit_object(mf, emitter)` calls `emitter.emit_function(mf)` to obtain a
`Section` (raw bytes + relocation records), then assembles an `ObjectFile`:

```rust
pub struct ObjectFile {
    pub format: ObjectFormat,    // Elf | MachO | Coff
    pub elf_machine: u16,        // e.g. 62 = EM_X86_64
    pub coff_machine: u16,       // e.g. 0x8664 = IMAGE_FILE_MACHINE_AMD64
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
    // ...
}
```

`serialize_elf(obj)` and `serialize_macho(obj)` convert to on-disk bytes.

**ELF-64**: up to 6 section headers (null / `.text` / `.symtab` / `.strtab` /
`.shstrtab` / optional `.rela.text`). The string table uses null-terminated
strings; the symbol table uses `Elf64_Sym` entries.

**Mach-O**: `mach_header_64` + `LC_SEGMENT_64` load command (with one
`section_64` for `__TEXT,__text`) + `LC_SYMTAB` + `LC_DYSYMTAB` + raw symbol
and string table data.

**COFF**: `.text` + `.pdata` + `.xdata` sections plus an image symbol table for
function symbols and external references.

---

## 5. Target Backends

Each target lives in its own crate and implements `IselBackend` + `Emitter`.
The split keeps target-specific register names, instruction encodings, and ABI
details isolated from the target-independent pipeline.

### 5.1 x86-64 (`llvm-target-x86`)

Source: `src/llvm-target-x86/src/`

**Register file** (`regs.rs`): `RAX=PReg(0)` through `R15=PReg(15)`, matching
the standard REX encoding. `ALLOCATABLE = [RAX, RCX, RDX, RSI, RDI, R8–R11]`
(caller-saved; excludes RSP, RBP). `CALLEE_SAVED = [RBX, RBP, R12–R15]`.

Helper functions:
- `reg_enc(r)` returns `r.0 & 7` (low 3 bits for ModRM).
- `is_extended(r)` returns `r.0 >= 8` (needs REX.R/REX.B bit set).

**Instruction form**: x86-64 uses a two-address form (destination is also the
left-hand source). Lowering generates a `MOV_RR` to copy the lhs to the
destination `VReg`, then an in-place operation `OP dst, rhs`.

**Encoding** (`encode.rs`): `X86Emitter` emits variable-length instructions.
REX prefix bytes are computed from `is_extended(reg)`. `ModRM` byte encodes
addressing mode (register-to-register: `0xC0 | reg_enc(dst) << 3 | reg_enc(src)`).

Branch patching uses a two-pass approach: branches emit a placeholder `rel32 = 0`
in the first pass and record `(byte_offset, target_block_index)` in
`branch_patches`. After all blocks are encoded, the second pass computes actual
relative offsets and back-patches the bytes.

**Calling convention** (`abi.rs`): `classify_sysv_args` / `classify_win64_args`
assign arguments to `ArgLocation::Reg(PReg)` or `ArgLocation::Stack(i32)`.
System V AMD64 uses RDI, RSI, RDX, RCX, R8, R9 for the first 6 integer/pointer
arguments.

**Prologue/epilogue**: if the function uses any spill slots or callee-saved
registers, the emitter emits `push rbp; mov rbp, rsp; push <callee-saved...>;
sub rsp, N` at the start and the mirror sequence at each return.

### 5.2 AArch64 (`llvm-target-arm`)

Source: `src/llvm-target-arm/src/`

**Register file** (`regs.rs`): `X0=PReg(0)` through `X30=PReg(30)`, `XZR=PReg(31)`.
`ALLOCATABLE` includes X0–X15 and X19–X28. `CALLEE_SAVED = [X19–X30]` (AAPCS64).
`ARG_REGS = [X0–X7]`, `RET_REG = X0`. `reg_enc(r) = r.0 & 0x1F` (5-bit encoding).

**Instruction form**: AArch64 is a three-address architecture. Each instruction
specifies separate destination and two source registers, so no extra copy
instruction is needed to implement binary operations.

**Fixed-width instructions**: every AArch64 instruction is exactly 4 bytes.
`emit4(w)` appends a `u32` as little-endian bytes. Branch patching is analogous
to x86: `B` and `BL` encode `imm26` in bits [25:0]; `B_COND` encodes `imm19` in
bits [23:5]. Both are patched in a second pass.

**64-bit immediate materialization**: `MOV_IMM` (single MOVZ) handles 16-bit
values. For full 64-bit constants, `MOV_WIDE` emits up to four instructions:
`MOVZ Xd, #lo16` followed by `MOVK Xd, #chunk, LSL #(16*i)` for each non-zero
16-bit chunk.

**Condition codes**: `CSET Xd, cond` is encoded as `CSINC Xd, XZR, XZR, inv_cond`
(base encoding `0x9ADF07E0` | `inv_cond << 12 | rd`). `cc_to_hw(cc)` maps the
internal `CC_*` constants to AArch64 hardware condition-code values.

**Prologue/epilogue**: `STP X29, X30, [SP, #-frame]!` saves the frame pointer
and link register atomically. Callee-saved registers X19–X28 are saved/restored
with `STR_FP`/`LDR_FP` at frame-pointer–relative offsets.

### 5.3 RISC-V (`llvm-target-riscv`)

Source: `src/llvm-target-riscv/src/`

**Register file** (`regs.rs`): RV64 integer registers X0 (zero) through X31
(t6), following the standard RISC-V ABI naming (zero / ra / sp / gp / tp /
t0–t2 / s0-s1 / a0–a7 / s2–s11 / t3–t6). `ALLOCATABLE` covers the temporary
and argument registers, excluding zero, sp, gp, tp. `ARG_REGS = [X10–X17]`
(a0–a7).

**Instruction formats** (`encode.rs`): the encoder implements all five base
RV64GC formats:

```
R-type:  [31:25] funct7 | [24:20] rs2 | [19:15] rs1 | [14:12] funct3 | [11:7] rd | [6:0] opcode
I-type:  [31:20] imm12  | [19:15] rs1 | [14:12] funct3 | [11:7] rd | [6:0] opcode
S-type:  [31:25] imm[11:5] | [24:20] rs2 | [19:15] rs1 | [14:12] funct3 | [11:7] imm[4:0] | opcode
B-type:  imm scattered as [31] | [30:25] | [11:8] | [7] | rs2 | rs1 | funct3 | opcode
U-type:  [31:12] imm20 | [11:7] rd | [6:0] opcode
J-type:  imm scattered as [31] | [30:21] | [20] | [19:12] | rd | opcode
```

Branches (`B_EQ`, `B_NE`, `B_LT`, etc.) patch a 13-bit PC-relative offset in
B-type format. Unconditional jumps use J-type (21-bit offset). Both are
back-patched in a second pass using `PatchKind::Branch13` and `PatchKind::Jal21`.

### 5.4 Adding a new backend

To support a new target:

1. Create a new crate `src/llvm-target-<name>/` with `Cargo.toml` that depends
   on `llvm-ir` and `llvm-codegen`.
2. Implement `regs.rs`: define `PReg` constants, `ALLOCATABLE`, `CALLEE_SAVED`,
   `ARG_REGS`, and a `reg_enc` function.
3. Implement `abi.rs`: a function that classifies arguments into
   `ArgLocation::Reg(PReg)` or `ArgLocation::Stack(i32)`.
4. Implement `instructions.rs`: `MOpcode` constants for every machine
   instruction variant you need.
5. Implement `lower.rs`: a struct (e.g. `MyBackend`) implementing
   `IselBackend::lower_function`. Map each `InstrKind` variant to `MInstr`
   sequences using `VReg`s.
6. Implement `encode.rs`: a struct implementing `Emitter::emit_function`. Walk
   `MachineFunction::blocks`, encode each `MInstr` to bytes, patch branches in
   a second pass.
7. Add the crate to the workspace `Cargo.toml` and re-export it from
   `src/llvm/src/lib.rs`.
8. To wire it into `compile_ir_to_object`, add a target selector in
   `src/llvm/src/compile.rs`.

---

## 6. IR Text Format (`llvm-ir-parser`)

Source: `src/llvm-ir-parser/src/`

Entry point: `parse(src: &str) -> Result<(Context, Module), ParseError>`
(`src/llvm-ir-parser/src/parser.rs`).

### 6.1 Lexer

`Lexer` (`src/llvm-ir-parser/src/lexer.rs`) is a hand-rolled tokeniser with a
1-token look-ahead and a push-back buffer (`unget`). It produces `Token`
variants:

- `LocalIdent(String)` — `%name` or `%0`
- `GlobalIdent(String)` — `@name`
- `Keyword(Keyword)` — `define`, `declare`, `i32`, `void`, `ret`, etc.
- `IntLit(u64)` — decimal integers
- `FloatLit(f64)` — decimal floats; hex floats (`0x...`) are decoded directly
- `StringLit(String)` — `"..."` with standard escape sequences
- punctuation tokens (`,`, `(`, `)`, `{`, `}`, `[`, `]`, `*`, `=`, `!`, `#`)

The `Keyword` enum covers all LLVM IR keywords (directives, type names,
opcode names, modifiers, fast-math flags, linkage kinds, etc.).

### 6.2 Parser

`Parser` is a recursive-descent parser. Internal state:

```rust
struct Parser<'src> {
    lex: Lexer<'src>,
    ctx: Context,
    module: Module,
    pending_blocks: HashMap<String, BlockId>,  // forward block refs
    current_func: Option<usize>,
    current_block: Option<BlockId>,
    locals: HashMap<String, ValueRef>,          // current function value table
    unnamed: HashMap<u64, ValueRef>,            // %0, %1, ... numbered slots
}
```

**Forward block references**: when parsing `br label %foo` before `%foo:` is
seen, `pending_blocks` allocates a `BlockId` speculatively. When `%foo:` is
encountered later, the block is matched to that `BlockId`. Unresolved forward
refs are an error.

**Phi value references**: phi incoming values are resolved via `locals` and
`unnamed` at parse time; SSA definitions in LLVM IR text always precede their
phi uses within the same function (dominance ordering is preserved in the `.ll`
format).

**`skip_trailing_fn_attrs`**: this helper consumes trailing function attributes
(like `nounwind`, `#0`, `align`, etc.) after a function signature. It must stop
when it sees top-level keywords (`define`, `declare`, `attributes`, `!`, etc.)
so that the parser correctly transitions to the next top-level item.

**`parse_typed_value`**: used for phi incoming values and switch case values
where both the type and value appear together in the source (`i32 42`, `i32 %x`).

The parser never backtracks. Every `expect(token)` call consumes exactly one
token or returns a `ParseError`.

---

## 7. LRIR Bitcode (`llvm-bitcode`)

Source: `src/llvm-bitcode/src/`

The bitcode crate provides full round-trip serialisation of a `(Context, Module)`
pair in a custom binary format called LRIR (not LLVM bitcode format, which uses
a complex bitstream encoding we do not replicate).

### 7.1 Format layout

```
[4B]   magic = "LRIR"  (0x4C 0x52 0x49 0x52)
[4B]   version = 1  (u32 LE)
[4B]   type_count (u32 LE)
[...]  type_count × TypeRecord
[4B]   const_count (u32 LE)
[...]  const_count × ConstRecord
[str]  module_name  (u32 length + UTF-8 bytes)
[4B]   func_count (u32 LE)
[...]  func_count × FunctionRecord
```

All strings are length-prefixed (`u32` byte count + UTF-8, no null terminator).
An absent optional string uses length 0. All multi-byte integers are little-endian.

Each `TypeRecord` begins with a 1-byte tag (`VOID=0`, `INTEGER=1`, `FLOAT=2`,
`POINTER=3`, `ARRAY=4`, `VECTOR=5`, `STRUCT=6`, `FUNCTION=7`, `LABEL=8`,
`METADATA=9`) followed by tag-specific fields. Type references inside records
use the serial index within the type table (which matches the `Context` pool
order used during writing).

Each `FunctionRecord` serialises the function header (name, type, linkage,
`is_declaration`), the argument list, the block list (each block is a sequence
of `InstrId` indices from the flat pool), and the flat instruction pool. Block
bodies and terminators are written as index lists so the original `InstrId`
assignments are preserved.

### 7.2 Writer

`write_bitcode(ctx, module) -> Vec<u8>` (`src/llvm-bitcode/src/writer.rs:33`)
serialises the full `Context` type table and constant pool first (so the reader
can reconstruct `TypeId`/`ConstId` mappings), then each function. Encoding is
infallible — there are no unrepresentable IR values.

### 7.3 Reader

`read_bitcode(bytes) -> Result<(Context, Module), BitcodeError>`
(`src/llvm-bitcode/src/reader.rs:15`) reconstructs the IR in two stages:

1. **Type table**: decode each `TypeRecord` into `TypeData` and intern it into a
   fresh `Context`. Build `type_id_map: Vec<TypeId>` mapping serial position →
   interned `TypeId` (because the new `Context` may assign different indices).
2. **Constant table**: decode each `ConstRecord`, translate embedded `TypeId`
   references through `type_id_map`, and push constants into the `Context`.
   Build `const_id_map: Vec<ConstId>`.
3. **Functions**: decode each `FunctionRecord`, translate all `TypeId` and
   `ConstId` references, reconstruct `Instruction` objects and `BasicBlock`
   body/terminator index lists.

`BitcodeError` variants: `InvalidMagic`, `TruncatedInput`, `UnexpectedEof`,
`UnsupportedRecord`, `InvalidType`, `ParseError(String)`.

---

## 8. Key Design Decisions

### 8.1 Pure Rust, no LLVM C API

The entire pipeline is implemented in Rust with no FFI to LLVM's C++ library.
This eliminates a large C++ build dependency, removes all risks of memory
unsafety at the FFI boundary, and makes the codebase easy to build and test on
any platform with a Rust toolchain. The trade-off is that we cannot reuse LLVM's
existing passes or backends directly; everything must be reimplemented.

### 8.2 Arena allocation over `Rc<RefCell<...>>`

Compiler IRs are graphs, and graph nodes in Rust naturally invite reference
counting (`Rc`) with interior mutability (`RefCell`). This approach has serious
costs: heap allocation per node, cache misses on traversal, overhead on every
reference count increment/decrement, and verbose ergonomics.

Arena allocation (flat `Vec<T>` indexed by newtype `u32`) gives:

- O(1) amortised allocation and deallocation (bulk-free the whole `Vec`).
- Cache-friendly traversal (sequential memory layout).
- `Copy` handles — no lifetime annotations needed at call sites.
- No reference cycles; the ownership tree is strictly `Context > Function > BasicBlock`.

The only cost is that "deleting" a single node requires a compaction pass; in
practice passes remove dead instructions by rebuilding block `body` vectors
with `retain`, which is acceptable.

### 8.3 SSA throughout (not two-address until machine IR)

The IR stays in SSA form through all optimisation passes. Conversion to
two-address form (where the destination register is also the left-hand source)
happens only during instruction selection for targets that require it (x86-64).
Keeping SSA form longer enables more powerful analyses (e.g. sparse conditional
constant propagation) and simplifies most passes since each value has exactly
one definition.

### 8.4 Trait-based pass system

`FunctionPass` and `ModulePass` are simple traits with a single
`run_on_function` / `run_on_module` method. This enables:

- **Composition**: `PassManager` holds `Vec<Box<dyn ModulePass>>`, so any
  combination of passes can be assembled at runtime.
- **Lifting**: `FunctionPassAdapter` wraps a `FunctionPass` into a `ModulePass`
  transparently.
- **Fixed-point iteration**: `run_until_fixed_point` is generic over the
  `PassManager` and requires no per-pass special casing.
- **Testing**: each pass can be unit-tested in isolation with a minimal
  hand-constructed IR fragment.

### 8.5 Separate `llvm-codegen` from `llvm-target-*`

Target-independent algorithms (live interval computation, linear scan, spill
insertion, object file layout) live in `llvm-codegen`. Target-specific details
(register names, calling conventions, instruction encodings) live in separate
crates. This split means adding a new target only requires implementing two
traits (`IselBackend` and `Emitter`) and adding register/ABI tables — no
changes to the shared pipeline code.

The top-level `llvm` crate (`src/llvm/src/lib.rs`) re-exports all sub-crates
under short names (`llvm::ir`, `llvm::analysis`, `llvm::transforms`,
`llvm::target_x86`, etc.) and provides the end-to-end `compile_ir_to_object`
convenience API (`src/llvm/src/compile.rs`).
