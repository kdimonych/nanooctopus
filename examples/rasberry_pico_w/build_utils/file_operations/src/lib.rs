#![cfg_attr(target_os = "none", no_std)]

use build_log as log;
use cargo_command as cargo;

#[cfg(not(target_os = "none"))]
use std::env;
#[cfg(not(target_os = "none"))]
use std::path::PathBuf;

#[cfg(not(target_os = "none"))]
pub fn copy_file(src: &str, dst: &str) -> Result<(), ()> {
    std::fs::copy(src, dst).map_err(|e| {
        log::error!("Failed to copy file from {} to {}. Error: {}", src, dst, e);
        ()
    })?;
    Ok(())
}

#[cfg(not(target_os = "none"))]
pub fn copy_memory_x() -> Result<(), ()> {
    let out_dir: std::ffi::OsString = env::var_os("OUT_DIR").unwrap();
    log::info!(
        "The memory.x was copied to the: {} directory",
        out_dir.to_string_lossy()
    );
    let out = &PathBuf::from(&out_dir);
    copy_file("memory.x", &out.join("memory.x").to_string_lossy())?;
    cargo::cmd!("rustc-link-search={}", out.display());
    cargo::cmd!("rerun-if-changed=memory.x");
    Ok(())
}

#[cfg(target_os = "none")]
pub fn copy_file(_src: &str, _dst: &str) -> Result<(), ()> {
    Err(())
}

#[cfg(target_os = "none")]
pub fn copy_memory_x() -> Result<(), ()> {
    Err(())
}
