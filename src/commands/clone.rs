// Git clone command implementation
// This module handles the complete Git clone process including:
// - Reference discovery
// - Pack file fetching and unpacking
// - Side-band protocol handling
// - Delta compression (REF_DELTA and OFS_DELTA)
// - File checkout

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use reqwest;
use sha1_smol::Sha1;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ============================================================================
// PUBLIC API
// ============================================================================

/// Main entry point for the clone command
pub fn run(args: &[String]) -> io::Result<()> {
    if args.len() != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid number of arguments",
        ));
    }

    let repo_url = &args[0];
    let target_dir = &args[1];

    println!("Cloning repository {} into {}...", repo_url, target_dir);

    // Create target directory and change into it
    fs::create_dir_all(target_dir)?;
    let _current_dir = std::env::current_dir()?;
    std::env::set_current_dir(target_dir)?;

    // Initialize git repository structure
    init_git_repo()?;

    // Clone the repository
    clone_repository(repo_url)?;

    Ok(())
}

// ============================================================================
// CORE CLONE LOGIC
// ============================================================================

/// Initialize basic Git repository structure
fn init_git_repo() -> io::Result<()> {
    fs::create_dir_all(".git/objects")?;
    fs::create_dir_all(".git/refs/heads")?;
    fs::create_dir_all(".git/refs/remotes/origin")?;

    // Write initial HEAD file
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;

    Ok(())
}

/// Main clone orchestration function
fn clone_repository(repo_url: &str) -> io::Result<()> {
    // Step 1: Discover references
    let (_, (head_ref, head_sha)) = discover_refs(repo_url)?;
    println!("Received head ref: {} and sha: {}", head_ref, head_sha);

    // Update HEAD and create reference
    println!("Updating HEAD to {}", head_ref);
    fs::write(".git/HEAD", format!("ref: {}\n", head_ref))?;

    let ref_path = Path::new(".git").join(&head_ref);
    fs::create_dir_all(ref_path.parent().unwrap())?;
    println!("Creating reference {}", head_ref);
    fs::write(&ref_path, format!("{}\n", head_sha))?;

    // Step 2: Fetch packfile
    let pack_data = fetch_packfile(repo_url, &head_sha)?;
    println!("Received packfile of size {}", pack_data.len());

    // Step 3: Unpack packfile
    println!("Unpacking packfile...");
    unpack_packfile(&pack_data)?;

    // Step 4: Checkout files
    println!("Checking out files...");
    checkout_files(&head_sha)?;

    Ok(())
}

// ============================================================================
// REFERENCE DISCOVERY
// ============================================================================

/// Discover references from the remote repository
fn discover_refs(repo_url: &str) -> io::Result<(Vec<(String, String)>, (String, String))> {
    let refs_url = if repo_url.ends_with(".git") {
        format!("{}/info/refs?service=git-upload-pack", repo_url)
    } else {
        format!("{}.git/info/refs?service=git-upload-pack", repo_url)
    };

    println!("Discovering references from {}", refs_url);

    let client = reqwest::blocking::Client::new();
    let response = client
        .get(&refs_url)
        .header("User-Agent", "git/2.0")
        .send()
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Error fetching refs: {:?}", e),
            )
        })?;

    if !response.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to fetch refs: {}", response.status()),
        ));
    }

    let body = response.text().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("Error parsing refs: {:?}", e))
    })?;

    parse_refs_response(&body)
}

/// Parse the refs response from git-upload-pack
fn parse_refs_response(body: &str) -> io::Result<(Vec<(String, String)>, (String, String))> {
    let mut refs = Vec::new();
    let mut head_ref = String::new();
    let mut head_sha = String::new();

    for line in body.lines() {
        if line.is_empty() || line.len() < 4 {
            continue;
        }

        if line.starts_with("001e# service=git-upload-pack") || line == "0000" {
            continue; // Skip service announcement and flush packets
        }

        let content = &line[4..];
        let fields: Vec<&str> = content.split_whitespace().collect();

        if fields.len() < 2 {
            continue;
        }

        if fields.len() == 2 {
            let sha = fields[0];
            let ref_name = fields[1];
            if sha.len() != 40 {
                println!("Skipping invalid SHA: {} for {}", sha, ref_name);
                continue;
            }
            if ref_name == head_ref {
                head_sha = sha.to_string();
            }
            refs.push((ref_name.to_string(), sha.to_string()));
            continue;
        }

        // Look for symref=HEAD:refs/heads/master
        for field in fields {
            if field.starts_with("symref=HEAD") {
                head_ref = field.split(':').collect::<Vec<&str>>()[1].to_string();
            }
        }
    }

    if head_sha.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "No HEAD reference found",
        ));
    }

    Ok((refs, (head_ref, head_sha)))
}

// ============================================================================
// PACK FILE FETCHING
// ============================================================================

/// Fetch packfile from the remote repository
fn fetch_packfile(repo_url: &str, head_sha: &str) -> io::Result<Vec<u8>> {
    let pack_url = if repo_url.ends_with(".git") {
        format!("{}/git-upload-pack", repo_url)
    } else {
        format!("{}.git/git-upload-pack", repo_url)
    };

    println!("Requesting pack from: {}", pack_url);

    let want_line = format!(
        "want {} multi_ack_detailed side-band-64k thin-pack ofs-delta\n",
        head_sha
    );
    let want_pkt = encode_pkt_line(&want_line);
    let done_pkt = encode_pkt_line("done\n");

    let mut request_body = Vec::new();
    request_body.extend_from_slice(&want_pkt);
    request_body.extend_from_slice(b"0000"); // flush packet
    request_body.extend_from_slice(&done_pkt);

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&pack_url)
        .header("User-Agent", "git/2.0")
        .header("Content-Type", "application/x-git-upload-pack-request")
        .body(request_body)
        .send()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    if !resp.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to fetch packfile: {}", resp.status()),
        ));
    }

    let pack_data = resp
        .bytes()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        .to_vec();

    Ok(pack_data)
}

/// Encode a line in Git's pkt-line format
/// Format: 4-byte hex length (including the 4 bytes) + data
fn encode_pkt_line(line: &str) -> Vec<u8> {
    let len = line.len() + 4;
    format!("{:04x}{}", len, line).into_bytes()
}

// ============================================================================
// SIDE-BAND PROTOCOL HANDLING
// ============================================================================

/// Find where the actual pack data starts
fn find_pack_start(data: &[u8]) -> io::Result<usize> {
    // Look for "PACK" signature
    for i in 0..data.len().saturating_sub(4) {
        if &data[i..i + 4] == b"PACK" {
            return Ok(i);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Could not find PACK signature",
    ))
}

/// Decode side-band data from the pack response
/// Git uses side-band protocol to interleave pack data with progress messages
fn decode_sideband_data(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoded = Vec::new();
    let mut offset = 0;

    // Find the PACK start first
    let pack_start = find_pack_start(data)?;

    // If PACK is at the beginning, no side-band encoding
    if pack_start == 0 {
        return Ok(data.to_vec());
    }

    // Process pkt-line formatted data
    while offset < data.len() {
        // Check for pkt-line format (4-byte hex length prefix)
        if offset + 4 <= data.len() {
            let length_str = String::from_utf8_lossy(&data[offset..offset + 4]);
            if let Ok(length) = u32::from_str_radix(&length_str, 16) {
                if length == 0 {
                    // Flush packet, skip it
                    offset += 4;
                    continue;
                } else if length >= 4 && length <= 65520 && offset + length as usize <= data.len() {
                    let pkt_data_start = offset + 4;
                    let pkt_data_end = offset + length as usize;

                    // Check for side-band prefix
                    if pkt_data_start < pkt_data_end {
                        let side_band = data[pkt_data_start];
                        match side_band {
                            1 => {
                                // Side-band 1: pack data
                                let actual_data = &data[pkt_data_start + 1..pkt_data_end];
                                decoded.extend_from_slice(actual_data);
                            }
                            2 | 3 => {
                                // Side-band 2/3: progress messages
                                let message = String::from_utf8_lossy(
                                    &data[pkt_data_start + 1..pkt_data_end],
                                );
                                println!("Git progress: {}", message.trim());
                            }
                            _ => {
                                // Unknown side-band, treat as raw data
                                let actual_data = &data[pkt_data_start..pkt_data_end];
                                decoded.extend_from_slice(actual_data);
                            }
                        }
                    }

                    offset = pkt_data_end;
                    continue;
                }
            }
        }

        // If we can't parse as pkt-line, copy raw data
        decoded.push(data[offset]);
        offset += 1;
    }

    Ok(decoded)
}

// ============================================================================
// PACK FILE UNPACKING
// ============================================================================

/// Pack object type enumeration
#[derive(Debug)]
enum PackObjectType {
    Commit,
    Tree,
    Blob,
    OfsDelta(#[allow(dead_code)] usize),
    RefDelta(String),
}

impl PackObjectType {
    fn as_str(&self) -> &str {
        match self {
            PackObjectType::Commit => "commit",
            PackObjectType::Tree => "tree",
            PackObjectType::Blob => "blob",
            _ => "unknown",
        }
    }
}

/// Unpack the pack file and extract all objects
fn unpack_packfile(pack_data: &[u8]) -> io::Result<()> {
    // Decode side-band data to get clean pack file
    let decoded_data = decode_sideband_data(pack_data)?;
    let pack_start = find_pack_start(&decoded_data)?;
    let pack_data = &decoded_data[pack_start..];

    // Validate pack header
    if pack_data.len() < 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Pack file too small",
        ));
    }

    // Check PACK signature
    if &pack_data[0..4] != b"PACK" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid pack signature: {:?}", &pack_data[0..4]),
        ));
    }

    // Parse version (big-endian uint32)
    let version = u32::from_be_bytes([pack_data[4], pack_data[5], pack_data[6], pack_data[7]]);
    if version != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unsupported pack version: {}", version),
        ));
    }

    // Parse object count (big-endian uint32)
    let object_count =
        u32::from_be_bytes([pack_data[8], pack_data[9], pack_data[10], pack_data[11]]);
    println!("Pack contains {} objects", object_count);

    // Process all objects
    process_pack_objects(pack_data, object_count)?;

    println!("Successfully unpacked {} objects", object_count);
    Ok(())
}

/// Process all objects in the pack file
fn process_pack_objects(pack_data: &[u8], object_count: u32) -> io::Result<()> {
    let mut offset = 12; // Skip pack header
    let mut objects = HashMap::new(); // SHA -> full object (with header)
    let mut objects_by_offset = HashMap::new(); // pack offset -> raw content
    let mut ref_delta_objects = Vec::new();
    let mut ofs_delta_objects = Vec::new();

    // First pass: process regular objects and collect deltas
    for i in 0..object_count {
        println!("Processing object {}/{}", i + 1, object_count);
        let pack_offset = offset;

        // Check if we're near the end of the pack (leave space for checksum)
        let remaining_bytes = pack_data.len().saturating_sub(offset);
        if remaining_bytes <= 20 {
            println!(
                "Reached end of pack file at offset {} with {} bytes remaining",
                offset, remaining_bytes
            );
            break;
        }

        let (obj_type, obj_data, bytes_consumed) = match parse_pack_object(&pack_data[offset..]) {
            Ok(result) => result,
            Err(e) => {
                println!(
                    "Error parsing object {} at pack offset {}: {}",
                    i + 1,
                    offset,
                    e
                );
                println!("Remaining pack data: {} bytes", pack_data.len() - offset);

                // Try error recovery
                if let Some((recovered_offset, recovered_obj)) =
                    attempt_error_recovery(pack_data, offset)?
                {
                    offset = recovered_offset;
                    recovered_obj
                } else {
                    println!("Could not recover, stopping at object {}", i + 1);
                    break;
                }
            }
        };

        // Store object based on type
        match obj_type {
            PackObjectType::Commit | PackObjectType::Tree | PackObjectType::Blob => {
                let sha = store_object(&obj_type, &obj_data)?;

                // Store full object with header for delta base lookup
                let header = format!("{} {}\0", obj_type.as_str(), obj_data.len());
                let mut full_object = Vec::new();
                full_object.extend_from_slice(header.as_bytes());
                full_object.extend_from_slice(&obj_data);
                objects.insert(sha.clone(), full_object);

                // Store raw content for OFS_DELTA
                objects_by_offset.insert(pack_offset, obj_data.clone());
                println!("  Stored {} as {}", obj_type.as_str(), sha);
            }
            PackObjectType::RefDelta(base_sha) => {
                println!("  Found REF_DELTA referencing {}", base_sha);
                ref_delta_objects.push((base_sha, obj_data));
            }
            PackObjectType::OfsDelta(ofs) => {
                println!("  Found OFS_DELTA with offset {}", ofs);
                ofs_delta_objects.push((pack_offset, ofs, obj_data));
            }
        }

        offset += bytes_consumed;
    }

    // Second pass: process delta objects
    process_ref_deltas(ref_delta_objects, &mut objects)?;
    process_ofs_deltas(ofs_delta_objects, &mut objects, &mut objects_by_offset)?;

    Ok(())
}

/// Attempt to recover from parsing errors by finding the next valid object
fn attempt_error_recovery(
    pack_data: &[u8],
    offset: usize,
) -> io::Result<Option<(usize, (PackObjectType, Vec<u8>, usize))>> {
    println!("Attempting to recover by finding next valid object...");

    let mut recovery_offset = 1;

    // Look ahead up to 1000 bytes for the next valid object
    while recovery_offset < 1000 && offset + recovery_offset < pack_data.len() - 20 {
        if let Ok((next_obj_type, next_obj_data, next_bytes_consumed)) =
            parse_pack_object(&pack_data[offset + recovery_offset..])
        {
            println!(
                "Found valid object at offset {}, skipping {} bytes",
                offset + recovery_offset,
                recovery_offset
            );
            return Ok(Some((
                offset + recovery_offset,
                (next_obj_type, next_obj_data, next_bytes_consumed),
            )));
        }
        recovery_offset += 1;
    }

    Ok(None)
}

/// Process REF_DELTA objects
fn process_ref_deltas(
    ref_delta_objects: Vec<(String, Vec<u8>)>,
    objects: &mut HashMap<String, Vec<u8>>,
) -> io::Result<()> {
    println!("Processing {} REF_DELTA objects", ref_delta_objects.len());

    for (base_sha, delta_data) in ref_delta_objects {
        let base_object_full = if let Some(obj) = objects.get(&base_sha) {
            obj.clone()
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("REF_DELTA base object {} not found in memory", base_sha),
            ));
        };

        // Extract raw content from full object (skip header)
        let null_pos = base_object_full
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid base object format")
            })?;
        let base_content = &base_object_full[null_pos + 1..];

        let result_content = apply_delta(base_content, &delta_data)?;
        let sha = store_raw_object(&result_content)?;
        objects.insert(sha.clone(), result_content);
        println!("  Applied REF_DELTA and stored as {}", sha);
    }

    Ok(())
}

/// Process OFS_DELTA objects
fn process_ofs_deltas(
    ofs_delta_objects: Vec<(usize, usize, Vec<u8>)>,
    objects: &mut HashMap<String, Vec<u8>>,
    objects_by_offset: &mut HashMap<usize, Vec<u8>>,
) -> io::Result<()> {
    println!("Processing {} OFS_DELTA objects", ofs_delta_objects.len());

    for (pack_offset, ofs, delta_data) in ofs_delta_objects {
        let base_offset = pack_offset - ofs;
        println!(
            "  OFS_DELTA at offset {} references base at offset {}",
            pack_offset, base_offset
        );

        let base_object = match objects_by_offset.get(&base_offset) {
            Some(obj) => obj,
            None => {
                println!(
                    "  Warning: OFS_DELTA base object not found at offset {}, skipping",
                    base_offset
                );
                continue;
            }
        };

        println!("  Base object size: {} bytes", base_object.len());
        println!("  Delta data size: {} bytes", delta_data.len());

        let result_content = apply_delta(base_object, &delta_data)?;
        println!("  Result content size: {} bytes", result_content.len());

        // Create full object with blob header (most common for deltas)
        let header = format!("blob {}\0", result_content.len());
        let mut full_object = Vec::new();
        full_object.extend_from_slice(header.as_bytes());
        full_object.extend_from_slice(&result_content);

        let sha = store_raw_object(&full_object)?;
        objects.insert(sha.clone(), full_object.clone());
        objects_by_offset.insert(pack_offset, result_content);
        println!("  Applied OFS_DELTA and stored as {}", sha);
    }

    Ok(())
}

// ============================================================================
// PACK OBJECT PARSING
// ============================================================================

/// Parse a single object from the pack file
fn parse_pack_object(data: &[u8]) -> io::Result<(PackObjectType, Vec<u8>, usize)> {
    if data.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No data to parse",
        ));
    }

    let mut offset = 0;
    let first_byte = data[offset];
    offset += 1;

    // Extract object type (bits 6-4) and initial size (bits 3-0)
    let obj_type_num = (first_byte >> 4) & 0x07;
    let mut size = (first_byte & 0x0F) as usize;
    let mut shift = 4;

    // Continue reading size if MSB is set (variable-length encoding)
    let mut current_byte = first_byte;
    while current_byte & 0x80 != 0 {
        if offset >= data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Incomplete size encoding",
            ));
        }

        current_byte = data[offset];
        offset += 1;

        // Prevent shift overflow
        if shift >= 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Size encoding too large",
            ));
        }

        size |= ((current_byte & 0x7F) as usize) << shift;
        shift += 7;
    }

    // Parse object type and handle special cases
    let obj_type = match obj_type_num {
        1 => PackObjectType::Commit,
        2 => PackObjectType::Tree,
        3 => PackObjectType::Blob,
        4 => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "TAG objects not supported",
            ))
        }
        6 => {
            // OFS_DELTA - read negative offset using Git's encoding
            let (ofs_offset, new_offset) = read_ofs_delta_offset(data, offset)?;
            offset = new_offset;
            PackObjectType::OfsDelta(ofs_offset)
        }
        7 => {
            // REF_DELTA - read 20-byte SHA1
            if offset + 20 > data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Incomplete REF_DELTA SHA",
                ));
            }
            let sha_bytes = &data[offset..offset + 20];
            offset += 20;
            let sha = hex::encode(sha_bytes);
            PackObjectType::RefDelta(sha)
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unknown object type: {}", obj_type_num),
            ))
        }
    };

    // Decompress object data
    let compressed_data = &data[offset..];
    let mut decoder = ZlibDecoder::new(compressed_data);
    let mut decompressed = Vec::new();

    decoder.read_to_end(&mut decompressed).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Failed to decompress object at header offset {}: {}",
                offset, e
            ),
        )
    })?;

    // Verify the decompressed size matches expected (with tolerance for minor differences)
    if decompressed.len() != size {
        println!(
            "Warning: Size mismatch for object - expected {}, got {}",
            size,
            decompressed.len()
        );
        if (decompressed.len() as i64 - size as i64).abs() > 1000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Significant size mismatch: expected {}, got {}",
                    size,
                    decompressed.len()
                ),
            ));
        }
    }

    // Calculate how many compressed bytes were consumed
    let total_in = decoder.total_in() as usize;

    Ok((obj_type, decompressed, offset + total_in))
}

/// Read OFS_DELTA offset using Git's variable-length encoding
fn read_ofs_delta_offset(data: &[u8], mut offset: usize) -> io::Result<(usize, usize)> {
    if offset >= data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No data for OFS_DELTA offset",
        ));
    }

    let mut c = data[offset];
    offset += 1;
    let mut ofs = (c & 0x7F) as usize;

    while c & 0x80 != 0 {
        if offset >= data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Incomplete OFS_DELTA offset",
            ));
        }

        c = data[offset];
        offset += 1;
        ofs = ((ofs + 1) << 7) + (c & 0x7F) as usize;
    }

    Ok((ofs, offset))
}

// ============================================================================
// DELTA COMPRESSION
// ============================================================================

/// Apply a delta to a base object to reconstruct the target object
fn apply_delta(base: &[u8], delta: &[u8]) -> io::Result<Vec<u8>> {
    let mut offset = 0;

    // Read base object size (variable length encoding)
    let mut _base_size = 0usize;
    let mut shift = 0;
    loop {
        if offset >= delta.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Incomplete base size in delta",
            ));
        }
        let byte = delta[offset];
        offset += 1;

        // Prevent shift overflow
        if shift >= 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Base size encoding too large",
            ));
        }

        _base_size |= ((byte & 0x7F) as usize) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
    }

    // Read result object size
    let mut result_size = 0usize;
    shift = 0;
    loop {
        if offset >= delta.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Incomplete result size in delta",
            ));
        }
        let byte = delta[offset];
        offset += 1;

        // Prevent shift overflow
        if shift >= 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Result size encoding too large",
            ));
        }

        result_size |= ((byte & 0x7F) as usize) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
    }

    let mut result = Vec::with_capacity(result_size);

    // Apply delta instructions
    while offset < delta.len() {
        let cmd = delta[offset];
        offset += 1;

        if cmd & 0x80 != 0 {
            // Copy command: copy bytes from base object
            let mut copy_offset = 0usize;
            let mut copy_size = 0usize;

            // Read offset (up to 4 bytes)
            if cmd & 0x01 != 0 {
                copy_offset |= delta[offset] as usize;
                offset += 1;
            }
            if cmd & 0x02 != 0 {
                copy_offset |= (delta[offset] as usize) << 8;
                offset += 1;
            }
            if cmd & 0x04 != 0 {
                copy_offset |= (delta[offset] as usize) << 16;
                offset += 1;
            }
            if cmd & 0x08 != 0 {
                copy_offset |= (delta[offset] as usize) << 24;
                offset += 1;
            }

            // Read size (up to 3 bytes)
            if cmd & 0x10 != 0 {
                copy_size |= delta[offset] as usize;
                offset += 1;
            }
            if cmd & 0x20 != 0 {
                copy_size |= (delta[offset] as usize) << 8;
                offset += 1;
            }
            if cmd & 0x40 != 0 {
                copy_size |= (delta[offset] as usize) << 16;
                offset += 1;
            }

            if copy_size == 0 {
                copy_size = 0x10000; // Default size when not specified
            }

            // Validate and copy from base
            if copy_offset + copy_size > base.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Delta copy out of bounds: offset={}, size={}, base_len={}",
                        copy_offset,
                        copy_size,
                        base.len()
                    ),
                ));
            }
            result.extend_from_slice(&base[copy_offset..copy_offset + copy_size]);
        } else if cmd != 0 {
            // Insert command: insert new data from delta
            let insert_size = cmd as usize;
            if offset + insert_size > delta.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Delta insert out of bounds",
                ));
            }
            result.extend_from_slice(&delta[offset..offset + insert_size]);
            offset += insert_size;
        } else {
            // cmd == 0 is invalid
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid delta command: 0",
            ));
        }
    }

    // Verify result size
    if result.len() != result_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Delta result size mismatch: expected {}, got {}",
                result_size,
                result.len()
            ),
        ));
    }

    Ok(result)
}

// ============================================================================
// OBJECT STORAGE
// ============================================================================

/// Store an object in the Git object database
fn store_object(obj_type: &PackObjectType, data: &[u8]) -> io::Result<String> {
    let header = format!("{} {}\0", obj_type.as_str(), data.len());
    let mut full_object = Vec::new();
    full_object.extend_from_slice(header.as_bytes());
    full_object.extend_from_slice(data);

    store_raw_object(&full_object)
}

/// Store raw object data (with header) in the Git object database
fn store_raw_object(data: &[u8]) -> io::Result<String> {
    // Calculate SHA1 hash
    let mut hasher = Sha1::new();
    hasher.update(data);
    let sha = hasher.digest().to_string();

    // Create object directory and file path
    let dir = format!(".git/objects/{}", &sha[..2]);
    fs::create_dir_all(&dir)?;
    let path = format!("{}/{}", dir, &sha[2..]);

    // Compress and write object
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;

    fs::write(&path, compressed)?;

    Ok(sha)
}

/// Read an object from the Git object database (content only, no header)
fn read_object_raw(sha: &str) -> io::Result<Vec<u8>> {
    let path = format!(".git/objects/{}/{}", &sha[..2], &sha[2..]);
    let compressed = fs::read(&path)?;

    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;

    // Find null byte to skip header
    let null_pos = decompressed
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid object format"))?;

    Ok(decompressed[null_pos + 1..].to_vec())
}

/// Read a complete Git object by SHA (including header)
fn read_git_object(sha: &str) -> io::Result<Vec<u8>> {
    let path = format!(".git/objects/{}/{}", &sha[..2], &sha[2..]);
    let compressed = fs::read(&path)?;

    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;

    Ok(decompressed)
}

// ============================================================================
// FILE CHECKOUT
// ============================================================================

/// Checkout files from the repository
fn checkout_files(head_sha: &str) -> io::Result<()> {
    // Read the commit object
    let commit_data = read_git_object(head_sha)?;

    // Parse commit to find tree SHA
    let tree_sha = parse_commit_tree(&commit_data)?;
    println!("Checking out tree {}", tree_sha);

    // Recursively checkout the tree
    checkout_tree(&tree_sha, Path::new("."))?;

    Ok(())
}

/// Parse commit object to extract tree SHA
fn parse_commit_tree(commit_data: &[u8]) -> io::Result<String> {
    let commit_str = String::from_utf8_lossy(commit_data);

    // Find the null byte that separates header from content
    let content_start = commit_data
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid commit format"))?;

    let content = &commit_str[content_start + 1..];

    // Find tree line
    for line in content.lines() {
        if line.starts_with("tree ") {
            return Ok(line[5..].trim().to_string());
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "No tree found in commit",
    ))
}

/// Recursively checkout a tree
fn checkout_tree(tree_sha: &str, base_path: &Path) -> io::Result<()> {
    let tree_data = read_git_object(tree_sha)?;

    // Find the null byte that separates header from content
    let content_start = tree_data
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid tree format"))?;

    let mut offset = content_start + 1;

    while offset < tree_data.len() {
        // Parse mode and name
        let space_pos = tree_data[offset..]
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid tree entry"))?;

        let mode = String::from_utf8_lossy(&tree_data[offset..offset + space_pos]);
        offset += space_pos + 1;

        let null_pos = tree_data[offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid tree entry name"))?;

        let name = String::from_utf8_lossy(&tree_data[offset..offset + null_pos]);
        offset += null_pos + 1;

        // Read 20-byte SHA
        if offset + 20 > tree_data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid tree entry SHA",
            ));
        }

        let sha = hex::encode(&tree_data[offset..offset + 20]);
        offset += 20;

        let entry_path = base_path.join(name.as_ref());

        if mode == "40000" {
            // Directory
            fs::create_dir_all(&entry_path)?;
            checkout_tree(&sha, &entry_path)?;
        } else {
            // File
            let blob_data = read_git_object(&sha)?;

            // Find the null byte that separates header from content
            let blob_content_start = blob_data
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid blob format"))?;

            let content = &blob_data[blob_content_start + 1..];

            // Ensure parent directory exists
            if let Some(parent) = entry_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&entry_path, content)?;

            // Set executable permission if needed (Unix-like systems only)
            #[cfg(unix)]
            {
                if mode == "100755" {
                    let mut perms = fs::metadata(&entry_path)?.permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&entry_path, perms)?;
                }
            }
        }
    }

    Ok(())
}
