pub mod errors;
pub mod row;
pub mod table;

pub mod constants {
    use crate::row::{Row, VarChar};
    use std::mem::{offset_of, size_of};

    // limited row schema constants
    pub const COLUMN_USERNAME_SIZE: usize = 32;
    pub const COLUMN_EMAIL_SIZE: usize = 255;

    // compact row representation constants
    pub const ID_OFFSET: usize = offset_of!(Row, id);
    pub const USERNAME_OFFSET: usize = offset_of!(Row, username);
    pub const EMAIL_OFFSET: usize = offset_of!(Row, email);

    pub const ID_SIZE: usize = USERNAME_OFFSET - ID_OFFSET;
    pub const USERNAME_SIZE: usize = EMAIL_OFFSET - USERNAME_OFFSET;
    pub const EMAIL_SIZE: usize = size_of::<VarChar<COLUMN_EMAIL_SIZE>>();
    pub const ROW_SIZE: usize = ID_SIZE + USERNAME_SIZE + EMAIL_SIZE;

    // table struct constants
    pub const PAGE_SIZE: usize = 4096;
    pub const TABLE_MAX_PAGES: usize = 100;
    pub const ROWS_PER_PAGE: usize = PAGE_SIZE / ROW_SIZE;
    pub const TABLE_MAX_ROWS: usize = ROWS_PER_PAGE * TABLE_MAX_PAGES;
}
