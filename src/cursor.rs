use crate::table::{Table, TableError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CursorError {
    #[error(transparent)]
    TableError(#[from] TableError),
}
pub struct Cursor<'a> {
    table: &'a mut Table,
    row_num: usize,
    end_of_table: bool,
}

impl<'a> Cursor<'a> {
    pub fn table_start(table: &'a mut Table) -> Self {
        let end_of_table = table.is_empty();
        Self {
            table,
            row_num: 0,
            end_of_table,
        }
    }

    pub fn table_end(table: &'a mut Table) -> Self {
        let row_num = table.len();
        Self {
            table,
            row_num,
            end_of_table: true,
        }
    }

    pub fn cursor_value(&mut self) -> Result<Option<&[u8]>, CursorError> {
        Ok(self.table.get_row(self.row_num)?)
    }

    pub fn cursor_value_mut(&mut self) -> Result<&mut [u8], CursorError> {
        Ok(self.table.get_row_mut(self.row_num)?)
    }

    pub fn cursor_advance(&mut self) {
        self.row_num += 1;
        if self.row_num >= self.table.len() {
            self.end_of_table = true;
        }
    }

    pub fn at_end(&self) -> bool {
        self.end_of_table
    }
}
