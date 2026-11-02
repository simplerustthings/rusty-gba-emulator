use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
};

use crate::BackupFile;
use std::path::Path;
/// Create a filesystem-backed backup file.
///
/// # Panics
///
/// This will panic if the file can't be created (or opened if it already exists).
pub fn make_backup_file(path: String) -> Box<dyn BackupFile> {
    /// File-backed BackupFile.
    struct DiskBackup {
        path: String,
        file: Option<fs::File>,
    }

    impl BackupFile for DiskBackup {
        fn read(&mut self, offset: usize, buffer: &mut [u8]) {
            if !buffer.is_empty() {
                let file = self
                    .file
                    .as_mut()
                    .expect("Failed to read from non-existant file");
                file.seek(SeekFrom::Start(offset as u64)).unwrap();
                file.read_exact(buffer).unwrap();
            }
        }

fn write(&mut self, offset: usize, data: &[u8]) {
            let file = self.file.get_or_insert_with(|| {
                // Lazily create the parent directories if they don't exist.
                if let Some(parent) = Path::new(&self.path).parent()
                    && !parent.as_os_str().is_empty() && !parent.exists() {
                        fs::create_dir_all(parent).unwrap_or_else(|e| {
                            panic!("Failed to create backup directory '{:?}': {}", parent, e)
                        });
                }

                // Lazily create the file.
                fs::File::options()
                    .create(true)
                    .truncate(true)
                    .read(true)
                    .write(true)
                    .open(&self.path)
                    .unwrap_or_else(|e| {
                        // THIS WILL SHOW US THE EXACT PATH CAUSING THE CRASH
                        panic!("Failed to create backup file at EXACT PATH '{}': {}", self.path, e)
                    })
            });

            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            file.write_all(data).unwrap();
        }

        fn size(&self) -> usize {
            self.file
                .as_ref()
                .map_or(0, |mut file| file.seek(SeekFrom::End(0)).unwrap() as usize)
        }
    }

    // Don't create the file immediately, and silently fail if it can't be opened.
    let file = fs::File::options().read(true).write(true).open(&path).ok();
    Box::new(DiskBackup { path, file })
}
