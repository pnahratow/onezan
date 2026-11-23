//! Host-side COM objects the plugin calls back into: the host application / component handler,
//! and the `IMessage` / `IAttributeList` objects that split component/controller plugins allocate
//! through `IHostApplication::createInstance` to talk over `IConnectionPoint`.

use crate::util::{tuid_bytes, write_c16};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;
use vst3::Steinberg::Vst::{
    IAttributeList, IAttributeList_::AttrID, IAttributeListTrait, IComponentHandler, IComponentHandlerTrait,
    IHostApplication, IHostApplicationTrait, IMessage, IMessageTrait, ParamID, ParamValue, String128, TChar,
};
use vst3::Steinberg::{
    FIDString, FUnknown, TUID, int32, int64, kInvalidArgument, kResultFalse, kResultOk, tresult, uint32,
};
use vst3::{Class, ComWrapper, Interface};

const HOST_NAME: &str = "onezan";

/// Handed to `initialize` (as `IHostApplication`) and to the controller (as `IComponentHandler`).
/// Edits made in the plugin's own editor arrive here; nothing consumes them yet.
#[derive(Default)]
pub struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication, IComponentHandler);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        // SAFETY: the plugin passes a writable String128, or null (handled).
        unsafe { name.as_mut() }.map_or(kInvalidArgument, |n| {
            write_c16(HOST_NAME, n);
            kResultOk
        })
    }

    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        // SAFETY: out-pointer from the plugin, or null (handled).
        let Some(out) = (unsafe { obj.as_mut() }) else {
            return kInvalidArgument;
        };
        *out = std::ptr::null_mut();
        // SAFETY: the plugin passes readable 16-byte TUIDs, or null (handled).
        let (Some(cid), Some(iid)) = (unsafe { (cid.as_ref(), iid.as_ref()) }) else {
            return kInvalidArgument;
        };
        let (cid, iid) = (tuid_bytes(cid), tuid_bytes(iid));
        let wants = |i: [u8; 16]| cid == i && (iid == i || iid == FUnknown::IID);
        let raw: *mut c_void = if wants(IMessage::IID) {
            ComWrapper::new(HostMessage::default())
                .to_com_ptr::<IMessage>()
                .map_or(std::ptr::null_mut(), |p| p.into_raw().cast())
        } else if wants(IAttributeList::IID) {
            ComWrapper::new(HostAttributeList::default())
                .to_com_ptr::<IAttributeList>()
                .map_or(std::ptr::null_mut(), |p| p.into_raw().cast())
        } else {
            return kResultFalse;
        };
        *out = raw;
        if raw.is_null() { kResultFalse } else { kResultOk }
    }
}

impl IComponentHandlerTrait for HostApp {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        log::debug!("vst3: restartComponent({flags:#x}) ignored");
        kResultOk
    }
}

// ---- IMessage / IAttributeList ---------------------------------------------------------------

enum Attr {
    Int(i64),
    Float(f64),
    /// UTF-16 including the terminating NUL.
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

#[derive(Default)]
pub struct HostAttributeList {
    attrs: Mutex<HashMap<CString, Attr>>,
}

impl Class for HostAttributeList {
    type Interfaces = (IAttributeList,);
}

fn attr_key(id: AttrID) -> Option<CString> {
    if id.is_null() {
        return None;
    }
    // SAFETY: the plugin passes a NUL-terminated attribute id.
    Some(unsafe { CStr::from_ptr(id) }.to_owned())
}

impl HostAttributeList {
    fn set(&self, id: AttrID, v: Attr) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        self.attrs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(k, v);
        kResultOk
    }
    fn with<R>(&self, id: AttrID, f: impl FnOnce(&Attr) -> Option<R>) -> Option<R> {
        let k = attr_key(id)?;
        let attrs = self.attrs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        attrs.get(&k).and_then(f)
    }
}

impl IAttributeListTrait for HostAttributeList {
    unsafe fn setInt(&self, id: AttrID, value: int64) -> tresult {
        self.set(id, Attr::Int(value))
    }
    unsafe fn getInt(&self, id: AttrID, value: *mut int64) -> tresult {
        // SAFETY: out-pointer from the plugin, or null (handled).
        let Some(out) = (unsafe { value.as_mut() }) else {
            return kInvalidArgument;
        };
        self.with(id, |a| if let Attr::Int(v) = a { Some(*v) } else { None })
            .map_or(kResultFalse, |v| {
                *out = v;
                kResultOk
            })
    }
    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Attr::Float(value))
    }
    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        // SAFETY: out-pointer from the plugin, or null (handled).
        let Some(out) = (unsafe { value.as_mut() }) else {
            return kInvalidArgument;
        };
        self.with(id, |a| if let Attr::Float(v) = a { Some(*v) } else { None })
            .map_or(kResultFalse, |v| {
                *out = v;
                kResultOk
            })
    }
    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut s = Vec::new();
        // SAFETY: the plugin passes a NUL-terminated UTF-16 string; bounded for safety.
        for i in 0..(1 << 20) {
            let c = unsafe { *string.add(i) };
            s.push(c);
            if c == 0 {
                break;
            }
        }
        self.set(id, Attr::Str(s))
    }
    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let cap = usize::try_from(size_in_bytes / 2).unwrap_or(0);
        match self.with(id, |a| if let Attr::Str(v) = a { Some(v.clone()) } else { None }) {
            Some(v) if cap > 0 => {
                // SAFETY: the plugin passes a writable buffer of `size_in_bytes` bytes.
                let dst = unsafe { std::slice::from_raw_parts_mut(string, cap) };
                // Copy what fits, then NUL-terminate on the last slot if the string was cut.
                let n = dst.iter_mut().zip(&v).map(|(d, &c)| *d = c).count();
                if let Some(last) = dst.get_mut(n.min(cap.saturating_sub(1))) {
                    *last = 0;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let bytes = if size_in_bytes == 0 || data.is_null() {
            Vec::new()
        } else {
            let Ok(len) = usize::try_from(size_in_bytes) else {
                return kInvalidArgument;
            };
            // SAFETY: the plugin passes `size_in_bytes` readable bytes.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) }.to_vec()
        };
        self.set(id, Attr::Bin(bytes))
    }
    unsafe fn getBinary(&self, id: AttrID, data: *mut *const c_void, size_in_bytes: *mut uint32) -> tresult {
        // SAFETY: out-pointers from the plugin, or null (handled).
        let (Some(d), Some(n)) = (unsafe { (data.as_mut(), size_in_bytes.as_mut()) }) else {
            return kInvalidArgument;
        };
        // The pointer stays valid while the list lives and the attribute is not overwritten,
        // which is the SDK's contract too (the Vec's heap buffer does not move with the map).
        match self.with(id, |a| {
            if let Attr::Bin(v) = a {
                Some((v.as_ptr(), uint32::try_from(v.len()).ok()?))
            } else {
                None
            }
        }) {
            Some((p, len)) => {
                *d = p.cast();
                *n = len;
                kResultOk
            }
            None => kResultFalse,
        }
    }
}

pub struct HostMessage {
    id: Mutex<CString>,
    attrs: ComWrapper<HostAttributeList>,
}

impl Default for HostMessage {
    fn default() -> Self {
        Self {
            id: Mutex::new(CString::default()),
            attrs: ComWrapper::new(HostAttributeList::default()),
        }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        // The CString's buffer stays put until the next `setMessageID`, as the SDK's does.
        self.id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ptr()
            .cast::<c_char>()
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        let s = if id.is_null() {
            CString::default()
        } else {
            // SAFETY: the plugin passes a NUL-terminated id.
            unsafe { CStr::from_ptr(id) }.to_owned()
        };
        *self.id.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = s;
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // No added reference: the SDK's HostMessage returns its list the same way.
        self.attrs
            .as_com_ref::<IAttributeList>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}
