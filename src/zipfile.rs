//! A minimal zip reader for DST's data bundles: the central directory is
//! read in one go and each member with one read, which keeps file access
//! cheap where every read is expensive (the web build reads the user's files
//! through the browser).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
struct Entry {
    local_header: u64,
    compressed: u64,
    size: u64,
    method: u16,
}

pub struct Zip {
    file: File,
    entries: HashMap<String, Entry>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn read_at(file: &mut File, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

impl Zip {
    pub fn open(path: &Path) -> Result<Zip, String> {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let mut file = File::open(path).map_err(|e| err(&e))?;
        let len = file.metadata().map_err(|e| err(&e))?.len();
        // End of central directory: 22 bytes plus a comment of up to 64 KiB.
        let tail_len = len.min(22 + 65535);
        let tail = read_at(&mut file, len - tail_len, tail_len as usize).map_err(|e| err(&e))?;
        let eocd = (0..=tail.len().saturating_sub(22))
            .rev()
            .find(|&i| u32_at(&tail, i) == 0x0605_4b50)
            .ok_or_else(|| err(&"not a zip file"))?;
        let count = u16_at(&tail, eocd + 10) as usize;
        let cd_size = u32_at(&tail, eocd + 12) as u64;
        let cd_offset = u32_at(&tail, eocd + 16) as u64;
        if cd_offset == 0xffff_ffff || count == 0xffff {
            return Err(err(&"zip64 archives are not supported"));
        }
        let cd = read_at(&mut file, cd_offset, cd_size as usize).map_err(|e| err(&e))?;
        let mut entries = HashMap::with_capacity(count);
        let mut i = 0;
        while i + 46 <= cd.len() && u32_at(&cd, i) == 0x0201_4b50 {
            let name_len = u16_at(&cd, i + 28) as usize;
            let extra_len = u16_at(&cd, i + 30) as usize;
            let comment_len = u16_at(&cd, i + 32) as usize;
            let name = String::from_utf8_lossy(&cd[i + 46..i + 46 + name_len]).replace('\\', "/");
            entries.insert(name, Entry {
                method: u16_at(&cd, i + 10),
                compressed: u32_at(&cd, i + 20) as u64,
                size: u32_at(&cd, i + 24) as u64,
                local_header: u32_at(&cd, i + 42) as u64,
            });
            i += 46 + name_len + extra_len + comment_len;
        }
        Ok(Zip { file, entries })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// The decompressed contents of a member (stored or deflated).
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let e = *self.entries.get(name)?;
        let header = read_at(&mut self.file, e.local_header, 30).ok()?;
        if u32_at(&header, 0) != 0x0403_4b50 {
            return None;
        }
        let data_start = e.local_header + 30 + u16_at(&header, 26) as u64 + u16_at(&header, 28) as u64;
        let data = read_at(&mut self.file, data_start, e.compressed as usize).ok()?;
        let out = match e.method {
            0 => data,
            8 => miniz_oxide::inflate::decompress_to_vec(&data).ok()?,
            _ => return None,
        };
        (out.len() as u64 == e.size).then_some(out)
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// An uncompressed (stored) zip of the given members, readable by [`Zip`].
pub fn stored_zip(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in members {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes()); // version made by
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // version needed
            h.extend_from_slice(&0u16.to_le_bytes()); // flags
            h.extend_from_slice(&0u16.to_le_bytes()); // method: stored
            h.extend_from_slice(&0u32.to_le_bytes()); // time, date
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra length
            if central {
                h.extend_from_slice(&[0; 6]); // comment length, disk, internal attributes
                h.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend_from_slice(&header(0x0403_4b50, false));
        out.extend_from_slice(data);
        central.extend_from_slice(&header(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // disk numbers
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_zip_round_trip() {
        let members = vec![("a/b.lua".to_string(), b"print(1)".to_vec()), ("empty".to_string(), Vec::new())];
        let path = std::env::temp_dir().join(format!("dsw-zip-test-{}.zip", std::process::id()));
        std::fs::write(&path, stored_zip(&members)).unwrap();
        let mut z = Zip::open(&path).unwrap();
        assert_eq!(z.read("a/b.lua").unwrap(), b"print(1)");
        assert_eq!(z.read("empty").unwrap(), b"");
        assert!(!z.contains("missing"));
        let _ = std::fs::remove_file(&path);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
