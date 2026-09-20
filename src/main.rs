use cstack_db::table::Table;
use cstack_db::vm::*;
use std::io::{BufRead, BufReader};

fn main() {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("Must supply a database filename.");
        std::process::exit(1);
    }

    let mut table = match Table::db_open(args.remove(0)) {
        Ok(t) => t,
        Err(e) => {
            println!("{e}");
            std::process::exit(1);
        }
    };
    let mut lines = BufReader::new(std::io::stdin()).lines();

    loop {
        print_prompt();
        let input = match read_input(&mut lines) {
            Ok(l) => l,
            Err(e) => {
                println!("{e}");
                std::process::exit(1);
            }
        };

        if input.starts_with('.') {
            match do_meta_command(&input, &mut table) {
                Ok(_) => continue,
                Err(e) => {
                    println!("{e}");
                    continue;
                }
            }
        }

        let statement = match prepare_statement(&input) {
            Ok(s) => s,
            Err(e) => {
                println!("{e}");
                continue;
            }
        };

        match execute_statement(statement, &mut table) {
            Ok(_) => println!("Executed."),
            Err(e) => {
                println!("{e}");
                if e.is_fatal() {
                    std::process::exit(1);
                }
            }
        };
    }
}
