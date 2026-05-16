//! WebAssembly binary format encoder.
//!
//! Produces a complete `.wasm` module from a lowered [`WasmModule`] — no
//! intermediate object file is needed since wasm is self-describing.

use crate::instructions::*;
use crate::lower::WasmFunction;
use crate::types::{ir_ty_to_valtype, BLOCKTYPE_VOID};
use llvm_ir::{Context, Module, TypeData};

// ── LEB128 helpers ────────────────────────────────────────────────────────

/// Encode `v` as an unsigned LEB128 integer into `buf`.
pub fn leb128_u(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

/// Encode `v` as a signed LEB128 integer into `buf`.
pub fn leb128_s(buf: &mut Vec<u8>, mut v: i64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        let done = (v == 0 && (byte & 0x40) == 0) || (v == -1 && (byte & 0x40) != 0);
        if done {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

// ── wasm section ids ──────────────────────────────────────────────────────

const SECTION_TYPE: u8 = 0x01;
const SECTION_FUNCTION: u8 = 0x03;
const SECTION_EXPORT: u8 = 0x07;
const SECTION_CODE: u8 = 0x0A;

// ── section serialization helper ──────────────────────────────────────────

fn write_section(out: &mut Vec<u8>, id: u8, content: &[u8]) {
    out.push(id);
    leb128_u(out, content.len() as u64);
    out.extend_from_slice(content);
}

// ── function body encoding ────────────────────────────────────────────────

/// Encode one function body (locals + instructions + `end`).
pub fn encode_function_body(wf: &WasmFunction) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();

    // Local declarations: group contiguous same-type locals.
    // Wasm local declarations are (count, valtype) pairs.
    // We always declare all locals as i32 or i64 individually grouped.
    let local_count = wf.local_types.len();
    if local_count == 0 {
        // Zero local declaration entries.
        leb128_u(&mut body, 0);
    } else {
        // Group locals by valtype.
        let mut groups: Vec<(u32, u8)> = Vec::new(); // (count, valtype)
        for &vt in &wf.local_types {
            if let Some(last) = groups.last_mut() {
                if last.1 == vt {
                    last.0 += 1;
                    continue;
                }
            }
            groups.push((1, vt));
        }
        leb128_u(&mut body, groups.len() as u64);
        for (count, vt) in groups {
            leb128_u(&mut body, count as u64);
            body.push(vt);
        }
    }

    // Instructions.
    body.extend_from_slice(&wf.code);

    // Function `end`.
    body.push(0x0B);

    // Wrap with length prefix.
    let mut out = Vec::new();
    leb128_u(&mut out, body.len() as u64);
    out.extend_from_slice(&body);
    out
}

// ── opcode emission ───────────────────────────────────────────────────────

/// Emit a single wasm instruction into `code`.
/// The instruction is described by `opcode` (from `MOpcode`) and an optional
/// immediate operand.
pub fn emit_opcode(code: &mut Vec<u8>, opcode: u32, imm: Option<i64>) {
    // Opcodes in our range are always single-byte.
    debug_assert!(opcode <= 0xFF, "multi-byte opcode not supported");
    code.push(opcode as u8);
    if let Some(v) = imm {
        leb128_s(code, v);
    }
}

/// Emit `local.get <idx>`.
pub fn emit_local_get(code: &mut Vec<u8>, local_idx: u32) {
    code.push(WASM_LOCAL_GET.0 as u8);
    leb128_u(code, local_idx as u64);
}

/// Emit `local.set <idx>`.
pub fn emit_local_set(code: &mut Vec<u8>, local_idx: u32) {
    code.push(WASM_LOCAL_SET.0 as u8);
    leb128_u(code, local_idx as u64);
}

/// Emit `i32.const <v>` or `i64.const <v>`.
pub fn emit_const(code: &mut Vec<u8>, is_i64: bool, v: i64) {
    if is_i64 {
        code.push(WASM_I64_CONST.0 as u8);
    } else {
        code.push(WASM_I32_CONST.0 as u8);
    }
    leb128_s(code, v);
}

/// Emit `br <depth>`.
pub fn emit_br(code: &mut Vec<u8>, depth: u32) {
    code.push(WASM_BR.0 as u8);
    leb128_u(code, depth as u64);
}

/// Emit `br_if <depth>`.
pub fn emit_br_if(code: &mut Vec<u8>, depth: u32) {
    code.push(WASM_BR_IF.0 as u8);
    leb128_u(code, depth as u64);
}

/// Emit `block <blocktype>`.
pub fn emit_block(code: &mut Vec<u8>) {
    code.push(WASM_BLOCK.0 as u8);
    code.push(BLOCKTYPE_VOID);
}

/// Emit `loop <blocktype>`.
pub fn emit_loop(code: &mut Vec<u8>) {
    code.push(WASM_LOOP.0 as u8);
    code.push(BLOCKTYPE_VOID);
}

/// Emit `if <blocktype>`.
pub fn emit_if(code: &mut Vec<u8>) {
    code.push(WASM_IF.0 as u8);
    code.push(BLOCKTYPE_VOID);
}

/// Emit `else`.
pub fn emit_else(code: &mut Vec<u8>) {
    code.push(WASM_ELSE.0 as u8);
}

/// Emit `end`.
pub fn emit_end(code: &mut Vec<u8>) {
    code.push(WASM_END.0 as u8);
}

/// Emit `return`.
pub fn emit_return(code: &mut Vec<u8>) {
    code.push(WASM_RETURN.0 as u8);
}

/// Emit `call <func_idx>`.
pub fn emit_call(code: &mut Vec<u8>, func_idx: u32) {
    code.push(WASM_CALL.0 as u8);
    leb128_u(code, func_idx as u64);
}

// ── functype encoding ─────────────────────────────────────────────────────

/// Encode a wasm `functype` into `buf`.
///
/// Format: `0x60` + param-count (LEB) + params... + result-count (LEB) + results...
fn encode_functype(buf: &mut Vec<u8>, params: &[u8], results: &[u8]) {
    buf.push(0x60); // functype marker
    leb128_u(buf, params.len() as u64);
    buf.extend_from_slice(params);
    leb128_u(buf, results.len() as u64);
    buf.extend_from_slice(results);
}

// ── full module assembly ──────────────────────────────────────────────────

/// Assemble a complete wasm binary from the given `WasmFunction` list.
///
/// The functions are expected to come from `WasmBackend::lower_module`.
/// Each function in the list that corresponds to a non-declaration IR
/// function will be placed in the type/function/export/code sections.
pub fn assemble_module(
    ctx: &Context,
    module: &Module,
    wasm_functions: &[WasmFunction],
) -> Vec<u8> {
    // ── magic + version ───────────────────────────────────────────────────
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"\0asm");
    out.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // ── build type section ────────────────────────────────────────────────
    // Collect unique function signatures (deduplicated).
    let mut type_entries: Vec<Vec<u8>> = Vec::new(); // encoded functype entries
    let mut type_indices: Vec<u32> = Vec::new(); // per-function index into type_entries

    for wf in wasm_functions {
        // Build the functype bytes for this function.
        let mut entry = Vec::new();
        let ir_func = &module.functions[wf.func_idx];
        let func_ty_data = ctx.get_type(ir_func.ty);
        let (params, results) = if let TypeData::Function(ft) = func_ty_data {
            let param_valtypes: Vec<u8> = ft
                .params
                .iter()
                .filter_map(|&p| ir_ty_to_valtype(ctx, p))
                .collect();
            let result_valtypes: Vec<u8> = ir_ty_to_valtype(ctx, ft.ret)
                .map(|v| vec![v])
                .unwrap_or_default();
            (param_valtypes, result_valtypes)
        } else {
            (Vec::new(), Vec::new())
        };

        encode_functype(&mut entry, &params, &results);

        // Deduplicate.
        let idx = if let Some(pos) = type_entries.iter().position(|e| *e == entry) {
            pos as u32
        } else {
            let pos = type_entries.len() as u32;
            type_entries.push(entry);
            pos
        };
        type_indices.push(idx);
    }

    // Encode type section.
    if !type_entries.is_empty() {
        let mut type_sec = Vec::new();
        leb128_u(&mut type_sec, type_entries.len() as u64);
        for entry in &type_entries {
            type_sec.extend_from_slice(entry);
        }
        write_section(&mut out, SECTION_TYPE, &type_sec);
    }

    // ── import section (empty, required by spec order) ────────────────────
    // Only write if non-empty; wasm binary format does not require it.

    // ── function section ──────────────────────────────────────────────────
    if !wasm_functions.is_empty() {
        let mut func_sec = Vec::new();
        leb128_u(&mut func_sec, wasm_functions.len() as u64);
        for &ti in &type_indices {
            leb128_u(&mut func_sec, ti as u64);
        }
        write_section(&mut out, SECTION_FUNCTION, &func_sec);
    }

    // ── export section ────────────────────────────────────────────────────
    if !wasm_functions.is_empty() {
        let mut exp_sec = Vec::new();
        leb128_u(&mut exp_sec, wasm_functions.len() as u64);
        for (func_wasm_idx, wf) in wasm_functions.iter().enumerate() {
            let name_bytes = wf.name.as_bytes();
            leb128_u(&mut exp_sec, name_bytes.len() as u64);
            exp_sec.extend_from_slice(name_bytes);
            exp_sec.push(0x00); // export kind: function
            leb128_u(&mut exp_sec, func_wasm_idx as u64);
        }
        write_section(&mut out, SECTION_EXPORT, &exp_sec);
    }

    // ── code section ─────────────────────────────────────────────────────
    if !wasm_functions.is_empty() {
        let mut code_sec = Vec::new();
        leb128_u(&mut code_sec, wasm_functions.len() as u64);
        for wf in wasm_functions {
            let body = encode_function_body(wf);
            code_sec.extend_from_slice(&body);
        }
        write_section(&mut out, SECTION_CODE, &code_sec);
    }

    out
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_u_single_byte() {
        let mut buf = Vec::new();
        leb128_u(&mut buf, 42);
        assert_eq!(buf, &[42]);
    }

    #[test]
    fn leb128_u_multibyte() {
        let mut buf = Vec::new();
        leb128_u(&mut buf, 300);
        // 300 = 0x12C → LEB: 0xAC 0x02
        assert_eq!(buf, &[0xAC, 0x02]);
    }

    #[test]
    fn leb128_s_negative() {
        let mut buf = Vec::new();
        leb128_s(&mut buf, -1);
        assert_eq!(buf, &[0x7F]);
    }

    #[test]
    fn leb128_s_positive() {
        let mut buf = Vec::new();
        leb128_s(&mut buf, 42);
        assert_eq!(buf, &[42]);
    }
}
