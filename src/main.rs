use std::io::{BufRead, BufReader, Write};
use std::io::{Lines, Stdin};

fn print_prompt() {
    print!("db > ");
    std::io::stdout().flush().unwrap();
}

fn read_input(lines: &mut Lines<BufReader<Stdin>>) -> Result<String, String> {
    if let Some(line) = lines.next() {
        match line {
            Ok(l) => Ok(l),
            Err(_) => Err(String::from("Error reading input")),
        }
    } else {
        Err(String::from("No input found"))
    }
}

fn main() {
    //let mut args: Vec<_> = std::env::args().skip(1).collect();
    let mut lines = BufReader::new(std::io::stdin()).lines();
    loop {
        print_prompt();
        let line = match read_input(&mut lines) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        if &line == ".exit" {
            std::process::exit(0);
        } else {
            eprintln!("Unrecognized command '{line}'");
        }
    }
}
