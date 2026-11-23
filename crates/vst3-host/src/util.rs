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

pub fn hex(cid: &[u8; 16]) -> String {
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
