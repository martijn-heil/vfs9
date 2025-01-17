/*
 * The filesystem as modelled by 9P2000
 */

use std::io::{Read, Write};
use std::convert::{TryFrom, TryInto};

pub struct Vfs9Error();

type Result<T> = std::result::Result<T, Vfs9Error>;

#[derive(Debug, PartialEq)]
pub struct FileType {
    pub is_dir: bool,
    pub is_append_only: bool,
    pub is_exclusive: bool,
    pub is_auth: bool,
    pub is_temporary: bool,
}

impl FileType {
    pub fn from_bits(b: u8) -> Self {
        Self {
            // bit 28 is skipped for 'historical reasons'
            is_dir:         b & 0b10000000 != 0,
            is_append_only: b & 0b01000000 != 0,
            is_exclusive:   b & 0b00100000 != 0,
            is_auth:        b & 0b00001000 != 0,
            is_temporary:   b & 0b00000100 != 0
        }
    }

    pub fn to_bits(&self) -> u8 {
        let mut b = 0x00;

        // bit 28 is skipped for 'historical reasons'
        if self.is_dir         { b |= 0b10000000; }
        if self.is_append_only { b |= 0b01000000; }
        if self.is_exclusive   { b |= 0b00100000; }
        if self.is_auth        { b |= 0b00001000; }
        if self.is_temporary   { b |= 0b00000100; }

        b
    }
}

/// The qid represents the server's unique identification for the file being accessed:
/// two files on the same server hierarchy are the same if and only if their qids are the same.
/// (The client may have multiple fids pointing to a single file on a server and hence having a single qid.)
#[derive(Debug, PartialEq)]
pub struct Qid {
    /// The type of qid, specifies whether this is a file, a directory, append-only file, etc.
    pub file_type: FileType,

    /// A version number for a file; typically, it is incremented every time the file is modified.
    pub version: u32,

    /// The path is an integer unique among all files in the hierarchy.
    /// If a file is deleted and recreated with the same name in the same directory,
    /// the old and new path components of the qids should be different.
    pub path: u64
}

/// The IoUnit field is the maximum number of bytes that are guaranteed to be read from or written to a given file,
/// without breaking the I/O transfer into multiple 9P messages; see read(5).
pub type IoUnit = u32;

#[derive(Debug, PartialEq)]
pub enum OpenSubMode {
    Write,
    Read,
    ReadWrite,
    Execute
}

impl TryFrom<u8> for OpenSubMode {
    type Error = Vfs9Error;

    fn try_from(bits: u8) -> std::result::Result<Self, Self::Error> {
        let mode: u8 = bits & 0b00000011;
        match mode {
            0 => Ok(Self::Read),
            1 => Ok(Self::Write),
            2 => Ok(Self::ReadWrite),
            3 => Ok(Self::Execute),
            _ => Err(Vfs9Error())
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct OpenMode {
    pub submode: OpenSubMode,

    /// if mode has the OTRUNC (0x10) bit set, the file is to be truncated,
    /// which requires write permission (if the file is append-only, and permission is granted,
    /// the open succeeds but the file will not be trun- cated)
    pub truncate: bool,

    /// if the mode has the ORCLOSE (0x40) bit set,
    /// the file is to be removed when the fid is clunked,
    /// which requires permission to remove the file from its directory.
    pub rclose: bool,
}

impl OpenMode {
    pub fn from_bits(fields: u8) -> Result<Self> {
        let mut s = Self {
            submode: fields.try_into()?,
            truncate: false,
            rclose: false
        };

        if fields & 0b00010000 != 0 { s.truncate = true; } // =0x10
        if fields & 0b01000000 != 0 { s.rclose = true; }   // =0x40

        Ok(s)
    }
}

#[derive(Debug, PartialEq)]
pub struct IndividualPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, PartialEq)]
pub struct Permissions {
    pub owner: IndividualPermissions,
    pub group: IndividualPermissions,
    pub other: IndividualPermissions,
}


const BIT_OTHER_EXECUTE: u32  = 0b00000000000000000000000000000001;
const BIT_OTHER_WRITE: u32    = 0b00000000000000000000000000000010;
const BIT_OTHER_READ: u32     = 0b00000000000000000000000000000100;

const BIT_GROUP_EXECUTE: u32 = BIT_OTHER_EXECUTE << 3;
const BIT_GROUP_WRITE: u32   = BIT_OTHER_WRITE << 3;
const BIT_GROUP_READ: u32    = BIT_OTHER_READ << 3;

const BIT_OWNER_EXECUTE: u32 = BIT_GROUP_EXECUTE << 3;
const BIT_OWNER_WRITE: u32   = BIT_GROUP_WRITE << 3;
const BIT_OWNER_READ: u32    = BIT_GROUP_READ << 3;

impl Permissions {
    pub fn from_bits(fields: u32) -> Self {
        let mut p = Self {
            owner: IndividualPermissions { read: false, write: false, execute: false },
            group: IndividualPermissions { read: false, write: false, execute: false },
            other: IndividualPermissions { read: false, write: false, execute: false }
        };

        //           0b00000000000000000000000000000000: 32 bit integer
        if (fields & BIT_OTHER_EXECUTE) != 0 { p.other.execute = true; }
        if (fields & BIT_OTHER_WRITE)   != 0 { p.other.write = true; }
        if (fields & BIT_OTHER_READ)    != 0 { p.other.read = true; }

        if (fields & BIT_GROUP_EXECUTE) != 0 { p.group.execute = true; }
        if (fields & BIT_GROUP_WRITE)   != 0 { p.group.write = true; }
        if (fields & BIT_GROUP_READ)    != 0 { p.group.read = true; }

        if (fields & BIT_OWNER_EXECUTE) != 0 { p.owner.execute = true; }
        if (fields & BIT_OWNER_WRITE)   != 0 { p.owner.write = true; }
        if (fields & BIT_OWNER_READ)    != 0 { p.owner.read = true; }

        p
    }

    pub fn to_bits(&self) -> u32 {
        let mut b: u32 = 0x00000000;

        if self.other.execute { b |= BIT_OTHER_EXECUTE; }
        if self.other.write   { b |= BIT_OTHER_WRITE;   }
        if self.other.read    { b |= BIT_OTHER_READ;    }

        if self.group.execute { b |= BIT_GROUP_EXECUTE; }
        if self.group.write   { b |= BIT_GROUP_WRITE;   }
        if self.group.read    { b |= BIT_GROUP_READ;    }

        if self.owner.execute { b |= BIT_OWNER_EXECUTE; }
        if self.owner.write   { b |= BIT_OWNER_WRITE;   }
        if self.owner.read    { b |= BIT_OWNER_READ;    }

        b
    }
}

#[derive(Debug, PartialEq)]
pub struct FileMode {
    pub permissions: Permissions,
    pub file_type: FileType,
}

impl FileMode {
    pub fn from_bits(fields: u32) -> Self {
        Self {
            permissions: Permissions::from_bits(fields),
            file_type: FileType::from_bits((fields >> 24) as u8)
        }
    }

    pub fn to_bits(&self) -> u32 {
        self.permissions.to_bits() | (self.file_type.to_bits() as u32) << 24
    }
}

#[derive(Debug, PartialEq)]
pub struct Stat {
    /// for kernel use
    pub type_: u16,

    /// for kernel use
    pub dev: u32,

    pub qid: Qid,

    /// permissions and flags
    pub mode: FileMode,

    /// last access time
    pub atime: u32,

    /// last modification time
    pub mtime: u32,

    /// length of file in bytes
    pub length: u64,

    /// file name; must be / if the file is the root directory of the server
    pub name: String,

    /// owner name
    pub uid: String,

    /// group name
    pub gid: String,

    /// name of the user who last modified the file
    pub muid: String,
}



/// A filesystem entity, either a directory or a file.
pub trait FsEntity {
  fn stat(&self) -> Result<Stat>;
  fn wstat(&self, stat: &Stat) -> Result<()>;
}

pub trait Directory<F: File>: FsEntity + std::marker::Sized {

    /// The walk request carries as arguments an existing fid and a proposed newfid (which must not be in use unless it is the same as fid)
    /// that the client wishes to associate with the result of traversing the directory hierarchy by `walking' the hierarchy using the
    /// successive path name elements wname. The fid must represent a directory unless zero path name elements are specified.
    ///
    /// The name ``..'' (dot-dot) represents the parent directory. The name ``.'' (dot), meaning the current directory, is not used in the protocol.
    fn walk(&self, name: &str) -> Result<DirectoryOrFile<F, Self>>;

    ///  The create request asks the file server to create a new file with the name supplied,
    /// in the directory (dir) represented by fid, and requires write permission in the directory.
    /// The owner of the file is the implied user id of the request, the group of the file is the same as dir, and the permissions are the value of
    ///
    ///   perm & (~0666 | (dir.perm & 0666))
    /// if a regular file is being created and
    ///
    ///   perm & (~0777 | (dir.perm & 0777))
    /// if a directory is being created.
    ///
    /// This means, for example, that if the create allows read permission to others,
    /// but the containing directory does not, then the created file will not allow others to read the file.
    ///
    /// The names . and .. are special; it is illegal to create files with these names.
    fn create_file(&mut self, name: &str, perm: &Permissions) -> Result<()>;

    fn create_dir(&mut self, name: &str, perm: &Permissions) -> Result<()>;
}

pub trait File: FsEntity + std::marker::Sized {

    /// The remove request asks the file server both to remove the file represented by fid and to clunk the fid,
    /// even if the remove fails.
    /// This request will fail if the client does not have write permission in the parent directory.
    fn remove(&mut self) -> Result<()>;

    /// The open request asks the file server to check permissions and prepare a fid for I/O with subsequent read and write messages.
    /// The mode field determines the type of I/O:
    ///  read,
    ///  write,
    ///  execute,
    /// to be checked against the permissions for the file.
    /// In addition, if mode has the *truncate* boolean set, the file is to be truncated,
    /// which requires write permission (if the file is append-only, and permission is granted, the open succeeds but the file will not be truncated);
    /// if the mode has the *rclose* boolean set, the file is to be removed when the fid is clunked,
    /// which requires permission to remove the file from its directory.
    ///
    /// It is illegal to write a directory, truncate it, or attempt to remove it on close.
    ///
    /// If the file is marked for exclusive use (see stat(5)), only one client can have the file open at any time.
    /// That is, after such a file has been opened, further opens will fail until fid has been clunked.
    /// All these permissions are checked at the time of the open request;
    /// subsequent changes to the permissions of files do not affect the ability to read, write, or remove an open file.
    ///
    /// The iounit field returned by open may be zero.
    /// If it is not, it is the maximum number of bytes that are guaranteed to be read from or written to the file,
    /// without breaking the I/O transfer into multiple 9P messages; see read(5).
    fn open(&mut self, mode: OpenMode) -> Result<(Qid, IoUnit)>;

    /// Returns the mode in which the file is opened. If the file is not open, it returns None.
    fn mode(&self) -> Option<OpenMode>;

    /// The read request asks for count bytes of data from the file identified by fid,
    /// which must be opened for reading, starting offset bytes after the beginning of the file.
    ///
    /// Returns the amount of bytes actually read.
    fn read(&self, to: &mut dyn Write, offset: u64, count: u32) -> Result<u32>;

    /// The write request asks that count bytes of data be recorded in the file identified by fid,
    /// which must be opened for writing, starting offset bytes after the beginning of the file.
    /// If the file is append-only, the data will be placed at the end of the file regardless of offset.
    ///
    /// Returns the amount of bytes actually written.
    fn write(&mut self, from: &mut dyn Read, count: u32) -> Result<u32>;
}

pub enum DirectoryOrFile<F, D> {
    File(F),
    Directory(D),
}
