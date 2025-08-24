use flate2::read::ZlibDecoder;
use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    InvalidFormat(String),
    Decompression(String),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

pub fn read_blob(object_id: &str) -> Result<Vec<u8>, Error> {
    if object_id.len() != 40 {
        return Err(Error::InvalidFormat(format!(
            "Expected 40 characters. Found: {}",
            object_id.len()
        )));
    }

    let dir_name = &object_id[..2];
    let object_hash = &object_id[2..];
    let path = Path::new(".git/objects").join(dir_name).join(object_hash);

    let content = fs::read(&path).map_err(Error::Io)?;

    let mut decoder = ZlibDecoder::new(&content[..]);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|err| Error::Decompression(err.to_string()))?;

    // Find null byte separator
    let null_pos = decompressed
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| Error::InvalidFormat("No null byte found in the object".to_string()))?;

    // validate header
    let header = String::from_utf8_lossy(&decompressed[..null_pos]);
    if !header.starts_with("blob ") {
        return Err(Error::InvalidFormat("Expected blob header".to_string()));
    }

    Ok(decompressed[null_pos + 1..].to_vec())
}
