//! Replaces the LGPL-licensed `libesedb` crate, which rookie uses only for Internet
//! Explorer's cookie store. Covers the API rookie calls. Opening a database always fails,
//! so rookie's `internet_explorer` returns an error and its other readers are unaffected.

use std::io;
use std::marker::PhantomData;
use std::path::Path;

pub enum EseDb {}
pub enum Value {}
pub struct Table<'a>(Never, PhantomData<&'a ()>);
pub struct Record<'a>(Never, PhantomData<&'a ()>);
enum Never {}

impl EseDb {
    pub fn open<P: AsRef<Path>>(_filename: P) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Internet Explorer cookies are not supported",
        ))
    }

    pub fn iter_tables(&self) -> io::Result<std::iter::Empty<io::Result<Table<'_>>>> {
        match *self {}
    }
}

impl Table<'_> {
    pub fn name(&self) -> io::Result<String> {
        match self.0 {}
    }

    pub fn iter_records(&self) -> io::Result<std::iter::Empty<io::Result<Record<'_>>>> {
        match self.0 {}
    }
}

impl Record<'_> {
    pub fn value(&self, _entry: i32) -> io::Result<Value> {
        match self.0 {}
    }
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match *self {}
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match *self {}
    }

    pub fn to_u64(&self) -> Option<u64> {
        match *self {}
    }
}
