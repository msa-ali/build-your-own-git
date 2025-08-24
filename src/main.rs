use flate2::read::GzDecoder;
use std::env;
use std::fs;
use std::io::prelude::*;

fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    eprintln!("Logs from your program will appear here!");

    let args: Vec<String> = env::args().collect();

    let command = args[1].to_string();

    if command == "init" {
        fs::create_dir(".git").unwrap();
        fs::create_dir(".git/objects").unwrap();
        fs::create_dir(".git/refs").unwrap();
        fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
        println!("Initialized git directory");
    }

    if command == "cat-file" {
        if args[2] == "-p" {
            if args.len() != 4 {
                return;
            }
            let object_id = args[3].to_string();
            let (dir_name, object_hash) = object_id.split_at(2);
            println!("Directory Name: {}", dir_name);
            println!("Object Hash: {}", object_hash);

            match fs::read_to_string(format!(".git/objects/{}/{}", dir_name, object_hash)) {
                Ok(content) => {
                    let mut d = GzDecoder::new(content.as_bytes());
                    let mut decompressed_content = String::new();
                    d.read_to_string(&mut decompressed_content).unwrap();
                    let file_content = content.split('\0').nth(1).unwrap();
                    println!("{}", file_content);
                }
                Err(e) => eprintln!("Error reading file: {}", e),
            }
        }
    }
}
