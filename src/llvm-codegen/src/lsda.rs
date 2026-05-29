//! `.gcc_except_table` LSDA (Language-Specific Data Area) builder.
//!
//! The LSDA maps instruction PC ranges to landing pads.  Personality routines
//! like `__gxx_personality_v0` read this table at unwind time to find the
//! correct handler block.
//!
//! Format follows the GCC/LLVM `.gcc_except_table` specification:
//! - `@LPStart` encoding = `DW_EH_PE_omit` (0xff) — no separate LPStart base
//! - `@TType` encoding  = `DW_EH_PE_omit` (0xff) — no type infos for now
//! - CallSite encoding  = `DW_EH_PE_uleb128` (0x01)
//! - ULEB128 length of the call-site table
//! - One record per call site (all fields ULEB128-encoded):
//!   - `cs_start`  — byte offset from function start to the call instruction
//!   - `cs_len`    — byte length of the call instruction
//!   - `cs_lp`     — byte offset from function start to the landing-pad (0 = none)
//!   - `cs_action` — 0 = cleanup / catch-all; ≥1 = index into action table

// ── Call-site record ──────────────────────────────────────────────────────

/// A single call-site record in the LSDA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSiteRecord {
    /// Byte offset from function start of the first byte of the call instruction.
    pub call_start: u32,
    /// Length in bytes of the call instruction (typically 5 for x86-64 CALL rel32).
    pub call_len: u32,
    /// Byte offset from function start of the landing-pad block's first instruction.
    /// `0` means "no landing pad for this call" (exception propagates to caller).
    pub landing_pad: u32,
    /// `0` = cleanup (always run the LP); `1+` = action table index (typed catch).
    pub action: u32,
}

// ── LsdaBuilder ───────────────────────────────────────────────────────────

/// Builder for a `.gcc_except_table` LSDA section.
///
/// Accumulates call-site records and serialises them to the `.gcc_except_table`
/// binary format consumed by `__gxx_personality_v0` (and compatible personalities).
pub struct LsdaBuilder {
    call_sites: Vec<CallSiteRecord>,
    /// Type-info entries.  Currently unused (TType encoding = `DW_EH_PE_omit`),
    /// reserved for future typed-catch support.
    #[allow(dead_code)]
    type_infos: Vec<u64>,
}

impl LsdaBuilder {
    /// Create a new, empty `LsdaBuilder`.
    pub fn new() -> Self {
        Self {
            call_sites: Vec::new(),
            type_infos: Vec::new(),
        }
    }

    /// Add a call-site record.
    pub fn add_call_site(&mut self, rec: CallSiteRecord) {
        self.call_sites.push(rec);
    }

    /// Returns `true` if there are no call-site records.
    ///
    /// Functions without any `invoke` instructions have an empty LSDA and
    /// typically do not need a `.gcc_except_table` entry at all.
    pub fn is_empty(&self) -> bool {
        self.call_sites.is_empty()
    }

    /// Serialize to bytes in `.gcc_except_table` format.
    ///
    /// ```text
    /// @LPStart encoding  = DW_EH_PE_omit (0xff)      [1 byte]
    /// @TType  encoding   = DW_EH_PE_omit (0xff)      [1 byte]
    /// CallSite encoding  = DW_EH_PE_uleb128 (0x01)   [1 byte]
    /// CallSite table len = ULEB128 (byte length of the following table)
    /// Call-site table:
    ///   For each record:
    ///     cs_start   ULEB128
    ///     cs_len     ULEB128
    ///     cs_lp      ULEB128   (0 = no landing pad)
    ///     cs_action  ULEB128   (0 = cleanup)
    /// ```
    pub fn build(&self) -> Vec<u8> {
        // Serialise the call-site table first so we know its byte length.
        let mut table: Vec<u8> = Vec::new();
        for rec in &self.call_sites {
            write_uleb128(&mut table, rec.call_start as u64);
            write_uleb128(&mut table, rec.call_len as u64);
            write_uleb128(&mut table, rec.landing_pad as u64);
            write_uleb128(&mut table, rec.action as u64);
        }

        let mut out: Vec<u8> = Vec::new();
        // @LPStart encoding = DW_EH_PE_omit (0xff)
        out.push(0xff);
        // @TType encoding = DW_EH_PE_omit (0xff)
        out.push(0xff);
        // CallSite encoding = DW_EH_PE_uleb128 (0x01)
        out.push(0x01);
        // CallSite table length (ULEB128)
        write_uleb128(&mut out, table.len() as u64);
        // Call-site table
        out.extend_from_slice(&table);
        out
    }
}

impl Default for LsdaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ── ULEB128 / SLEB128 helpers ─────────────────────────────────────────────

/// Write a `val` as ULEB128 into `buf`.
pub fn write_uleb128(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        let byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            buf.push(byte | 0x80);
        } else {
            buf.push(byte);
            break;
        }
    }
}

/// Write a `val` as SLEB128 into `buf`.
pub fn write_sleb128(buf: &mut Vec<u8>, mut val: i64) {
    loop {
        let byte = (val & 0x7f) as u8;
        val >>= 7;
        let more = !((val == 0 && (byte & 0x40) == 0) || (val == -1 && (byte & 0x40) != 0));
        buf.push(if more { byte | 0x80 } else { byte });
        if !more {
            break;
        }
    }
}

// ── XdataBuilder (MSVC COFF minimal unwind info) ──────────────────────────

/// Minimal MSVC `__CxxFrameHandler3`-compatible `UnwindInfo` for COFF targets.
///
/// Produces a `FuncInfo` struct that the MSVC EH personality can interpret to
/// locate IP-to-state mappings.  Only the `magic` number, `maxState`, and the
/// inline IP-to-state array are emitted; action records and type descriptors are
/// left empty (all exceptions propagate to the next frame).
pub struct XdataBuilder {
    /// Each entry maps `(ip_offset, state)` — sorted by `ip_offset` at build time.
    ip_to_state: Vec<(u32, i32)>,
}

impl XdataBuilder {
    /// Create a new, empty `XdataBuilder`.
    pub fn new() -> Self {
        Self {
            ip_to_state: Vec::new(),
        }
    }

    /// Record an IP-to-state transition at `ip_offset` bytes from function start.
    ///
    /// `state` is the C++ EH state index (−1 = outside any try region).
    pub fn add_ip_state(&mut self, ip_offset: u32, state: i32) {
        self.ip_to_state.push((ip_offset, state));
    }

    /// Serialise to a minimal `FuncInfo` binary blob.
    ///
    /// Layout:
    /// ```text
    /// magic        u32  = 0x19930522   (__CxxFrameHandler3 magic)
    /// maxState     u32  = number of entries
    /// ip_count     u32  = number of entries
    /// entries:
    ///   ip_offset  u32
    ///   state      i32
    /// ```
    pub fn build(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        // Sort by IP offset for the personality routine's binary search.
        let mut entries = self.ip_to_state.clone();
        entries.sort_by_key(|e| e.0);

        let count = entries.len() as u32;
        // Magic number identifying __CxxFrameHandler3.
        out.extend_from_slice(&0x1993_0522u32.to_le_bytes());
        // maxState = number of unique states.
        out.extend_from_slice(&count.to_le_bytes());
        // ip_count = number of IP-to-state entries.
        out.extend_from_slice(&count.to_le_bytes());
        for (ip, st) in &entries {
            out.extend_from_slice(&ip.to_le_bytes());
            out.extend_from_slice(&(*st as u32).to_le_bytes());
        }
        out
    }
}

impl Default for XdataBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ── emit_object helper ────────────────────────────────────────────────────

/// Find or create a [`Section`] with `name` inside `obj`, returning a mutable
/// reference to its `data` buffer.
///
/// If no section with that name exists yet, a new empty section is appended
/// and a reference to its data is returned.  Callers can then `extend` the
/// data with their serialised LSDA / xdata bytes.
///
/// [`Section`]: crate::emit::Section
pub fn get_or_create_section_data<'a>(
    sections: &'a mut Vec<crate::emit::Section>,
    name: &str,
) -> &'a mut Vec<u8> {
    // Find existing section with this name.
    if let Some(pos) = sections.iter().position(|s| s.name == name) {
        return &mut sections[pos].data;
    }
    // Create a new one.
    sections.push(crate::emit::Section {
        name: name.to_string(),
        data: Vec::new(),
        relocs: Vec::new(),
        debug_rows: Vec::new(),
    });
    let last = sections.len() - 1;
    &mut sections[last].data
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: decode a single ULEB128 value from the front of `buf`.
    // Returns (value, bytes_consumed).
    pub fn decode_uleb128(buf: &[u8]) -> (u64, usize) {
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

    // ── ULEB128 unit tests ────────────────────────────────────────────────

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

    // ── LsdaBuilder tests ────────────────────────────────────────────────

    #[test]
    fn lsda_empty_produces_no_call_sites() {
        let lsda = LsdaBuilder::new();
        assert!(lsda.is_empty());
        // build() on an empty LSDA is valid — returns header-only bytes.
        let bytes = lsda.build();
        // Header: 0xff, 0xff, 0x01, ULEB128(0) = 4 bytes minimum.
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
        // Header bytes.
        assert_eq!(bytes[0], 0xff); // @LPStart = omit
        assert_eq!(bytes[1], 0xff); // @TType   = omit
        assert_eq!(bytes[2], 0x01); // CallSite encoding = uleb128
                                    // Must have more bytes beyond the 3-byte header.
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
        // At least 3 call-site records beyond the header.
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
        // The byte value 0x00 must appear somewhere for the landing_pad=0 field.
        assert!(bytes.contains(&0));
    }

    #[test]
    fn lsda_round_trip_decode() {
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
        let (_cs_table_len, advance) = decode_uleb128(&bytes[pos..]);
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

    // ── XdataBuilder tests ───────────────────────────────────────────────

    #[test]
    fn xdata_magic_bytes_correct() {
        let xd = XdataBuilder::new();
        let bytes = xd.build();
        // First 4 bytes = 0x19930522 little-endian = [0x22, 0x05, 0x93, 0x19]
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
        // First entry should be ip=10 (sorted).
        let first_ip = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        assert_eq!(first_ip, 10);
    }

    // ── get_or_create_section_data test ──────────────────────────────────

    #[test]
    fn get_or_create_section_creates_new() {
        use crate::emit::Section;
        let mut sections: Vec<Section> = Vec::new();
        let data = get_or_create_section_data(&mut sections, ".gcc_except_table");
        data.push(0xAB);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, ".gcc_except_table");
        assert_eq!(sections[0].data, vec![0xAB]);
    }

    #[test]
    fn get_or_create_section_finds_existing() {
        use crate::emit::Section;
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
}
