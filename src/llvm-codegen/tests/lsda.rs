//! Tests for the LSDA (`.gcc_except_table`) builder.
//!
//! Covers `LsdaBuilder`, `XdataBuilder`, ULEB128/SLEB128 helpers, and the
//! `get_or_create_section_data` utility.

use llvm_codegen::lsda::{
    get_or_create_section_data, write_sleb128, write_uleb128, CallSiteRecord, LsdaBuilder,
    XdataBuilder,
};

// ── Helper ────────────────────────────────────────────────────────────────

/// Decode a single ULEB128 value from the front of `buf`.
/// Returns `(value, bytes_consumed)`.
fn decode_uleb128(buf: &[u8]) -> (u64, usize) {
    let mut val: u64 = 0;
    let mut shift = 0u32;
    for (i, &b) in buf.iter().enumerate() {
        val |= ((b & 0x7f) as u64) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            return (val, i + 1);
        }
    }
    (val, buf.len())
}

// ── ULEB128 tests ─────────────────────────────────────────────────────────

#[test]
fn uleb128_small_values() {
    let mut buf = Vec::new();

    write_uleb128(&mut buf, 0);
    assert_eq!(buf, vec![0x00]);
    buf.clear();

    write_uleb128(&mut buf, 1);
    assert_eq!(buf, vec![0x01]);
    buf.clear();

    write_uleb128(&mut buf, 127);
    assert_eq!(buf, vec![0x7f]);
    buf.clear();

    write_uleb128(&mut buf, 128);
    assert_eq!(buf, vec![0x80, 0x01]);
    buf.clear();

    write_uleb128(&mut buf, 300);
    assert_eq!(buf, vec![0xac, 0x02]);
    buf.clear();
}

#[test]
fn sleb128_values() {
    let mut buf = Vec::new();

    write_sleb128(&mut buf, 0);
    assert_eq!(buf, vec![0x00]);
    buf.clear();

    write_sleb128(&mut buf, -1);
    assert_eq!(buf, vec![0x7f]);
    buf.clear();

    write_sleb128(&mut buf, 63);
    assert_eq!(buf, vec![0x3f]);
    buf.clear();

    write_sleb128(&mut buf, -64);
    assert_eq!(buf, vec![0x40]);
    buf.clear();
}

// ── LsdaBuilder tests ─────────────────────────────────────────────────────

#[test]
fn lsda_empty_produces_no_bytes() {
    let lsda = LsdaBuilder::new();
    assert!(lsda.is_empty());
    // build() on empty is fine — returns header-only or minimal bytes.
    let bytes = lsda.build();
    // Header is at least 4 bytes: 0xff 0xff 0x01 ULEB128(0)
    assert!(bytes.len() >= 4);
}

#[test]
fn lsda_cleanup_only_encodes_correctly() {
    let mut lsda = LsdaBuilder::new();
    lsda.add_call_site(CallSiteRecord {
        call_start: 0,
        call_len: 5,
        landing_pad: 20,
        action: 0,
    });
    let bytes = lsda.build();
    // Check header: 0xff (LPStart=omit), 0xff (TType=omit), 0x01 (CS enc=uleb128)
    assert_eq!(bytes[0], 0xff);
    assert_eq!(bytes[1], 0xff);
    assert_eq!(bytes[2], 0x01);
    // Must have at least one call-site entry beyond the 3-byte header.
    assert!(bytes.len() > 4);
}

#[test]
fn lsda_multiple_call_sites() {
    let mut lsda = LsdaBuilder::new();
    lsda.add_call_site(CallSiteRecord {
        call_start: 0,
        call_len: 5,
        landing_pad: 30,
        action: 0,
    });
    lsda.add_call_site(CallSiteRecord {
        call_start: 10,
        call_len: 5,
        landing_pad: 50,
        action: 0,
    });
    lsda.add_call_site(CallSiteRecord {
        call_start: 20,
        call_len: 5,
        landing_pad: 0,
        action: 0,
    });
    let bytes = lsda.build();
    // At least 3 call-site records (each ≥ 4 ULEB128 bytes).
    assert!(bytes.len() > 10);
}

#[test]
fn lsda_no_landing_pad_zero() {
    let mut lsda = LsdaBuilder::new();
    lsda.add_call_site(CallSiteRecord {
        call_start: 4,
        call_len: 5,
        landing_pad: 0,
        action: 0,
    });
    let bytes = lsda.build();
    // landing_pad=0 must be encoded as ULEB128(0) = 0x00 somewhere in the bytes.
    assert!(bytes.contains(&0));
}

#[test]
fn lsda_round_trip_decode() {
    // Build an LSDA, then manually parse the call-site table back out
    // and verify the values match.
    let mut lsda = LsdaBuilder::new();
    lsda.add_call_site(CallSiteRecord {
        call_start: 8,
        call_len: 5,
        landing_pad: 42,
        action: 0,
    });
    let bytes = lsda.build();

    // Parse: skip 3-byte header, read cs_table_len ULEB128, then first record.
    let mut pos = 3usize;
    let (_cs_len, advance) = decode_uleb128(&bytes[pos..]);
    pos += advance;

    let (cs_start, a) = decode_uleb128(&bytes[pos..]);
    pos += a;
    let (cs_len, b) = decode_uleb128(&bytes[pos..]);
    pos += b;
    let (cs_lp, _) = decode_uleb128(&bytes[pos..]);

    assert_eq!(cs_start, 8);
    assert_eq!(cs_len, 5);
    assert_eq!(cs_lp, 42);
}

#[test]
fn lsda_large_offset_uleb128() {
    // Offsets > 127 require multi-byte ULEB128 encoding.
    let mut lsda = LsdaBuilder::new();
    lsda.add_call_site(CallSiteRecord {
        call_start: 200,
        call_len: 5,
        landing_pad: 500,
        action: 0,
    });
    let bytes = lsda.build();
    // Multi-byte ULEB128: 200→2B, 5→1B, 500→2B, 0→1B = 6 table bytes.
    // Header: 3 + cs_table_len(1) + table(6) = 10 bytes.
    assert!(bytes.len() >= 10);
}

#[test]
fn lsda_cs_table_length_field_correct() {
    // Verify the ULEB128 call-site table length field matches the actual table size.
    let mut lsda = LsdaBuilder::new();
    lsda.add_call_site(CallSiteRecord {
        call_start: 0,
        call_len: 5,
        landing_pad: 10,
        action: 0,
    });
    let bytes = lsda.build();

    // Position 3 is the start of the cs_table_length ULEB128.
    let (cs_table_len, advance) = decode_uleb128(&bytes[3..]);
    let table_start = 3 + advance;
    let table_end = bytes.len();
    assert_eq!(
        cs_table_len as usize,
        table_end - table_start,
        "cs_table_length field must match actual call-site table byte count"
    );
}

// ── XdataBuilder tests ────────────────────────────────────────────────────

#[test]
fn xdata_magic_bytes_correct() {
    let xd = XdataBuilder::new();
    let bytes = xd.build();
    // __CxxFrameHandler3 magic = 0x19930522 little-endian = [0x22, 0x05, 0x93, 0x19]
    assert_eq!(&bytes[0..4], &[0x22, 0x05, 0x93, 0x19]);
}

#[test]
fn xdata_empty_has_zero_count() {
    let xd = XdataBuilder::new();
    let bytes = xd.build();
    // maxState (bytes 4..8) = 0
    let max_state = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(max_state, 0);
    // ip_count (bytes 8..12) = 0
    let ip_count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    assert_eq!(ip_count, 0);
    // Total size = 12 bytes (magic + maxState + ip_count), no entries.
    assert_eq!(bytes.len(), 12);
}

#[test]
fn xdata_entries_sorted_by_ip() {
    let mut xd = XdataBuilder::new();
    xd.add_ip_state(30, 1);
    xd.add_ip_state(10, 0);
    xd.add_ip_state(20, 2);
    let bytes = xd.build();
    // ip_count = 3 (bytes 8..12)
    let ip_count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    assert_eq!(ip_count, 3);
    // First entry should be ip=10 (sorted by IP offset ascending).
    let first_ip = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    assert_eq!(first_ip, 10);
    // Second entry ip=20.
    let second_ip = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    assert_eq!(second_ip, 20);
    // Third entry ip=30.
    let third_ip = u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
    assert_eq!(third_ip, 30);
}

#[test]
fn xdata_entry_states_preserved() {
    let mut xd = XdataBuilder::new();
    xd.add_ip_state(0, -1);
    xd.add_ip_state(8, 0);
    let bytes = xd.build();
    // state for ip=0 at bytes [16..20] as i32
    let state0 = i32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    assert_eq!(state0, -1);
    // state for ip=8 at bytes [24..28]
    let state1 = i32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    assert_eq!(state1, 0);
}

// ── get_or_create_section_data tests ─────────────────────────────────────

#[test]
fn get_or_create_section_creates_new() {
    use llvm_codegen::emit::Section;
    let mut sections: Vec<Section> = Vec::new();
    let data = get_or_create_section_data(&mut sections, ".gcc_except_table");
    data.push(0xAB);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].name, ".gcc_except_table");
    assert_eq!(sections[0].data, vec![0xAB]);
}

#[test]
fn get_or_create_section_finds_existing() {
    use llvm_codegen::emit::Section;
    let mut sections: Vec<Section> = vec![Section {
        name: ".gcc_except_table".into(),
        data: vec![0x01],
        relocs: Vec::new(),
        debug_rows: Vec::new(),
    }];
    let data = get_or_create_section_data(&mut sections, ".gcc_except_table");
    data.push(0x02);
    // Still only one section.
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].data, vec![0x01, 0x02]);
}

#[test]
fn get_or_create_section_does_not_confuse_different_names() {
    use llvm_codegen::emit::Section;
    let mut sections: Vec<Section> = vec![Section {
        name: ".text".into(),
        data: vec![0x90],
        relocs: Vec::new(),
        debug_rows: Vec::new(),
    }];
    let data = get_or_create_section_data(&mut sections, ".gcc_except_table");
    data.push(0xFF);
    // Now two sections.
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[1].name, ".gcc_except_table");
    assert_eq!(sections[1].data, vec![0xFF]);
}
