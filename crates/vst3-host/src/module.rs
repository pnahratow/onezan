//! Loading a `.vst3` bundle: the shared library, the platform module entry and the factory.

use crate::util::{ok, read_c8, tuid_bytes};
use crate::{Error, Result};
use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2, PFactoryInfo,
};
use vst3::{ComPtr, Interface};

/// One class exported by a module.
#[derive(Debug, Clone)]
pub struct RawClass {
    pub cid: [u8; 16],
    pub category: String,
    pub name: String,
    pub vendor: String,
    pub sub_categories: String,
}

pub struct Module {
    pub path: PathBuf,
    factory: ComPtr<IPluginFactory>,
}

// SAFETY: the factory is only used from the main (control) thread; the struct is shared through
// the module cache, which needs `Send + Sync` for its `Mutex`.
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

/// Modules are loaded once and never unloaded: plugins commonly misbehave when their library is
/// unmapped and reloaded, so the library handle is leaked and the module exit is never called.
fn cache() -> &'static Mutex<Vec<Arc<Module>>> {
    static CACHE: OnceLock<Mutex<Vec<Arc<Module>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(target_os = "linux")]
const ARCH_FOLDER: &str = if cfg!(target_arch = "aarch64") {
    "aarch64-linux"
} else {
    "x86_64-linux"
};
#[cfg(windows)]
const ARCH_FOLDER: &str = if cfg!(target_arch = "aarch64") {
    "arm64-win"
} else {
    "x86_64-win"
};
#[cfg(target_os = "macos")]
const ARCH_FOLDER: &str = "MacOS";

/// The shared library inside a bundle. A plain file (Windows legacy single-file `.vst3`, or a
/// `.so` given directly) is its own binary.
pub fn bundle_binary(bundle: &Path) -> Option<PathBuf> {
    if bundle.is_file() {
        return Some(bundle.to_path_buf());
    }
    let dir = bundle.join("Contents").join(ARCH_FOLDER);
    let stem = bundle.file_stem()?.to_string_lossy().into_owned();
    let ext = if cfg!(windows) {
        "vst3"
    } else if cfg!(target_os = "macos") {
        ""
    } else {
        "so"
    };
    let named = if ext.is_empty() {
        dir.join(&stem)
    } else {
        dir.join(format!("{stem}.{ext}"))
    };
    if named.is_file() {
        return Some(named);
    }
    // Some bundles name the binary differently from the folder: take the only matching file.
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.is_file() && (ext.is_empty() || p.extension().is_some_and(|e| e == ext)))
}

impl Module {
    // The cache stays locked for the whole load, so two threads cannot load one bundle twice.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the lock is held on purpose until the module is cached"
    )]
    pub fn load(bundle: &Path) -> Result<Arc<Self>> {
        type GetFactory = unsafe extern "system" fn() -> *mut IPluginFactory;
        let key = bundle.canonicalize().unwrap_or_else(|_| bundle.to_path_buf());
        let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(m) = cache.iter().find(|m| m.path == key) {
            return Ok(m.clone());
        }
        let fail = |m: String| Error::Load(key.clone(), m);
        let binary =
            bundle_binary(&key).ok_or_else(|| fail("no plugin binary for this platform in the bundle".into()))?;
        let lib = open_library(&binary).map_err(&fail)?;
        // SAFETY: `GetPluginFactory` is the VST3-mandated export with this signature.
        let get = unsafe { lib.get::<GetFactory>(b"GetPluginFactory\0") }.map_err(|e| fail(e.to_string()))?;
        // SAFETY: called after the module entry, as the SDK requires; returns an owned reference.
        let raw = unsafe { get() };
        // SAFETY: non-null results are a factory pointer with one reference owned by us.
        let factory = unsafe { ComPtr::from_raw(raw) }.ok_or_else(|| fail("GetPluginFactory returned null".into()))?;
        std::mem::forget(lib);
        let module = Arc::new(Self { path: key, factory });
        cache.push(module.clone());
        Ok(module)
    }

    fn factory_vendor(&self) -> String {
        // SAFETY: plain data; all-zero is valid.
        let mut info: PFactoryInfo = unsafe { std::mem::zeroed() };
        // SAFETY: valid factory, writable struct.
        if ok(unsafe { self.factory.getFactoryInfo(&raw mut info) }) {
            read_c8(&info.vendor)
        } else {
            String::new()
        }
    }

    pub fn classes(&self) -> Vec<RawClass> {
        // SAFETY: valid factory.
        let n = unsafe { self.factory.countClasses() }.clamp(0, 1024);
        let f2 = self.factory.cast::<IPluginFactory2>();
        let vendor = self.factory_vendor();
        let mut out = Vec::new();
        for i in 0..n {
            if let Some(f2) = &f2 {
                // SAFETY: plain data; all-zero is valid.
                let mut c: PClassInfo2 = unsafe { std::mem::zeroed() };
                // SAFETY: index below the count; `c` is writable.
                if ok(unsafe { f2.getClassInfo2(i, &raw mut c) }) {
                    let v = read_c8(&c.vendor);
                    out.push(RawClass {
                        cid: tuid_bytes(&c.cid),
                        category: read_c8(&c.category),
                        name: read_c8(&c.name),
                        vendor: if v.is_empty() { vendor.clone() } else { v },
                        sub_categories: read_c8(&c.subCategories),
                    });
                    continue;
                }
            }
            // SAFETY: plain data; all-zero is valid.
            let mut c: PClassInfo = unsafe { std::mem::zeroed() };
            // SAFETY: index below the count; `c` is writable.
            if ok(unsafe { self.factory.getClassInfo(i, &raw mut c) }) {
                out.push(RawClass {
                    cid: tuid_bytes(&c.cid),
                    category: read_c8(&c.category),
                    name: read_c8(&c.name),
                    vendor: vendor.clone(),
                    sub_categories: String::new(),
                });
            }
        }
        out
    }

    /// Creates an object of class `cid` and queries interface `I` on it.
    pub fn create<I: Interface>(&self, cid: &[u8; 16]) -> Option<ComPtr<I>> {
        let mut obj: *mut c_void = std::ptr::null_mut();
        let iid = I::IID;
        // SAFETY: valid factory; both ids are 16 readable bytes; `obj` is writable.
        let r = unsafe {
            self.factory.createInstance(
                cid.as_ptr().cast::<c_char>(),
                iid.as_ptr().cast::<c_char>(),
                &raw mut obj,
            )
        };
        if !ok(r) {
            return None;
        }
        // SAFETY: on success `obj` is an `I` pointer with one reference owned by us.
        unsafe { ComPtr::from_raw(obj.cast::<I>()) }
    }
}

/// Opens the library and runs the platform module entry (`ModuleEntry` on Linux, `InitDll` on
/// Windows). macOS (`bundleEntry`) is not supported yet.
fn open_library(binary: &Path) -> std::result::Result<libloading::Library, String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
        type ModuleEntry = unsafe extern "C" fn(*mut c_void) -> bool;
        // SAFETY: loading a library runs its initialisers; inherent to hosting plugins.
        let lib = unsafe { Library::open(Some(binary), RTLD_NOW | RTLD_LOCAL) }.map_err(|e| e.to_string())?;
        // SAFETY: `ModuleEntry` is the VST3-mandated Linux entry with this signature.
        let entry = unsafe { lib.get::<ModuleEntry>(b"ModuleEntry\0") }.ok().map(|s| *s);
        let raw = lib.into_raw();
        // SAFETY: `raw` is the handle just taken out of `lib`.
        let lib = unsafe { Library::from_raw(raw) };
        // SAFETY: called once, with the module's own `dlopen` handle, as the spec requires.
        if let Some(f) = entry
            && !unsafe { f(raw) }
        {
            return Err("ModuleEntry failed".into());
        }
        Ok(lib.into())
    }
    #[cfg(windows)]
    {
        type InitDll = unsafe extern "system" fn() -> bool;
        // SAFETY: loading a library runs its initialisers; inherent to hosting plugins.
        let lib = unsafe { libloading::Library::new(binary) }.map_err(|e| e.to_string())?;
        // SAFETY: `InitDll` is the VST3-mandated Windows entry with this signature.
        if let Ok(f) = unsafe { lib.get::<InitDll>(b"InitDll\0") }
            && !unsafe { f() }
        {
            return Err("InitDll failed".into());
        }
        Ok(lib)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = binary;
        Err("macOS is not supported yet".into())
    }
}
