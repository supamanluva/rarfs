use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::aset::{parse_set, AssembledMember};
use crate::volumes::group_volumes;

#[derive(Debug, Clone)]
pub enum Node {
    Dir,
    Passthrough(PathBuf),
    Member(Arc<AssembledMember>),
}

struct DirCache {
    mtime: SystemTime,
    entries: Vec<(String, Node)>,
}

pub struct Catalog {
    root: PathBuf,
    cache: Mutex<HashMap<PathBuf, DirCache>>,
}

impl Catalog {
    pub fn new(root: PathBuf) -> Catalog {
        Catalog {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list(&self, rel: &Path) -> io::Result<Vec<(String, Node)>> {
        let dir = self.root.join(rel);
        let mtime = fs::metadata(&dir)?.modified()?;
        {
            let cache = self.cache.lock().unwrap();
            if let Some(c) = cache.get(rel) {
                if c.mtime == mtime {
                    return Ok(c.entries.clone());
                }
            }
        }
        let entries = self.scan(&dir)?;
        self.cache
            .lock()
            .unwrap()
            .insert(rel.to_path_buf(), DirCache { mtime, entries: entries.clone() });
        Ok(entries)
    }

    pub fn lookup(&self, rel: &Path) -> io::Result<Option<Node>> {
        if rel.as_os_str().is_empty() {
            return Ok(Some(Node::Dir));
        }
        let parent = rel.parent().unwrap_or(Path::new(""));
        let name = rel.file_name().unwrap().to_string_lossy().into_owned();
        Ok(self
            .list(parent)?
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, node)| node))
    }

    fn scan(&self, dir: &Path) -> io::Result<Vec<(String, Node)>> {
        let mut out: Vec<(String, Node)> = Vec::new();
        let mut file_names: Vec<String> = Vec::new();
        let mut dirs: Vec<String> = Vec::new();
        for e in fs::read_dir(dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            let ft = e.file_type()?;
            if ft.is_dir() {
                dirs.push(name);
            } else if ft.is_file() {
                file_names.push(name);
            }
        }
        for d in dirs {
            out.push((d, Node::Dir));
        }
        let sets = group_volumes(&file_names);
        for f in &file_names {
            out.push((f.clone(), Node::Passthrough(dir.join(f))));
        }
        for set in sets {
            let paths: Vec<PathBuf> = set.iter().map(|n| dir.join(n)).collect();
            match parse_set(&paths) {
                Ok(members) => {
                    for m in members {
                        // Flatten: base name only, per spec.
                        let base = Path::new(&m.name)
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| m.name.clone());
                        out.push((base, Node::Member(Arc::new(m))));
                    }
                }
                Err(e) => {
                    tracing::warn!("hiding unparseable set {}: {e}", paths[0].display());
                }
            }
        }
        Ok(out)
    }
}
