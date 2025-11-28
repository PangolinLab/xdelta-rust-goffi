// src/lib.rs
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use thiserror::Error;
use std::cell::RefCell;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

fn set_last_error(err: &str) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = Some(CString::new(err).unwrap_or_else(|_| CString::new("internal error").unwrap()));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn xdelta_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

#[derive(Error, Debug)]
enum XDeltaError {
    #[error("invalid argument: {0}")]
    InvalidArg(String),
}

/// A simple rsync-style rolling checksum (a,b) described in rsync tech report.
/// Weak checksum is (b << 16) | a (u32).
#[derive(Clone, Copy, Debug)]
struct Rolling {
    a: u32,
    b: u32,
    len: usize,
}
impl Rolling {
    fn new() -> Self {
        Rolling { a: 0, b: 0, len: 0 }
    }

    fn from_slice(buf: &[u8]) -> Self {
        let mut a: u32 = 0;
        let mut b: u32 = 0;
        for (i, &v) in buf.iter().enumerate() {
            a = a.wrapping_add(v as u32);
            b = b.wrapping_add((buf.len() - i) as u32 * v as u32);
        }
        Rolling {
            a,
            b,
            len: buf.len(),
        }
    }

    /// roll window: remove `prev` byte, add `next` byte
    fn roll(&mut self, prev: u8, next: u8) {
        let len = self.len as u32;
        // based on rsync-style weak checksum updates
        self.a = self.a.wrapping_sub(prev as u32).wrapping_add(next as u32);
        self.b = self.b.wrapping_sub((len) * (prev as u32)).wrapping_add(self.a);
    }

    fn chksum(&self) -> u32 {
        ((self.b & 0xffff_ffff) << 16) ^ (self.a & 0xffff)
    }
}

/// Block signature entry
struct SigEntry {
    block_index: u64,
    strong_hash: [u8; 32], // sha256
}

/// Build signatures for the "old" file
fn build_signatures(old: &[u8], block_size: usize) -> HashMap<u32, Vec<SigEntry>> {
    let mut map: HashMap<u32, Vec<SigEntry>> = HashMap::new();
    let mut idx: u64 = 0;
    let mut offset = 0usize;
    while offset < old.len() {
        let end = usize::min(offset + block_size, old.len());
        let slice = &old[offset..end];
        let weak = Rolling::from_slice(slice).chksum();
        let mut hasher = Sha256::new();
        hasher.update(slice);
        let strong = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&strong);
        map.entry(weak).or_default().push(SigEntry {
            block_index: idx,
            strong_hash: arr,
        });
        idx += 1;
        offset += block_size;
    }
    map
}

/// Patch format (simple custom):
/// [records...] where each record is:
/// opcode: u8 (0x00 = ADD, 0x01 = COPY)
/// If ADD:
///   length: u32 (little-endian)
///   data: [length] bytes
/// If COPY:
///   offset: u64 (little-endian)  // offset in old file
///   length: u32 (little-endian)
///
/// This is simple, versionable, and easy to apply.
fn create_patch_bytes(old: &[u8], new: &[u8], block_size: usize) -> Result<Vec<u8>, XDeltaError> {
    if block_size == 0 {
        return Err(XDeltaError::InvalidArg("block_size must be > 0".into()));
    }
    let sigs = build_signatures(old, block_size);

    let mut out: Vec<u8> = Vec::with_capacity(new.len() / 4);
    let mut pos: usize = 0;
    let mut pending_add: Vec<u8> = Vec::new();

    // helper to flush pending adds
    let flush_add = |out: &mut Vec<u8>, pending: &mut Vec<u8>| {
        if !pending.is_empty() {
            out.push(0x00); // ADD
            let len = pending.len() as u32;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&pending[..]);
            pending.clear();
        }
    };

    while pos < new.len() {
        let remaining = new.len() - pos;
        let try_len = usize::min(block_size, remaining);
        if try_len < 1 {
            // shouldn't happen, but safety
            pending_add.push(new[pos]);
            pos += 1;
            continue;
        }

        if pos + try_len <= new.len() {
            let window = &new[pos..pos + try_len];
            let weak = Rolling::from_slice(window).chksum();
            let candidates = sigs.get(&weak);
            let mut matched = false;
            if let Some(vec) = candidates {
                // Compute strong for this window and compare
                let mut hasher = Sha256::new();
                hasher.update(window);
                let strong = hasher.finalize();

                for e in vec {
                    if e.strong_hash[..] == strong[..] {
                        // Found a match. Flush any pending adds.
                        flush_add(&mut out, &mut pending_add);
                        out.push(0x01); // COPY
                        let offset_in_old: u64 = e.block_index * (block_size as u64);
                        out.extend_from_slice(&offset_in_old.to_le_bytes());
                        let copy_len = try_len as u32;
                        out.extend_from_slice(&copy_len.to_le_bytes());
                        pos += try_len;
                        matched = true;
                        break;
                    }
                }
            }

            if !matched {
                // sliding by 1 byte: add first byte to pending_add and continue
                pending_add.push(new[pos]);
                pos += 1;
                // To avoid pathological O(n^2) behavior for huge pending_add, flush periodically:
                if pending_add.len() >= block_size {
                    flush_add(&mut out, &mut pending_add);
                }
            }
        } else {
            // leftover bytes less than try_len (end of file)
            pending_add.extend_from_slice(&new[pos..]);
            break;
        }
    }

    // flush remaining adds
    if !pending_add.is_empty() {
        out.push(0x00);
        let len = pending_add.len() as u32;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&pending_add[..]);
    }

    Ok(out)
}

/// Apply the simple patch format to `old` -> produces reconstructed `new`.
fn apply_patch_bytes(old: &[u8], patch: &[u8]) -> Result<Vec<u8>, XDeltaError> {
    let mut pos = 0usize;
    let mut out: Vec<u8> = Vec::new();
    while pos < patch.len() {
        let opcode = patch[pos];
        pos += 1;
        match opcode {
            0x00 => {
                if pos + 4 > patch.len() {
                    return Err(XDeltaError::InvalidArg("truncated ADD length".into()));
                }
                let mut lenb = [0u8; 4];
                lenb.copy_from_slice(&patch[pos..pos + 4]);
                pos += 4;
                let len = u32::from_le_bytes(lenb) as usize;
                if pos + len > patch.len() {
                    return Err(XDeltaError::InvalidArg("truncated ADD data".into()));
                }
                out.extend_from_slice(&patch[pos..pos + len]);
                pos += len;
            }
            0x01 => {
                if pos + 8 + 4 > patch.len() {
                    return Err(XDeltaError::InvalidArg("truncated COPY entry".into()));
                }
                let mut offb = [0u8; 8];
                offb.copy_from_slice(&patch[pos..pos + 8]);
                pos += 8;
                let offset = u64::from_le_bytes(offb) as usize;
                let mut lenb = [0u8; 4];
                lenb.copy_from_slice(&patch[pos..pos + 4]);
                pos += 4;
                let len = u32::from_le_bytes(lenb) as usize;
                if offset + len > old.len() {
                    return Err(XDeltaError::InvalidArg("COPY out of range".into()));
                }
                out.extend_from_slice(&old[offset..offset + len]);
            }
            other => {
                return Err(XDeltaError::InvalidArg(format!("unknown opcode {:#x}", other)));
            }
        }
    }
    Ok(out)
}

/// 创建补丁数据（内存版本）
/// 成功时返回0，失败返回-1
#[unsafe(no_mangle)]
pub extern "C" fn xdelta_create_patch_data(
    old_data: *const u8,
    old_len: usize,
    new_data: *const u8,
    new_len: usize,
    patch_data: *mut *mut u8,
    patch_len: *mut usize,
    block_size: u32,
) -> c_int {
    let r = (|| -> Result<Vec<u8>, XDeltaError> {
        if old_data.is_null() || new_data.is_null() || patch_data.is_null() || patch_len.is_null() {
            return Err(XDeltaError::InvalidArg("null pointer".into()));
        }

        let old_bytes = unsafe { std::slice::from_raw_parts(old_data, old_len) };
        let new_bytes = unsafe { std::slice::from_raw_parts(new_data, new_len) };

        create_patch_bytes(old_bytes, new_bytes, block_size as usize)
    })();

    match r {
        Ok(data) => {
            unsafe {
                *patch_len = data.len();
                *patch_data = libc::malloc(data.len()) as *mut u8;
                if (*patch_data).is_null() {
                    set_last_error("failed to allocate memory");
                    return -1;
                }
                std::ptr::copy_nonoverlapping(data.as_ptr(), *patch_data, data.len());
            }
            0
        },
        Err(e) => {
            set_last_error(&format!("{}", e));
            -1
        }
    }
}

/// 应用补丁数据（内存版本）
/// 成功时返回0，失败返回-1
#[unsafe(no_mangle)]
pub extern "C" fn xdelta_apply_patch_data(
    old_data: *const u8,
    old_len: usize,
    patch_data: *const u8,
    patch_len: usize,
    new_data: *mut *mut u8,
    new_len: *mut usize,
) -> c_int {
    let r = (|| -> Result<Vec<u8>, XDeltaError> {
        if old_data.is_null() || patch_data.is_null() || new_data.is_null() || new_len.is_null() {
            return Err(XDeltaError::InvalidArg("null pointer".into()));
        }

        let old_bytes = unsafe { std::slice::from_raw_parts(old_data, old_len) };
        let patch_bytes = unsafe { std::slice::from_raw_parts(patch_data, patch_len) };

        apply_patch_bytes(old_bytes, patch_bytes)
    })();

    match r {
        Ok(data) => {
            unsafe {
                *new_len = data.len();
                *new_data = libc::malloc(data.len()) as *mut u8;
                if (*new_data).is_null() {
                    set_last_error("failed to allocate memory");
                    return -1;
                }
                std::ptr::copy_nonoverlapping(data.as_ptr(), *new_data, data.len());
            }
            0
        },
        Err(e) => {
            set_last_error(&format!("{}", e));
            -1
        }
    }
}

/// 释放通过xdelta_create_patch_data或xdelta_apply_patch_data分配的内存
#[unsafe(no_mangle)]
pub extern "C" fn xdelta_free_data(data: *mut u8) {
    if !data.is_null() {
        unsafe {
            libc::free(data as *mut libc::c_void);
        }
    }
}
