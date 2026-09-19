use std::io::{BufRead, BufReader, Write};
use std::io::{Lines, Stdin};

enum MetaCommandResult {
    Success,
    Unrecognized,
}

enum PrepareResult {
    Success(Statement),
    Unrecognized,
}

enum Statement {
    Insert,
    Select,
}

fn print_prompt() {
    print!("db > ");
    std::io::stdout().flush().unwrap();
}

fn read_input(lines: &mut Lines<BufReader<Stdin>>) -> Result<String, String> {
    if let Some(Ok(line)) = lines.next() {
        Ok(line)
    } else {
        Err(String::from("Error reading input"))
    }
}

fn do_meta_command(input: &str) -> MetaCommandResult {
    if input == ".exit" {
        std::process::exit(0);
    }
    MetaCommandResult::Unrecognized
}

fn prepare_statement(input: &str) -> PrepareResult {
    match input {
        s if s.starts_with("insert") => PrepareResult::Success(Statement::Insert),
        s if s.starts_with("select") => PrepareResult::Success(Statement::Select),
        _ => PrepareResult::Unrecognized,
    }
}

fn execute_statement(statement: Statement) {
    match statement {
        Statement::Insert => {
            println!("This is where we would do an insert.");
        }
        Statement::Select => {
            println!("This is where we would do a select.");
        }
    }
}

fn main() {
    //let mut args: Vec<_> = std::env::args().skip(1).collect();
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
            match do_meta_command(&input) {
                MetaCommandResult::Success => continue,
                MetaCommandResult::Unrecognized => {
                    println!("Unrecognized command '{input}'");
                    continue;
                }
            }
        }

        let statement = match prepare_statement(&input) {
            PrepareResult::Success(s) => s,
            PrepareResult::Unrecognized => {
                println!("Unrecognized keyword at start of '{input}'");
                continue;
            }
        };

        execute_statement(statement);
        println!("Executed.");
    }
}
