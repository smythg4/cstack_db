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
}

#[derive(Error, Debug)]
pub enum TableError {
    #[error(transparent)]
    PagerError(#[from] PagerError),
}

pub type Page = [u8; PAGE_SIZE];

pub struct Pager {
    file_descriptor: File,
    file_length: usize,
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
        const EMPTY: Option<Box<Page>> = None;
        let pages = [EMPTY; TABLE_MAX_PAGES];
        Ok(Self {
            file_descriptor,
            file_length,
            pages,
        })
    }

    pub fn len(&self) -> usize {
        self.file_length
    }

    pub fn is_empty(&self) -> bool {
        self.file_length == 0
    }

    pub fn get_row(&mut self, row_num: usize) -> Result<Option<&[u8]>, PagerError> {
        let page_num = row_num / ROWS_PER_PAGE;
        let page = match self.get_page(page_num)? {
            Some(p) => p,
            None => return Ok(None),
        };
        let row_offset = row_num % ROWS_PER_PAGE;
        let byte_offset = row_offset * ROW_SIZE;
        Ok(Some(&page[byte_offset..byte_offset + ROW_SIZE]))
    }

    pub fn get_row_mut(&mut self, row_num: usize) -> Result<&mut [u8], PagerError> {
        let page_num = row_num / ROWS_PER_PAGE;
        let page = self.get_page_mut(page_num)?;
        let row_offset = row_num % ROWS_PER_PAGE;
        let byte_offset = row_offset * ROW_SIZE;
        Ok(&mut page[byte_offset..byte_offset + ROW_SIZE])
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
        }

        Ok(self.pages[page_num].as_deref_mut().unwrap())
    }

    fn pager_flush(&mut self, page_num: usize, size: usize) -> Result<(), PagerError> {
        if self.is_cached(page_num) {
            match self.pages.get(page_num) {
                Some(Some(page)) => {
                    self.file_descriptor
                        .seek(std::io::SeekFrom::Start((page_num * PAGE_SIZE) as u64))?;
                    self.file_descriptor.write_all(&page[..size])?;
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
    num_rows: usize,
    pager: Pager,
}

impl Table {
    pub fn db_open<P: AsRef<Path>>(path: P) -> Result<Self, TableError> {
        let pager = Pager::pager_open(path)?;
        let num_rows = pager.len() / ROW_SIZE;
        Ok(Self { num_rows, pager })
    }

    pub fn get_row(&mut self, row_num: usize) -> Result<Option<&[u8]>, TableError> {
        Ok(self.pager.get_row(row_num)?)
    }

    pub fn get_row_mut(&mut self, row_num: usize) -> Result<&mut [u8], TableError> {
        Ok(self.pager.get_row_mut(row_num)?)
    }

    pub fn get_page(&mut self, page_num: usize) -> Result<Option<&[u8; PAGE_SIZE]>, TableError> {
        Ok(self.pager.get_page(page_num)?)
    }

    pub fn get_page_mut(&mut self, page_num: usize) -> Result<&mut [u8; PAGE_SIZE], TableError> {
        Ok(self.pager.get_page_mut(page_num)?)
    }

    pub fn len(&self) -> usize {
        self.num_rows
    }

    pub fn is_empty(&self) -> bool {
        self.num_rows == 0
    }

    pub fn incr_rows(&mut self) {
        self.num_rows += 1;
    }

    pub fn flush_all(&mut self) -> Result<(), TableError> {
        let num_full_pages = self.num_rows / ROWS_PER_PAGE;
        for page_num in 0..num_full_pages {
            if self.pager.is_cached(page_num) {
                self.pager.pager_flush(page_num, PAGE_SIZE)?;
            }
        }
        let num_additional_rows = self.num_rows % ROWS_PER_PAGE;
        if num_additional_rows > 0 {
            let page_num = num_full_pages;
            if self.pager.is_cached(page_num) {
                self.pager
                    .pager_flush(page_num, num_additional_rows * ROW_SIZE)?;
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
