//! `IBStream` over a byte buffer in memory: what a plugin writes its state into (`getState`) and
//! reads it back from (`setState`).

use std::ffi::c_void;
use std::sync::{Mutex, PoisonError};
use vst3::Class;
use vst3::Steinberg::IBStream_::IStreamSeekMode_::{kIBSeekCur, kIBSeekEnd, kIBSeekSet};
use vst3::Steinberg::{IBStream, IBStreamTrait, int32, int64, kInvalidArgument, kResultOk, tresult};

/// The bytes, and the position the next read or write starts at.
#[derive(Default)]
pub struct MemoryStream {
    inner: Mutex<(Vec<u8>, usize)>,
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl MemoryStream {
    /// A stream to read `bytes` from, positioned at the start.
    pub const fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            inner: Mutex::new((bytes, 0)),
        }
    }

    /// Everything written so far.
    pub fn bytes(&self) -> Vec<u8> {
        self.lock().0.clone()
    }

    /// Back to the start, to be read again.
    #[cfg(test)]
    fn rewind(&self) {
        self.lock().1 = 0;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (Vec<u8>, usize)> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Writes `n` through `out` when the plugin passed somewhere to write it.
unsafe fn report<T>(out: *mut T, n: T) {
    if !out.is_null() {
        // SAFETY: the caller passes a valid pointer or null.
        unsafe { *out = n };
    }
}

#[expect(
    clippy::significant_drop_tightening,
    reason = "each lock is held by a block that ends when the buffer work does"
)]
impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, num_bytes: int32, num_bytes_read: *mut int32) -> tresult {
        let Ok(want) = usize::try_from(num_bytes) else {
            return kInvalidArgument;
        };
        if buffer.is_null() {
            return kInvalidArgument;
        }
        let n = {
            let mut g = self.lock();
            let (bytes, pos) = &mut *g;
            let src = bytes.get(*pos..).unwrap_or_default();
            let n = want.min(src.len());
            // SAFETY: the plugin's buffer holds `num_bytes`; `n` is no more than that and `src`.
            unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), buffer.cast::<u8>(), n) };
            *pos = pos.saturating_add(n);
            n
        };
        // SAFETY: valid or null, as the API allows.
        unsafe { report(num_bytes_read, crate::util::int32(n)) };
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, num_bytes: int32, num_bytes_written: *mut int32) -> tresult {
        let Ok(n) = usize::try_from(num_bytes) else {
            return kInvalidArgument;
        };
        if buffer.is_null() && n > 0 {
            return kInvalidArgument;
        }
        let data: &[u8] = if n == 0 {
            &[]
        } else {
            // SAFETY: the plugin's buffer holds `num_bytes` readable bytes.
            unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), n) }
        };
        {
            let mut g = self.lock();
            let (bytes, pos) = &mut *g;
            let end = pos.saturating_add(n);
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            if let Some(dst) = bytes.get_mut(*pos..end) {
                dst.copy_from_slice(data);
            }
            *pos = end;
        }
        // SAFETY: valid or null, as the API allows.
        unsafe { report(num_bytes_written, num_bytes) };
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let new = {
            let mut g = self.lock();
            let (bytes, at) = &mut *g;
            let base = match mode {
                m if m == crate::util::venum::<_, int32>(kIBSeekSet) => 0,
                m if m == crate::util::venum::<_, int32>(kIBSeekCur) => i64::try_from(*at).unwrap_or(i64::MAX),
                m if m == crate::util::venum::<_, int32>(kIBSeekEnd) => i64::try_from(bytes.len()).unwrap_or(i64::MAX),
                _ => return kInvalidArgument,
            };
            let Some(new) = base.checked_add(pos).and_then(|p| usize::try_from(p).ok()) else {
                return kInvalidArgument;
            };
            // Past the end is allowed, as in a file: a write there fills the gap with zeros.
            *at = new;
            new
        };
        // SAFETY: valid or null, as the API allows.
        unsafe { report(result, i64::try_from(new).unwrap_or(i64::MAX)) };
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        let at = self.lock().1;
        // SAFETY: checked non-null; the plugin passes a writable pointer.
        unsafe { report(pos, i64::try_from(at).unwrap_or(i64::MAX)) };
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComWrapper;

    #[test]
    fn writes_seeks_and_reads_back() {
        let s = ComWrapper::new(MemoryStream::default());
        let mut data = *b"hello world";
        let mut n = 0;
        unsafe {
            assert_eq!(s.write(data.as_mut_ptr().cast(), 11, &raw mut n), kResultOk);
            assert_eq!(n, 11);
            let mut at = 0;
            s.seek(6, crate::util::venum(kIBSeekSet), &raw mut at);
            assert_eq!(at, 6);
            let mut buf = [0u8; 16];
            s.read(buf.as_mut_ptr().cast(), 16, &raw mut n);
            assert_eq!((n, &buf[..5]), (5, &b"world"[..]));
            // At the end: nothing more, and that is not an error.
            assert_eq!(s.read(buf.as_mut_ptr().cast(), 4, &raw mut n), kResultOk);
            assert_eq!(n, 0);
            s.seek(-5, crate::util::venum(kIBSeekEnd), &raw mut at);
            assert_eq!(at, 6);
            assert_eq!(
                s.seek(-1, crate::util::venum(kIBSeekSet), &raw mut at),
                kInvalidArgument
            );
            let mut tell = 0;
            s.tell(&raw mut tell);
            assert_eq!(tell, 6);
        }
        assert_eq!(s.bytes(), b"hello world");
    }

    #[test]
    fn overwrites_in_place_and_grows() {
        let s = ComWrapper::new(MemoryStream::from_bytes(b"abcdef".to_vec()));
        let mut x = *b"XYZ";
        unsafe {
            s.seek(4, crate::util::venum(kIBSeekSet), std::ptr::null_mut());
            s.write(x.as_mut_ptr().cast(), 3, std::ptr::null_mut());
        }
        assert_eq!(s.bytes(), b"abcdXYZ");
        s.rewind();
        let mut buf = [0u8; 2];
        let mut n = 0;
        unsafe { s.read(buf.as_mut_ptr().cast(), 2, &raw mut n) };
        assert_eq!(&buf, b"ab");
    }
}
