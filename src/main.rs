use cstack_db::constants::TABLE_MAX_ROWS;
use cstack_db::errors::*;
use cstack_db::row::{Row, VarChar};
use cstack_db::table::Table;
use std::io::{BufRead, BufReader, Write};
use std::io::{Lines, Stdin};

enum Statement {
    Insert(Box<Row>),
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

fn do_meta_command(input: &str, table: &mut Table) -> Result<(), MetaCommandError> {
    if input == ".exit" {
        table.flush_all().expect("failed to flush on exit");
        std::process::exit(0);
    }
    Err(MetaCommandError::Unrecognized(input.to_string()))
}

fn prepare_statement(input: &str) -> Result<Statement, PrepareError> {
    match input {
        s if s.starts_with("insert") => {
            let row = prepare_insert(s)?;
            Ok(Statement::Insert(Box::new(row)))
        }
        s if s.starts_with("select") => Ok(Statement::Select),
        _ => Err(PrepareError::Unrecognized(input.to_string())),
    }
}

fn prepare_insert(input: &str) -> Result<Row, PrepareError> {
    let mut parts = input.split_whitespace();

    parts
        .next()
        .filter(|c| *c == "insert")
        .ok_or(PrepareError::SyntaxError)?;

    let id: i64 = parts
        .next()
        .ok_or(PrepareError::SyntaxError)?
        .parse()
        .map_err(|_| PrepareError::SyntaxError)?;

    if id < 0 {
        return Err(PrepareError::NegativeId);
    }

    let id: u32 = id.try_into().map_err(|_| PrepareError::SyntaxError)?;

    let username = parts.next().ok_or(PrepareError::SyntaxError)?;
    let email = parts.next().ok_or(PrepareError::SyntaxError)?;

    if parts.next().is_some() {
        return Err(PrepareError::SyntaxError);
    }

    let username = VarChar::try_from(username)?;
    let email = VarChar::try_from(email)?;

    Ok(Row::new(id, username, email))
}

fn execute_insert(row: Row, table: &mut Table) -> Result<(), ExecuteError> {
    if table.len() >= TABLE_MAX_ROWS {
        return Err(ExecuteError::TableFull);
    }
    let row_num = table.len();
    let mut row_ref = table.get_row_mut(row_num)?;
    row.serialize_row(&mut row_ref)?;
    table.incr_rows();
    Ok(())
}

fn execute_select(table: &mut Table) -> Result<(), ExecuteError> {
    for i in 0..table.len() {
        let mut row_reader = match table.get_row(i) {
            Ok(Some(rr)) => rr,
            Ok(None) => return Err(ExecuteError::PageNotFound),
            Err(e) => return Err(e.into()),
        };
        let row = Row::deserialize_row(&mut row_reader)?;
        println!("{row}");
    }
    Ok(())
}

fn execute_statement(statement: Statement, table: &mut Table) -> Result<(), ExecuteError> {
    match statement {
        Statement::Insert(row) => execute_insert(*row, table),
        Statement::Select => execute_select(table),
    }
}

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
