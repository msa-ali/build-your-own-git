use flate2::read::{ZlibDecoder, ZlibEncoder};
use flate2::Compression;
use sha1_smol::Sha1;
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
    let decompressed = read_object(object_id, 40)?;

    // Find null byte separator
    let null_pos = decompressed
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| Error::InvalidFormat("No null byte found in the object".to_string()))?;

    // validate header
    let header = String::from_utf8_lossy(&decompressed[..null_pos]);
    let header_parts: Vec<&str> = header.split_whitespace().collect();
    if header_parts.len() != 2
        || (header_parts[0] != "blob" && header_parts[0] != "tree" && header_parts[0] != "commit")
    {
        return Err(Error::InvalidFormat(
            "Expected blob, tree, or commit header".to_string(),
        ));
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

pub fn read_tree_object(object_id: &str) -> Result<(String, usize, Vec<u8>), Error> {
    let decompressed = read_object(object_id, 40)?;

    let null_pos = decompressed
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| {
            Error::InvalidFormat(format!("No null byte found in object {}", object_id))
        })?;
    let header = String::from_utf8_lossy(&decompressed[..null_pos]);
    let header_parts: Vec<&str> = header.split_whitespace().collect();
    if header_parts.len() != 2 || header_parts[0] != "tree" {
        return Err(Error::InvalidFormat(format!(
            "Expected tree header in object {}, got '{}'",
            object_id, header
        )));
    }

    let size: usize = header_parts[1].parse().map_err(|_| {
        Error::InvalidFormat(format!("Invalid size in header for object {}", object_id))
    })?;

    let content = decompressed[null_pos + 1..].to_vec();

    Ok((header_parts[0].to_string(), size, content))
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

fn read_object(object_id: &str, expected_hash_size: usize) -> Result<Vec<u8>, Error> {
    if object_id.len() != expected_hash_size {
        return Err(Error::InvalidFormat(format!(
            "Expected {} characters. Found: {}",
            expected_hash_size,
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

    Ok(decompressed)
}

pub fn create_file_hash(file_path: &str, should_write: bool) -> Result<String, Error> {
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
        write_blob(&store, &hash)?;
    }

    Ok(hash)
}
