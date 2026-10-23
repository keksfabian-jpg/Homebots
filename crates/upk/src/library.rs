//! Index of all packages in a Homefront install, plus reads of bulk data that is
//! stored in another package file ("separate file" mips).

use crate::texture::{BulkData, BULK_LZO, BULK_UNUSED};
use crate::{decompress_chunk_stream, Package};
use std::collections::HashMap;
use std::fs::File;
#[allow(unused_imports)]
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

type BulkKey = (PathBuf, u64);
static PREFETCHED: std::sync::LazyLock<std::sync::Mutex<HashMap<BulkKey, Vec<u8>>>> = std::sync::LazyLock::new(Default::default);
static FILES: std::sync::LazyLock<std::sync::Mutex<HashMap<PathBuf, std::sync::Arc<File>>>> = std::sync::LazyLock::new(Default::default);
static STATS: std::sync::Mutex<(f64, u64, u64, u64)> = std::sync::Mutex::new((0.0, 0, 0, 0));

fn stat_add(t: std::time::Instant, bytes: usize, hit: bool) {
    if let Ok(mut s) = STATS.lock() {
        s.0 += t.elapsed().as_secs_f64();
        s.1 += 1;
        s.2 += bytes as u64;
        s.3 += hit as u64;
    }
}

/// (seconds spent in read_separate, reads, bytes, prefetch hits) since the last call.
pub fn take_read_stats() -> (f64, u64, u64, u64) {
    STATS.lock().map(|mut s| std::mem::replace(&mut *s, (0.0, 0, 0, 0))).unwrap_or_default()
}

/// One separate-file bulk read: file, absolute offset, size on disk, LZO compressed.
pub type BulkJob = (PathBuf, u64, usize, bool);
/// Reads recorded between `start_recording` and `stop_recording` (job, taken from the prefetch).
static RECORD: std::sync::Mutex<Option<Vec<(BulkJob, bool)>>> = std::sync::Mutex::new(None);

/// Record every `read_separate` from now on (a level build: its reads become the next
/// prefetch list for the same level, see `save_manifest`).
pub fn start_recording() {
    if let Ok(mut r) = RECORD.lock() {
        *r = Some(Vec::new());
    }
}

/// Stop recording; the reads in order (duplicates removed) and whether each came from the prefetch.
pub fn stop_recording() -> Vec<(BulkJob, bool)> {
    let v = RECORD.lock().ok().and_then(|mut r| r.take()).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|(j, _)| seen.insert((j.0.clone(), j.1))).collect()
}

/// Prefetch list file: one read per line, "offset<TAB>size<TAB>lzo<TAB>path".
pub fn save_manifest(path: &Path, jobs: &[BulkJob]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut s = String::new();
    for (p, off, size, lzo) in jobs {
        s.push_str(&format!("{off}\t{size}\t{}\t{}\n", *lzo as u8, p.display()));
    }
    let tmp = path.with_extension("tmp");
    std::fs::File::create(&tmp)?.write_all(s.as_bytes())?;
    std::fs::rename(tmp, path)
}

pub fn load_manifest(path: &Path) -> Option<Vec<BulkJob>> {
    let s = std::fs::read_to_string(path).ok()?;
    let mut v = Vec::new();
    for l in s.lines() {
        let mut it = l.splitn(4, '\t');
        let (off, size, lzo, p) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()? == "1", PathBuf::from(it.next()?));
        v.push((p, off, size, lzo));
    }
    Some(v)
}

/// Drop prefetched payloads nobody took (after a level load).
pub fn clear_prefetched() -> usize {
    PREFETCHED.lock().map(|mut m| {
        let n = m.len();
        m.clear();
        n
    }).unwrap_or(0)
}

fn read_at(f: &File, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], off + done as u64)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            done += n;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let mut f = f.try_clone()?;
        f.seek(SeekFrom::Start(off))?;
        f.read_exact(buf)
    }
}

fn read_bulk(path: &Path, off: u64, size: usize, lzo: bool) -> Option<Vec<u8>> {
    let f = {
        let mut m = FILES.lock().ok()?;
        match m.get(path) {
            Some(f) => f.clone(),
            None => {
                let f = std::sync::Arc::new(File::open(path).ok()?);
                m.insert(path.to_path_buf(), f.clone());
                f
            }
        }
    };
    // never allocate past the file end (unused mips carry offset/size 0xFFFFFFFF)
    let len = f.metadata().ok()?.len();
    if off.checked_add(size as u64)? > len {
        return None;
    }
    let mut raw = vec![0u8; size];
    read_at(&f, off, &mut raw).ok()?;
    if lzo {
        decompress_chunk_stream(0x2, &raw).ok()
    } else {
        Some(raw)
    }
}

pub struct Library {
    pub root: PathBuf,
    /// lowercase package stem -> file path
    pub files: HashMap<String, PathBuf>,
}

impl Library {
    /// `root` is the game folder (containing `GCGame/CookedPC`).
    pub fn scan(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut files = HashMap::new();
        let mut stack = vec![root.join("GCGame").join("CookedPC")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let p = entry?.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    if matches!(ext.to_ascii_lowercase().as_str(), "upk" | "u" | "map") {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            files.entry(stem.to_ascii_lowercase()).or_insert(p.clone());
                        }
                    }
                }
            }
        }
        Ok(Self { root, files })
    }

    pub fn path_of(&self, package: &str) -> Option<&PathBuf> {
        self.files.get(&package.to_ascii_lowercase())
    }

    /// Map and localisation files (FL-*.map, *_LOC_<lang>.upk, *_LOC_INT.upk): packages that
    /// only exist cooked into them (A2_Droids_C, A2_Airstrike_S, ...) are found there, their
    /// objects under the full path "Package.Group.Name".
    pub fn cooked_files(&self, lang: &str) -> Vec<PathBuf> {
        let loc = format!("_loc_{}", lang.to_ascii_lowercase());
        let mut v: Vec<(u8, PathBuf)> = self
            .files
            .iter()
            .filter_map(|(stem, p)| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if stem.starts_with("fl-") && ext == "map" {
                    Some((0, p.clone()))
                } else if stem.ends_with(&loc) && stem.starts_with("fl-") {
                    Some((1, p.clone()))
                } else if stem.ends_with("_loc_int") || stem.ends_with(&loc) {
                    Some((2, p.clone()))
                } else if stem.ends_with("_audio") && ext == "map" {
                    // the levels' ambience packages (A2_AmbientAudio_*)
                    Some((3, p.clone()))
                } else {
                    None
                }
            })
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    }

    pub fn open(&self, package: &str) -> Option<Package> {
        Package::open(self.path_of(package)?).ok()
    }

    /// Read a bulk payload that lives in `package`'s file at its absolute offset. The file stays
    /// open (one handle per package for the whole run, positional reads): opening it for every
    /// mip cost a file open per mip, slow on Windows with a virus scanner. Payloads prefetched
    /// in the background (`prefetch`) are taken from memory.
    pub fn read_separate(&self, package: &str, bulk: &BulkData) -> Option<Vec<u8>> {
        let t = std::time::Instant::now();
        let path = self.path_of(package)?;
        let key = (path.clone(), bulk.offset_in_file as u64);
        let lzo = bulk.flags & BULK_LZO != 0;
        let pre = PREFETCHED.lock().ok().and_then(|mut m| m.remove(&key));
        if let Ok(mut r) = RECORD.lock() {
            if let Some(r) = r.as_mut() {
                r.push(((path.clone(), key.1, bulk.size_on_disk as usize, lzo), pre.is_some()));
            }
        }
        if let Some(d) = pre {
            stat_add(t, d.len(), true);
            return Some(d);
        }
        let out = read_bulk(path, bulk.offset_in_file as u64, bulk.size_on_disk as usize, lzo);
        stat_add(t, out.as_ref().map(|d| d.len()).unwrap_or(0), false);
        out
    }

    /// Read and decompress separate-file bulk payloads on worker threads ahead of use (while the
    /// loading movie plays); `read_separate` then takes them from memory.
    pub fn prefetch(&self, items: Vec<(String, BulkData)>) {
        let jobs: Vec<BulkJob> = items
            .into_iter()
            .filter(|(_, b)| b.flags & BULK_UNUSED == 0)
            .filter_map(|(pkg, b)| Some((self.path_of(&pkg)?.clone(), b.offset_in_file as u64, b.size_on_disk as usize, b.flags & BULK_LZO != 0)))
            .collect();
        prefetch_jobs(jobs);
    }
}

/// `Library::prefetch` with resolved files (a recorded list, see `load_manifest`).
pub fn prefetch_jobs(jobs: Vec<BulkJob>) {
    {
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(2).clamp(1, 6);
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..workers {
                let _ = std::thread::Builder::new().stack_size(16 << 20).spawn_scoped(s, || loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((path, off, size, lzo)) = jobs.get(k) else { break };
                    if *size == 0 || *size == u32::MAX as usize {
                        continue;
                    }
                    let key = (path.clone(), *off);
                    if PREFETCHED.lock().map(|m| m.contains_key(&key)).unwrap_or(true) {
                        continue;
                    }
                    if let Some(d) = read_bulk(path, *off, *size, *lzo) {
                        if let Ok(mut m) = PREFETCHED.lock() {
                            m.insert(key, d);
                        }
                    }
                });
            }
        });
    }
}
