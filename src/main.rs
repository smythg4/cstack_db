use std::io::{BufRead, BufReader, Write};
use std::io::{Lines, Stdin};

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

fn main() {
    //let mut args: Vec<_> = std::env::args().skip(1).collect();
    let mut lines = BufReader::new(std::io::stdin()).lines();
    loop {
        print_prompt();
        let line = match read_input(&mut lines) {
            Ok(l) => l,
            Err(e) => {
                println!("{e}");
                std::process::exit(1);
            }
        };
        if line == ".exit" {
            std::process::exit(0);
        } else {
            println!("Unrecognized command '{line}'");
        }
    }
}
