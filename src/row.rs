use crate::constants::*;
use crate::errors::*;
use std::fmt::Debug;
use std::io::{Read, Write};

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VarChar<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> VarChar<N> {
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

impl<const N: usize> From<&VarChar<N>> for String {
    fn from(value: &VarChar<N>) -> Self {
        String::from_utf8_lossy(&value.data)
            .trim_end_matches('\0')
            .to_string()
    }
}

impl<const N: usize> From<VarChar<N>> for String {
    fn from(value: VarChar<N>) -> Self {
        String::from_utf8_lossy(&value.data)
            .trim_end_matches('\0')
            .to_string()
    }
}

impl<const N: usize> TryFrom<&str> for VarChar<N> {
    type Error = PrepareError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() > N {
            return Err(PrepareError::StringTooLong);
        }
        let mut data = [0u8; N];
        data[..value.len()].copy_from_slice(value.as_bytes());
        Ok(VarChar { data })
    }
}

#[repr(C)]
pub struct Row {
    pub(crate) id: u32,
    pub(crate) username: VarChar<COLUMN_USERNAME_SIZE>,
    pub(crate) email: VarChar<COLUMN_EMAIL_SIZE>,
}

impl Row {
    pub fn new(
        id: u32,
        username: VarChar<COLUMN_USERNAME_SIZE>,
        email: VarChar<COLUMN_EMAIL_SIZE>,
    ) -> Self {
        Self {
            id,
            username,
            email,
        }
    }

    pub fn serialize_row<W: Write>(&self, w: &mut W) -> Result<(), std::io::Error> {
        w.write_all(&self.id.to_be_bytes())?;
        w.write_all(self.username.as_bytes())?;
        w.write_all(self.email.as_bytes())?;
        Ok(())
    }

    pub fn deserialize_row<R: Read>(r: &mut R) -> Result<Row, std::io::Error> {
        let mut id_buf = [0u8; ID_SIZE];
        r.read_exact(&mut id_buf)?;
        let id: u32 = u32::from_be_bytes(id_buf);

        let mut username_buf = [0u8; USERNAME_SIZE];
        r.read_exact(&mut username_buf)?;
        let username = VarChar { data: username_buf };

        let mut email_buf = [0u8; EMAIL_SIZE];
        r.read_exact(&mut email_buf)?;
        let email = VarChar { data: email_buf };

        Ok(Row::new(id, username, email))
    }

    pub fn id(&self) -> usize {
        self.id as usize
    }
}

impl Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Row")
            .field("id", &self.id)
            .field("username", &String::from(&self.username))
            .field("email", &String::from(&self.email))
            .finish()
    }
}

impl std::fmt::Display for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({}, {}, {})",
            self.id,
            String::from(&self.username),
            String::from(&self.email)
        )
    }
}
