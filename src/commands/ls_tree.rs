use object::Error;
use std::io::{self, Write};

use crate::git::object;

pub fn run(tree_sha: &str, name_only: bool) -> io::Result<()> {
    let (_, _, content) = object::read_tree_object(&tree_sha)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

    if !name_only {
        io::stdout().write_all(&content)?;
    } else {
        let entries = parse_tree_content(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;
        for (_, name, _) in entries {
            io::stdout().write_all(name.as_bytes())?;
            io::stdout().write_all(b"\n")?;
        }
    }
    io::stdout().flush()?;
    Ok(())
}

fn parse_tree_content(content: &[u8]) -> Result<Vec<(String, String, [u8; 20])>, Error> {
    let mut entries = Vec::new();
    let mut pos = 0;

    while pos < content.len() {
        // Find space separator (between mode and name)
        let space_pos = content[pos..]
            .iter()
            .position(|&b| b == b' ')
            .map(|p| pos + p)
            .ok_or_else(|| Error::InvalidFormat("Invalid tree entry format".to_string()))?;

        let mode = String::from_utf8_lossy(&content[pos..space_pos]).to_string();
        pos = space_pos + 1;

        // Find null byte separator (between name and SHA1)
        let null_pos = content[pos..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| pos + p)
            .ok_or_else(|| Error::InvalidFormat("Invalid tree entry format".to_string()))?;
        let name = String::from_utf8_lossy(&content[pos..null_pos]).to_string();
        pos = null_pos + 1;

        // Extract 20-byte SHA1
        if pos + 20 > content.len() {
            return Err(Error::InvalidFormat(
                "Incomplete SHA1 in tree entry".to_string(),
            ));
        }

        let sha1: [u8; 20] = content[pos..pos + 20]
            .try_into()
            .map_err(|_| Error::InvalidFormat("Invalid SHA1 length".to_string()))?;
        pos += 20;

        entries.push((mode, name, sha1))
    }
    Ok(entries)
}
