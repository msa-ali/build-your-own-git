use crate::git::object;
use hex;
use sha1_smol::Sha1;
use std::env;
use std::fs;
use std::fs::FileType;
use std::io::{self, Write};
use std::path::Path;

pub fn run() -> io::Result<()> {
    let current_directory = env::current_dir()?;

    let hash = write_tree(&current_directory)?;

    io::stdout().write_all(hash.as_bytes())?;
    io::stdout().flush()?;

    Ok(())
}

#[derive(Debug)]
struct TreeEntry {
    mode: &'static str,
    name: String,
    hash: String,
}

fn write_tree(directory: &Path) -> io::Result<String> {
    let mut entries: Vec<TreeEntry> = Vec::new();

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();

        let name = entry.file_name().to_string_lossy().into_owned();

        if name == ".git" {
            continue;
        }

        let file_type = entry.file_type()?;
        if !file_type.is_dir() && !file_type.is_symlink() && !file_type.is_file() {
            continue;
        }
        let mode = get_mode_for_file(&file_type)?;

        let hash = if file_type.is_dir() {
            write_tree(&path)?
        } else {
            // println!("Creating file hash for {:?}", path);
            object::create_file_hash(&path.to_string_lossy(), true).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Error creating file hash: {:?}", e),
                )
            })?
        };

        // println!("Hash created for {:?}", path);

        entries.push(TreeEntry { mode, name, hash });
    }

    // sort the map by name
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let mut content = Vec::new();
    for entry in &entries {
        content.extend_from_slice(format!("{} {}\0", entry.mode, entry.name).as_bytes());

        let hash_bytes = hex::decode(&entry.hash).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Error decoding hash: {:?}", e),
            )
        })?;
        if hash_bytes.len() != 20 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Hash must be 20 bytes, got {}", hash_bytes.len()),
            ));
        }
        content.extend_from_slice(&hash_bytes);
    }

    let header = format!("tree {}\0", content.len());
    let header_bytes = header.as_bytes();

    let mut store = Vec::with_capacity(header_bytes.len() + content.len());
    store.extend_from_slice(header_bytes);
    store.extend_from_slice(&content);

    // Compute SHA-1 hash
    let mut hasher = Sha1::new();
    hasher.update(&store);
    let tree_hash = hasher.digest().to_string();

    object::write_blob(&store, &tree_hash).map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("Error writing tree: {:?}", e))
    })?;

    Ok(tree_hash)
}

fn get_mode_for_file(file_type: &FileType) -> io::Result<&'static str> {
    if file_type.is_dir() {
        Ok("040000")
    } else if file_type.is_symlink() {
        Ok("120000")
    } else if file_type.is_file() {
        // Not handling executable files
        Ok("100644")
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Unsupported file type",
        ))
    }
}
