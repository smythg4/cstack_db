use crate::constants::*;

pub type Page = [u8; PAGE_SIZE];

pub struct Table {
    num_rows: usize,
    pages: [Option<Box<Page>>; TABLE_MAX_PAGES],
}

impl Default for Table {
    fn default() -> Self {
        const EMPTY: Option<Box<Page>> = None;
        Self {
            num_rows: 0,
            pages: [EMPTY; TABLE_MAX_PAGES],
        }
    }
}

impl Table {
    pub fn get_row(&self, row_num: usize) -> Option<&[u8]> {
        let page_num = row_num / ROWS_PER_PAGE;
        let page = self.get_page(page_num)?;
        let row_offset = row_num % ROWS_PER_PAGE;
        let byte_offset = row_offset * ROW_SIZE;
        Some(&page[byte_offset..byte_offset + ROW_SIZE])
    }

    pub fn get_row_mut(&mut self, row_num: usize) -> &mut [u8] {
        let page_num = row_num / ROWS_PER_PAGE;
        let page = self.get_page_mut(page_num);
        let row_offset = row_num % ROWS_PER_PAGE;
        let byte_offset = row_offset * ROW_SIZE;
        &mut page[byte_offset..byte_offset + ROW_SIZE]
    }

    pub fn get_page(&self, page_num: usize) -> Option<&[u8; PAGE_SIZE]> {
        // find the page in the table or initialize it to an empty page
        self.pages.get(page_num)?.as_deref()
    }

    pub fn get_page_mut(&mut self, page_num: usize) -> &mut [u8; PAGE_SIZE] {
        // find the page in the table or initialize it to an empty page
        self.pages[page_num].get_or_insert_with(|| Box::new([0u8; PAGE_SIZE]))
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
}
