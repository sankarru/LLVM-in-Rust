//! WebAssembly opcode constants used as `MOpcode` values in machine IR.

use llvm_codegen::isel::MOpcode;

// ── control flow ──────────────────────────────────────────────────────────

/// `unreachable` — trap immediately.
pub const WASM_UNREACHABLE: MOpcode = MOpcode(0x00);
/// `nop` — no operation.
pub const WASM_NOP: MOpcode = MOpcode(0x01);
/// `block` — begin a block construct (forward branch target).
pub const WASM_BLOCK: MOpcode = MOpcode(0x02);
/// `loop` — begin a loop construct (backward branch target).
pub const WASM_LOOP: MOpcode = MOpcode(0x03);
/// `if` — begin an if construct.
pub const WASM_IF: MOpcode = MOpcode(0x04);
/// `else` — begin else branch.
pub const WASM_ELSE: MOpcode = MOpcode(0x05);
/// `end` — end a block/loop/if/function.
pub const WASM_END: MOpcode = MOpcode(0x0B);
/// `br` — unconditional branch to label depth N.
pub const WASM_BR: MOpcode = MOpcode(0x0C);
/// `br_if` — conditional branch to label depth N.
pub const WASM_BR_IF: MOpcode = MOpcode(0x0D);
/// `return` — return from function.
pub const WASM_RETURN: MOpcode = MOpcode(0x0F);
/// `call` — call a function by index.
pub const WASM_CALL: MOpcode = MOpcode(0x10);

// ── local variable access ─────────────────────────────────────────────────

/// `local.get` — push value of local N onto the stack.
pub const WASM_LOCAL_GET: MOpcode = MOpcode(0x20);
/// `local.set` — pop stack top into local N.
pub const WASM_LOCAL_SET: MOpcode = MOpcode(0x21);

// ── memory ────────────────────────────────────────────────────────────────

/// `i32.load` — load 4 bytes from memory.
pub const WASM_I32_LOAD: MOpcode = MOpcode(0x28);
/// `i64.load` — load 8 bytes from memory.
pub const WASM_I64_LOAD: MOpcode = MOpcode(0x29);
/// `i32.store` — store 4 bytes to memory.
pub const WASM_I32_STORE: MOpcode = MOpcode(0x36);
/// `i64.store` — store 8 bytes to memory.
pub const WASM_I64_STORE: MOpcode = MOpcode(0x37);

// ── constants ─────────────────────────────────────────────────────────────

/// `i32.const` — push a 32-bit integer constant.
pub const WASM_I32_CONST: MOpcode = MOpcode(0x41);
/// `i64.const` — push a 64-bit integer constant.
pub const WASM_I64_CONST: MOpcode = MOpcode(0x42);

// ── i32 comparisons ───────────────────────────────────────────────────────

/// `i32.eqz`
pub const WASM_I32_EQZ: MOpcode = MOpcode(0x45);
/// `i32.eq`
pub const WASM_I32_EQ: MOpcode = MOpcode(0x46);
/// `i32.ne`
pub const WASM_I32_NE: MOpcode = MOpcode(0x47);
/// `i32.lt_s`
pub const WASM_I32_LT_S: MOpcode = MOpcode(0x48);
/// `i32.lt_u`
pub const WASM_I32_LT_U: MOpcode = MOpcode(0x49);
/// `i32.gt_s`
pub const WASM_I32_GT_S: MOpcode = MOpcode(0x4A);
/// `i32.gt_u`
pub const WASM_I32_GT_U: MOpcode = MOpcode(0x4B);
/// `i32.le_s`
pub const WASM_I32_LE_S: MOpcode = MOpcode(0x4C);
/// `i32.le_u`
pub const WASM_I32_LE_U: MOpcode = MOpcode(0x4D);
/// `i32.ge_s`
pub const WASM_I32_GE_S: MOpcode = MOpcode(0x4E);
/// `i32.ge_u`
pub const WASM_I32_GE_U: MOpcode = MOpcode(0x4F);

// ── i64 comparisons ───────────────────────────────────────────────────────

/// `i64.eqz`
pub const WASM_I64_EQZ: MOpcode = MOpcode(0x50);
/// `i64.eq`
pub const WASM_I64_EQ: MOpcode = MOpcode(0x51);
/// `i64.ne`
pub const WASM_I64_NE: MOpcode = MOpcode(0x52);
/// `i64.lt_s`
pub const WASM_I64_LT_S: MOpcode = MOpcode(0x53);
/// `i64.lt_u`
pub const WASM_I64_LT_U: MOpcode = MOpcode(0x54);
/// `i64.gt_s`
pub const WASM_I64_GT_S: MOpcode = MOpcode(0x55);
/// `i64.gt_u`
pub const WASM_I64_GT_U: MOpcode = MOpcode(0x56);
/// `i64.le_s`
pub const WASM_I64_LE_S: MOpcode = MOpcode(0x57);
/// `i64.le_u`
pub const WASM_I64_LE_U: MOpcode = MOpcode(0x58);
/// `i64.ge_s`
pub const WASM_I64_GE_S: MOpcode = MOpcode(0x59);
/// `i64.ge_u`
pub const WASM_I64_GE_U: MOpcode = MOpcode(0x5A);

// ── i32 arithmetic ────────────────────────────────────────────────────────

/// `i32.add`
pub const WASM_I32_ADD: MOpcode = MOpcode(0x6A);
/// `i32.sub`
pub const WASM_I32_SUB: MOpcode = MOpcode(0x6B);
/// `i32.mul`
pub const WASM_I32_MUL: MOpcode = MOpcode(0x6C);
/// `i32.div_s`
pub const WASM_I32_DIV_S: MOpcode = MOpcode(0x6D);
/// `i32.div_u`
pub const WASM_I32_DIV_U: MOpcode = MOpcode(0x6E);
/// `i32.rem_s`
pub const WASM_I32_REM_S: MOpcode = MOpcode(0x6F);
/// `i32.rem_u`
pub const WASM_I32_REM_U: MOpcode = MOpcode(0x70);
/// `i32.and`
pub const WASM_I32_AND: MOpcode = MOpcode(0x71);
/// `i32.or`
pub const WASM_I32_OR: MOpcode = MOpcode(0x72);
/// `i32.xor`
pub const WASM_I32_XOR: MOpcode = MOpcode(0x73);
/// `i32.shl`
pub const WASM_I32_SHL: MOpcode = MOpcode(0x74);
/// `i32.shr_s`
pub const WASM_I32_SHR_S: MOpcode = MOpcode(0x75);
/// `i32.shr_u`
pub const WASM_I32_SHR_U: MOpcode = MOpcode(0x76);

// ── i64 arithmetic ────────────────────────────────────────────────────────

/// `i64.add`
pub const WASM_I64_ADD: MOpcode = MOpcode(0x7C);
/// `i64.sub`
pub const WASM_I64_SUB: MOpcode = MOpcode(0x7D);
/// `i64.mul`
pub const WASM_I64_MUL: MOpcode = MOpcode(0x7E);
/// `i64.div_s`
pub const WASM_I64_DIV_S: MOpcode = MOpcode(0x7F);
/// `i64.div_u`
pub const WASM_I64_DIV_U: MOpcode = MOpcode(0x80);
/// `i64.rem_s`
pub const WASM_I64_REM_S: MOpcode = MOpcode(0x81);
/// `i64.rem_u`
pub const WASM_I64_REM_U: MOpcode = MOpcode(0x82);
/// `i64.and`
pub const WASM_I64_AND: MOpcode = MOpcode(0x83);
/// `i64.or`
pub const WASM_I64_OR: MOpcode = MOpcode(0x84);
/// `i64.xor`
pub const WASM_I64_XOR: MOpcode = MOpcode(0x85);
/// `i64.shl`
pub const WASM_I64_SHL: MOpcode = MOpcode(0x86);
/// `i64.shr_s`
pub const WASM_I64_SHR_S: MOpcode = MOpcode(0x87);
/// `i64.shr_u`
pub const WASM_I64_SHR_U: MOpcode = MOpcode(0x88);

// ── conversion ────────────────────────────────────────────────────────────

/// `i32.wrap_i64` — truncate i64 → i32.
pub const WASM_I32_WRAP_I64: MOpcode = MOpcode(0xA7);
/// `i64.extend_i32_s` — sign-extend i32 → i64.
pub const WASM_I64_EXTEND_I32_S: MOpcode = MOpcode(0xAC);
/// `i64.extend_i32_u` — zero-extend i32 → i64.
pub const WASM_I64_EXTEND_I32_U: MOpcode = MOpcode(0xAD);
