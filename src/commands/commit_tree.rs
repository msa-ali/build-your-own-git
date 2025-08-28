use chrono::Local;
use sha1_smol::Sha1;
use std::io::{self, Write};

use crate::git::object;

// ./your_program.sh commit-tree <tree_sha> -p <commit_sha> -m <message>
//
// Root commit:
// tree cb8f767c198336462f552a08b98b4e4d16b5bd7b
// author codecrafters-bot <hello@codecrafters.io> 1755991527 +0000
// committer codecrafters-bot <hello@codecrafters.io> 1755991527 +0000

// init [skip ci]
//
// Child commit:
// tree e322e4305dc18e199f4b0749764ffd51184b09be
// parent 20d0f3ed7f014a71c4fa2f9303c90f109def9e06
// author Muhammad Sultan Altamash Ali <altamashattari786@gmail.com> 1756208876 +0530
// committer Muhammad Sultan Altamash Ali <altamashattari786@gmail.com> 1756208876 +0530

// fix mode for directory
//
const DEFAULT_USERNAME: &str = "Muhammad Sultan Altamash Ali";
const DEFAULT_EMAIL: &str = "altamashattari786@gmail.com";

pub fn run(args: &[String]) -> io::Result<()> {
    let (tree_sha, parent_commit, commit_message) = parse_args(args)?;

    let mut content = Vec::new();

    // Add the tree SHA to the content
    content.extend_from_slice(format!("tree {}", tree_sha).as_bytes());
    content.push(b'\n');

    // Add the parent commit SHA to the content
    if let Some(parent) = parent_commit {
        content.extend_from_slice(format!("parent {}", parent).as_bytes());
        content.push(b'\n');
    } else {
        // do nothing
    }

    let current_timestamp = format_current_timestamp();

    // Add author info
    let formatted_author = format!(
        "author {} <{}> {}",
        DEFAULT_USERNAME, DEFAULT_EMAIL, current_timestamp
    );
    content.extend_from_slice(formatted_author.as_bytes());
    content.push(b'\n');

    // Add committer info
    let formatted_committer = format!(
        "committer {} <{}> {}",
        DEFAULT_USERNAME, DEFAULT_EMAIL, current_timestamp
    );
    content.extend_from_slice(formatted_committer.as_bytes());
    content.push(b'\n');

    // Add commit message
    content.push(b'\n');
    content.extend_from_slice(commit_message.as_bytes());
    content.push(b'\n');

    let header = format!("commit {}\0", content.len());
    let header_bytes = header.as_bytes();

    let mut store = Vec::with_capacity(header_bytes.len() + content.len());
    store.extend_from_slice(header_bytes);
    store.extend_from_slice(&content);

    // Compute SHA-1 hash
    let mut hasher = Sha1::new();
    hasher.update(&store);
    let commit_hash = hasher.digest().to_string();

    object::write_blob(&store, &commit_hash).map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("Error writing tree: {:?}", e))
    })?;

    io::stdout().write_all(commit_hash.as_bytes())?;
    io::stdout().flush()?;

    Ok(())
}

fn parse_args(args: &[String]) -> io::Result<(&str, Option<&str>, &str)> {
    if args.len() < 3 || args.len() > 5 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Usage: commit-tree <tree_sha> [-p <commit_sha>] -m <message>",
        ));
    }

    let tree_sha = &args[0];
    let mut parent_commit = None;
    let mut commit_message = None;

    let mut i = 1;
    if i < args.len() && args[i] == "-p" {
        if i + 1 >= args.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing parent commit SHA after -p",
            ));
        }
        parent_commit = Some(args[i + 1].as_str());
        i += 2;
    }

    if i < args.len() && args[i] == "-m" {
        if i + 1 >= args.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing commit message after -m",
            ));
        }
        commit_message = Some(args[i + 1].as_str());
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing -m flag or commit message",
        ));
    }

    Ok((tree_sha, parent_commit, commit_message.unwrap()))
}

fn format_current_timestamp() -> String {
    let now_local = Local::now();
    format!("{} {:+05}", now_local.timestamp(), now_local.offset())
}
