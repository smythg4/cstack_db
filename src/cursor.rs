use crate::row::Row;
use crate::table::{
    LEAF_NODE_LEFT_SPLIT_COUNT, LEAF_NODE_MAX_CELLS, LEAF_NODE_RIGHT_SPLIT_COUNT, NodeKind, Table,
};
use crate::table::{PagerError, TableError};
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
    #[error("Need to implement updating parent after split")]
    ParentUpdate,
    #[error("Node not found {0}")]
    NodeNotFound(usize),
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

        let node = table.get_node_mut(cursor.page_num)?;

        let num_cells = node.leaf_node_num_cells();
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
        let num_cells = self
            .table
            .get_node_mut(self.page_num)?
            .leaf_node_num_cells() as usize;
        if num_cells >= LEAF_NODE_MAX_CELLS {
            return self.leaf_node_split_and_insert(key, row);
        }

        let mut node = self.table.get_node_mut(self.page_num)?;
        if self.cell_num < num_cells {
            let key_at_index = node.get_leaf_node_key(self.cell_num);
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
            .map_err(|e| TableError::PagerError(e.into()))?;
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
            let key_at_index = node.get_leaf_node_key(index);
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

    pub fn internal_node_find(
        table: &'a Table,
        page_num: usize,
        key: u32,
    ) -> Result<Self, CursorError> {
        let child_num = {
            let node = match table.get_node(page_num)? {
                Some(n) => n,
                None => return Err(CursorError::NodeNotFound(page_num)),
            };
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
            node.get_internal_node_child(min_index) as usize
        }; // node's guard drops here

        let child_type = table.get_node_mut(child_num)?.get_node_type();

        match child_type {
            NodeKind::Internal => Self::internal_node_find(table, child_num, key),
            NodeKind::Leaf => Self::leaf_node_find(table, child_num, key),
        }
    }

    pub fn leaf_node_split_and_insert(&self, key: u32, row: &Row) -> Result<(), CursorError> {
        let mut old_node = self.table.get_node_mut(self.page_num)?;
        let old_next = old_node.get_leaf_node_next_leaf();

        let new_page_num = self.table.get_unused_page_num();
        old_node.set_leaf_node_next_leaf(new_page_num as u32);

        let mut new_node = self.table.get_node_mut(new_page_num)?;
        new_node.initialize_leaf_node();
        new_node.set_leaf_node_next_leaf(old_next);

        for i in (0..=LEAF_NODE_MAX_CELLS).rev() {
            let index_within_node = i % LEAF_NODE_LEFT_SPLIT_COUNT;
            let dest_is_new = i >= LEAF_NODE_LEFT_SPLIT_COUNT;

            if i == self.cell_num {
                let dest_node = if dest_is_new {
                    &mut new_node
                } else {
                    &mut old_node
                };
                dest_node.set_leaf_node_key(index_within_node, key);
                row.serialize_row(&mut dest_node.leaf_node_value(index_within_node))
                    .map_err(|e| TableError::PagerError(PagerError::IoError(e)))?;
            } else {
                let src_index = if i > self.cell_num { i - 1 } else { i };
                if dest_is_new {
                    new_node.copy_cell_from(index_within_node, &old_node, src_index);
                } else {
                    old_node.copy_cell(src_index, index_within_node);
                }
            }
        }
        old_node.set_leaf_node_num_cells(LEAF_NODE_LEFT_SPLIT_COUNT as u32);
        new_node.set_leaf_node_num_cells(LEAF_NODE_RIGHT_SPLIT_COUNT as u32);

        let is_root = self.is_node_root();
        drop(old_node);
        drop(new_node);

        if is_root {
            self.table.create_new_root(new_page_num)?;
            Ok(())
        } else {
            Err(CursorError::ParentUpdate)
        }
    }

    pub fn is_node_root(&self) -> bool {
        self.page_num == self.table.root_page_num()
    }
}
