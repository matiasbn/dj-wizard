use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use colored::*;
use error_stack::Report;

use crate::errors::{CleanerError, CleanerResult};

// Represents a file entry with its path and content hash.
struct FileEntry {
    path: PathBuf,
    hash: u64,
}

// Computes the hash of a file's contents.
fn compute_hash(file_path: &Path) -> io::Result<u64> {
    let file_content = fs::read(file_path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&file_content);
    Ok(hasher.finish())
}

// Recursively scans a directory for duplicate files and removes all but one copy.
// Deletes empty folders as well.
fn remove_duplicates_and_empty_folders(
    dir_path: &Path,
    file_map: &mut HashMap<u64, FileEntry>,
) -> io::Result<()> {
    if dir_path.is_dir() {
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let file_path = entry.path();

            if file_type.is_file() {
                if let Ok(hash) = compute_hash(&file_path) {
                    let file_entry = FileEntry {
                        path: file_path.clone(),
                        hash,
                    };
                    if let Some(existing_entry) = file_map.get(&hash) {
                        // Found a duplicate file, remove it.
                        println!(
                            "{} {}",
                            "Removing duplicate file:".red(),
                            existing_entry.path.display()
                        );
                        println!(
                            "{} {}\n",
                            "   - Duplicate:".bright_red(),
                            file_path.display()
                        );
                        fs::remove_file(&file_path)?;
                    } else {
                        // Store the first occurrence of the file.
                        file_map.insert(hash, file_entry);
                    }
                } else {
                    eprintln!(
                        "{} {:?}",
                        "Failed to compute hash for file:".bright_red(),
                        file_path
                    );
                }
            } else if file_type.is_dir() {
                // Recursively search for duplicates in subdirectories.
                remove_duplicates_and_empty_folders(&file_path, file_map)?;
                // Check if the directory is empty after removing duplicates.
                if fs::read_dir(&file_path)?.next().is_none() {
                    println!("{} {:?}", "Deleting empty folder:".yellow(), file_path);
                    fs::remove_dir(&file_path)?;
                }
            }
        }
    }

    Ok(())
}

pub fn clean_repeated_files(start_path: PathBuf) -> CleanerResult<()> {
    let mut file_map: HashMap<u64, FileEntry> = HashMap::new();
    if let Err(err) = remove_duplicates_and_empty_folders(Path::new(&start_path), &mut file_map) {
        eprintln!("An error occurred: {}", err);
        return Err(
            Report::new(CleanerError).attach_printable(format!("An error occurred: {}", err))
        );
    }
    Ok(())
}
