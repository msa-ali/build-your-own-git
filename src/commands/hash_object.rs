use std::io::{self, Write};

use crate::git::object;

pub fn run(file_path: &str, should_write: bool) -> io::Result<()> {
    let hash = object::create_file_hash(file_path, should_write)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

    io::stdout().write_all(hash.as_bytes())?;
    io::stdout().flush()?;
    Ok(())
}
