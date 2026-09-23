use crate::constants::*;
use crate::row::Row;
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
    #[error("Tried to fetch page number ({0}) out of bounds. Max: {TABLE_MAX_PAGES}")]
    OutOfBounds(usize),
    #[error("Tried to flush null page")]
    NullFlush,
    #[error("Db file is not a whole number of pages. Corrupt file.")]
    PartialPageSize,
}

#[derive(Error, Debug)]
pub enum TableError {
    #[error(transparent)]
    PagerError(#[from] PagerError),
    #[error("Need to implement splitting internal node")]
    InternalNodeFull,
    #[error("Duplicate key: {0}")]
    DuplicateKey(u32),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("Node not found {0}")]
    NodeNotFound(usize),
    #[error("Tried to access right child of node, but was invalid page")]
    RightChildInvalid(u32),
    #[error("Tried to access child %d of node, but was invalid page")]
    ChildInvalid(u32),
    #[error("Tried to access child_num {0} > num_keys {1}")]
    PageOutOfBounds(usize, usize),
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
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
pub const LEAF_NODE_NEXT_LEAF_SIZE: usize = size_of::<u32>();
pub const LEAF_NODE_NEXT_LEAF_OFFSET: usize = LEAF_NODE_NUM_CELLS_OFFSET + LEAF_NODE_NUM_CELLS_SIZE;
pub const LEAF_NODE_HEADER_SIZE: usize =
    COMMON_NODE_HEADER_SIZE + LEAF_NODE_NUM_CELLS_SIZE + LEAF_NODE_NUM_CELLS_SIZE;

// Leaf node body layout
pub const LEAF_NODE_KEY_SIZE: usize = size_of::<u32>();
pub const LEAF_NODE_KEY_OFFSET: usize = 0;
pub const LEAF_NODE_VALUE_SIZE: usize = ROW_SIZE;
pub const LEAF_NODE_VALUE_OFFSET: usize = LEAF_NODE_KEY_OFFSET + LEAF_NODE_KEY_SIZE;
pub const LEAF_NODE_CELL_SIZE: usize = LEAF_NODE_KEY_SIZE + LEAF_NODE_VALUE_SIZE;
pub const LEAF_NODE_SPACE_FOR_CELLS: usize = PAGE_SIZE - LEAF_NODE_HEADER_SIZE;
pub const LEAF_NODE_MAX_CELLS: usize = LEAF_NODE_SPACE_FOR_CELLS / LEAF_NODE_CELL_SIZE;

// Leaf node split parameters
pub const LEAF_NODE_RIGHT_SPLIT_COUNT: usize = LEAF_NODE_MAX_CELLS.div_ceil(2);
pub const LEAF_NODE_LEFT_SPLIT_COUNT: usize =
    (LEAF_NODE_MAX_CELLS + 1) - LEAF_NODE_RIGHT_SPLIT_COUNT;

// Internal Node header layout
pub const INTERNAL_NODE_NUM_KEY_SIZE: usize = size_of::<u32>();
pub const INTERNAL_NODE_NUM_KEYS_OFFSET: usize = COMMON_NODE_HEADER_SIZE;
pub const INTERNAL_NODE_RIGHT_CHILD_SIZE: usize = size_of::<u32>();
pub const INTERNAL_NODE_RIGHT_CHILD_OFFSET: usize =
    INTERNAL_NODE_NUM_KEYS_OFFSET + INTERNAL_NODE_NUM_KEY_SIZE;
pub const INTERNAL_NODE_HEADER_SIZE: usize =
    COMMON_NODE_HEADER_SIZE + INTERNAL_NODE_NUM_KEY_SIZE + INTERNAL_NODE_RIGHT_CHILD_SIZE;

// Internal Node body layout
pub const INTERNAL_NODE_KEY_SIZE: usize = size_of::<u32>();
pub const INTERNAL_NODE_CHILD_SIZE: usize = size_of::<u32>();
pub const INTERNAL_NODE_CELL_SIZE: usize = INTERNAL_NODE_CHILD_SIZE + INTERNAL_NODE_KEY_SIZE;
pub const INTERNAL_NODE_MAX_CELLS: usize = 3;
pub const INVALID_PAGE_NUM: usize = u32::MAX as usize;

// Internal node split parameters
pub const INTERNAL_NODE_LEFT_SPLIT_COUNT: usize = (INTERNAL_NODE_MAX_CELLS + 2) / 2;
pub const INTERNAL_NODE_RIGHT_SPLIT_COUNT: usize =
    (INTERNAL_NODE_MAX_CELLS + 2) - INTERNAL_NODE_LEFT_SPLIT_COUNT;

pub struct Node<G> {
    page: G,
}

impl<G: DerefMut<Target = [u8]>> Node<G> {
    pub fn initialize_leaf_node(&mut self) {
        self.set_node_type(NodeKind::Leaf);
        self.set_root_node(false);
        self.set_leaf_node_num_cells(0);
        self.set_leaf_node_next_leaf(0);
    }

    pub fn initialize_internal_node(&mut self) {
        self.set_node_type(NodeKind::Internal);
        self.set_root_node(false);
        self.set_internal_node_right_child(INVALID_PAGE_NUM);
        self.set_internal_node_num_cells(0)
    }

    pub fn set_leaf_node_num_cells(&mut self, n: u32) {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .copy_from_slice(&n.to_be_bytes());
    }

    pub fn set_internal_node_num_cells(&mut self, n: u32) {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        self.page[INTERNAL_NODE_NUM_KEYS_OFFSET..INTERNAL_NODE_NUM_KEYS_OFFSET + 4]
            .copy_from_slice(&n.to_be_bytes());
    }

    pub fn leaf_node_cell(&mut self, cell_num: usize) -> &mut [u8] {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        &mut self.page[base..base + LEAF_NODE_CELL_SIZE]
    }

    pub fn set_leaf_node_key(&mut self, cell_num: usize, key: u32) {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        self.page[base..base + 4].copy_from_slice(&key.to_be_bytes());
    }

    pub fn leaf_node_value(&mut self, cell_num: usize) -> &mut [u8] {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        let cell = self.leaf_node_cell(cell_num);
        &mut cell[LEAF_NODE_KEY_SIZE..]
    }

    pub fn copy_leaf_cell(&mut self, from_cell: usize, to_cell: usize) {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        let from_base = LEAF_NODE_HEADER_SIZE + from_cell * LEAF_NODE_CELL_SIZE;
        let to_base = LEAF_NODE_HEADER_SIZE + to_cell * LEAF_NODE_CELL_SIZE;
        self.page
            .copy_within(from_base..from_base + LEAF_NODE_CELL_SIZE, to_base);
    }

    pub fn copy_internal_cell(&mut self, from_cell: usize, to_cell: usize) {
        assert_eq!(self.get_node_type(), NodeKind::Internal);

        let from_base = INTERNAL_NODE_HEADER_SIZE + from_cell * INTERNAL_NODE_CELL_SIZE;
        let to_base = INTERNAL_NODE_HEADER_SIZE + to_cell * INTERNAL_NODE_CELL_SIZE;
        self.page
            .copy_within(from_base..from_base + INTERNAL_NODE_CELL_SIZE, to_base);
    }

    pub fn copy_cell_from<G2: Deref<Target = [u8]>>(
        &mut self,
        to_cell: usize,
        source: &Node<G2>,
        from_cell: usize,
    ) {
        let to_base = LEAF_NODE_HEADER_SIZE + to_cell * LEAF_NODE_CELL_SIZE;
        let from_base = LEAF_NODE_HEADER_SIZE + from_cell * LEAF_NODE_CELL_SIZE;
        self.page[to_base..to_base + LEAF_NODE_CELL_SIZE]
            .copy_from_slice(&source.page[from_base..from_base + LEAF_NODE_CELL_SIZE]);
    }

    pub fn set_node_type(&mut self, kind: NodeKind) {
        self.page[NODE_TYPE_OFFSET] = match kind {
            NodeKind::Internal => 0,
            NodeKind::Leaf => 1,
        };
    }

    pub fn set_internal_node_num_keys(&mut self, num_keys: usize) {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        self.page[INTERNAL_NODE_NUM_KEYS_OFFSET
            ..INTERNAL_NODE_NUM_KEYS_OFFSET + INTERNAL_NODE_NUM_KEY_SIZE]
            .copy_from_slice(&(num_keys as u32).to_be_bytes());
    }

    pub fn set_internal_node_right_child(&mut self, right_child_index: usize) {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        self.page[INTERNAL_NODE_RIGHT_CHILD_OFFSET
            ..INTERNAL_NODE_RIGHT_CHILD_OFFSET + INTERNAL_NODE_RIGHT_CHILD_SIZE]
            .copy_from_slice(&(right_child_index as u32).to_be_bytes());
    }

    pub fn set_internal_node_key(&mut self, key_num: usize, key: u32) {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let cell = self.internal_node_cell(key_num);
        cell[INTERNAL_NODE_CHILD_SIZE..INTERNAL_NODE_CHILD_SIZE + INTERNAL_NODE_KEY_SIZE]
            .copy_from_slice(&key.to_be_bytes());
    }

    pub fn set_internal_node_child(&mut self, child_num: usize, page_num: usize) {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let num_keys = self.get_internal_node_num_keys() as usize;
        if child_num == num_keys {
            self.set_internal_node_right_child(page_num);
        } else {
            self.internal_node_cell(child_num)[..INTERNAL_NODE_CHILD_SIZE]
                .copy_from_slice(&(page_num as u32).to_be_bytes());
        }
    }

    pub fn set_leaf_node_next_leaf(&mut self, next_leaf: u32) {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        self.page
            [LEAF_NODE_NEXT_LEAF_OFFSET..LEAF_NODE_NEXT_LEAF_OFFSET + LEAF_NODE_NEXT_LEAF_SIZE]
            .copy_from_slice(&next_leaf.to_be_bytes());
    }

    pub fn internal_node_cell(&mut self, cell_num: usize) -> &mut [u8] {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE + cell_num * INTERNAL_NODE_CELL_SIZE;
        &mut self.page[base..base + INTERNAL_NODE_CELL_SIZE]
    }

    pub fn internal_node_child(&mut self, child_num: usize) -> &mut [u8] {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let num_keys = self.get_internal_node_num_keys() as usize;
        if child_num > num_keys {
            println!("Tried to access child_num {child_num} > num_keys {num_keys}");
            std::process::exit(1);
        } else if child_num == num_keys {
            self.internal_node_right_child()
        } else {
            self.internal_node_cell(child_num)
        }
    }

    pub fn internal_node_right_child(&mut self) -> &mut [u8] {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE + INTERNAL_NODE_RIGHT_CHILD_OFFSET;
        &mut self.page[base..base + INTERNAL_NODE_RIGHT_CHILD_SIZE]
    }

    pub fn set_root_node(&mut self, root: bool) {
        self.page[IS_ROOT_OFFSET] = match root {
            false => 0u8,
            true => 1u8,
        }
    }

    pub fn set_node_parent(&mut self, parent_id: u32) {
        self.page[PARENT_POINTER_OFFSET..PARENT_POINTER_OFFSET + PARENT_POINTER_SIZE]
            .copy_from_slice(&parent_id.to_be_bytes())
    }

    fn write_entries(&mut self, entries: &[(u32, u32)]) {
        self.set_internal_node_num_keys(entries.len() - 1);
        for (i, &(child, key)) in entries.iter().enumerate() {
            if i == entries.len() - 1 {
                self.set_internal_node_right_child(child as usize);
            } else {
                self.set_internal_node_child(i, child as usize);
                self.set_internal_node_key(i, key);
            }
        }
    }
}

impl<G: Deref<Target = [u8]>> Node<G> {
    pub fn leaf_node_num_cells(&self) -> u32 {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
        let bytes: [u8; 4] = self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_leaf_node_key(&self, cell_num: usize) -> u32 {
        assert_eq!(self.get_node_type(), NodeKind::Leaf);
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

    pub fn get_internal_node_num_keys(&self) -> u32 {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let bytes: [u8; 4] = self.page[INTERNAL_NODE_NUM_KEYS_OFFSET
            ..INTERNAL_NODE_NUM_KEYS_OFFSET + INTERNAL_NODE_NUM_KEY_SIZE]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_internal_node_child(&self, child_num: usize) -> Result<u32, TableError> {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let num_keys = self.get_internal_node_num_keys() as usize;
        if child_num > num_keys {
            Err(TableError::PageOutOfBounds(child_num, num_keys))
        } else if child_num == num_keys {
            let right_child = self.get_internal_node_right_child();
            if right_child as usize == INVALID_PAGE_NUM {
                Err(TableError::RightChildInvalid(right_child))
            } else {
                Ok(right_child)
            }
        } else {
            let bytes: [u8; 4] = self.get_internal_node_cell(child_num)[..INTERNAL_NODE_CHILD_SIZE]
                .try_into()
                .unwrap();
            let child = u32::from_be_bytes(bytes);
            if child as usize == INVALID_PAGE_NUM {
                Err(TableError::ChildInvalid(child))
            } else {
                Ok(child)
            }
        }
    }

    pub fn get_internal_node_right_child(&self) -> u32 {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let bytes: [u8; 4] = self.page[INTERNAL_NODE_RIGHT_CHILD_OFFSET
            ..INTERNAL_NODE_RIGHT_CHILD_OFFSET + INTERNAL_NODE_RIGHT_CHILD_SIZE]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_internal_node_cell(&self, cell_num: usize) -> &[u8] {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE + cell_num * INTERNAL_NODE_CELL_SIZE;
        &self.page[base..base + INTERNAL_NODE_CELL_SIZE]
    }

    pub fn get_internal_node_key(&self, key_num: usize) -> u32 {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE
            + key_num * INTERNAL_NODE_CELL_SIZE
            + INTERNAL_NODE_CHILD_SIZE;
        let bytes: [u8; 4] = self.page[base..base + 4].try_into().unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn internal_node_find_child(&self, key: u32) -> usize {
        assert_eq!(self.get_node_type(), NodeKind::Internal);
        let num_keys = self.get_internal_node_num_keys() as usize;
        let mut min_index = 0;
        let mut max_index = num_keys;
        while min_index != max_index {
            let index = (min_index + max_index) / 2;
            if self.get_internal_node_key(index) >= key {
                max_index = index;
            } else {
                min_index = index + 1;
            }
        }
        min_index
    }

    pub fn get_node_max_key(&self) -> u32 {
        match self.get_node_type() {
            NodeKind::Internal => {
                self.get_internal_node_key(self.get_internal_node_num_keys() as usize - 1)
            }
            NodeKind::Leaf => self.get_leaf_node_key(self.leaf_node_num_cells() as usize - 1),
        }
    }

    pub fn is_root_node(&self) -> bool {
        match self.page[IS_ROOT_OFFSET] {
            0 => false,
            1 => true,
            _ => unreachable!(),
        }
    }

    pub fn get_leaf_node_next_leaf(&self) -> u32 {
        assert!(self.get_node_type() == NodeKind::Leaf);
        let bytes: [u8; 4] = self.page
            [LEAF_NODE_NEXT_LEAF_OFFSET..LEAF_NODE_NEXT_LEAF_OFFSET + LEAF_NODE_NEXT_LEAF_SIZE]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_node_parent(&self) -> u32 {
        let bytes: [u8; 4] = self.page
            [PARENT_POINTER_OFFSET..PARENT_POINTER_OFFSET + PARENT_POINTER_SIZE]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
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
            return Err(PagerError::OutOfBounds(page_num));
        }
        let guard = self.pages[page_num].borrow();
        Ok(Ref::filter_map(guard, |opt| opt.as_deref().map(|p| &p[..])).ok())
    }

    pub fn get_page_mut(&self, page_num: usize) -> Result<RefMut<'_, [u8]>, PagerError> {
        if !self.is_in_bounds(page_num) {
            return Err(PagerError::OutOfBounds(page_num));
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
            .ok_or(PagerError::OutOfBounds(page_num))?
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
        page_num < TABLE_MAX_PAGES
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
            root_node.set_root_node(true);
        }

        Ok(Self {
            pager,
            root_page_num,
        })
    }

    pub fn num_pages(&self) -> usize {
        self.pager.num_pages.get()
    }

    pub fn get_page(&self, page_num: usize) -> Result<Option<Ref<'_, [u8]>>, TableError> {
        Ok(self.pager.get_page(page_num)?)
    }

    pub fn get_page_mut(&self, page_num: usize) -> Result<RefMut<'_, [u8]>, TableError> {
        Ok(self.pager.get_page_mut(page_num)?)
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

    pub fn is_empty(&self) -> bool {
        let node = self.get_node_mut(self.root_page_num).unwrap();
        match node.get_node_type() {
            NodeKind::Internal => false,
            NodeKind::Leaf => node.leaf_node_num_cells() == 0,
        }
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
        for page_num in 0..self.num_pages() {
            if self.pager.is_cached(page_num) {
                self.pager.pager_flush(page_num)?;
            }
        }
        Ok(())
    }

    pub fn get_unused_page_num(&self) -> usize {
        self.pager.num_pages.get()
    }

    pub fn create_new_root(&self, right_child_page_num: usize) -> Result<(), TableError> {
        let mut root = self.get_node_mut(self.root_page_num)?;

        let mut right_child = self.get_node_mut(right_child_page_num)?;
        right_child.set_node_parent(self.root_page_num as u32);
        drop(right_child);

        let left_child_page_num = self.get_unused_page_num();
        let mut left_child = self.get_node_mut(left_child_page_num)?;
        left_child.page.copy_from_slice(&root.page); // copy it whole-hog whether Leaf or Internal
        left_child.set_root_node(false);
        left_child.set_node_parent(self.root_page_num as u32);
        let left_child_max_key = left_child.get_node_max_key();
        drop(left_child);

        root.initialize_internal_node();
        root.set_internal_node_num_keys(1);
        root.set_internal_node_child(0, left_child_page_num);
        root.set_internal_node_key(0, left_child_max_key);
        root.set_internal_node_right_child(right_child_page_num);

        root.set_root_node(true);

        assert_eq!(
            root.get_node_type(),
            NodeKind::Internal,
            "new root isn't internal!"
        );

        Ok(())
    }

    pub fn leaf_node_insert(
        &self,
        page_num: usize,
        cell_num: usize,
        key: u32,
        row: &Row,
    ) -> Result<(), TableError> {
        let num_cells = self.get_node_mut(page_num)?.leaf_node_num_cells() as usize;
        if num_cells >= LEAF_NODE_MAX_CELLS {
            //println!("SPLITTING LEAF NODE");
            return self.leaf_node_split_and_insert(page_num, cell_num, key, row);
        }
        let mut node = self.get_node_mut(page_num)?;
        if cell_num < num_cells {
            let key_at_index = node.get_leaf_node_key(cell_num);
            if key_at_index == key {
                return Err(TableError::DuplicateKey(key));
            }
            // make room for a new cell
            for i in (cell_num + 1..=num_cells).rev() {
                node.copy_leaf_cell(i - 1, i);
            }
        }

        node.set_leaf_node_num_cells(node.leaf_node_num_cells() + 1);
        node.set_leaf_node_key(cell_num, key);
        row.serialize_row(&mut node.leaf_node_value(cell_num))?;
        Ok(())
    }

    pub fn internal_node_insert(
        &self,
        parent_page_num: usize,
        child_page_num: usize,
    ) -> Result<(), TableError> {
        assert_eq!(
            self.get_node_mut(parent_page_num)?.get_node_type(),
            NodeKind::Internal
        );
        let child_max_key = {
            let child = self.get_node_mut(child_page_num)?;
            child.get_node_max_key()
        }; // child's guard drops here — we only needed one value out of it
        let mut parent = self.get_node_mut(parent_page_num)?;
        let index = parent.internal_node_find_child(child_max_key);

        let original_num_keys = parent.get_internal_node_num_keys() as usize;
        if original_num_keys >= INTERNAL_NODE_MAX_CELLS {
            drop(parent);
            //println!("SPLITTING INTERNAL NODE");
            return self.internal_node_split_and_insert(parent_page_num, child_page_num);
        }
        parent.set_internal_node_num_keys(original_num_keys + 1);

        let right_child_page_num = parent.get_internal_node_right_child() as usize;
        if right_child_page_num == INVALID_PAGE_NUM {
            parent.set_internal_node_right_child(child_page_num);
            return Ok(());
        }
        let right_child_max_key = {
            let right_child = self.get_node_mut(right_child_page_num)?;
            right_child.get_node_max_key()
        }; // same treatment — drop before continuing

        if child_max_key > right_child_max_key {
            // new child becomes the rightmost; old right child moves into a regular cell
            parent.set_internal_node_child(original_num_keys, right_child_page_num);
            parent.set_internal_node_key(original_num_keys, right_child_max_key);
            parent.set_internal_node_right_child(child_page_num);
        } else {
            // make room for the new cell
            for i in (index + 1..=original_num_keys).rev() {
                parent.copy_internal_cell(i - 1, i);
            }
            parent.set_internal_node_child(index, child_page_num);
            parent.set_internal_node_key(index, child_max_key);
        }

        Ok(())
    }

    pub fn leaf_node_split_and_insert(
        &self,
        page_num: usize,
        cell_num: usize,
        key: u32,
        row: &Row,
    ) -> Result<(), TableError> {
        let mut old_node = self.get_node_mut(page_num)?;
        let old_max = old_node.get_node_max_key() as usize;
        let old_parent = old_node.get_node_parent() as usize;
        let old_next = old_node.get_leaf_node_next_leaf() as usize;

        let new_page_num = self.get_unused_page_num();
        old_node.set_leaf_node_next_leaf(new_page_num as u32);

        let mut new_node = self.get_node_mut(new_page_num)?;
        new_node.initialize_leaf_node();
        new_node.set_node_parent(old_parent as u32);
        new_node.set_leaf_node_next_leaf(old_next as u32);

        for i in (0..=LEAF_NODE_MAX_CELLS).rev() {
            let index_within_node = i % LEAF_NODE_LEFT_SPLIT_COUNT;
            let dest_is_new = i >= LEAF_NODE_LEFT_SPLIT_COUNT;

            if i == cell_num {
                let dest_node = if dest_is_new {
                    &mut new_node
                } else {
                    &mut old_node
                };
                dest_node.set_leaf_node_key(index_within_node, key);
                row.serialize_row(&mut dest_node.leaf_node_value(index_within_node))
                    .map_err(|e| TableError::PagerError(PagerError::IoError(e)))?;
            } else {
                let src_index = if i > cell_num { i - 1 } else { i };
                if dest_is_new {
                    new_node.copy_cell_from(index_within_node, &old_node, src_index);
                } else {
                    old_node.copy_leaf_cell(src_index, index_within_node);
                }
            }
        }
        old_node.set_leaf_node_num_cells(LEAF_NODE_LEFT_SPLIT_COUNT as u32);
        new_node.set_leaf_node_num_cells(LEAF_NODE_RIGHT_SPLIT_COUNT as u32);

        let splitting_root = old_node.is_root_node();
        let parent_page_num = old_node.get_node_parent() as usize;
        let new_max = old_node.get_node_max_key() as usize;
        drop(old_node);
        drop(new_node);

        if splitting_root {
            self.create_new_root(new_page_num)?;
        } else {
            assert_ne!(
                parent_page_num, INVALID_PAGE_NUM,
                "Parent page number is invalid"
            );
            let mut parent = self.get_node_mut(parent_page_num)?;
            let old_index = parent.internal_node_find_child(old_max as u32);
            parent.set_internal_node_key(old_index, new_max as u32);
            drop(parent);
            self.internal_node_insert(parent_page_num, new_page_num)?;
        }
        Ok(())
    }

    pub fn internal_node_split_and_insert(
        &self,
        parent_page_num: usize,
        child_page_num: usize,
    ) -> Result<(), TableError> {
        // find the parent of the node we're splitting
        let old_node = self.get_node_mut(parent_page_num)?;
        let old_parent_page_num = old_node.get_node_parent();

        // extract all the children of the node we're splitting into
        // `old_parent_entries`
        let mut old_parent_entries = Vec::new();
        for i in 0..old_node.get_internal_node_num_keys() as usize {
            let key = old_node.get_internal_node_key(i);
            let child = old_node.get_internal_node_child(i)?;
            old_parent_entries.push((child, key));
        }
        // add the rightmost child to `old_parent_entries` -- now we have a complete list
        let right_child = old_node.get_internal_node_right_child();
        let right_child_key = self.get_node_mut(right_child as usize)?.get_node_max_key();
        old_parent_entries.push((right_child, right_child_key));

        // find the key that represented where this child was in the old parent
        let new_child_key = self.get_node_mut(child_page_num)?.get_node_max_key();
        // find the location to insert a new page into the old_parent for our new_child
        let insert_index = old_parent_entries
            .iter()
            .position(|&(_, key)| key >= new_child_key)
            .unwrap_or(old_parent_entries.len());
        // add the new_child_key to our collection in the right spot
        old_parent_entries.insert(insert_index, (child_page_num as u32, new_child_key));

        // determine if we're splitting the root node and store the resulting flag
        let splitting_root = old_node.is_root_node();

        drop(old_node);

        // the references that will stay in the child node
        let left_group = &old_parent_entries[..INTERNAL_NODE_LEFT_SPLIT_COUNT];
        // the referense that will go in the new child node
        let right_group = &old_parent_entries[INTERNAL_NODE_LEFT_SPLIT_COUNT..];
        // store the highest page_num that will stay in the child node
        let old_max = left_group.last().unwrap().1; // parent_page_num's new max, after truncation

        // 'allocate' a new page and write the right_groups entries into it
        let new_page_num = self.get_unused_page_num(); // holds right_group
        let mut new_node = self.get_node_mut(new_page_num)?;
        new_node.initialize_internal_node();
        new_node.write_entries(right_group);
        drop(new_node);

        // if we're splitting a root node, then we need to make a new left side
        let left_dest_page_num = if splitting_root {
            self.get_unused_page_num() // called only after new_page_num was faulted in above — no collision
        } else {
            parent_page_num
        };

        // populate the left_node (new_node or existing node) with the left_group entries
        let mut left_node = self.get_node_mut(left_dest_page_num)?;
        if splitting_root {
            left_node.initialize_internal_node();
        }
        left_node.write_entries(left_group);
        drop(left_node);

        // update the parents of each child in the right group to point to our new_page
        for &(child, _key) in right_group {
            self.get_node_mut(child as usize)?
                .set_node_parent(new_page_num as u32);
        }
        // update the parents of each child in the left group to point to either the
        // old parent or the new_node if we're splitting the root
        for &(child, _key) in left_group {
            self.get_node_mut(child as usize)?
                .set_node_parent(left_dest_page_num as u32);
        }

        if splitting_root {
            // if we had to split the root node, transform `parent_page_num` into a proper
            // root internal node. `create_new_root` doesn't handle the key redistribution properly
            // so we're manually doing it here
            let mut root = self.get_node_mut(parent_page_num)?;
            root.initialize_internal_node();
            root.set_internal_node_num_keys(1);
            root.set_internal_node_child(0, left_dest_page_num);
            root.set_internal_node_key(0, old_max);
            root.set_internal_node_right_child(new_page_num);
            drop(root);
            self.get_node_mut(left_dest_page_num)?
                .set_node_parent(parent_page_num as u32);
            self.get_node_mut(new_page_num)?
                .set_node_parent(parent_page_num as u32);
        } else {
            let mut parent = self.get_node_mut(old_parent_page_num as usize)?;
            // search with the STALE value the grandparent currently has for parent_page_num...
            let idx = parent.internal_node_find_child(right_child_key);
            // ...then overwrite it with parent_page_num's new, post-truncation max.
            parent.set_internal_node_key(idx, old_max);
            drop(parent);
            // update the new_page's parent to the parent node
            self.get_node_mut(new_page_num)?
                .set_node_parent(old_parent_page_num);
            // insert the new page into the parent node
            self.internal_node_insert(old_parent_page_num as usize, new_page_num)?;
        }

        Ok(())
    }

    fn indent(level: usize) {
        print!("{}", "  ".repeat(level));
    }
    pub fn print_tree(&self, page_num: usize, indentation_level: usize) -> Result<(), TableError> {
        let node_type = self.get_node_mut(page_num)?.get_node_type();
        match node_type {
            NodeKind::Leaf => {
                let num_keys = self.get_node_mut(page_num)?.leaf_node_num_cells();
                Self::indent(indentation_level);
                println!("- leaf (size {num_keys})");
                for i in 0..num_keys as usize {
                    Self::indent(indentation_level + 1);
                    println!("- {}", self.get_node_mut(page_num)?.get_leaf_node_key(i));
                }
            }
            NodeKind::Internal => {
                let num_keys = self.get_node_mut(page_num)?.get_internal_node_num_keys();
                Self::indent(indentation_level);
                println!("- internal (size {num_keys})");
                if num_keys > 0 {
                    let right_child = self.get_node_mut(page_num)?.get_internal_node_right_child();
                    for i in 0..num_keys as usize {
                        let child = self.get_node_mut(page_num)?.get_internal_node_child(i)?;
                        self.print_tree(child as usize, indentation_level + 1)?;
                        Self::indent(indentation_level + 1);
                        println!(
                            "- key {}",
                            self.get_node_mut(page_num)?.get_internal_node_key(i)
                        );
                    }
                    self.print_tree(right_child as usize, indentation_level + 1)?;
                }
            }
        };
        Ok(())
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        self.flush_all().expect("failed to flush a page");
    }
}
