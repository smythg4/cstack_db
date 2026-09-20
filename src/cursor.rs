use crate::row::Row;
use crate::table::TableError::PagerError;
use crate::table::{LEAF_NODE_MAX_CELLS, Table, TableError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CursorError {
    #[error(transparent)]
    TableError(#[from] TableError),
    #[error("Need to implement splitting a leaf node.")]
    LeafNodeFull,
}
pub struct Cursor<'a> {
    table: &'a mut Table,
    page_num: usize,
    cell_num: usize,
    end_of_table: bool,
}

impl<'a> Cursor<'a> {
    pub fn table_start(table: &'a mut Table) -> Self {
        let end_of_table = table.is_empty();
        let page_num = table.root_page_num();
        Self {
            table,
            page_num,
            cell_num: 0,
            end_of_table,
        }
    }

    pub fn table_end(table: &'a mut Table) -> Self {
        let page_num = table.root_page_num();
        let cell_num = table.root_node_num_cells();
        Self {
            table,
            page_num,
            cell_num,
            end_of_table: true,
        }
    }

    pub fn cursor_value_mut(&mut self) -> Result<&mut [u8], CursorError> {
        Ok(self
            .table
            .get_leaf_value_mut(self.page_num, self.cell_num)?)
    }

    pub fn cursor_advance(&mut self) -> Result<(), CursorError> {
        let node = self.table.get_node_mut(self.page_num)?;
        self.cell_num += 1;
        let num_cells = node.leaf_node_num_cells() as usize;
        if self.cell_num >= num_cells {
            self.end_of_table = true;
        }
        Ok(())
    }

    pub fn at_end(&self) -> bool {
        self.end_of_table
    }

    pub fn leaf_node_insert(&mut self, key: u32, row: &Row) -> Result<(), CursorError> {
        let mut node = self.table.get_node_mut(self.page_num)?;
        let num_cells = node.leaf_node_num_cells() as usize;
        if num_cells >= LEAF_NODE_MAX_CELLS {
            // TODO: implement splitting, then this error goes away
            return Err(CursorError::LeafNodeFull);
        }

        if self.cell_num < num_cells {
            // make room for a new cell
            for i in (self.cell_num..num_cells).rev() {
                node.copy_cell(i - 1, i);
            }
        }

        node.set_leaf_node_num_cells(node.leaf_node_num_cells() + 1);
        node.set_leaf_node_key(self.cell_num, key);
        row.serialize_row(&mut node.leaf_node_value(self.cell_num))
            .map_err(|e| PagerError(e.into()))?;
        Ok(())
    }
}
