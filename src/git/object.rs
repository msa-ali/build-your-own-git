use flate2::read::{ZlibDecoder, ZlibEncoder};
use flate2::Compression;
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

pub fn read_blob(object_id: &str) -> Result<(String, usize, Vec<u8>), Error> {
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
    let header_parts: Vec<&str> = header.split_whitespace().collect();
    if header_parts.len() != 2 || header_parts[0] != "blob" {
        return Err(Error::InvalidFormat("Expected blob header".to_string()));
    }

    let size: usize = header_parts[1]
        .parse()
        .map_err(|_| Error::InvalidFormat("Invalid size in header".to_string()))?;

    Ok((
        header_parts[0].to_string(),
        size,
        decompressed[null_pos + 1..].to_vec(),
    ))
}

pub fn write_blob(blob_data: &[u8], hash: &str) -> Result<(), Error> {
    let dir_name = &hash[..2];
    let object_hash = &hash[2..];
    let path = Path::new(".git/objects").join(dir_name).join(object_hash);

    let dir = path
        .parent()
        .ok_or_else(|| Error::Io(io::Error::new(io::ErrorKind::Other, "Invalid path")))?;
    fs::create_dir_all(dir).map_err(Error::Io)?;

    // Compress the blob data
    let mut encoder = ZlibEncoder::new(&blob_data[..], Compression::default());
    let mut compressed = Vec::new();
    encoder
        .read_to_end(&mut compressed)
        .map_err(|err| Error::Io(err))?;

    // Write to file
    fs::write(&path, &compressed).map_err(Error::Io)?;

    Ok(())
}
