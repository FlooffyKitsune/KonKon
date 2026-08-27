//! V1 (`BurikoCompiledScriptVer1.00`) bytecode disassembler.
//!
//! Faithful port of VNTextPatch's `EthornellV1Disassembler` (+ the base
//! `EthornellDisassembler` operand-template reader). It walks the `i32` opcode
//! stream from `code_offset`, tracking a stack of pushed string addresses, and
//! emits `(operand_offset, address, kind)` for every string reference site in
//! the order the C# `StringAddressEncountered` event fires.
//!
//! Every read is bounds-checked; an opcode absent from [`OPERAND_TEMPLATES`] or
//! any structural inconsistency yields [`KonkonError::Format`] with
//! `container: "script"` — the walk never panics and never silently skips.

use crate::{KonkonError, Result};

use super::layout::StrKind;

/// The 0x1C-byte signature at the start of every V1 script.
pub(super) const MAGIC: &[u8] = b"BurikoCompiledScriptVer1.00\0";

/// Build a `KonkonError::Format { container: "script", .. }`. Shared by every
/// read in this module and by [`super::script`].
pub(super) fn format_err(detail: impl Into<String>) -> KonkonError {
    KonkonError::Format {
        container: "script",
        detail: detail.into(),
    }
}

/// Resolve a string-address operand (`code_offset + address`) to an absolute
/// offset, bounds-checked against `data`. Shared by the walk and by
/// [`super::script`].
pub(super) fn resolve_string_address(
    data: &[u8],
    code_offset: usize,
    address: i32,
) -> Result<usize> {
    let abs = code_offset as i64 + address as i64;
    if abs < 0 || abs as u64 >= data.len() as u64 {
        return Err(format_err(format!(
            "string address {address} resolves outside the script"
        )));
    }
    Ok(abs as usize)
}

/// Operand templates, verbatim from `EthornellV1Disassembler.OperandTemplates`.
///
/// * `h` — `i16`
/// * `i` — `i32`
/// * `c` — `i32` code address (tracked for the termination test)
/// * `n` — `i32` [`StrKind::CharacterName`] string address
/// * `m` — `i32` [`StrKind::Message`] string address
/// * `z` — inline zero-terminated Shift-JIS string (skipped)
/// * `""` — no operands
///
/// Sorted by opcode; looked up by binary search. An opcode not present here is
/// an error (desync / unknown variant), never a guess.
#[rustfmt::skip]
static OPERAND_TEMPLATES: [(i32, &str); 561] = [
    (0x0000, "i"), (0x0001, "c"), (0x0002, "i"), (0x0003, "m"), (0x0008, "i"), (0x0009, "i"),
    (0x000A, "i"), (0x0010, ""), (0x0011, ""), (0x0015, ""), (0x0016, ""), (0x0017, "i"),
    (0x0018, ""), (0x0019, "i"), (0x001A, ""), (0x001B, ""), (0x001C, ""), (0x001D, ""),
    (0x001E, ""), (0x001F, ""), (0x0020, ""), (0x0021, ""), (0x0022, ""), (0x0023, ""),
    (0x0024, ""), (0x0025, ""), (0x0026, ""), (0x0027, ""), (0x0028, ""), (0x0029, ""),
    (0x002A, ""), (0x002B, ""), (0x0030, ""), (0x0031, ""), (0x0032, ""), (0x0033, ""),
    (0x0034, ""), (0x0035, ""), (0x0038, ""), (0x0039, ""), (0x003A, ""), (0x003E, ""),
    (0x003F, "i"), (0x0040, ""), (0x0048, ""), (0x007B, "iii"), (0x007E, "i"), (0x007F, "ii"),
    (0x0080, ""), (0x0081, ""), (0x0082, ""), (0x0083, ""), (0x0090, ""), (0x0091, ""),
    (0x0092, ""), (0x0093, ""), (0x0094, ""), (0x0095, ""), (0x0098, ""), (0x0099, ""),
    (0x00A0, ""), (0x00A8, ""), (0x00AA, ""), (0x00AC, ""), (0x00C0, ""), (0x00C1, ""),
    (0x00C2, ""), (0x00D0, ""), (0x00E0, ""), (0x00E1, ""), (0x00E2, ""), (0x00E3, ""),
    (0x00E4, ""), (0x00E5, ""), (0x00E6, ""), (0x00E7, ""), (0x00E8, ""), (0x00E9, ""),
    (0x00EA, ""), (0x00EB, ""), (0x00EC, ""), (0x00ED, ""), (0x00EE, ""), (0x00EF, ""),
    (0x00F0, ""), (0x00F1, ""), (0x00F3, ""), (0x00F4, ""), (0x00F5, ""), (0x00F6, ""),
    (0x00F7, ""), (0x00F8, ""), (0x00F9, ""), (0x00FA, ""), (0x00FB, ""), (0x00FC, ""),
    (0x00FD, ""), (0x00FE, ""), (0x00FF, ""), (0x0100, ""), (0x0101, ""), (0x0102, ""),
    (0x0103, ""), (0x0104, ""), (0x0105, ""), (0x0106, ""), (0x0107, ""), (0x0108, ""),
    (0x0109, ""), (0x010A, ""), (0x010B, ""), (0x010D, ""), (0x010E, ""), (0x010F, ""),
    (0x0110, ""), (0x0111, ""), (0x0112, ""), (0x0113, ""), (0x0114, ""), (0x0115, ""),
    (0x0116, ""), (0x0117, ""), (0x0118, ""), (0x0119, ""), (0x011A, ""), (0x011B, ""),
    (0x011C, ""), (0x011D, ""), (0x011E, ""), (0x011F, ""), (0x0120, ""), (0x0121, ""),
    (0x0122, ""), (0x0123, ""), (0x0124, ""), (0x0125, ""), (0x0126, ""), (0x0127, ""),
    (0x0128, ""), (0x0129, ""), (0x012A, ""), (0x012C, ""), (0x012D, ""), (0x012E, ""),
    (0x012F, ""), (0x0130, ""), (0x0131, ""), (0x0132, ""), (0x0133, ""), (0x0134, ""),
    (0x0135, ""), (0x0136, ""), (0x0137, ""), (0x0138, ""), (0x0139, ""), (0x013A, ""),
    (0x013B, ""), (0x013C, ""), (0x013D, ""), (0x013E, ""), (0x013F, ""), (0x0140, ""),
    (0x0141, ""), (0x0142, ""), (0x0143, ""), (0x0144, ""), (0x0145, ""), (0x0146, ""),
    (0x0147, ""), (0x0148, ""), (0x0149, ""), (0x014A, ""), (0x014B, ""), (0x014C, ""),
    (0x014D, ""), (0x014E, ""), (0x014F, ""), (0x0150, ""), (0x0151, ""), (0x0152, ""),
    (0x0153, ""), (0x0156, ""), (0x0157, ""), (0x0158, ""), (0x0159, ""), (0x015A, ""),
    (0x015C, ""), (0x015D, ""), (0x015E, ""), (0x015F, ""), (0x0160, ""), (0x0161, ""),
    (0x0163, ""), (0x0164, ""), (0x0165, ""), (0x0166, ""), (0x0167, ""), (0x0168, ""),
    (0x0169, ""), (0x016A, ""), (0x016B, ""), (0x016C, ""), (0x016D, ""), (0x016E, ""),
    (0x016F, ""), (0x0170, ""), (0x0171, ""), (0x0172, ""), (0x0173, ""), (0x0174, ""),
    (0x0175, ""), (0x0176, ""), (0x0178, ""), (0x0179, ""), (0x017A, ""), (0x017D, ""),
    (0x017E, ""), (0x017F, ""), (0x0180, ""), (0x0181, ""), (0x0182, ""), (0x0184, ""),
    (0x0185, ""), (0x0186, ""), (0x0187, ""), (0x0188, ""), (0x0189, ""), (0x018A, ""),
    (0x018B, ""), (0x018D, ""), (0x018E, ""), (0x018F, ""), (0x0190, ""), (0x0191, ""),
    (0x0194, ""), (0x0195, ""), (0x0196, ""), (0x0197, ""), (0x0198, ""), (0x0199, ""),
    (0x019C, ""), (0x019D, ""), (0x019E, ""), (0x019F, ""), (0x01A0, ""), (0x01A1, ""),
    (0x01A2, ""), (0x01A3, ""), (0x01A4, ""), (0x01A5, ""), (0x01A6, ""), (0x01A7, ""),
    (0x01A8, ""), (0x01A9, ""), (0x01AA, ""), (0x01AB, ""), (0x01AC, ""), (0x01AD, ""),
    (0x01AE, ""), (0x01AF, ""), (0x01B0, ""), (0x01B1, ""), (0x01B2, ""), (0x01B4, ""),
    (0x01B5, ""), (0x01B6, ""), (0x01B7, ""), (0x01BF, ""), (0x01D0, ""), (0x01D4, ""),
    (0x01D8, ""), (0x01D9, ""), (0x01E0, ""), (0x01F0, ""), (0x0200, ""), (0x0204, ""),
    (0x0205, ""), (0x0208, ""), (0x0209, ""), (0x020A, ""), (0x020C, ""), (0x020E, ""),
    (0x020F, ""), (0x0210, ""), (0x0220, ""), (0x0222, ""), (0x0225, ""), (0x0226, ""),
    (0x0228, ""), (0x0229, ""), (0x022A, ""), (0x0230, ""), (0x0231, ""), (0x0232, ""),
    (0x0233, ""), (0x0234, ""), (0x0235, ""), (0x0236, ""), (0x0237, ""), (0x0238, ""),
    (0x0239, ""), (0x023A, ""), (0x023B, ""), (0x023C, ""), (0x023D, ""), (0x0240, ""),
    (0x0241, ""), (0x0242, ""), (0x0244, ""), (0x0245, ""), (0x0248, ""), (0x024C, ""),
    (0x024D, ""), (0x024E, ""), (0x0250, ""), (0x0251, ""), (0x0252, ""), (0x0254, ""),
    (0x0255, ""), (0x0256, ""), (0x0257, ""), (0x0258, ""), (0x025E, ""), (0x025F, ""),
    (0x0260, ""), (0x0261, ""), (0x0262, ""), (0x0266, ""), (0x0268, ""), (0x027F, ""),
    (0x0280, ""), (0x0281, ""), (0x0284, ""), (0x0288, ""), (0x0289, ""), (0x028A, ""),
    (0x0290, ""), (0x0294, ""), (0x0295, ""), (0x0296, ""), (0x0297, ""), (0x0298, ""),
    (0x0299, ""), (0x029C, ""), (0x02A0, ""), (0x02A1, ""), (0x02A2, ""), (0x02A3, ""),
    (0x02A4, ""), (0x02A8, ""), (0x02C0, ""), (0x02C1, ""), (0x02C2, ""), (0x02C3, ""),
    (0x02C4, ""), (0x02C5, ""), (0x02C6, ""), (0x02C7, ""), (0x02C8, ""), (0x02CA, ""),
    (0x02CB, ""), (0x02CC, ""), (0x02CD, ""), (0x02CE, ""), (0x02CF, ""), (0x02D0, ""),
    (0x02D2, ""), (0x02D4, ""), (0x02D5, ""), (0x02D6, ""), (0x02D7, ""), (0x02D8, ""),
    (0x02D9, ""), (0x02DA, ""), (0x02DB, ""), (0x02DC, ""), (0x02DD, ""), (0x02DE, ""),
    (0x02DF, ""), (0x02E0, ""), (0x02E1, ""), (0x02E2, ""), (0x02E3, ""), (0x02E4, ""),
    (0x02E5, ""), (0x02E6, ""), (0x02E7, ""), (0x02E8, ""), (0x02E9, ""), (0x02EA, ""),
    (0x02EB, ""), (0x02EC, ""), (0x02EE, ""), (0x02F0, ""), (0x02F1, ""), (0x02F3, ""),
    (0x02F4, ""), (0x02F8, ""), (0x02FA, ""), (0x02FC, ""), (0x02FD, ""), (0x0300, ""),
    (0x0301, ""), (0x0302, ""), (0x0303, ""), (0x0304, ""), (0x0306, ""), (0x0307, ""),
    (0x0308, ""), (0x0309, ""), (0x030A, ""), (0x030C, ""), (0x030D, ""), (0x030E, ""),
    (0x0310, ""), (0x0311, ""), (0x0314, ""), (0x031E, ""), (0x031F, ""), (0x0320, ""),
    (0x0328, ""), (0x032C, ""), (0x0330, ""), (0x0331, ""), (0x0334, ""), (0x0335, ""),
    (0x0336, ""), (0x0337, ""), (0x0338, ""), (0x0339, ""), (0x033F, ""), (0x0340, ""),
    (0x0341, ""), (0x0348, ""), (0x0350, ""), (0x0351, ""), (0x0352, ""), (0x0353, ""),
    (0x0354, ""), (0x0355, ""), (0x0358, ""), (0x0360, ""), (0x0368, ""), (0x0380, ""),
    (0x0388, ""), (0x038D, ""), (0x038E, ""), (0x038F, ""), (0x0390, ""), (0x0391, ""),
    (0x0392, ""), (0x0393, ""), (0x0394, ""), (0x03AF, ""), (0x03C0, ""), (0x03C1, ""),
    (0x03C2, ""), (0x03C4, ""), (0x03C5, ""), (0x03C6, ""), (0x03C7, ""), (0x03C8, ""),
    (0x03C9, ""), (0x03CA, ""), (0x03D0, ""), (0x03D2, ""), (0x03D4, ""), (0x03D5, ""),
    (0x03D6, ""), (0x03D8, ""), (0x03DC, ""), (0x03F0, ""), (0x03F1, ""), (0x03F4, ""),
    (0x03F5, ""), (0x03F6, ""), (0x03F7, ""), (0x03F8, ""), (0x03FA, ""), (0x03FB, ""),
    (0x03FC, ""), (0x03FD, ""), (0x03FE, ""), (0x03FF, ""), (0x0400, ""), (0x0401, ""),
    (0x0402, ""), (0x0403, ""), (0x0404, ""), (0x0405, ""), (0x0408, ""), (0x0409, ""),
    (0x040A, ""), (0x040B, ""), (0x040C, ""), (0x040D, ""), (0x040F, ""), (0x0410, ""),
    (0x0411, ""), (0x0412, ""), (0x0413, ""), (0x0418, ""), (0x0427, ""), (0x0428, ""),
    (0x0429, ""), (0x042A, ""), (0x042B, ""), (0x042C, ""), (0x042D, ""), (0x042F, ""),
    (0x0430, ""), (0x0431, ""), (0x0432, ""), (0x0440, ""), (0x0441, ""), (0x0442, ""),
    (0x0444, ""), (0x0448, ""), (0x0449, ""), (0x0450, ""), (0x0451, ""), (0x0452, ""),
    (0x0453, ""), (0x0454, ""), (0x0455, ""), (0x0458, ""), (0x0459, ""), (0x045C, ""),
    (0x045D, ""), (0x045E, ""), (0x0480, ""), (0x0481, ""), (0x0482, ""), (0x0483, ""),
    (0x0484, ""), (0x0485, ""), (0x04C0, ""), (0x04C1, ""), (0x04C2, ""), (0x04C3, ""),
    (0x04C4, ""), (0x04C5, ""), (0x04C6, ""), (0x04C7, ""), (0x04C8, ""), (0x04C9, ""),
    (0x04CA, ""), (0x04CB, ""), (0x04D0, ""), (0x04D1, ""), (0x04D5, ""), (0x04D8, ""),
    (0x04D9, ""), (0x04DA, ""), (0x04E0, ""), (0x04E4, ""), (0x04E5, ""), (0x04E8, ""),
    (0x04E9, ""), (0x04EA, ""), (0x04EB, ""),
];

fn operand_template(opcode: i32) -> Option<&'static str> {
    OPERAND_TEMPLATES
        .binary_search_by_key(&opcode, |&(op, _)| op)
        .ok()
        .map(|i| OPERAND_TEMPLATES[i].1)
}

/// One emitted string reference site: `(operand_offset, address, kind)`.
pub(super) struct StringSite {
    pub operand_offset: usize,
    pub address: i32,
    pub kind: StrKind,
}

/// Result of a successful disassembly.
pub(super) struct Disassembly {
    /// Byte position immediately after the terminating opcode.
    pub end_pos: usize,
    pub sites: Vec<StringSite>,
}

struct StackItem {
    operand_offset: usize,
    address: i32,
}

struct Disassembler<'a> {
    data: &'a [u8],
    code_offset: usize,
    pos: usize,
    largest_code_address: i64,
    stack: Vec<StackItem>,
    sites: Vec<StringSite>,
}

/// Disassemble the V1 script `data` whose bytecode begins at `code_offset`.
pub(super) fn disassemble(data: &[u8], code_offset: usize) -> Result<Disassembly> {
    let mut d = Disassembler {
        data,
        code_offset,
        pos: code_offset,
        largest_code_address: 0,
        stack: Vec::new(),
        sites: Vec::new(),
    };
    d.run()?;
    Ok(Disassembly {
        end_pos: d.pos,
        sites: d.sites,
    })
}

impl Disassembler<'_> {
    fn run(&mut self) -> Result<()> {
        loop {
            let opcode_pos = self.pos;
            let opcode = self.read_i32()?;

            match opcode {
                0x0003 => self.op_push_string_address()?,
                0x001C => self.op_user_function_call()?,
                0x0140 | 0x0143 => self.op_message()?,
                0x0160 => self.op_choice_screen(),
                _ => {
                    let template = operand_template(opcode).ok_or_else(|| {
                        format_err(format!(
                            "unknown opcode 0x{opcode:04X} at offset {opcode_pos} \
                             (bytecode desync or unsupported script variant)"
                        ))
                    })?;
                    self.read_operands(template)?;
                }
            }

            let consumed = self.pos as i64 - self.code_offset as i64;
            if (opcode == 0x001B || opcode == 0x00F4) && self.largest_code_address < consumed {
                break;
            }

            if opcode == 0x007E || opcode == 0x007F || opcode == 0x00FE {
                self.flush_stack_as_internal();
            }
        }
        self.flush_stack_as_internal();
        Ok(())
    }

    fn read_operands(&mut self, template: &str) -> Result<()> {
        for c in template.chars() {
            match c {
                'h' => {
                    self.read_i16()?;
                }
                'i' => {
                    self.read_i32()?;
                }
                'c' => {
                    let address = self.read_i32()?;
                    self.note_code_address(address);
                }
                'n' => {
                    let operand_offset = self.pos;
                    let address = self.read_i32()?;
                    self.sites.push(StringSite {
                        operand_offset,
                        address,
                        kind: StrKind::CharacterName,
                    });
                }
                'm' => {
                    let operand_offset = self.pos;
                    let address = self.read_i32()?;
                    self.sites.push(StringSite {
                        operand_offset,
                        address,
                        kind: StrKind::Message,
                    });
                }
                'z' => self.skip_inline_string()?,
                other => {
                    return Err(format_err(format!(
                        "bad operand-template character '{other}'"
                    )))
                }
            }
        }
        Ok(())
    }

    // --- specialized opcode handlers (mirror EthornellV1Disassembler) ---

    fn op_push_string_address(&mut self) -> Result<()> {
        let operand_offset = self.pos;
        let address = self.read_i32()?;
        self.stack.push(StackItem {
            operand_offset,
            address,
        });
        Ok(())
    }

    fn op_user_function_call(&mut self) -> Result<()> {
        let Some(item) = self.stack.pop() else {
            return Ok(());
        };
        self.sites.push(StringSite {
            operand_offset: item.operand_offset,
            address: item.address,
            kind: StrKind::Internal,
        });
        let func_name = self.read_string_at_address(item.address)?;
        if func_name == b"_SelectEx" {
            self.op_choice_screen();
        }
        Ok(())
    }

    fn op_message(&mut self) -> Result<()> {
        let message = self
            .stack
            .pop()
            .ok_or_else(|| format_err("show-message opcode with empty string stack"))?;

        if let Some(name) = self.stack.pop() {
            let kind = if self.is_empty_string_at(name.address)? {
                StrKind::Internal
            } else {
                StrKind::CharacterName
            };
            self.sites.push(StringSite {
                operand_offset: name.operand_offset,
                address: name.address,
                kind,
            });
        }

        let kind = if self.is_empty_string_at(message.address)? {
            StrKind::Internal
        } else {
            StrKind::Message
        };
        self.sites.push(StringSite {
            operand_offset: message.operand_offset,
            address: message.address,
            kind,
        });
        Ok(())
    }

    fn op_choice_screen(&mut self) {
        // C# pops the whole stack and restores push order before emitting; the
        // Vec is already in push order, so drain it front-to-back.
        for item in std::mem::take(&mut self.stack) {
            self.sites.push(StringSite {
                operand_offset: item.operand_offset,
                address: item.address,
                kind: StrKind::Message,
            });
        }
    }

    fn flush_stack_as_internal(&mut self) {
        while let Some(item) = self.stack.pop() {
            self.sites.push(StringSite {
                operand_offset: item.operand_offset,
                address: item.address,
                kind: StrKind::Internal,
            });
        }
    }

    // --- primitive reads (all bounds-checked) ---

    fn read_i32(&mut self) -> Result<i32> {
        let end = self
            .pos
            .checked_add(4)
            .ok_or_else(|| format_err("position overflow"))?;
        let slice = self.data.get(self.pos..end).ok_or_else(|| {
            format_err(format!(
                "unexpected end of script reading i32 at {}",
                self.pos
            ))
        })?;
        self.pos = end;
        Ok(i32::from_le_bytes(slice.try_into().unwrap()))
    }

    fn read_i16(&mut self) -> Result<i16> {
        let end = self
            .pos
            .checked_add(2)
            .ok_or_else(|| format_err("position overflow"))?;
        let slice = self.data.get(self.pos..end).ok_or_else(|| {
            format_err(format!(
                "unexpected end of script reading i16 at {}",
                self.pos
            ))
        })?;
        self.pos = end;
        Ok(i16::from_le_bytes(slice.try_into().unwrap()))
    }

    fn skip_inline_string(&mut self) -> Result<()> {
        let rest = self.data.get(self.pos..).ok_or_else(|| {
            format_err(format!(
                "inline string starts past end of script at {}",
                self.pos
            ))
        })?;
        let nul = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| format_err("unterminated inline string operand"))?;
        self.pos += nul + 1;
        Ok(())
    }

    fn resolve_address(&self, address: i32) -> Result<usize> {
        resolve_string_address(self.data, self.code_offset, address)
    }

    fn is_empty_string_at(&self, address: i32) -> Result<bool> {
        let at = self.resolve_address(address)?;
        Ok(self.data[at] == 0)
    }

    fn read_string_at_address(&self, address: i32) -> Result<Vec<u8>> {
        let at = self.resolve_address(address)?;
        read_zero_terminated(self.data, at)
    }

    fn note_code_address(&mut self, address: i32) {
        self.largest_code_address = self.largest_code_address.max(address as i64);
    }
}

/// The zero-terminated byte string at `offset` (NUL excluded).
fn read_zero_terminated(data: &[u8], offset: usize) -> Result<Vec<u8>> {
    let rest = data
        .get(offset..)
        .ok_or_else(|| format_err(format!("string offset {offset} is past end of script")))?;
    let nul = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| format_err(format!("unterminated string at offset {offset}")))?;
    Ok(rest[..nul].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operand_table_is_sorted_and_deduplicated() {
        for w in OPERAND_TEMPLATES.windows(2) {
            assert!(
                w[0].0 < w[1].0,
                "table not strictly sorted at 0x{:04X}",
                w[1].0
            );
        }
    }

    #[test]
    fn operand_table_lookup() {
        assert_eq!(operand_template(0x0000), Some("i"));
        assert_eq!(operand_template(0x007B), Some("iii"));
        assert_eq!(operand_template(0x0140), Some(""));
        assert_eq!(operand_template(0x04EB), Some(""));
        assert_eq!(operand_template(0x0004), None);
        assert_eq!(operand_template(0x9999), None);
    }
}
