mod commands;
mod git;

use std::env;
use std::io;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <command> [<args>]", args[0]);
        process::exit(1);
    }

    let command = &args[1];
    let result: io::Result<()> = match command.as_str() {
        "init" => commands::init::run(),
        "cat-file" => {
            if args.len() != 4 {
                eprintln!("Usage: {} cat-file -<flag> <object_id>", args[0]);
                process::exit(1);
            }
            commands::cat_file::run(&args[3], &args[2])
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
