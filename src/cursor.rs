use crate::row::Row;
use crate::table::TableError;
use crate::table::{NodeKind, Table};
use std::cell::{Ref, RefMut};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CursorError {
    #[error(transparent)]
    TableError(#[from] TableError),
    #[error("Need to implement searching an internal node")]
    InternalNodeSearch,
    #[error("Need to implement splitting a leaf node.")]
    LeafNodeFull,
    #[error("Need to implement updating parent after split")]
    ParentUpdate,
}
pub struct Cursor<'a> {
    table: &'a Table,
    page_num: usize,
    cell_num: usize,
    end_of_table: bool,
}

impl<'a> Cursor<'a> {
    pub fn table_start(table: &'a Table) -> Result<Self, CursorError> {
        let root_page_num = table.root_page_num();
        let mut cursor = Self::table_find(table, root_page_num as u32)?;
        let num_cells = table.get_node_mut(cursor.page_num)?.leaf_node_num_cells();
        cursor.end_of_table = num_cells == 0;

        Ok(cursor)
    }

    pub fn table_find(table: &'a Table, key: u32) -> Result<Self, CursorError> {
        let root_page_num = table.root_page_num();
        let node_type = table.get_node_mut(root_page_num)?.get_node_type();
        match node_type {
            NodeKind::Leaf => Self::leaf_node_find(table, root_page_num, key),
            NodeKind::Internal => Self::internal_node_find(table, root_page_num, key),
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
            let next_page_num = node.get_leaf_node_next_leaf();
            if next_page_num == 0 {
                self.end_of_table = true;
            } else {
                self.page_num = next_page_num as usize;
                self.cell_num = 0;
            }
        }
        Ok(())
    }

    pub fn at_end(&self) -> bool {
        self.end_of_table
    }

    pub fn leaf_node_insert(&self, key: u32, row: &Row) -> Result<(), CursorError> {
        Ok(self
            .table
            .leaf_node_insert(self.page_num, self.cell_num, key, row)?)
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
            let key_at_index = node.get_leaf_node_key(index);
            if key == key_at_index {
                return Ok(Self {
                    table,
                    page_num,
                    cell_num: index,
                    end_of_table: num_cells == 0,
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
            end_of_table: num_cells == 0,
        })
    }

    pub fn internal_node_find(
        table: &'a Table,
        page_num: usize,
        key: u32,
    ) -> Result<Self, CursorError> {
        let child_index = Self::internal_node_find_child(table, page_num, key)?;
        let node = table.get_node_mut(page_num)?;
        let child_num = node.get_internal_node_child(child_index)? as usize;
        drop(node);

        let child_type = table.get_node_mut(child_num)?.get_node_type();

        match child_type {
            NodeKind::Internal => Self::internal_node_find(table, child_num, key),
            NodeKind::Leaf => Self::leaf_node_find(table, child_num, key),
        }
    }

    pub fn internal_node_find_child(
        table: &'a Table,
        page_num: usize,
        key: u32,
    ) -> Result<usize, CursorError> {
        let node = table.get_node_mut(page_num)?;
        let num_keys = node.get_internal_node_num_keys() as usize;

        let mut min_index = 0;
        let mut max_index = num_keys;
        while max_index != min_index {
            let index = (min_index + max_index) / 2;
            if node.get_internal_node_key(index) >= key {
                max_index = index;
            } else {
                min_index = index + 1;
            }
        }
        Ok(min_index)
    }
}
