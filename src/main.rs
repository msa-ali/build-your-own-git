use flate2::read::ZlibDecoder;
use std::env;
use std::fs;
use std::io::Write;
use std::io::{self, Read};
use std::process;

fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    eprintln!("Logs from your program will appear here!");

    let args: Vec<String> = env::args().collect();

    let command = &args[1];

    if command == "init" {
        fs::create_dir(".git").unwrap();
        fs::create_dir(".git/objects").unwrap();
        fs::create_dir(".git/refs").unwrap();
        fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
        println!("Initialized git directory");
    } else if command == "cat-file" {
        if args[2] == "-p" {
            if args.len() != 4 {
                return;
            }
            let object_id = &args[3];
            // let (dir_name, object_hash) = object_id.split_at(2);
            let dir_name = &object_id[..2];
            let object_hash = &object_id[2..];
            let path = format!(".git/objects/{}/{}", dir_name, object_hash);

            let content = match fs::read(path) {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("Error reading file: {}", e);
                    process::exit(1);
                }
            };

            let mut decoder = ZlibDecoder::new(&content[..]);
            let mut decompressed = Vec::new();
            if let Err(e) = decoder.read_to_end(&mut decompressed) {
                eprintln!("Error decompressing object {}: {}", object_id, e);
                process::exit(1);
            }
            let null_pos = decompressed.iter().position(|&b| b == 0);
            let null_pos = match null_pos {
                Some(pos) => pos,
                None => {
                    eprintln!("Invalid object format");
                    process::exit(1);
                }
            };
            let header = String::from_utf8_lossy(&decompressed[..null_pos]);
            if !header.starts_with("blob") {
                eprintln!("Invalid object format: expected blob header");
                process::exit(1);
            }
            let file_content = &decompressed[null_pos + 1..];
            io::stdout().write_all(file_content).unwrap();
            io::stdout().flush().unwrap();
        }
    }
}
