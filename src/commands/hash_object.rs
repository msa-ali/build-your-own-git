use sha1_smol::Sha1;
use std::fs;
use std::io::{self, Write};

use crate::git::object;

pub fn run(file_path: &str, should_write: bool) -> io::Result<()> {
    let content = fs::read(file_path)?;

    let header = format!("blob {}\0", content.len());
    let header_bytes = header.as_bytes();

    let mut store = Vec::with_capacity(header_bytes.len() + content.len());
    store.extend_from_slice(header_bytes);
    store.extend_from_slice(&content);

    let mut hasher = Sha1::new();
    hasher.update(&store);
    let hash = hasher.digest().to_string();

    if should_write {
        object::write_blob(&store, &hash)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;
    }
    io::stdout().write_all(hash.as_bytes())?;
    io::stdout().flush()?;
    Ok(())
}
