use crate::row::Row;
use crate::table::TableError::PagerError;
use crate::table::{LEAF_NODE_MAX_CELLS, NodeKind, Table, TableError};
use std::cell::{Ref, RefMut};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CursorError {
    #[error(transparent)]
    TableError(#[from] TableError),
    #[error("Duplicate key: {0}")]
    DuplicateKey(u32),
    #[error("Need to implement searching an internal node")]
    InternalNodeSearch,
    #[error("Need to implement splitting a leaf node.")]
    LeafNodeFull,
}
pub struct Cursor<'a> {
    table: &'a Table,
    page_num: usize,
    cell_num: usize,
    end_of_table: bool,
}

impl<'a> Cursor<'a> {
    pub fn table_start(table: &'a Table) -> Self {
        let end_of_table = table.is_empty();
        let page_num = table.root_page_num();
        Self {
            table,
            page_num,
            cell_num: 0,
            end_of_table,
        }
    }

    pub fn table_end(table: &'a Table) -> Self {
        let page_num = table.root_page_num();
        let cell_num = table.root_node_num_cells();
        Self {
            table,
            page_num,
            cell_num,
            end_of_table: true,
        }
    }

    pub fn table_find(table: &'a Table, key: u32) -> Result<Self, CursorError> {
        let root_page_num = table.root_page_num();
        let node_type = table.get_node_mut(root_page_num)?.get_node_type();
        match node_type {
            NodeKind::Leaf => Self::leaf_node_find(table, root_page_num, key),
            NodeKind::Internal => Err(CursorError::InternalNodeSearch),
        }
    }

    pub fn cursor_value(&self) -> Result<Option<Ref<'_, [u8]>>, CursorError> {
        Ok(self.table.get_leaf_value(self.page_num, self.cell_num)?)
    }

    pub fn cursor_value_mut(&self) -> Result<RefMut<'_, [u8]>, CursorError> {
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

    pub fn leaf_node_insert(&self, key: u32, row: &Row) -> Result<(), CursorError> {
        let mut node = self.table.get_node_mut(self.page_num)?;
        let num_cells = node.leaf_node_num_cells() as usize;
        if num_cells >= LEAF_NODE_MAX_CELLS {
            // TODO: implement splitting, then this error goes away
            return Err(CursorError::LeafNodeFull);
        }

        if self.cell_num < num_cells {
            let key_at_index = node.leaf_node_key(self.cell_num);
            if key_at_index == key {
                return Err(CursorError::DuplicateKey(key));
            }
            // make room for a new cell
            for i in (self.cell_num + 1..=num_cells).rev() {
                node.copy_cell(i - 1, i);
            }
        }

        node.set_leaf_node_num_cells(node.leaf_node_num_cells() + 1);
        node.set_leaf_node_key(self.cell_num, key);
        row.serialize_row(&mut node.leaf_node_value(self.cell_num))
            .map_err(|e| PagerError(e.into()))?;
        Ok(())
    }

    pub fn leaf_node_find(
        table: &'a Table,
        page_num: usize,
        key: u32,
    ) -> Result<Self, CursorError> {
        let node = table.get_node_mut(page_num)?;
        let num_cells = node.leaf_node_num_cells() as usize;

        // Binary search
        let mut min_index = 0;
        let mut one_past_max_index = num_cells;
        while one_past_max_index != min_index {
            let index = (min_index + one_past_max_index) / 2;
            let key_at_index = node.leaf_node_key(index);
            if key == key_at_index {
                return Ok(Self {
                    table,
                    page_num,
                    cell_num: index,
                    end_of_table: false,
                });
            }
            if key < key_at_index {
                one_past_max_index = index;
            } else {
                min_index = index + 1;
            }
        }

        Ok(Self {
            table,
            page_num,
            cell_num: min_index,
            end_of_table: false,
        })
    }
}
