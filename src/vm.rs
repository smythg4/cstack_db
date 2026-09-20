use crate::constants::*;
use crate::cursor::Cursor;
use crate::errors::*;
use crate::row::{Row, VarChar};
use crate::table::Table;
use std::io::{BufReader, Write};
use std::io::{Lines, Stdin};

pub enum Statement {
    Insert(Box<Row>),
    Select,
}

pub fn print_prompt() {
    print!("db > ");
    std::io::stdout().flush().unwrap();
}

pub fn read_input(lines: &mut Lines<BufReader<Stdin>>) -> Result<String, String> {
    if let Some(Ok(line)) = lines.next() {
        Ok(line)
    } else {
        Err(String::from("Error reading input"))
    }
}

pub fn do_meta_command(input: &str, table: &mut Table) -> Result<(), MetaCommandError> {
    if input == ".exit" {
        table.flush_all().expect("failed to flush on exit");
        std::process::exit(0);
    }
    Err(MetaCommandError::Unrecognized(input.to_string()))
}

pub fn prepare_statement(input: &str) -> Result<Statement, PrepareError> {
    match input {
        s if s.starts_with("insert") => {
            let row = prepare_insert(s)?;
            Ok(Statement::Insert(Box::new(row)))
        }
        s if s.starts_with("select") => Ok(Statement::Select),
        _ => Err(PrepareError::Unrecognized(input.to_string())),
    }
}

pub fn prepare_insert(input: &str) -> Result<Row, PrepareError> {
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

pub fn execute_insert(row: Row, table: &mut Table) -> Result<(), ExecuteError> {
    if table.len() >= TABLE_MAX_ROWS {
        return Err(ExecuteError::TableFull);
    }
    let mut cursor = Cursor::table_end(table);
    row.serialize_row(&mut cursor.cursor_value_mut()?)?;
    table.incr_rows();
    Ok(())
}

pub fn execute_select(table: &mut Table) -> Result<(), ExecuteError> {
    let mut cursor = Cursor::table_start(table);
    while !cursor.at_end() {
        let mut row_reader = match cursor.cursor_value() {
            Ok(Some(rr)) => rr,
            Ok(None) => return Err(ExecuteError::PageNotFound),
            Err(e) => return Err(e.into()),
        };
        let row = Row::deserialize_row(&mut row_reader)?;
        println!("{row}");
        cursor.cursor_advance();
    }
    Ok(())
}

pub fn execute_statement(statement: Statement, table: &mut Table) -> Result<(), ExecuteError> {
    match statement {
        Statement::Insert(row) => execute_insert(*row, table),
        Statement::Select => execute_select(table),
    }
}
