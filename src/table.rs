use crate::constants::*;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PagerError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("Tried to fetch page number out of bounds. {TABLE_MAX_PAGES}")]
    OutOfBounds,
    #[error("Tried to flush null page")]
    NullFlush,
    #[error("Db file is not a whole number of pages. Corrupt file.")]
    PartialPageSize,
}

#[derive(Error, Debug)]
pub enum TableError {
    #[error(transparent)]
    PagerError(#[from] PagerError),
}

pub enum NodeType {
    Leaf,
    Internal,
}

// Common node header layout
pub const NODE_TYPE_SIZE: usize = size_of::<u8>();
pub const NODE_TYPE_OFFSET: usize = 0;
pub const IS_ROOT_SIZE: usize = size_of::<u8>();
pub const IS_ROOT_OFFSET: usize = NODE_TYPE_SIZE;
pub const PARENT_POINTER_SIZE: usize = size_of::<u32>();
pub const PARENT_POINTER_OFFSET: usize = IS_ROOT_OFFSET + IS_ROOT_SIZE;
pub const COMMON_NODE_HEADER_SIZE: usize = NODE_TYPE_SIZE + IS_ROOT_SIZE + PARENT_POINTER_SIZE;

// Leaf node header layout
pub const LEAF_NODE_NUM_CELLS_SIZE: usize = size_of::<u32>();
pub const LEAF_NODE_NUM_CELLS_OFFSET: usize = COMMON_NODE_HEADER_SIZE;
pub const LEAF_NODE_HEADER_SIZE: usize = COMMON_NODE_HEADER_SIZE + LEAF_NODE_NUM_CELLS_SIZE;

// Left node body layout
pub const LEAF_NODE_KEY_SIZE: usize = size_of::<u32>();
pub const LEAF_NODE_KEY_OFFSET: usize = 0;
pub const LEAF_NODE_VALUE_SIZE: usize = ROW_SIZE;
pub const LEAF_NODE_VALUE_OFFSET: usize = LEAF_NODE_KEY_OFFSET + LEAF_NODE_KEY_SIZE;
pub const LEAF_NODE_CELL_SIZE: usize = LEAF_NODE_KEY_SIZE + LEAF_NODE_VALUE_SIZE;
pub const LEAF_NODE_SPACE_FOR_CELLS: usize = PAGE_SIZE - LEAF_NODE_HEADER_SIZE;
pub const LEAF_NODE_MAX_CELLS: usize = LEAF_NODE_SPACE_FOR_CELLS / LEAF_NODE_CELL_SIZE;

pub struct Node<'a> {
    page: &'a mut [u8],
}

impl<'a> Node<'a> {
    pub fn initialize_leaf_node(&mut self) {
        self.set_leaf_node_num_cells(0)
    }

    pub fn set_leaf_node_num_cells(&mut self, n: u32) {
        self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .copy_from_slice(&n.to_be_bytes());
    }

    pub fn leaf_node_num_cells(&self) -> u32 {
        let bytes: [u8; 4] = self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn leaf_node_cell(&mut self, cell_num: usize) -> &mut [u8] {
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        &mut self.page[base..base + LEAF_NODE_CELL_SIZE]
    }

    pub fn set_leaf_node_key(&mut self, cell_num: usize, key: u32) {
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        self.page[base..base + 4].copy_from_slice(&key.to_be_bytes());
    }

    pub fn leaf_node_key(&self, cell_num: usize) -> u32 {
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        let bytes: [u8; 4] = self.page[base..base + 4].try_into().unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn leaf_node_value(&mut self, cell_num: usize) -> &mut [u8] {
        let cell = self.leaf_node_cell(cell_num);
        &mut cell[LEAF_NODE_KEY_SIZE..]
    }

    pub fn copy_cell(&mut self, from_cell: usize, to_cell: usize) {
        let from_base = LEAF_NODE_HEADER_SIZE + from_cell * LEAF_NODE_CELL_SIZE;
        let to_base = LEAF_NODE_HEADER_SIZE + to_cell * LEAF_NODE_CELL_SIZE;
        self.page
            .copy_within(from_base..from_base + LEAF_NODE_CELL_SIZE, to_base);
    }
}

impl<'a> std::fmt::Display for Node<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let num_cells = self.leaf_node_num_cells() as usize;
        writeln!(f, "leaf (size {num_cells})")?;
        for i in 0..num_cells {
            let key = self.leaf_node_key(i);
            writeln!(f, "  - {i} : {key}")?;
        }
        Ok(())
    }
}

pub type Page = [u8; PAGE_SIZE];

pub struct Pager {
    file_descriptor: File,
    file_length: usize,
    num_pages: usize,
    pages: [Option<Box<Page>>; TABLE_MAX_PAGES],
}

impl Pager {
    pub fn pager_open<P: AsRef<Path>>(path: P) -> Result<Self, PagerError> {
        let path = path.as_ref();
        let file_descriptor = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .read(true)
            .open(path)?;
        let file_length = file_descriptor.metadata()?.len() as usize;
        if !file_length.is_multiple_of(PAGE_SIZE) {
            return Err(PagerError::PartialPageSize);
        }
        let num_pages = file_length / PAGE_SIZE;
        const EMPTY: Option<Box<Page>> = None;
        let pages = [EMPTY; TABLE_MAX_PAGES];
        Ok(Self {
            file_descriptor,
            file_length,
            num_pages,
            pages,
        })
    }

    pub fn len(&self) -> usize {
        self.file_length
    }

    pub fn is_empty(&self) -> bool {
        self.file_length == 0
    }

    pub fn get_page(&mut self, page_num: usize) -> Result<Option<&[u8; PAGE_SIZE]>, PagerError> {
        Ok(Some(&*self.get_page_mut(page_num)?))
    }

    pub fn get_page_mut(&mut self, page_num: usize) -> Result<&mut [u8; PAGE_SIZE], PagerError> {
        if self.is_in_bounds(page_num) {
            return Err(PagerError::OutOfBounds);
        }

        if self.pages[page_num].is_none() {
            let mut page = [0u8; PAGE_SIZE];
            let mut num_pages = self.file_length / PAGE_SIZE;
            if !self.file_length.is_multiple_of(PAGE_SIZE) {
                num_pages += 1;
            }

            if page_num <= num_pages {
                self.file_descriptor
                    .seek(std::io::SeekFrom::Start((page_num * PAGE_SIZE) as u64))?;
                let _ = self.file_descriptor.read(&mut page)?;
            }

            self.pages[page_num] = Some(Box::new(page));
            if page_num >= self.num_pages {
                self.num_pages = page_num + 1;
            }
        }

        Ok(self.pages[page_num].as_deref_mut().unwrap())
    }

    pub fn get_node_mut(&mut self, page_num: usize) -> Result<Node<'_>, PagerError> {
        Ok(Node {
            page: self.get_page_mut(page_num)?,
        })
    }

    fn pager_flush(&mut self, page_num: usize) -> Result<(), PagerError> {
        if self.is_cached(page_num) {
            match self.pages.get(page_num) {
                Some(Some(page)) => {
                    self.file_descriptor
                        .seek(std::io::SeekFrom::Start((page_num * PAGE_SIZE) as u64))?;
                    self.file_descriptor.write_all(&page[..PAGE_SIZE])?;
                }
                Some(None) => return Err(PagerError::NullFlush),
                None => return Err(PagerError::OutOfBounds),
            }
        }
        Ok(())
    }

    fn is_cached(&self, page_num: usize) -> bool {
        self.pages[page_num].is_some()
    }

    fn is_in_bounds(&self, page_num: usize) -> bool {
        page_num > TABLE_MAX_PAGES
    }
}

pub struct Table {
    pager: Pager,
    root_page_num: usize,
}

impl Table {
    pub fn db_open<P: AsRef<Path>>(path: P) -> Result<Self, TableError> {
        let mut pager = Pager::pager_open(path)?;
        let root_page_num = 0;

        if pager.is_empty() {
            let mut root_node = pager.get_node_mut(root_page_num)?;
            root_node.initialize_leaf_node();
        }

        Ok(Self {
            pager,
            root_page_num,
        })
    }

    pub fn get_page(&mut self, page_num: usize) -> Result<Option<&[u8; PAGE_SIZE]>, TableError> {
        Ok(self.pager.get_page(page_num)?)
    }

    pub fn get_page_mut(&mut self, page_num: usize) -> Result<&mut [u8; PAGE_SIZE], TableError> {
        Ok(self.pager.get_page_mut(page_num)?)
    }

    pub fn len(&mut self) -> usize {
        self.root_node_num_cells()
    }

    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    pub fn root_node_num_cells(&mut self) -> usize {
        self.get_node_mut(self.root_page_num)
            .unwrap()
            .leaf_node_num_cells() as usize
    }

    pub fn get_node_mut(&mut self, page_num: usize) -> Result<Node<'_>, TableError> {
        Ok(self.pager.get_node_mut(page_num)?)
    }

    pub fn root_page_num(&self) -> usize {
        self.root_page_num
    }

    pub fn get_leaf_value_mut(
        &mut self,
        page_num: usize,
        cell_num: usize,
    ) -> Result<&mut [u8], TableError> {
        let page = self.get_page_mut(page_num)?;
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE + LEAF_NODE_KEY_SIZE;
        Ok(&mut page[base..base + LEAF_NODE_VALUE_SIZE])
    }

    pub fn flush_all(&mut self) -> Result<(), TableError> {
        for page_num in 0..self.len() {
            if self.pager.is_cached(page_num) {
                self.pager.pager_flush(page_num)?;
            }
        }
        Ok(())
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        self.flush_all().expect("failed to flush a page");
    }
}
