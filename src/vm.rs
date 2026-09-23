use crate::constants::*;
use crate::cursor::{Cursor, CursorError};
use crate::errors::*;
use crate::row::{Row, VarChar};
use crate::table::{
    COMMON_NODE_HEADER_SIZE, LEAF_NODE_CELL_SIZE, LEAF_NODE_HEADER_SIZE, LEAF_NODE_MAX_CELLS,
    LEAF_NODE_SPACE_FOR_CELLS, Table, TableError,
};
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

pub fn do_meta_command(input: &str, table: &Table) -> Result<(), MetaCommandError> {
    match input {
        ".exit" => {
            table.flush_all().expect("failed to flush on exit");
            std::process::exit(0);
        }
        ".constants" => {
            println!("Constants:");
            print_constants();
            Ok(())
        }
        ".btree" => {
            println!("Tree:");
            table.print_tree(table.root_page_num(), 0)?;
            Ok(())
        }
        _ => Err(MetaCommandError::Unrecognized(input.to_string())),
    }
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

pub fn execute_insert(row: Row, table: &Table) -> Result<(), ExecuteError> {
    let key_to_insert = row.id;
    let cursor = Cursor::table_find(table, key_to_insert)?;
    match cursor.leaf_node_insert(row.id, &row) {
        Ok(_) => {}
        Err(CursorError::TableError(TableError::DuplicateKey(_))) => {
            return Err(ExecuteError::DuplicateKey);
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub fn execute_select(table: &Table) -> Result<(), ExecuteError> {
    let mut cursor = Cursor::table_start(table)?;
    while !cursor.at_end() {
        let row_reader = cursor.cursor_value_mut()?;
        let row = Row::deserialize_row(&mut &*row_reader)?;
        println!("{row}");
        drop(row_reader);
        cursor.cursor_advance()?;
    }
    Ok(())
}

pub fn execute_statement(statement: Statement, table: &Table) -> Result<(), ExecuteError> {
    match statement {
        Statement::Insert(row) => execute_insert(*row, table),
        Statement::Select => execute_select(table),
    }
}

pub fn print_constants() {
    println!("ROW_SIZE: {ROW_SIZE}");
    println!("COMMON_NODE_HEADER_SIZE: {COMMON_NODE_HEADER_SIZE}");
    println!("LEAF_NODE_HEADER_SIZE: {LEAF_NODE_HEADER_SIZE}");
    println!("LEAF_NODE_CELL_SIZE: {LEAF_NODE_CELL_SIZE}");
    println!("LEAF_NODE_SPACE_FOR_CELLS: {LEAF_NODE_SPACE_FOR_CELLS}");
    println!("LEAF_NODE_MAX_CELLS: {LEAF_NODE_MAX_CELLS}");
}
