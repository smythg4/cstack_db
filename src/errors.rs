use crate::cursor::CursorError;
use crate::table::TableError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetaCommandError {
    #[error("Unrecognized command '{0}'.")]
    Unrecognized(String),
}

#[derive(Error, Debug)]
pub enum PrepareError {
    #[error("Syntax error. Could not parse statement.")]
    SyntaxError,
    #[error("String is too long.")]
    StringTooLong,
    #[error("ID must be positive.")]
    NegativeId,
    #[error("Unrecognized keyword at start of '{0}'.")]
    Unrecognized(String),
}

#[derive(Error, Debug)]
pub enum ExecuteError {
    #[error("Error: Table full.")]
    TableFull,
    #[error("Error: Page Not Found")]
    PageNotFound,
    #[error("Error: Io Error")]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    TableError(#[from] TableError),
    #[error(transparent)]
    CursorError(#[from] CursorError),
}

impl ExecuteError {
    pub fn is_fatal(&self) -> bool {
        match self {
            ExecuteError::TableFull => false,
            ExecuteError::PageNotFound => true,
            ExecuteError::IoError(_) => true,
            ExecuteError::TableError(_) => true,
            ExecuteError::CursorError(_) => true,
        }
    }
}
