use anyhow::{anyhow, Result};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WofAlgo {
    Lzx,
    Xpress8k,
}

impl WofAlgo {
    pub fn compact_flag(&self) -> &'static str {
        match self {
            WofAlgo::Lzx => "/EXE:LZX",
            WofAlgo::Xpress8k => "/EXE:XPRESS8K",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            WofAlgo::Lzx => "LZX",
            WofAlgo::Xpress8k => "XPRESS8K",
        }
    }
}

/// Returns the true physical bytes occupied on disk (NTFS compressed size on Windows).
pub fn get_disk_size(path: &Path) -> u64 {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{GetCompressedFileSizeW, INVALID_FILE_SIZE};

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut high: u32 = 0;
        let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
        if low == INVALID_FILE_SIZE {
            return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        }
        ((high as u64) << 32) | (low as u64)
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

/// Atomically replaces `duplicate_path` with an NTFS hardlink pointing to `canonical_path`.
/// Uses a temporary backup rename so a failure can never lose file data.
pub fn replace_with_hardlink(duplicate_path: &Path, canonical_path: &Path) -> Result<()> {
    if duplicate_path == canonical_path {
        return Ok(());
    }

    let backup_path = duplicate_path.with_extension("aethercrush_hl_bak");
    std::fs::rename(duplicate_path, &backup_path)?;

    match std::fs::hard_link(canonical_path, duplicate_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::rename(&backup_path, duplicate_path);
            Err(anyhow!("hard_link failed: {}", e))
        }
    }
}

/// Breaks an NTFS hardlink by cloning the file data to a fresh independent inode.
pub fn break_hardlink(path: &Path) -> Result<()> {
    let tmp = path.with_extension("aethercrush_unhl_tmp");
    std::fs::copy(path, &tmp)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Applies Windows NTFS WOF compression (`LZX` or `XPRESS8K`) transparently so `game.exe` runs directly.
pub fn compress_file_wof(path: &Path, algo: WofAlgo) -> Result<u64> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let status = Command::new("compact.exe")
            .arg("/C")
            .arg(algo.compact_flag())
            .arg("/F")
            .arg("/Q")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;

        if !status.success() {
            return Err(anyhow!("compact.exe failed on {}", path.display()));
        }
        Ok(get_disk_size(path))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = algo;
        Ok(get_disk_size(path))
    }
}

/// Decompresses a WOF-compressed file back to raw uncompressed NTFS storage.
pub fn decompress_file_wof(path: &Path) -> Result<u64> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let status = Command::new("compact.exe")
            .arg("/U")
            .arg("/F")
            .arg("/Q")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;

        if !status.success() {
            return Err(anyhow!("compact.exe /U failed on {}", path.display()));
        }
        Ok(get_disk_size(path))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(get_disk_size(path))
    }
}
