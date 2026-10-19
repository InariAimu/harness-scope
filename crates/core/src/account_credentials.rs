//! Device-local secrets, separate from usage facts and the synchronization database.
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct Vault(PathBuf);
impl Vault {
    pub fn new(database: &Path) -> Self {
        Self(database.with_extension("provider-keys"))
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(id).context("Invalid provider account ID")?;
        Ok(self.0.join(id))
    }
    pub fn exists(&self, id: &str) -> bool {
        self.path(id).is_ok_and(|p| p.is_file())
    }
    pub fn raw(&self, id: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(self.path(id)?) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => anyhow::bail!("Cannot read the local credential store"),
        }
    }
    pub fn read(&self, id: &str) -> Result<String> {
        let data = self.raw(id)?.context("API key is not configured")?;
        String::from_utf8(unprotect(&data)?).context("Cannot decode the local credential")
    }
    pub fn write(&self, id: &str, key: &str) -> Result<()> {
        self.restore(id, Some(&protect(key.as_bytes())?))
    }
    pub fn restore(&self, id: &str, data: Option<&[u8]>) -> Result<()> {
        let path = self.path(id)?;
        let Some(data) = data else {
            return match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => anyhow::bail!("Cannot remove the local credential"),
            };
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.0)?;
        }
        #[cfg(not(unix))]
        fs::create_dir_all(&self.0)?;
        let temporary = self.0.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            use std::io::Write;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(data)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result.context("Cannot update the local credential store")
    }
}

#[cfg(windows)]
fn crypt(data: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    use std::ptr::{null, null_mut};
    #[repr(C)]
    struct Blob {
        size: u32,
        data: *mut u8,
    }
    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *mut std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *mut std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    anyhow::ensure!(data.len() <= 65536, "Invalid local credential size");
    let input = Blob {
        size: data.len() as u32,
        data: data.as_ptr() as *mut u8,
    };
    let mut output = Blob {
        size: 0,
        data: null_mut(),
    };
    // DPAPI owns the output allocation. UI is forbidden, including in a background refresh.
    unsafe {
        let ok = if decrypt {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null_mut(),
                null_mut(),
                1,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                null(),
                null(),
                null_mut(),
                null_mut(),
                1,
                &mut output,
            )
        };
        anyhow::ensure!(
            ok != 0,
            "Windows could not unlock the credential for this user"
        );
        let bytes = std::slice::from_raw_parts(output.data, output.size as usize).to_vec();
        LocalFree(output.data.cast());
        Ok(bytes)
    }
}
#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    crypt(data, false)
}
#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    crypt(data, true)
}
// Unix uses an owner-only directory and files; secrets never enter the database or UI.
#[cfg(not(windows))]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}
#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}
