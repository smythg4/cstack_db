use crate::constants::*;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::{Deref, DerefMut};
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

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum NodeKind {
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

pub struct Node<G> {
    page: G,
}

impl<G: DerefMut<Target = [u8]>> Node<G> {
    pub fn initialize_leaf_node(&mut self) {
        self.set_node_type(NodeKind::Leaf);
        self.set_leaf_node_num_cells(0)
    }

    pub fn set_leaf_node_num_cells(&mut self, n: u32) {
        self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .copy_from_slice(&n.to_be_bytes());
    }

    pub fn leaf_node_cell(&mut self, cell_num: usize) -> &mut [u8] {
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        &mut self.page[base..base + LEAF_NODE_CELL_SIZE]
    }

    pub fn set_leaf_node_key(&mut self, cell_num: usize, key: u32) {
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        self.page[base..base + 4].copy_from_slice(&key.to_be_bytes());
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

    pub fn set_node_type(&mut self, kind: NodeKind) {
        self.page[NODE_TYPE_OFFSET] = match kind {
            NodeKind::Internal => 0,
            NodeKind::Leaf => 1,
        };
    }
}

impl<G: Deref<Target = [u8]>> std::fmt::Display for Node<G> {
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

impl<G: Deref<Target = [u8]>> Node<G> {
    pub fn leaf_node_num_cells(&self) -> u32 {
        let bytes: [u8; 4] = self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }
    pub fn leaf_node_key(&self, cell_num: usize) -> u32 {
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        let bytes: [u8; 4] = self.page[base..base + 4].try_into().unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_node_type(&self) -> NodeKind {
        match self.page[NODE_TYPE_OFFSET] {
            0 => NodeKind::Internal,
            1 => NodeKind::Leaf,
            _ => unreachable!(),
        }
    }
}

pub type Page = [u8; PAGE_SIZE];

pub struct Pager {
    file_descriptor: RefCell<File>,
    file_length: usize,
    num_pages: Cell<usize>,
    pages: [RefCell<Option<Box<Page>>>; TABLE_MAX_PAGES],
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
        let num_pages = Cell::new(file_length / PAGE_SIZE);
        #[allow(clippy::declare_interior_mutable_const)]
        const EMPTY: RefCell<Option<Box<Page>>> = RefCell::new(None);
        let pages = [EMPTY; TABLE_MAX_PAGES];
        Ok(Self {
            file_descriptor: RefCell::new(file_descriptor),
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

    pub fn get_page(&self, page_num: usize) -> Result<Option<Ref<'_, [u8]>>, PagerError> {
        if !self.is_in_bounds(page_num) {
            return Err(PagerError::OutOfBounds);
        }
        let guard = self.pages[page_num].borrow();
        Ok(Ref::filter_map(guard, |opt| opt.as_deref().map(|p| &p[..])).ok())
    }

    pub fn get_page_mut(&self, page_num: usize) -> Result<RefMut<'_, [u8]>, PagerError> {
        if !self.is_in_bounds(page_num) {
            return Err(PagerError::OutOfBounds);
        }

        if self.pages[page_num].borrow().is_none() {
            let mut page = [0u8; PAGE_SIZE];
            let mut num_pages = self.file_length / PAGE_SIZE;
            if !self.file_length.is_multiple_of(PAGE_SIZE) {
                num_pages += 1;
            }

            let mut fd_guard = self.file_descriptor.borrow_mut();

            if page_num <= num_pages {
                fd_guard.seek(std::io::SeekFrom::Start((page_num * PAGE_SIZE) as u64))?;
                let _ = fd_guard.read(&mut page)?;
            }

            *self.pages[page_num].borrow_mut() = Some(Box::new(page));
            if page_num >= self.num_pages.get() {
                self.num_pages.set(page_num + 1);
            }
        }

        let guard = self.pages[page_num].borrow_mut();
        Ok(RefMut::map(
            guard,
            |opt| &mut opt.as_deref_mut().unwrap()[..],
        ))
    }

    pub fn get_node_mut(&self, page_num: usize) -> Result<Node<RefMut<'_, [u8]>>, PagerError> {
        Ok(Node {
            page: self.get_page_mut(page_num)?,
        })
    }

    pub fn get_node(&self, page_num: usize) -> Result<Option<Node<Ref<'_, [u8]>>>, PagerError> {
        let Some(page) = self.get_page(page_num)? else {
            return Ok(None);
        };
        Ok(Some(Node { page }))
    }

    fn pager_flush(&self, page_num: usize) -> Result<(), PagerError> {
        let guard = self
            .pages
            .get(page_num)
            .ok_or(PagerError::OutOfBounds)?
            .borrow();
        match guard.as_deref() {
            Some(page) => {
                let mut fd_guard = self.file_descriptor.borrow_mut();
                fd_guard.seek(SeekFrom::Start((page_num * PAGE_SIZE) as u64))?;
                fd_guard.write_all(page)?;
                Ok(())
            }
            None => Err(PagerError::NullFlush),
        }
    }

    fn is_cached(&self, page_num: usize) -> bool {
        self.pages[page_num].borrow().is_some()
    }

    fn is_in_bounds(&self, page_num: usize) -> bool {
        page_num <= TABLE_MAX_PAGES
    }
}

pub struct Table {
    pager: Pager,
    root_page_num: usize,
}

impl Table {
    pub fn db_open<P: AsRef<Path>>(path: P) -> Result<Self, TableError> {
        let pager = Pager::pager_open(path)?;
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

    pub fn get_page(&self, page_num: usize) -> Result<Option<Ref<'_, [u8]>>, TableError> {
        Ok(self.pager.get_page(page_num)?)
    }

    pub fn get_page_mut(&self, page_num: usize) -> Result<RefMut<'_, [u8]>, TableError> {
        Ok(self.pager.get_page_mut(page_num)?)
    }

    pub fn len(&self) -> usize {
        self.root_node_num_cells()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn root_node_num_cells(&self) -> usize {
        self.get_node_mut(self.root_page_num)
            .unwrap()
            .leaf_node_num_cells() as usize
    }

    pub fn get_node(&self, page_num: usize) -> Result<Option<Node<Ref<'_, [u8]>>>, TableError> {
        Ok(self.pager.get_node(page_num)?)
    }

    pub fn get_node_mut(&self, page_num: usize) -> Result<Node<RefMut<'_, [u8]>>, TableError> {
        Ok(self.pager.get_node_mut(page_num)?)
    }

    pub fn root_page_num(&self) -> usize {
        self.root_page_num
    }

    pub fn get_leaf_value_mut(
        &self,
        page_num: usize,
        cell_num: usize,
    ) -> Result<RefMut<'_, [u8]>, TableError> {
        let page = self.get_page_mut(page_num)?;
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE + LEAF_NODE_KEY_SIZE;
        Ok(RefMut::map(page, |p| {
            &mut p[base..base + LEAF_NODE_VALUE_SIZE]
        }))
    }

    pub fn get_leaf_value(
        &self,
        page_num: usize,
        cell_num: usize,
    ) -> Result<Option<Ref<'_, [u8]>>, TableError> {
        let page = self.get_page(page_num)?;
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE + LEAF_NODE_KEY_SIZE;
        Ok(page
            .map(|page_ref| Ref::map(page_ref, |bytes| &bytes[base..base + LEAF_NODE_VALUE_SIZE])))
    }

    pub fn flush_all(&self) -> Result<(), TableError> {
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
