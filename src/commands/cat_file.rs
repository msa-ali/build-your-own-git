use crate::git::object;
use std::io::{self, Write};

// Runs the cat-file command with -p flag.
pub fn run(object_id: &str, flag: &str) -> io::Result<()> {
    let (content_type, size, content) = object::read_blob(object_id)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

    let mut stdout = io::stdout();

    if flag == "-t" {
        stdout.write_all(content_type.as_bytes())?;
    } else if flag == "-s" {
        stdout.write_all(size.to_string().as_bytes())?;
    } else if flag == "-p" {
        stdout.write_all(&content)?;
    } else {
        return Err(io::Error::new(io::ErrorKind::Other, "Invalid flag"));
    }

    stdout.flush()?;
    Ok(())
}
