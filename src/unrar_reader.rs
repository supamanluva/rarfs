use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;

use crate::reader::MemberReader;
use crate::unrar_ffi::UnrarArchive;

const WINDOW: usize = 8 * 1024 * 1024; // 8 MiB tail window per spec

struct Shared {
    buf: VecDeque<u8>,
    produced: u64,   // total bytes ever pushed by the decoder
    dropped: u64,    // total bytes dropped from the front (window start)
    done: bool,      // decoder finished (or errored)
    err: Option<i32>,
    stop: bool, // request decoder thread to abort
}

pub struct UnrarReader {
    first_volume: PathBuf,
    member_name: String,
    size: u64,
    shared: std::sync::Arc<(Mutex<Shared>, Condvar)>,
    decoder: Option<JoinHandle<()>>,
    // Decode-side handle for the callback (passed as a raw pointer; the Arc
    // is kept alive here for the reader's whole lifetime).
    dec: std::sync::Arc<DecState>,
}

struct DecState {
    shared: std::sync::Arc<(Mutex<Shared>, Condvar)>,
}

extern "C" fn on_data(user_data: usize, data: *const u8, len: usize) -> i32 {
    let dec = unsafe { &*(user_data as *const DecState) };
    let (lock, cv) = &*dec.shared;
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    let mut off = 0usize;
    while off < bytes.len() {
        let mut s = lock.lock().unwrap();
        while !s.stop && s.buf.len() >= WINDOW {
            s = cv.wait(s).unwrap(); // consumer must drain first
        }
        if s.stop {
            return -1; // abort unrar
        }
        let room = WINDOW - s.buf.len();
        let n = room.min(bytes.len() - off);
        s.buf.extend(bytes[off..off + n].iter().copied());
        s.produced += n as u64;
        off += n;
        cv.notify_all();
    }
    1
}

fn decode_loop(first_volume: PathBuf, member_name: String, dec: std::sync::Arc<DecState>) {
    let (lock, cv) = &*dec.shared;
    let result = (|| -> Result<(), i32> {
        let a = UnrarArchive::open(&first_volume)?;
        a.set_callback(on_data, std::sync::Arc::as_ptr(&dec) as usize);
        let mut name_buf = vec![0u8; 4096];
        loop {
            match a.read_next_name(&mut name_buf) {
                Ok(n) => {
                    let name = String::from_utf8_lossy(&name_buf[..n]).into_owned();
                    if name == member_name {
                        a.process_current()?;
                        return Ok(());
                    } else {
                        a.skip_current()?;
                    }
                }
                Err(e) => return Err(e), // includes END_ARCHIVE: member not found
            }
        }
    })();
    let mut s = lock.lock().unwrap();
    s.done = true;
    if let Err(e) = result {
        if !s.stop {
            s.err = Some(e);
        }
    }
    cv.notify_all();
}

impl UnrarReader {
    pub fn new(first_volume: &Path, member_name: &str, size: u64) -> io::Result<UnrarReader> {
        let shared = std::sync::Arc::new((
            Mutex::new(Shared {
                buf: VecDeque::new(),
                produced: 0,
                dropped: 0,
                done: false,
                err: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let dec = std::sync::Arc::new(DecState {
            shared: shared.clone(),
        });
        let mut r = UnrarReader {
            first_volume: first_volume.to_path_buf(),
            member_name: member_name.to_string(),
            size,
            shared,
            decoder: None,
            dec,
        };
        r.start_decoder();
        Ok(r)
    }

    fn start_decoder(&mut self) {
        {
            let (lock, _) = &*self.shared;
            let mut s = lock.lock().unwrap();
            *s = Shared {
                buf: VecDeque::new(),
                produced: 0,
                dropped: 0,
                done: false,
                err: None,
                stop: false,
            };
        }
        let dec = self.dec.clone();
        let vol = self.first_volume.clone();
        let name = self.member_name.clone();
        self.decoder = Some(std::thread::spawn(move || decode_loop(vol, name, dec)));
    }

    fn stop_decoder(&mut self) {
        {
            let (lock, cv) = &*self.shared;
            let mut s = lock.lock().unwrap();
            s.stop = true;
            cv.notify_all();
        }
        if let Some(h) = self.decoder.take() {
            let _ = h.join();
        }
    }
}

impl MemberReader for UnrarReader {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.size {
            return Ok(0);
        }
        // Reads larger than WINDOW are served WINDOW bytes at a time; the
        // only current caller (FUSE read) is kernel-capped well below this.
        let want = ((self.size - offset).min(buf.len() as u64)) as usize;
        let want = want.min(WINDOW);
        // Clone the Arc so the borrow of `self` ends before the mutable
        // `stop_decoder`/`start_decoder` calls in the restart branch below.
        let shared = self.shared.clone();
        let (lock, cv) = &*shared;
        loop {
            {
                let s = lock.lock().unwrap();
                if offset < s.dropped {
                    // Requested data already fell out of the window: restart.
                    drop(s);
                    self.stop_decoder();
                    self.start_decoder();
                    continue;
                }
            }
            let mut s = lock.lock().unwrap();
            // Wait until the buffered range covers [offset, offset+want)
            // (or EOF) — buf.len() alone is wrong when the decoder has not
            // produced up to `offset` yet. saturating_sub: produced may be
            // behind `offset` (reads ahead of the decoder).
            while !s.done && (s.dropped + s.buf.len() as u64).saturating_sub(offset) < want as u64 {
                // Forward seek past the frontier of decoded data: everything
                // buffered so far precedes `offset` and will be discarded by
                // the skip path below anyway. If we waited for coverage
                // without draining, the producer would stay blocked on the
                // full window and coverage could never be reached — a
                // deadlock. Discard now so the producer can advance.
                if offset > s.dropped + s.buf.len() as u64 && !s.buf.is_empty() {
                    s.dropped += s.buf.len() as u64;
                    s.buf.clear();
                    cv.notify_all(); // wake producer waiting for room
                }
                s = cv.wait(s).unwrap();
            }
            // Drop everything before the requested offset.
            let skip = (offset - s.dropped) as usize;
            if skip > 0 {
                let n = skip.min(s.buf.len());
                s.buf.drain(..n);
                s.dropped += n as u64;
                cv.notify_all(); // wake producer waiting for room
            }
            if let Some(e) = s.err {
                if s.buf.len() < want {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unrar error {e}")));
                }
            }
            let n = want.min(s.buf.len());
            if n == 0 && s.done {
                return Ok(0); // EOF
            }
            let tmp: Vec<u8> = s.buf.iter().take(n).copied().collect();
            buf[..n].copy_from_slice(&tmp);
            return Ok(n);
        }
    }
}

impl Drop for UnrarReader {
    fn drop(&mut self) {
        self.stop_decoder();
    }
}
