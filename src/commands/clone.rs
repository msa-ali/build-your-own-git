use reqwest;
use std::fs;
use std::io;
use std::path::Path;

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

    fs::create_dir_all(target_dir)?;

    let _current_dir = std::env::current_dir()?;

    std::env::set_current_dir(target_dir)?;

    // Initialize git repository structure
    init_git_repo()?;

    // Clone the repository
    clone_repository(repo_url)?;

    // Implementation of the clone command
    Ok(())
}

fn init_git_repo() -> io::Result<()> {
    fs::create_dir_all(".git/objects")?;
    fs::create_dir_all(".git/refs/heads")?;
    fs::create_dir_all(".git/refs/remotes/origin")?;

    // Write initial HEAD file
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;

    Ok(())
}

fn clone_repository(repo_url: &str) -> io::Result<()> {
    // Step-1: Discover references
    let (_, (head_ref, head_sha)) = discover_refs(repo_url)?;
    println!("Received head ref: {} and sha: {}", head_ref, head_sha);
    println!("Updating HEAD to {}", head_ref);
    fs::write(".git/HEAD", format!("ref: {}\n", head_ref))?;
    let ref_path = Path::new(".git").join(&head_ref);
    fs::create_dir_all(ref_path.parent().unwrap())?;
    println!("Creating reference {}", head_ref);
    fs::write(&ref_path, format!("{}\n", head_sha))?;

    // Step-2: Fetching packfile
    let pack_data = fetch_packfile(repo_url, &head_sha)?;
    println!("Received packfile of size {}", pack_data.len());

    // Step 3: Unpacking packfile
    println!("Unpacking packfile...");
    // unpack_packfile(&pack_data)?;

    // Step 4: Checking out files
    // checkout_files(&head_sha)?;

    Ok(())
}

fn discover_refs(repo_url: &str) -> io::Result<(Vec<(String, String)>, (String, String))> {
    // let refs_url = format!("{}/info/refs?service=git-upload-pack", repo_url);
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
            io::Error::new(io::ErrorKind::Other, format!("Error writing tree: {:?}", e))
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

fn parse_refs_response(body: &str) -> io::Result<(Vec<(String, String)>, (String, String))> {
    let mut refs = Vec::new();
    let mut head_ref = String::new();
    let mut head_sha = String::new();

    for line in body.lines() {
        // println!("processing Ref line: {}", line);

        if line.is_empty() || line.len() < 4 {
            continue; // Parse pkt-line format (4-byte length prefix + data)
        }

        if line.starts_with("001e# service=git-upload-pack") || line == "0000" {
            continue; // Skip service announcement and flush packets
        }

        let content = &line[4..];
        let fields: Vec<&str> = content.split_whitespace().collect();

        if fields.len() < 2 {
            continue;
        }

        if (fields.len() == 2) {
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

        for field in fields {
            if field.starts_with("symref=HEAD") {
                head_ref = field.split(":").collect::<Vec<&str>>()[1].to_string();
            }
        }
    }

    if head_sha.is_empty() {
        println!("No HEAD reference found");
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "No HEAD reference found",
        ));
    }

    Ok((refs, (head_ref, head_sha)))
}

fn fetch_packfile(repo_url: &str, head_sha: &str) -> io::Result<Vec<u8>> {
    // let pack_url = format!("{}/git-upload-pack", repo_url);
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
