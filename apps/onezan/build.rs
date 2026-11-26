//! Embeds the built-in piano (a SoundFont) when `build/fetch-piano.sh` has put it in
//! `assets/piano/`; without it the program builds, but has no built-in piano.
//! On Windows, also gives the exe its icon (a resource, compiled with MinGW's `windres`).

use std::path::Path;
use std::process::Command;

fn main() {
    piano();
    if let Err(e) = windows_icon() {
        println!("cargo::error=the exe's icon: {e}");
    }
}

fn piano() {
    let sf2 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/piano/UprightPianoKW-20220221.sf2");
    println!("cargo::rustc-check-cfg=cfg(onezan_piano)");
    // The file, and its folder: a file put back keeps its old time, the folder's changes.
    println!("cargo::rerun-if-changed={}", sf2.display());
    if let Some(dir) = sf2.parent() {
        println!("cargo::rerun-if-changed={}", dir.display());
    }
    if sf2.is_file() {
        println!("cargo::rustc-cfg=onezan_piano");
        println!("cargo::rustc-env=ONEZAN_PIANO_SF2={}", sf2.display());
    } else {
        println!(
            "cargo::warning=no built-in piano: {} is missing (run build/fetch-piano.sh)",
            sf2.display()
        );
    }
}

/// The icon Explorer and the task bar show for the exe. Only for the MinGW target, the one the
/// Windows builds use (`build/windows-release.sh`).
fn windows_icon() -> Result<(), String> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "gnu" {
        return Ok(());
    }
    let ico = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon/onezan.ico");
    println!("cargo::rerun-if-changed={}", ico.display());
    println!("cargo::rerun-if-env-changed=WINDRES");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {e}"))?);
    let rc = out.join("onezan.rc");
    // windres reads the path as a C string: forward slashes need no escaping.
    let ico_path = ico.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{ico_path}\"\n"))
        .map_err(|e| format!("cannot write {}: {e}", rc.display()))?;
    let obj = out.join("onezan-icon.o");
    // On Linux the cross tool has the target's prefix; MSYS2 on Windows calls it plain windres.
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| {
        let cross = "x86_64-w64-mingw32-windres";
        if Command::new(cross).arg("--version").output().is_ok() {
            cross.into()
        } else {
            "windres".into()
        }
    });
    let status = Command::new(&windres)
        .arg(&rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .status()
        .map_err(|e| format!("cannot run {windres} (MinGW's resource compiler): {e}"))?;
    if !status.success() {
        return Err(format!("{windres} failed on {} ({status})", rc.display()));
    }
    // An object file on the link line: its resource section goes into the exe.
    println!("cargo::rustc-link-arg-bins={}", obj.display());
    Ok(())
}
