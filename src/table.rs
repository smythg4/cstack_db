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
    #[error("Need to implement splitting internal node")]
    InternalNodeFull,
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
        self.set_internal_node_num_cells(0)
    }

    pub fn set_leaf_node_num_cells(&mut self, n: u32) {
        assert!(self.get_node_type() == NodeKind::Leaf);
        self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .copy_from_slice(&n.to_be_bytes());
    }

    pub fn set_internal_node_num_cells(&mut self, n: u32) {
        assert!(self.get_node_type() == NodeKind::Internal);
        self.page[INTERNAL_NODE_NUM_KEYS_OFFSET..INTERNAL_NODE_NUM_KEYS_OFFSET + 4]
            .copy_from_slice(&n.to_be_bytes());
    }

    pub fn leaf_node_cell(&mut self, cell_num: usize) -> &mut [u8] {
        assert!(self.get_node_type() == NodeKind::Leaf);
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        &mut self.page[base..base + LEAF_NODE_CELL_SIZE]
    }

    pub fn set_leaf_node_key(&mut self, cell_num: usize, key: u32) {
        assert!(self.get_node_type() == NodeKind::Leaf);
        let base = LEAF_NODE_HEADER_SIZE + cell_num * LEAF_NODE_CELL_SIZE;
        self.page[base..base + 4].copy_from_slice(&key.to_be_bytes());
    }

    pub fn leaf_node_value(&mut self, cell_num: usize) -> &mut [u8] {
        assert!(self.get_node_type() == NodeKind::Leaf);
        let cell = self.leaf_node_cell(cell_num);
        &mut cell[LEAF_NODE_KEY_SIZE..]
    }

    pub fn copy_leaf_cell(&mut self, from_cell: usize, to_cell: usize) {
        let from_base = LEAF_NODE_HEADER_SIZE + from_cell * LEAF_NODE_CELL_SIZE;
        let to_base = LEAF_NODE_HEADER_SIZE + to_cell * LEAF_NODE_CELL_SIZE;
        self.page
            .copy_within(from_base..from_base + LEAF_NODE_CELL_SIZE, to_base);
    }

    pub fn copy_internal_cell(&mut self, from_cell: usize, to_cell: usize) {
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
        assert!(self.get_node_type() == NodeKind::Internal);
        self.page[INTERNAL_NODE_NUM_KEYS_OFFSET
            ..INTERNAL_NODE_NUM_KEYS_OFFSET + INTERNAL_NODE_NUM_KEY_SIZE]
            .copy_from_slice(&(num_keys as u32).to_be_bytes());
    }

    pub fn set_internal_node_right_child(&mut self, right_child_index: usize) {
        assert!(self.get_node_type() == NodeKind::Internal);
        self.page[INTERNAL_NODE_RIGHT_CHILD_OFFSET
            ..INTERNAL_NODE_RIGHT_CHILD_OFFSET + INTERNAL_NODE_RIGHT_CHILD_SIZE]
            .copy_from_slice(&(right_child_index as u32).to_be_bytes());
    }

    pub fn set_internal_node_key(&mut self, key_num: usize, key: u32) {
        assert!(self.get_node_type() == NodeKind::Internal);
        let cell = self.internal_node_cell(key_num);
        cell[INTERNAL_NODE_CHILD_SIZE..INTERNAL_NODE_CHILD_SIZE + INTERNAL_NODE_KEY_SIZE]
            .copy_from_slice(&key.to_be_bytes());
    }

    pub fn set_internal_node_child(&mut self, child_num: usize, page_num: usize) {
        assert!(self.get_node_type() == NodeKind::Internal);
        let num_keys = self.get_internal_node_num_keys() as usize;
        if child_num == num_keys {
            self.set_internal_node_right_child(page_num);
        } else {
            self.internal_node_cell(child_num)[..INTERNAL_NODE_CHILD_SIZE]
                .copy_from_slice(&(page_num as u32).to_be_bytes());
        }
    }

    pub fn set_leaf_node_next_leaf(&mut self, next_leaf: u32) {
        assert!(self.get_node_type() == NodeKind::Leaf);
        self.page
            [LEAF_NODE_NEXT_LEAF_OFFSET..LEAF_NODE_NEXT_LEAF_OFFSET + LEAF_NODE_NEXT_LEAF_SIZE]
            .copy_from_slice(&next_leaf.to_be_bytes());
    }

    pub fn internal_node_cell(&mut self, cell_num: usize) -> &mut [u8] {
        assert!(self.get_node_type() == NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE + cell_num * INTERNAL_NODE_CELL_SIZE;
        &mut self.page[base..base + INTERNAL_NODE_CELL_SIZE]
    }

    pub fn internal_node_child(&mut self, child_num: usize) -> &mut [u8] {
        assert!(self.get_node_type() == NodeKind::Internal);
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
        assert!(self.get_node_type() == NodeKind::Internal);
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
}

impl<G: Deref<Target = [u8]>> Node<G> {
    pub fn leaf_node_num_cells(&self) -> u32 {
        assert!(self.get_node_type() == NodeKind::Leaf);
        let bytes: [u8; 4] = self.page[LEAF_NODE_NUM_CELLS_OFFSET..LEAF_NODE_NUM_CELLS_OFFSET + 4]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_leaf_node_key(&self, cell_num: usize) -> u32 {
        assert!(self.get_node_type() == NodeKind::Leaf);
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
        assert!(self.get_node_type() == NodeKind::Internal);
        let bytes: [u8; 4] = self.page[INTERNAL_NODE_NUM_KEYS_OFFSET
            ..INTERNAL_NODE_NUM_KEYS_OFFSET + INTERNAL_NODE_NUM_KEY_SIZE]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_internal_node_child(&self, child_num: usize) -> u32 {
        assert!(self.get_node_type() == NodeKind::Internal);
        let num_keys = self.get_internal_node_num_keys() as usize;
        if child_num > num_keys {
            println!("Tried to access child_num {child_num} > num_keys {num_keys}");
            std::process::exit(1);
        } else if child_num == num_keys {
            self.get_internal_node_right_child()
        } else {
            let bytes: [u8; 4] = self.get_internal_node_cell(child_num)[..INTERNAL_NODE_CHILD_SIZE]
                .try_into()
                .unwrap();
            u32::from_be_bytes(bytes)
        }
    }

    pub fn get_internal_node_right_child(&self) -> u32 {
        assert!(self.get_node_type() == NodeKind::Internal);
        let bytes: [u8; 4] = self.page[INTERNAL_NODE_RIGHT_CHILD_OFFSET
            ..INTERNAL_NODE_RIGHT_CHILD_OFFSET + INTERNAL_NODE_RIGHT_CHILD_SIZE]
            .try_into()
            .unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn get_internal_node_cell(&self, cell_num: usize) -> &[u8] {
        assert!(self.get_node_type() == NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE + cell_num * INTERNAL_NODE_CELL_SIZE;
        &self.page[base..base + INTERNAL_NODE_CELL_SIZE]
    }

    pub fn get_internal_node_key(&self, key_num: usize) -> u32 {
        assert!(self.get_node_type() == NodeKind::Internal);
        let base = INTERNAL_NODE_HEADER_SIZE
            + key_num * INTERNAL_NODE_CELL_SIZE
            + INTERNAL_NODE_CHILD_SIZE;
        let bytes: [u8; 4] = self.page[base..base + 4].try_into().unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn internal_node_find_child(&self, key: u32) -> usize {
        assert!(self.get_node_type() == NodeKind::Internal);
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

        let left_child_page_num = self.get_unused_page_num();
        let mut left_child = self.get_node_mut(left_child_page_num)?;

        right_child.set_node_parent(self.root_page_num as u32);
        left_child.set_node_parent(self.root_page_num as u32);

        left_child.page.copy_from_slice(&root.page);

        root.initialize_internal_node();
        root.set_internal_node_num_keys(1);
        root.set_internal_node_child(0, left_child_page_num);
        let left_child_max_key = left_child.get_node_max_key();
        root.set_internal_node_key(0, left_child_max_key);
        root.set_internal_node_right_child(right_child_page_num);

        Ok(())
    }

    pub fn internal_node_insert(
        &self,
        parent_page_num: usize,
        child_page_num: usize,
    ) -> Result<(), TableError> {
        let child_max_key = {
            let child = self.get_node_mut(child_page_num)?;
            child.get_node_max_key()
        }; // child's guard drops here — we only needed one value out of it
        let mut parent = self.get_node_mut(parent_page_num)?;
        let index = parent.internal_node_find_child(child_max_key);

        let original_num_keys = parent.get_internal_node_num_keys() as usize;
        if original_num_keys >= INTERNAL_NODE_MAX_CELLS {
            return Err(TableError::InternalNodeFull);
        }
        parent.set_internal_node_num_keys(original_num_keys + 1);

        let right_child_page_num = parent.get_internal_node_right_child() as usize;
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

    fn indent(level: usize) {
        print!("{}", "  ".repeat(level));
    }
    pub fn print_tree(&self, page_num: usize, indentation_level: usize) -> Result<(), TableError> {
        let node = self.get_node_mut(page_num)?;
        match node.get_node_type() {
            NodeKind::Leaf => {
                let num_keys = node.leaf_node_num_cells();
                Self::indent(indentation_level);
                println!("- leaf (size {num_keys})");
                for i in 0..num_keys as usize {
                    Self::indent(indentation_level + 1);
                    println!("- {}", node.get_leaf_node_key(i));
                }
            }
            NodeKind::Internal => {
                let num_keys = node.get_internal_node_num_keys();
                Self::indent(indentation_level);
                println!("- internal (size {num_keys})");
                for i in 0..num_keys as usize {
                    let child = node.get_internal_node_child(i);
                    self.print_tree(child as usize, indentation_level + 1)?;
                    Self::indent(indentation_level + 1);
                    println!("- key {}", node.get_internal_node_key(i));
                }
                let right_child = node.get_internal_node_right_child();
                self.print_tree(right_child as usize, indentation_level + 1)?;
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
