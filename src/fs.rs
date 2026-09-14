use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::fs::{FileExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyStatfs, Request,
};

use crate::aset::AssembledMember;
use crate::catalog::{Catalog, Node};
use crate::rarhdr::Method;
use crate::reader::{MemberReader, StoreReader};

const TTL: Duration = Duration::from_secs(1);

enum Handle {
    Passthrough(File),
    Member(Box<dyn MemberReader>),
}

// All FUSE callbacks below hand off to a freshly spawned thread and return
// immediately. Catalog lookups can block for seconds (rescanning a directory
// means re-parsing every RAR set in it, over what may be a network mount),
// and fuser dispatches requests one at a time on a single thread — without
// this, a slow lookup for one file stalls reads for every other file on the
// mount, including ones already mid-playback. fuser's Reply types are
// designed to be moved to another thread and completed asynchronously (see
// the module doc in the `fuser` crate's reply.rs), so this is the supported
// way to get concurrency out of it.
struct Shared {
    catalog: Catalog,
    by_ino: Mutex<HashMap<u64, PathBuf>>,
    by_path: Mutex<HashMap<PathBuf, u64>>,
    next_ino: AtomicU64,
    handles: Mutex<HashMap<u64, Arc<Mutex<Handle>>>>,
    next_fh: AtomicU64,
}

impl Shared {
    fn ino_for(&self, rel: &Path) -> u64 {
        if let Some(&ino) = self.by_path.lock().unwrap().get(rel) {
            return ino;
        }
        let ino = self.next_ino.fetch_add(1, AtomicOrdering::SeqCst);
        self.by_path.lock().unwrap().insert(rel.to_path_buf(), ino);
        self.by_ino.lock().unwrap().insert(ino, rel.to_path_buf());
        ino
    }

    fn path_for(&self, ino: u64) -> Option<PathBuf> {
        self.by_ino.lock().unwrap().get(&ino).cloned()
    }

    fn attr_for(ino: u64, node: &Node) -> io::Result<FileAttr> {
        let now = SystemTime::now();
        let attr = match node {
            Node::Dir => FileAttr {
                ino,
                size: 0,
                blocks: 0,
                atime: now,
                mtime: now,
                ctime: now,
                crtime: now,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                rdev: 0,
                blksize: 4096,
                flags: 0,
            },
            Node::Member(m) => FileAttr {
                ino,
                size: m.size,
                blocks: m.size.div_ceil(512),
                atime: now,
                mtime: now,
                ctime: now,
                crtime: now,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                rdev: 0,
                blksize: 4096,
                flags: 0,
            },
            Node::Passthrough(p) => {
                let md = std::fs::metadata(p)?;
                FileAttr {
                    ino,
                    size: md.len(),
                    blocks: md.blocks(),
                    atime: SystemTime::UNIX_EPOCH + Duration::new(md.atime() as u64, md.atime_nsec() as u32),
                    mtime: SystemTime::UNIX_EPOCH + Duration::new(md.mtime() as u64, md.mtime_nsec() as u32),
                    ctime: SystemTime::UNIX_EPOCH + Duration::new(md.ctime() as u64, md.ctime_nsec() as u32),
                    crtime: now,
                    kind: if md.is_dir() { FileType::Directory } else { FileType::RegularFile },
                    perm: (md.permissions().mode() & 0o7777) as u16,
                    nlink: md.nlink() as u32,
                    uid: md.uid(),
                    gid: md.gid(),
                    rdev: md.rdev() as u32,
                    blksize: md.blksize() as u32,
                    flags: 0,
                }
            }
        };
        Ok(attr)
    }

    fn member_reader(m: &AssembledMember) -> io::Result<Box<dyn MemberReader>> {
        match m.method {
            Method::Store => Ok(Box::new(StoreReader::new(m.size, m.segments.clone()))),
            Method::Compressed(_) => Ok(Box::new(crate::unrar_reader::UnrarReader::new(
                &m.segments[0].volume,
                &m.name,
                m.size,
            )?)),
        }
    }

    fn catalog_root(&self) -> PathBuf {
        self.catalog.root().to_path_buf()
    }
}

pub struct RarFs(Arc<Shared>);

impl RarFs {
    pub fn new(root: PathBuf) -> RarFs {
        let mut by_ino = HashMap::new();
        let mut by_path = HashMap::new();
        by_ino.insert(1u64, PathBuf::new());
        by_path.insert(PathBuf::new(), 1u64);
        RarFs(Arc::new(Shared {
            catalog: Catalog::new(root),
            by_ino: Mutex::new(by_ino),
            by_path: Mutex::new(by_path),
            next_ino: AtomicU64::new(2),
            handles: Mutex::new(HashMap::new()),
            next_fh: AtomicU64::new(1),
        }))
    }
}

impl Filesystem for RarFs {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let shared = Arc::clone(&self.0);
        let name = name.to_os_string();
        std::thread::spawn(move || {
            let Some(parent_path) = shared.path_for(parent) else {
                reply.error(libc::ENOENT);
                return;
            };
            let rel = parent_path.join(&name);
            match shared.catalog.lookup(&rel) {
                Ok(Some(node)) => {
                    let ino = shared.ino_for(&rel);
                    match Shared::attr_for(ino, &node) {
                        Ok(attr) => reply.entry(&TTL, &attr, 0),
                        Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
                    }
                }
                Ok(None) => reply.error(libc::ENOENT),
                Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        });
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let shared = Arc::clone(&self.0);
        std::thread::spawn(move || {
            let Some(rel) = shared.path_for(ino) else {
                reply.error(libc::ENOENT);
                return;
            };
            match shared.catalog.lookup(&rel) {
                Ok(Some(node)) => match Shared::attr_for(ino, &node) {
                    Ok(attr) => reply.attr(&TTL, &attr),
                    Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
                },
                Ok(None) => reply.error(libc::ENOENT),
                Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        });
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let shared = Arc::clone(&self.0);
        std::thread::spawn(move || {
            let Some(rel) = shared.path_for(ino) else {
                reply.error(libc::ENOENT);
                return;
            };
            let entries = match shared.catalog.list(&rel) {
                Ok(e) => e,
                Err(e) => {
                    reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                    return;
                }
            };
            let mut items: Vec<(u64, FileType, String)> = vec![
                (ino, FileType::Directory, ".".into()),
                (1, FileType::Directory, "..".into()),
            ];
            for (name, node) in entries {
                let child_rel = rel.join(&name);
                let child_ino = shared.ino_for(&child_rel);
                let kind = match node {
                    Node::Dir => FileType::Directory,
                    _ => FileType::RegularFile,
                };
                items.push((child_ino, kind, name));
            }
            for (i, (child_ino, kind, name)) in items.into_iter().enumerate().skip(offset as usize) {
                if reply.add(child_ino, (i + 1) as i64, kind, name) {
                    break;
                }
            }
            reply.ok();
        });
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        if flags & libc::O_ACCMODE != libc::O_RDONLY {
            reply.error(libc::EROFS);
            return;
        }
        let shared = Arc::clone(&self.0);
        std::thread::spawn(move || {
            let Some(rel) = shared.path_for(ino) else {
                reply.error(libc::ENOENT);
                return;
            };
            let node = match shared.catalog.lookup(&rel) {
                Ok(Some(n)) => n,
                Ok(None) => {
                    reply.error(libc::ENOENT);
                    return;
                }
                Err(e) => {
                    reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                    return;
                }
            };
            let handle = match node {
                Node::Dir => {
                    reply.error(libc::EISDIR);
                    return;
                }
                Node::Passthrough(p) => match File::open(&p) {
                    Ok(f) => Handle::Passthrough(f),
                    Err(e) => {
                        reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                        return;
                    }
                },
                Node::Member(m) => match Shared::member_reader(&m) {
                    Ok(r) => Handle::Member(r),
                    Err(e) => {
                        reply.error(e.raw_os_error().unwrap_or(libc::ENOSYS));
                        return;
                    }
                },
            };
            let fh = shared.next_fh.fetch_add(1, AtomicOrdering::SeqCst);
            shared.handles.lock().unwrap().insert(fh, Arc::new(Mutex::new(handle)));
            reply.opened(fh, 0);
        });
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let shared = Arc::clone(&self.0);
        std::thread::spawn(move || {
            // Clone the Arc and drop the map lock before doing I/O so one slow
            // or hung decode does not serialize/freeze reads on other files.
            let h = {
                let handles = shared.handles.lock().unwrap();
                match handles.get(&fh) {
                    Some(h) => h.clone(),
                    None => {
                        reply.error(libc::EBADF);
                        return;
                    }
                }
            };
            // Per-handle serialization: MemberReader::read_at takes &mut self
            // and is not re-entrant.
            let mut h = h.lock().unwrap();
            let mut buf = vec![0u8; size as usize];
            let result = match &mut *h {
                Handle::Passthrough(f) => f.read_at(&mut buf, offset as u64),
                Handle::Member(r) => r.read_at(offset as u64, &mut buf),
            };
            match result {
                Ok(n) => reply.data(&buf[..n]),
                Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        });
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.0.handles.lock().unwrap().remove(&fh);
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
        let shared = Arc::clone(&self.0);
        std::thread::spawn(move || {
            let root = shared.catalog_root();
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            let c = std::ffi::CString::new(root.as_os_str().as_encoded_bytes()).unwrap();
            if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
                reply.statfs(
                    st.f_blocks,
                    st.f_bfree,
                    st.f_bavail,
                    st.f_files,
                    st.f_ffree,
                    st.f_bsize as u32,
                    st.f_namemax as u32,
                    st.f_frsize as u32,
                );
            } else {
                reply.error(libc::EIO);
            }
        });
    }
}
