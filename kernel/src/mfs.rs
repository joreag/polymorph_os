use crate::nvme::NVME_DRIVE;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::str;

pub struct MictFileSystem;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub start_lba: u64,
    pub block_count: u64,
}

impl MictFileSystem {
    /// Reads and parses the entire Master File Table from LBA 0.
    pub fn read_mft() -> Result<Vec<FileEntry>, &'static str> {
        let mut nvme_lock = NVME_DRIVE.lock();
        let nvme = nvme_lock.as_mut().ok_or("NVMe offline")?;

        let mut mft_buffer = [0u8; 4096];
        nvme.read_block(0, &mut mft_buffer).map_err(|_| "MFT Read Failed")?;

        let mft_str = str::from_utf8(&mft_buffer).unwrap_or("");
        let valid_len = mft_str.find('\0').unwrap_or(mft_str.len());
        let clean_mft = &mft_str[..valid_len];

        let mut entries = Vec::new();
        if clean_mft.starts_with("MFS_V1") {
            for line in clean_mft.lines().skip(1) {
                let mut parts = line.split(':');
                if let (Some(name), Some(lba_str), Some(count_str)) = (parts.next(), parts.next(), parts.next()) {
                    if let (Ok(lba), Ok(count)) = (lba_str.parse(), count_str.parse()) {
                        entries.push(FileEntry { name: name.to_string(), start_lba: lba, block_count: count });
                    }
                }
            }
        }
        Ok(entries)
    }

    /// Takes a list of file entries and writes them back to LBA 0.
    fn write_mft(entries: &Vec<FileEntry>) -> Result<(), &'static str> {
        let mut nvme_lock = NVME_DRIVE.lock();
        let nvme = nvme_lock.as_mut().ok_or("NVMe offline")?;
        
        let mut new_mft_string = String::from("MFS_V1\n");
        for entry in entries {
            new_mft_string.push_str(&alloc::format!("{}:{}:{}\n", entry.name, entry.start_lba, entry.block_count));
        }

        let mut new_mft_buffer = [0u8; 4096];
        let bytes_to_copy = core::cmp::min(new_mft_string.len(), 4096);
        new_mft_buffer[..bytes_to_copy].copy_from_slice(&new_mft_string.as_bytes()[..bytes_to_copy]);

        nvme.write_block(0, &new_mft_buffer).map_err(|_| "MFT Write Failed")
    }

    /// Finds a free block, writes the multi-block payload, and updates the MFT.
    pub fn save_file(filename: &str, data: &[u8]) -> Result<(), &'static str> {
        let mut entries = Self::read_mft()?;
        
        if entries.iter().any(|e| e.name == filename) {
            return Err("File already exists");
        }

        let mut next_lba = 1;
        if let Some(last_entry) = entries.last() {
            next_lba = last_entry.start_lba + last_entry.block_count;
        }

        let blocks_needed = (data.len() as u64 + 4095) / 4096;

        { // Scoped lock for writing
            let mut nvme_lock = NVME_DRIVE.lock();
            let nvme = nvme_lock.as_mut().ok_or("NVMe offline")?;

            for i in 0..blocks_needed {
                let offset = i as usize * 4096;
                let chunk_end = core::cmp::min(offset + 4096, data.len());
                let chunk = &data[offset..chunk_end];
                nvme.write_block(next_lba + i, chunk).map_err(|_| "Data block write failed")?;
            }
        }

        entries.push(FileEntry { name: filename.to_string(), start_lba: next_lba, block_count: blocks_needed });
        Self::write_mft(&entries)
    }

    /// Locates the file in the MFT, then fetches all its data blocks.
        pub fn find_file(target_filename: &str) -> Option<FileEntry> {
        // We need to lock the drive for this operation
        let mut nvme_lock = NVME_DRIVE.lock();
        let nvme = nvme_lock.as_mut()?; 

        let mut mft_buffer = [0u8; 4096];
        if nvme.read_block(0, &mut mft_buffer).is_err() {
            return None;
        }

        let mft_str = str::from_utf8(&mft_buffer).unwrap_or("");
        let valid_len = mft_str.find('\0').unwrap_or(mft_str.len());
        let clean_mft = &mft_str[..valid_len];

        for line in clean_mft.lines().skip(1) { // Skip "MFS_V1" header
            let mut parts = line.split(':');
            if let (Some(name), Some(lba_str), Some(count_str)) = (parts.next(), parts.next(), parts.next()) {
                //[MICT: CASE-INSENSITIVE FIX]
                if name.trim().eq_ignore_ascii_case(target_filename.trim()) {
                    if let (Ok(lba), Ok(count)) = (lba_str.parse(), count_str.parse()) {
                        return Some(FileEntry { name: name.to_string(), start_lba: lba, block_count: count });
                    }
                }
            }
        }
        None
    }

    /// Locates the file in the MFT, then fetches all its data blocks.
    pub fn read_file(filename: &str) -> Result<Vec<u8>, &'static str> {
        if let Some(entry) = Self::find_file(filename) {
            
            let mut file_data = Vec::with_capacity((entry.block_count * 4096) as usize);
            let mut nvme_lock = NVME_DRIVE.lock();
            let nvme = nvme_lock.as_mut().ok_or("NVMe offline")?;

            for i in 0..entry.block_count {
                let mut buffer = [0u8; 4096];
                nvme.read_block(entry.start_lba + i, &mut buffer).map_err(|_| "Hardware read failed")?;
                file_data.extend_from_slice(&buffer);
            }
            
            Ok(file_data)
        } else {
            Err("File not found")
        }
    }
}