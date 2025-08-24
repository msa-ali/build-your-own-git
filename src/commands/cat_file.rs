use crate::git::object;
use std::io::{self, Write};

// Runs the cat-file command with -p flag.
pub fn run(object_id: &str) -> io::Result<()> {
    let content = object::read_blob(object_id)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

    let mut stdout = io::stdout();
    stdout.write_all(&content)?;
    stdout.flush()?;
    Ok(())
}
