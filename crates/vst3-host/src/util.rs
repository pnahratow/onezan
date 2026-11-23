use std::ffi::c_char;
use std::fmt::Write as _;
use vst3::Steinberg::{TUID, kResultOk, kResultTrue, tresult};

pub const fn ok(r: tresult) -> bool {
    r == kResultOk || r == kResultTrue
}

/// A VST3 enum constant as the fixed-width integer an API parameter or field takes.
///
/// The bindings type enum constants as `c_uint` on Linux and `c_int` on Windows, while the API
/// takes `int32`, `uint16` or `uint32`, so a plain `as` would be a no-op on one platform and a
/// sign change on the other. Every constant used is a small non-negative number, so the
/// fallback (0) is never taken.
pub fn venum<T, U>(v: T) -> U
where
    U: TryFrom<T> + Default,
{
    U::try_from(v).unwrap_or_default()
}

/// A `c_char` (signed on x86, unsigned on Arm) as its byte.
pub const fn c_byte(c: c_char) -> u8 {
    u8::from_ne_bytes(c.to_ne_bytes())
}

/// A NUL-terminated `char8` array (factory and class info strings) as a Rust string.
pub fn read_c8(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c_byte(c)).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Writes `s` as a NUL-terminated UTF-16 string into `dst`, truncating if needed.
pub fn write_c16(s: &str, dst: &mut [u16]) {
    let n = dst.iter_mut().zip(s.encode_utf16()).map(|(d, c)| *d = c).count();
    let end = n.min(dst.len().saturating_sub(1));
    if let Some(d) = dst.get_mut(end) {
        *d = 0;
    }
}

pub fn tuid_bytes(t: &TUID) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (o, &b) in out.iter_mut().zip(t.iter()) {
        *o = c_byte(b);
    }
    out
}

/// A class id as the VST3 SDK prints it (`FUID::toString`): the same 32 hex digits for a plugin
/// on every platform. On Windows the id is kept in COM's GUID layout (the first four bytes
/// reversed, then two pairs swapped), so the bytes are put back in order first.
pub fn class_id_string(cid: &[u8; 16]) -> String {
    hex(&canonical(cid, cfg!(windows)))
}

/// `cid` in the SDK's canonical byte order; `com` when it is in COM's GUID layout.
fn canonical(cid: &[u8; 16], com: bool) -> [u8; 16] {
    if !com {
        return *cid;
    }
    let order = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut out = [0u8; 16];
    for (o, &i) in out.iter_mut().zip(order.iter()) {
        *o = cid.get(i).copied().unwrap_or(0);
    }
    out
}

fn hex(cid: &[u8; 16]) -> String {
    cid.iter().fold(String::with_capacity(32), |mut s, b| {
        // Writing to a String cannot fail.
        let _ = write!(s, "{b:02X}");
        s
    })
}

/// A count or index as VST3's `int32`, saturating. Every count handed over is capped far below.
pub fn int32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Surge XT's class id, as Linux and Windows hold it in memory.
    #[test]
    fn class_ids_print_the_same_on_every_platform() {
        let linux: [u8; 16] = [
            0xAB, 0xCD, 0xEF, 0x01, 0x91, 0x82, 0xFA, 0xEB, 0x56, 0x6D, 0x62, 0x41, 0x53, 0x67, 0x58, 0x54,
        ];
        let windows: [u8; 16] = [
            0x01, 0xEF, 0xCD, 0xAB, 0x82, 0x91, 0xEB, 0xFA, 0x56, 0x6D, 0x62, 0x41, 0x53, 0x67, 0x58, 0x54,
        ];
        let want = "ABCDEF019182FAEB566D624153675854";
        assert_eq!(hex(&canonical(&linux, false)), want);
        assert_eq!(hex(&canonical(&windows, true)), want);
    }
}
