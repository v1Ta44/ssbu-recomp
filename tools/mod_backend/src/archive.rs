//! Read-only virtual data.arc. Metadata and new groups are appended to a sparse view;
//! original file bytes are never rewritten. Layout reference: smash-arc 0.6.0 and
//! runtime/loose_files.c (13.0.4). Every index is checked before the view is published.
use crate::{
    packages::{Config, Diagnostic, Plan},
    paths::{self, MASK, NONE},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
pub fn u32at(b: &[u8], i: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(i..i + 4).ok_or("truncated u32")?.try_into().unwrap(),
    ))
}
pub fn u64at(b: &[u8], i: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        b.get(i..i + 8).ok_or("truncated u64")?.try_into().unwrap(),
    ))
}
fn put32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], i: usize, v: u64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}
fn idx(n: usize) -> Result<u32, String> {
    if n >= NONE as usize {
        Err("archive index limit exceeded".into())
    } else {
        Ok(n as u32)
    }
}
fn size(n: u64) -> Result<u32, String> {
    n.try_into()
        .map_err(|_| "resource exceeds 32-bit ARC size".into())
}
#[derive(Clone)]
struct Table<const N: usize>(Vec<[u8; N]>);
impl<const N: usize> Table<N> {
    fn parse(b: &[u8], pos: &mut usize, n: usize) -> Result<Self, String> {
        let len = n.checked_mul(N).ok_or("table overflow")?;
        let end = pos.checked_add(len).ok_or("table overflow")?;
        let data = b.get(*pos..end).ok_or("truncated archive table")?;
        *pos = end;
        Ok(Self(
            data.chunks_exact(N)
                .map(|c| c.try_into().unwrap())
                .collect(),
        ))
    }
    fn r32(&self, i: usize, o: usize) -> Result<u32, String> {
        u32at(self.0.get(i).ok_or("archive index out of range")?, o)
    }
    fn r64(&self, i: usize, o: usize) -> Result<u64, String> {
        u64at(self.0.get(i).ok_or("archive index out of range")?, o)
    }
    fn w32(&mut self, i: usize, o: usize, v: u32) {
        put32(&mut self.0[i], o, v);
    }
    fn w64(&mut self, i: usize, o: usize, v: u64) {
        put64(&mut self.0[i], o, v);
    }
    fn add(&mut self, row: [u8; N]) -> Result<u32, String> {
        let i = idx(self.0.len())?;
        self.0.push(row);
        Ok(i)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn write(&self, out: &mut Vec<u8>) {
        for row in &self.0 {
            out.extend(row);
        }
    }
}
fn read_file(f: &File, off: u64, b: &mut [u8]) -> Result<usize, String> {
    let mut n = 0;
    while n < b.len() {
        #[cfg(windows)]
        let got = {
            use std::os::windows::fs::FileExt;
            f.seek_read(&mut b[n..], off + n as u64)
        };
        #[cfg(unix)]
        let got = {
            use std::os::unix::fs::FileExt;
            f.read_at(&mut b[n..], off + n as u64)
        };
        let got = got.map_err(|e| e.to_string())?;
        if got == 0 {
            break;
        }
        n += got;
    }
    Ok(n)
}
fn block(f: &File, off: u64, len: usize) -> Result<Vec<u8>, String> {
    let mut b = vec![0; len];
    if read_file(f, off, &mut b)? != len {
        return Err("short archive read".into());
    }
    Ok(b)
}
fn section(f: &File, off: u64) -> Result<Vec<u8>, String> {
    let h = block(f, off, 16)?;
    if u32at(&h, 0)? != 16 {
        return Err("invalid ARC section header".into());
    }
    let dec = u32at(&h, 4)? as usize;
    let comp = u32at(&h, 8)? as usize;
    if dec > 512 * 1024 * 1024 || comp > 512 * 1024 * 1024 {
        return Err("ARC metadata section too large".into());
    }
    let b = zstd::bulk::decompress(&block(f, off + 16, comp)?, dec).map_err(|e| e.to_string())?;
    if b.len() != dec {
        return Err("ARC metadata length mismatch".into());
    }
    Ok(b)
}
fn compressed_section(b: &[u8]) -> Result<Vec<u8>, String> {
    let comp = zstd::bulk::compress(b, 3).map_err(|e| e.to_string())?;
    let len = (comp.len() + 15) & !15;
    let mut out = Vec::with_capacity(16 + len);
    out.extend(16u32.to_le_bytes());
    out.extend((b.len() as u32).to_le_bytes());
    out.extend((comp.len() as u32).to_le_bytes());
    out.extend((len as u32).to_le_bytes());
    out.extend(comp);
    out.resize(16 + len, 0);
    Ok(out)
}
struct Fs {
    header: Vec<u8>,
    quick: Table<12>,
    stream_hash: Table<8>,
    streams: Table<12>,
    stream_indices: Table<4>,
    stream_data: Table<16>,
    buckets: usize,
    paths: Table<32>,
    indices: Table<8>,
    dirs: Table<52>,
    folders: Table<28>,
    children: Table<8>,
    infos: Table<16>,
    itds: Table<12>,
    data: Table<16>,
    tail: Vec<u8>,
}
impl Fs {
    fn parse(b: &[u8]) -> Result<Self, String> {
        let h = |o| u32at(b, o).map(|v| v as usize);
        let mut p = 0x100;
        let q = u32at(b, p)? as usize;
        let s = u32at(b, p + 4)? as usize;
        let si = u32at(b, p + 8)? as usize;
        let sd = u32at(b, p + 12)? as usize;
        p += 16;
        let quick = Table::parse(b, &mut p, q)?;
        let stream_hash = Table::parse(b, &mut p, s)?;
        let streams = Table::parse(b, &mut p, s)?;
        let stream_indices = Table::parse(b, &mut p, si)?;
        let stream_data = Table::parse(b, &mut p, sd)?;
        let ne = u32at(b, p)? as usize;
        let buckets = u32at(b, p + 4)? as usize;
        p += 8;
        if buckets == 0 || buckets > 0x100000 {
            return Err("invalid ARC buckets".into());
        }
        let _ = Table::<8>::parse(b, &mut p, buckets)?;
        let _ = Table::<8>::parse(b, &mut p, ne)?;
        let paths = Table::parse(b, &mut p, h(4)?)?;
        let indices = Table::parse(b, &mut p, h(8)?)?;
        let _ = Table::<8>::parse(b, &mut p, h(12)?)?;
        let dirs = Table::parse(b, &mut p, h(12)?)?;
        let folders = Table::parse(b, &mut p, h(16)? + h(36)? + h(64)?)?;
        let children = Table::parse(b, &mut p, h(20)?)?;
        let infos = Table::parse(b, &mut p, h(24)? + h(40)? + h(68)?)?;
        let itds = Table::parse(b, &mut p, h(28)? + h(40)? + h(80)?)?;
        let data = Table::parse(b, &mut p, h(32)? + h(40)? + h(84)?)?;
        Ok(Self {
            header: b[..256].to_vec(),
            quick,
            stream_hash,
            streams,
            stream_indices,
            stream_data,
            buckets,
            paths,
            indices,
            dirs,
            folders,
            children,
            infos,
            itds,
            data,
            tail: b[p..].to_vec(),
        })
    }
    fn repartition(&mut self) -> Result<(), String> {
        // Native version fixups locate auxiliary rows relative to the primary counts.
        // Therefore append-only working arrays must be packed as primary + new + auxiliary.
        // Every foreign key is relocated once before encoding; the opaque version tail stays intact.
        #[derive(Clone, Copy)]
        struct Relocate {
            primary: usize,
            old_total: usize,
            added: usize,
        }
        impl Relocate {
            fn map(self, value: u32) -> Result<u32, String> {
                if value == NONE {
                    return Ok(value);
                }
                let i = value as usize;
                if i >= self.old_total + self.added {
                    return Err("archive foreign key exceeds its table".into());
                }
                idx(if i < self.primary {
                    i
                } else if i < self.old_total {
                    i + self.added
                } else {
                    self.primary + i - self.old_total
                })
            }
            fn pack<const N: usize>(self, table: &mut Table<N>) {
                if self.added == 0 {
                    return;
                }
                let tail = table.0.split_off(self.old_total);
                let auxiliary = table.0.split_off(self.primary);
                table.0.extend(tail);
                table.0.extend(auxiliary);
            }
        }
        let make = |primary: usize, aux: usize, total: usize| -> Result<Relocate, String> {
            let old_total = primary.checked_add(aux).ok_or("table overflow")?;
            Ok(Relocate {
                primary,
                old_total,
                added: total.checked_sub(old_total).ok_or("table shrank")?,
            })
        };
        let fo = make(
            u32at(&self.header, 16)? as usize,
            (u32at(&self.header, 36)? + u32at(&self.header, 64)?) as usize,
            self.folders.len(),
        )?;
        let info = make(
            u32at(&self.header, 24)? as usize,
            (u32at(&self.header, 40)? + u32at(&self.header, 68)?) as usize,
            self.infos.len(),
        )?;
        let itd = make(
            u32at(&self.header, 28)? as usize,
            (u32at(&self.header, 40)? + u32at(&self.header, 80)?) as usize,
            self.itds.len(),
        )?;
        let data = make(
            u32at(&self.header, 32)? as usize,
            (u32at(&self.header, 40)? + u32at(&self.header, 84)?) as usize,
            self.data.len(),
        )?;
        let old_dirs = u32at(&self.header, 12)?;
        let mut symlinks = HashSet::new();
        for i in 0..self.dirs.len() {
            let q = self.dirs.r64(i, 0)?;
            let f = (q >> 40) as u32;
            if self.dirs.r32(i, 48)? & (1 << 28) != 0 {
                symlinks.insert(f as usize);
            }
            self.dirs
                .w64(i, 0, (q & MASK) | ((fo.map(f)? as u64) << 40));
            let start = self.dirs.r32(i, 32)?;
            self.dirs.w32(i, 32, info.map(start)?);
        }
        for i in 0..self.indices.len() {
            let d = self.indices.r32(i, 0)?;
            if d >= old_dirs && d != NONE {
                self.indices.w32(i, 0, fo.map(d)?);
            }
            let v = self.indices.r32(i, 4)?;
            self.indices.w32(i, 4, (v & !NONE) | info.map(v & NONE)?);
        }
        for i in 0..self.infos.len() {
            let v = self.infos.r32(i, 8)?;
            self.infos.w32(i, 8, itd.map(v)?);
        }
        for i in 0..self.itds.len() {
            let f = self.itds.r32(i, 0)?;
            let d = self.itds.r32(i, 4)?;
            let b = self.itds.r32(i, 8)?;
            self.itds.w32(i, 0, fo.map(f)?);
            self.itds.w32(i, 4, data.map(d)?);
            self.itds.w32(i, 8, (b & !NONE) | info.map(b & NONE)?);
        }
        for i in 0..self.folders.len() {
            let start = self.folders.r32(i, 16)?;
            let mapped = if i >= fo.primary && i < fo.old_total {
                info.map(start)?
            } else {
                data.map(start)?
            };
            self.folders.w32(i, 16, mapped);
            let d = self.folders.r32(i, 24)?;
            if d != NONE && !symlinks.contains(&i) {
                self.folders.w32(i, 24, fo.map(d)?);
            }
        }
        fo.pack(&mut self.folders);
        info.pack(&mut self.infos);
        itd.pack(&mut self.itds);
        data.pack(&mut self.data);
        Ok(())
    }
    fn encode(&self) -> Result<Vec<u8>, String> {
        let mut h = self.header.clone();
        for (o, n) in [
            (4, self.paths.len()),
            (8, self.indices.len()),
            (12, self.dirs.len()),
            (20, self.children.len()),
        ] {
            put32(&mut h, o, size(n as u64)?);
        }
        // Preserve the version/DLC auxiliary counts: the native constructor uses
        // them again to parse the opaque tail. Added rows belong to primary tables.
        for (o, total, auxiliary) in [
            (16, self.folders.len(), u32at(&h, 36)? + u32at(&h, 64)?),
            (24, self.infos.len(), u32at(&h, 40)? + u32at(&h, 68)?),
            (28, self.itds.len(), u32at(&h, 40)? + u32at(&h, 80)?),
            (32, self.data.len(), u32at(&h, 40)? + u32at(&h, 84)?),
        ] {
            put32(
                &mut h,
                o,
                size(total as u64)?
                    .checked_sub(auxiliary)
                    .ok_or("invalid table counts")?,
            );
        }
        let mut out = h;
        for n in [
            self.quick.len(),
            self.streams.len(),
            self.stream_indices.len(),
            self.stream_data.len(),
        ] {
            out.extend(size(n as u64)?.to_le_bytes());
        }
        self.quick.write(&mut out);
        self.stream_hash.write(&mut out);
        self.streams.write(&mut out);
        self.stream_indices.write(&mut out);
        self.stream_data.write(&mut out);
        out.extend(size(self.paths.len() as u64)?.to_le_bytes());
        out.extend(size(self.buckets as u64)?.to_le_bytes());
        let mut hashes: Vec<_> = (0..self.paths.len())
            .map(|i| Ok((self.paths.r64(i, 0)? & MASK) | ((i as u64) << 40)))
            .collect::<Result<_, String>>()?;
        hashes.sort_by_key(|v| ((v & MASK) % (self.buckets as u64), v & MASK));
        let mut start = 0;
        for bucket in 0..self.buckets {
            let first = start;
            while start < hashes.len()
                && (hashes[start] & MASK) % (self.buckets as u64) == bucket as u64
            {
                start += 1;
            }
            out.extend((first as u32).to_le_bytes());
            out.extend(((start - first) as u32).to_le_bytes());
        }
        for h in hashes {
            out.extend(h.to_le_bytes());
        }
        self.paths.write(&mut out);
        self.indices.write(&mut out);
        let mut dirs: Vec<_> = (0..self.dirs.len())
            .map(|i| Ok((self.dirs.r64(i, 0)? & MASK) | ((i as u64) << 40)))
            .collect::<Result<_, String>>()?;
        dirs.sort_by_key(|v| v & MASK);
        for h in dirs {
            out.extend(h.to_le_bytes());
        }
        self.dirs.write(&mut out);
        self.folders.write(&mut out);
        self.children.write(&mut out);
        self.infos.write(&mut out);
        self.itds.write(&mut out);
        self.data.write(&mut out);
        out.extend(&self.tail);
        let len = size(out.len() as u64)?;
        put32(&mut out, 0, len);
        Ok(out)
    }
    fn itd(&self, info: usize, region: usize) -> Result<usize, String> {
        let mut i = self.infos.r32(info, 8)? as usize;
        let f = self.infos.r32(info, 12)?;
        if f & 0x8000 != 0 {
            i += region;
        } else if f & 0x10000 != 0 {
            i += paths::locale(region);
        }
        if i >= self.itds.len() {
            return Err("invalid regional index".into());
        }
        Ok(i)
    }
    fn info(&self, path: usize) -> Result<usize, String> {
        let xi = (self.paths.r64(path, 0)? >> 40) as usize;
        Ok((self.indices.r32(xi, 4)? & NONE) as usize)
    }
    fn record(&self, info: usize, region: usize, file_off: u64) -> Result<(u64, [u8; 16]), String> {
        let itd = self.itd(info, region)?;
        let fo = self.itds.r32(itd, 0)? as usize;
        let di = self.itds.r32(itd, 4)? as usize;
        let d = *self.data.0.get(di).ok_or("invalid FileData")?;
        let offset = file_off
            .checked_add(self.folders.r64(fo, 0)?)
            .and_then(|v| v.checked_add(u32at(&d, 0).ok()? as u64 * 4))
            .ok_or("file offset overflow")?;
        Ok((offset, d))
    }
}
struct Search {
    folder_map: HashMap<u64, usize>,
    path_set: HashSet<u64>,
    header: Vec<u8>,
    folders: Table<32>,
    folder_hash: Table<8>,
    indices: Table<4>,
    hashes: Table<8>,
    paths: Table<32>,
    tail: Vec<u8>,
}
impl Search {
    fn parse(b: &[u8]) -> Result<Self, String> {
        let nf = u32at(b, 8)? as usize;
        let ni = u32at(b, 12)? as usize;
        let np = u32at(b, 16)? as usize;
        let mut p = 20;
        let folder_hash = Table::parse(b, &mut p, nf)?;
        let folders = Table::parse(b, &mut p, nf)?;
        let hashes = Table::parse(b, &mut p, ni)?;
        let indices = Table::parse(b, &mut p, ni)?;
        let paths = Table::parse(b, &mut p, np)?;
        let folder_map = (0..folders.len())
            .map(|i| (folders.r64(i, 0).unwrap() & MASK, i))
            .collect();
        let path_set = hashes
            .0
            .iter()
            .map(|r| u64at(r, 0).unwrap() & MASK)
            .collect();
        Ok(Self {
            folder_map,
            path_set,
            header: b[..20].to_vec(),
            folders,
            folder_hash,
            indices,
            hashes,
            paths,
            tail: b[p..].to_vec(),
        })
    }
    fn encode(&self) -> Result<Vec<u8>, String> {
        let mut b = self.header.clone();
        put32(&mut b, 8, self.folders.len() as u32);
        put32(&mut b, 12, self.indices.len() as u32);
        put32(&mut b, 16, self.paths.len() as u32);
        let mut h = self.folder_hash.0.clone();
        h.sort_by_key(|r| u64at(r, 0).unwrap() & MASK);
        for r in h {
            b.extend(r);
        }
        self.folders.write(&mut b);
        let mut h = self.hashes.0.clone();
        h.sort_by_key(|r| u64at(r, 0).unwrap() & MASK);
        for r in h {
            b.extend(r);
        }
        self.indices.write(&mut b);
        self.paths.write(&mut b);
        b.extend(&self.tail);
        let len = b.len() as u64;
        put64(&mut b, 0, len);
        Ok(b)
    }
    fn add_spec(&mut self, spec: &paths::FileSpec) -> Result<(), String> {
        fn folder(s: &mut Search, f: &paths::FolderSpec) -> Result<(), String> {
            if s.folder_map.contains_key(&f.hash) {
                return Ok(());
            }
            if let Some(p) = &f.parent {
                folder(s, p)?;
            }
            s.insert_hash(
                f.hash,
                f.parent.as_ref().map(|p| p.hash).unwrap_or(paths::hash("")),
                f.name.ok_or("unknown hash-only parent folder")?,
                0,
                true,
            )
        }
        folder(self, &spec.parent)?;
        self.insert_hash(spec.hash, spec.parent.hash, spec.name, spec.ext, false)
    }
    fn insert_hash(
        &mut self,
        h: u64,
        parent: u64,
        name: u64,
        ext: u64,
        is_dir: bool,
    ) -> Result<(), String> {
        if self.path_set.contains(&h) {
            return Ok(());
        }
        let ip = self.indices.len();
        let mut next = NONE;
        if let Some(&f) = self.folder_map.get(&parent) {
            next = (self.folders.r64(f, 24)? & 0xffffff) as u32;
            let e = self.folders.r64(f, 24)?;
            self.folders.w64(f, 24, (e & !0xffffff) | ip as u64);
            let q = self.folders.r64(f, 8)?;
            self.folders.w64(f, 8, q + (1u64 << 40));
        }
        let mut row = [0; 32];
        put64(&mut row, 0, h | ((next as u64) << 40));
        put64(
            &mut row,
            8,
            parent | if is_dir { 0x400000u64 << 40 } else { 0 },
        );
        put64(&mut row, 16, name);
        put64(&mut row, 24, ext);
        let pp = self.paths.add(row)?;
        self.indices.add(pp.to_le_bytes())?;
        self.hashes.add((h | ((ip as u64) << 40)).to_le_bytes())?;
        self.path_set.insert(h);
        if is_dir {
            let fi = self.folders.len();
            put64(&mut row, 0, h | ((NONE as u64) << 40));
            put64(&mut row, 8, parent);
            put64(&mut row, 24, NONE as u64);
            self.folders.add(row)?;
            self.folder_hash
                .add((h | ((fi as u64) << 40)).to_le_bytes())?;
            self.folder_map.insert(h, fi);
        }
        Ok(())
    }
    fn add(&mut self, path: &str) -> Result<(), String> {
        self.add_kind(path, false)
    }
    fn add_folder(&mut self, path: &str) -> Result<(), String> {
        self.add_kind(path, true)
    }
    fn add_kind(&mut self, path: &str, is_folder: bool) -> Result<(), String> {
        let mut parts: Vec<_> = path
            .match_indices('/')
            .map(|(i, _)| (&path[..i], true))
            .collect();
        parts.push((path, is_folder));
        for (p, is_dir) in parts {
            let h = paths::path_hash(p);
            if (is_dir && self.folder_map.contains_key(&h))
                || (!is_dir && self.path_set.contains(&h))
            {
                continue;
            }
            let (parent, name) = p.rsplit_once('/').unwrap_or(("", p));
            let ph = paths::hash(parent);
            let ip = self.indices.len();
            let mut next = NONE;
            if let Some(&f) = self.folder_map.get(&ph) {
                next = (self.folders.r64(f, 24)? & 0xffffff) as u32;
                let ext = self.folders.r64(f, 24)?;
                self.folders.w64(f, 24, (ext & !0xffffff) | ip as u64);
                let q = self.folders.r64(f, 8)?;
                self.folders.w64(f, 8, q + (1u64 << 40));
            }
            let mut row = [0u8; 32];
            put64(&mut row, 0, h | ((next as u64) << 40));
            put64(&mut row, 8, ph | if is_dir { 0x400000u64 << 40 } else { 0 });
            put64(&mut row, 16, paths::name_hash(name));
            if !is_dir {
                put64(
                    &mut row,
                    24,
                    paths::hash(name.rsplit_once('.').map(|(_, e)| e).unwrap_or("")),
                );
            }
            let pp = self.paths.add(row)?;
            self.indices.add(pp.to_le_bytes())?;
            self.hashes.add((h | ((ip as u64) << 40)).to_le_bytes())?;
            self.path_set.insert(h);
            if is_dir {
                let fi = self.folders.len();
                put64(&mut row, 0, h | ((NONE as u64) << 40));
                put64(&mut row, 8, ph);
                put64(&mut row, 24, NONE as u64);
                self.folders.add(row)?;
                self.folder_hash
                    .add((h | ((fi as u64) << 40)).to_le_bytes())?;
                self.folder_map.insert(h, fi);
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
enum Bytes {
    Memory(Arc<[u8]>),
    File(Arc<File>, u64),
    Zero,
}
#[derive(Clone)]
struct Segment {
    start: u64,
    len: u64,
    bytes: Bytes,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub replacements: usize,
    pub additions: usize,
    pub streams: usize,
    pub groups: usize,
    pub shared_sources: usize,
    pub patches: usize,
    pub original_size: u64,
    pub virtual_size: u64,
    pub metadata_bytes: usize,
}
pub struct VirtualArchive {
    original: Arc<File>,
    segments: Vec<Segment>,
    pub size: u64,
    pub stats: Stats,
    pub diagnostics: Vec<Diagnostic>,
    pub source: PathBuf,
}
impl VirtualArchive {
    fn append(&mut self, bytes: Bytes, len: u64) -> Result<u64, String> {
        let start = (self.size + 15) & !15;
        let end = start.checked_add(len).ok_or("virtual ARC overflow")?;
        if start > self.size {
            self.segments.push(Segment {
                start: self.size,
                len: start - self.size,
                bytes: Bytes::Zero,
            });
        }
        if len > 0 {
            self.segments.push(Segment { start, len, bytes });
        }
        self.size = end;
        Ok(start)
    }
    fn memory(&mut self, b: Vec<u8>) -> Result<u64, String> {
        let n = b.len() as u64;
        self.append(Bytes::Memory(b.into()), n)
    }
    pub fn read(&self, off: u64, b: &mut [u8]) -> Result<usize, String> {
        let count = (self.size.saturating_sub(off)).min(b.len() as u64) as usize;
        let mut done = 0;
        while done < count {
            let at = off + done as u64;
            let n = self.segments.partition_point(|s| s.start + s.len <= at);
            if n < self.segments.len() && self.segments[n].start <= at {
                let s = &self.segments[n];
                let local = at - s.start;
                let len = (s.len - local).min((count - done) as u64) as usize;
                let out = &mut b[done..done + len];
                match &s.bytes {
                    Bytes::Memory(data) => {
                        out.copy_from_slice(&data[local as usize..local as usize + len])
                    }
                    Bytes::File(f, base) => {
                        if read_file(f, base + local, out)? != len {
                            return Err("short virtual segment read".into());
                        }
                    }
                    Bytes::Zero => out.fill(0),
                }
                done += len;
            } else {
                let end = self
                    .segments
                    .get(n)
                    .map(|s| s.start)
                    .unwrap_or(self.stats.original_size)
                    .min(off + count as u64);
                if end <= at {
                    return Err("unmapped virtual archive range".into());
                }
                let len = (end - at) as usize;
                if read_file(&self.original, at, &mut b[done..done + len])? != len {
                    return Err("short original archive read".into());
                }
                done += len;
            }
        }
        Ok(done)
    }
    pub fn build(path: &Path, c: &Config, plan: &Plan) -> Result<Self, String> {
        build(path, c, plan)
    }
}

fn immutable_file(path: &Path) -> Result<Arc<File>, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    options
        .open(path)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn read_resource(
    fs: &Fs,
    view: &VirtualArchive,
    info: usize,
    region: usize,
    file_off: u64,
) -> Result<Vec<u8>, String> {
    let (off, d) = fs.record(info, region, file_off)?;
    let n = u32at(&d, 4)? as usize;
    let len = u32at(&d, 8)? as usize;
    if len > 1024 * 1024 * 1024 {
        return Err("patch base too large".into());
    }
    let b = block(&view.original, off, n)?;
    if u32at(&d, 12)? & 1 != 0 {
        if u32at(&d, 12)? & 2 == 0 {
            return Err("unsupported archive compression".into());
        }
        zstd::bulk::decompress(&b, len).map_err(|e| e.to_string())
    } else if b.len() == len {
        Ok(b)
    } else {
        Err("uncompressed file size mismatch".into())
    }
}
#[derive(Clone)]
struct Payload {
    bytes: Bytes,
    len: u64,
    size: u64,
    flags: u32,
}
fn original_payload(
    fs: &Fs,
    view: &VirtualArchive,
    info: usize,
    region: usize,
    file_off: u64,
) -> Result<Payload, String> {
    let (off, d) = fs.record(info, region, file_off)?;
    Ok(Payload {
        bytes: Bytes::File(view.original.clone(), off),
        len: u32at(&d, 4)? as u64,
        size: u32at(&d, 8)? as u64,
        flags: u32at(&d, 12)?,
    })
}
fn prepare(
    entry: &crate::packages::Entry,
    fs: &Fs,
    view: &VirtualArchive,
    pi: Option<usize>,
    c: &Config,
    file_off: u64,
) -> Result<Payload, String> {
    if entry.patches.is_empty() {
        let src = entry.base.as_ref().ok_or("resource has no source")?;
        let f = immutable_file(&src.disk)?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        size(len)?;
        return Ok(Payload {
            bytes: Bytes::File(f, 0),
            len,
            size: len,
            flags: 0,
        });
    }
    let base = if let Some(src) = &entry.base {
        std::fs::read(&src.disk).map_err(|e| e.to_string())?
    } else {
        read_resource(
            fs,
            view,
            fs.info(pi.ok_or_else(|| {
                format!(
                    "{}: patch target is not in the archive and has no replacement",
                    entry.path
                )
            })?)?,
            crate::packages::resolved_region(c)?,
            file_off,
        )?
    };
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"ssbu-mod-cache-v1");
    digest.update(&base);
    for p in &entry.patches {
        digest.update(p.kind.as_bytes());
        digest.update(std::fs::read(&p.disk).map_err(|e| e.to_string())?);
    }
    let key = format!("{:x}", digest.finalize());
    let cache = c.cache.join(format!("{key}.blob"));
    if let Ok(bytes) = std::fs::read(&cache) {
        if bytes.len() >= 48
            && &bytes[..8] == b"SSBUMOD1"
            && u64at(&bytes, 8)? as usize == bytes.len() - 48
            && Sha256::digest(&bytes[48..])[..] == bytes[16..48]
        {
            let f = immutable_file(&cache)?;
            return Ok(Payload {
                bytes: Bytes::File(f, 48),
                len: (bytes.len() - 48) as u64,
                size: (bytes.len() - 48) as u64,
                flags: 0,
            });
        }
    }
    let kind = entry.patches[0].kind.as_str();
    if entry.patches.iter().any(|p| p.kind != kind) {
        return Err("incompatible patch handlers for the same target".into());
    }
    let bytes = crate::patches::apply(kind, base, &entry.patches)
        .map_err(|e| format!("{}: {e}", entry.path))?;
    size(bytes.len() as u64)?;
    std::fs::create_dir_all(&c.cache).map_err(|e| e.to_string())?;
    let len = bytes.len() as u64;
    let mut blob = Vec::with_capacity(bytes.len() + 48);
    blob.extend(b"SSBUMOD1");
    blob.extend(len.to_le_bytes());
    blob.extend(Sha256::digest(&bytes));
    blob.extend(bytes);
    // Existing cache blobs are never overwritten (another process may be reading one).
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&cache)
    {
        f.write_all(&blob).map_err(|e| e.to_string())?;
    }
    Ok(Payload {
        bytes: Bytes::Memory(Arc::from(&blob[48..])),
        len,
        size: len,
        flags: 0,
    })
}
fn build(path: &Path, c: &Config, plan: &Plan) -> Result<VirtualArchive, String> {
    let original = immutable_file(path)?;
    let original_size = original.metadata().map_err(|e| e.to_string())?.len();
    let mut view = VirtualArchive {
        original,
        segments: vec![],
        size: original_size,
        stats: Stats {
            original_size,
            ..Stats::default()
        },
        source: path.to_path_buf(),
        diagnostics: vec![],
    };
    let mut header = block(&view.original, 0, 56)?;
    if u64at(&header, 0)? != 0xabcdef9876543210 {
        return Err("not a Smash ARC archive".into());
    }
    if plan.entries.is_empty()
        && plan.rules.shares.is_empty()
        && plan.rules.directories.is_empty()
        && plan.rules.directory_bases.is_empty()
        && plan.rules.directory_files.is_empty()
    {
        view.stats.virtual_size = original_size;
        return Ok(view);
    }
    let file_off = u64at(&header, 16)?;
    let mut fs = Fs::parse(&section(&view.original, u64at(&header, 32)?)?)?;
    if u32at(&fs.header, 60)? != 0x000d0004 {
        return Err("resource adapter requires game 13.0.4".into());
    }
    let mut search = Search::parse(&section(&view.original, u64at(&header, 40)?)?)?;
    let mut path_map: HashMap<u64, usize> = (0..fs.paths.len())
        .map(|i| Ok((fs.paths.r64(i, 0)? & MASK, i)))
        .collect::<Result<_, String>>()?;
    let mut dir_map: HashMap<u64, usize> = (0..fs.dirs.len())
        .map(|i| Ok((fs.dirs.r64(i, 0)? & MASK, i)))
        .collect::<Result<_, String>>()?;
    let mut stream_map: HashMap<u64, usize> = (0..fs.streams.len())
        .map(|i| Ok((fs.streams.r64(i, 0)? & MASK, i)))
        .collect::<Result<_, String>>()?;
    let stream_start = u64at(&header, 8)?;
    let original_stream_data = fs.stream_data.len();
    let mut stream_shift = 0;
    if plan
        .entries
        .iter()
        .any(|e| e.path.starts_with("stream:/") || stream_map.contains_key(&e.hash))
    {
        if stream_start < 56 || stream_start > original_size {
            return Err("invalid original stream section".into());
        }
        // Music's native consumer narrows the stream base to u32 at
        // 0x710373c958. Tail-appended files in a 17 GB ARC are byte-readable
        // through our API but cannot play. Insert replacement streams before
        // the original stream section in the virtual address space only.
        // The original bytes remain backed by the same read-only file handle.
        view.size = stream_start;
        if stream_start > 56 {
            view.segments.push(Segment {
                start: 56,
                len: stream_start - 56,
                bytes: Bytes::File(view.original.clone(), 56),
            });
        }
        for e in &plan.entries {
            if !e.path.starts_with("stream:/") && !stream_map.contains_key(&e.hash) {
                continue;
            }
            if !e.patches.is_empty() {
                return Err(format!("{}: patch format cannot target a stream", e.path));
            }
            let src = e.base.as_ref().ok_or("stream source missing")?;
            let f = immutable_file(&src.disk)?;
            let len = f.metadata().map_err(|e| e.to_string())?.len();
            if view
                .size
                .checked_add(15)
                .and_then(|n| (n & !15).checked_add(len))
                .is_none_or(|end| end > u32::MAX as u64)
            {
                return Err(
                    "replacement streams exceed the native 32-bit stream address range".into(),
                );
            }
            let off = view.append(Bytes::File(f, 0), len)?;
            let mut sd = [0; 16];
            put64(&mut sd, 0, len);
            put64(&mut sd, 8, off);
            let di = fs.stream_data.add(sd)?;
            let fi = fs.stream_indices.add(di.to_le_bytes())?;
            if let Some(&i) = stream_map.get(&e.hash) {
                fs.streams.w64(i, 0, e.hash | ((fi as u64) << 40));
                fs.streams.w32(i, 8, 0);
            } else {
                let mut row = [0; 12];
                put64(&mut row, 0, e.hash | ((fi as u64) << 40));
                let i = fs.streams.add(row)?;
                fs.stream_hash
                    .add((e.hash | ((i as u64) << 40)).to_le_bytes())?;
                stream_map.insert(e.hash, i as usize);
            }
            view.stats.streams += 1;
        }
        view.append(Bytes::Zero, 0)?;
        stream_shift = view.size - stream_start;
        for i in 0..original_stream_data {
            let off = fs.stream_data.r64(i, 8)?;
            let len = fs.stream_data.r64(i, 0)?;
            let end = off
                .checked_add(len)
                .ok_or("original stream range overflow")?;
            if len == 0 && off == 0 {
                continue;
            }
            if off < stream_start || end > file_off {
                return Err("original stream is outside its stream section".into());
            }
            let relocated = off
                .checked_add(stream_shift)
                .ok_or("stream relocation overflow")?;
            if relocated
                .checked_add(len)
                .is_none_or(|end| end > u32::MAX as u64)
            {
                return Err(
                    "relocated streams exceed the native 32-bit stream address range".into(),
                );
            }
            fs.stream_data.w64(i, 8, relocated);
        }
        view.segments.push(Segment {
            start: view.size,
            len: original_size - stream_start,
            bytes: Bytes::File(view.original.clone(), stream_start),
        });
        view.size = original_size
            .checked_add(stream_shift)
            .ok_or("virtual ARC overflow")?;
        for field in [16, 24] {
            let relocated = u64at(&header, field)?
                .checked_add(stream_shift)
                .ok_or("section relocation overflow")?;
            put64(&mut header, field, relocated);
        }
    }
    let virtual_file_off = file_off + stream_shift;
    let region = crate::packages::resolved_region(c)?;
    let mut payloads = BTreeMap::<usize, Payload>::new();
    let mut new_paths = HashMap::<usize, String>::new();
    for (number, e) in plan.entries.iter().enumerate() {
        crate::progress(2, number, plan.entries.len());
        if e.path.starts_with("stream:/") || stream_map.contains_key(&e.hash) {
            continue;
        }
        let old = path_map.get(&e.hash).copied();
        let payload = prepare(e, &fs, &view, old, c, file_off)?;
        view.stats.patches += e.patches.len();
        let pi = if let Some(i) = old {
            view.stats.replacements += 1;
            i
        } else {
            let i = fs.paths.len();
            let mut row = [0; 32];
            let (parent, name) = e.path.rsplit_once('/').unwrap_or(("", e.path.as_str()));
            put64(&mut row, 0, e.hash);
            put64(
                &mut row,
                8,
                paths::hash(name.rsplit_once('.').map(|(_, x)| x).unwrap_or("")),
            );
            put64(&mut row, 16, paths::hash(&format!("{parent}/")));
            put64(&mut row, 24, paths::name_hash(name));
            fs.paths.add(row)?;
            path_map.insert(e.hash, i);
            new_paths.insert(i, e.path.clone());
            search.add(&e.path)?;
            view.stats.additions += 1;
            i
        };
        payloads.insert(pi, payload);
    }
    let mut shares = BTreeMap::<usize, usize>::new();
    for (dependent, source) in &plan.rules.preprocess {
        let dst = *dir_map
            .get(dependent)
            .ok_or("preprocess dependent directory does not exist")?;
        let src = *dir_map
            .get(source)
            .ok_or("preprocess source directory does not exist")?;
        let mut canonical = HashMap::<u64, usize>::new();
        for j in
            fs.dirs.r32(src, 32)? as usize..(fs.dirs.r32(src, 32)? + fs.dirs.r32(src, 36)?) as usize
        {
            let pi = fs.infos.r32(j, 0)? as usize;
            canonical.insert(fs.paths.r64(pi, 0)? >> 40, pi);
        }
        for j in
            fs.dirs.r32(dst, 32)? as usize..(fs.dirs.r32(dst, 32)? + fs.dirs.r32(dst, 36)?) as usize
        {
            let pi = fs.infos.r32(j, 0)? as usize;
            if let Some(&source) = canonical.get(&(fs.paths.r64(pi, 0)? >> 40)) {
                if pi != source {
                    shares.insert(pi, source);
                }
            }
        }
    }
    let mut share_relations = vec![];
    for (source, targets) in &plan.rules.shares {
        for target in targets {
            let spec = paths::file_spec(target)?;
            let dst = if let Some(&pi) = path_map.get(&spec.hash) {
                pi
            } else {
                let pi = fs.paths.len();
                let mut row = [0; 32];
                put64(&mut row, 0, spec.hash);
                put64(&mut row, 8, spec.ext);
                put64(
                    &mut row,
                    16,
                    hash40::Hash40(spec.parent.hash)
                        .concat(hash40::Hash40(paths::hash("/")))
                        .0,
                );
                put64(&mut row, 24, spec.name);
                fs.paths.add(row)?;
                path_map.insert(spec.hash, pi);
                search.add_spec(&spec)?;
                view.stats.additions += 1;
                pi
            };
            share_relations.push((dst, *source));
        }
    }
    for (dst, source) in share_relations {
        let source = *path_map
            .get(&source)
            .ok_or_else(|| format!("shared source {source:#x} does not exist"))?;
        if dst != source {
            shares.insert(dst, source);
        }
    }
    for (&dst, &src) in &shares {
        let mut at = src;
        let mut seen = HashSet::from([dst]);
        while let Some(&next) = shares.get(&at) {
            if !seen.insert(at) {
                return Err("cycle in resource sharing configuration".into());
            }
            at = next;
        }
        let p = if let Some(p) = payloads.get(&at) {
            p.clone()
        } else {
            original_payload(&fs, &view, fs.info(at)?, region, file_off)?
        };
        payloads.insert(dst, p);
    }
    // ARCropolis renumbers a bank only when it is actually unshared (or added).
    // Standalone banks such as common narration keep the identity used by the
    // game's sound events. Detect both aliases and their canonical source.
    let original_path_count = u32at(&fs.header, 4)? as usize;
    let bank_ext = paths::hash("nus3bank");
    let mut shared_banks = HashSet::new();
    for pi in 0..original_path_count {
        if fs.paths.r64(pi, 8)? & MASK != bank_ext {
            continue;
        }
        let mut at = pi;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(at) {
                return Err("cycle in original sound bank sharing".into());
            }
            let next = fs.infos.r32(fs.info(at)?, 0)? as usize;
            if next == at {
                break;
            }
            if next >= original_path_count {
                return Err("invalid shared sound bank path".into());
            }
            at = next;
        }
        if at != pi {
            shared_banks.insert(pi);
            shared_banks.insert(at);
        }
    }
    let mut banks = BTreeMap::<u64, String>::new();
    for e in &plan.entries {
        if e.path.ends_with(".nus3audio")
            && !e.path.starts_with("stream:/")
            && !plan.rules.keep_shared.contains(&e.hash)
        {
            let bank = format!("{}.nus3bank", e.path.trim_end_matches(".nus3audio"));
            let hash = paths::path_hash(&bank);
            if let Some(&pi) = path_map.get(&hash) {
                if !plan.rules.keep_shared.contains(&hash)
                    && (pi >= original_path_count || shared_banks.contains(&pi))
                {
                    banks.insert(hash, bank);
                }
            }
        }
    }
    for (n, (h, _path)) in banks.into_iter().enumerate() {
        if let Some(&pi) = path_map.get(&h) {
            let mut bytes = if let Some(p) = payloads.get(&pi) {
                let mut b = vec![0; p.len as usize];
                match &p.bytes {
                    Bytes::Memory(v) => b.copy_from_slice(v),
                    Bytes::File(f, o) => {
                        if read_file(f, *o, &mut b)? != b.len() {
                            return Err("short sound bank".into());
                        }
                    }
                    Bytes::Zero => {}
                }
                if p.flags & 1 != 0 {
                    if p.flags & 2 == 0 {
                        return Err("unsupported sound bank compression".into());
                    }
                    zstd::bulk::decompress(&b, p.size as usize).map_err(|e| e.to_string())?
                } else {
                    b
                }
            } else {
                read_resource(&fs, &view, fs.info(pi)?, region, file_off)?
            };
            if let Some(i) = bytes
                .get(0x30..)
                .and_then(|b| b.windows(4).position(|b| b == b"GRP "))
            {
                let at = 0x30 + i;
                if at >= 4 {
                    put32(
                        &mut bytes,
                        at - 4,
                        7420u32.checked_add(n as u32).ok_or("bank id overflow")?,
                    );
                    let len = bytes.len() as u64;
                    payloads.insert(
                        pi,
                        Payload {
                            bytes: Bytes::Memory(bytes.into()),
                            len,
                            size: len,
                            flags: 0,
                        },
                    );
                }
            } else {
                return Err(format!("sound bank {h:#x} has no GRP identifier"));
            }
        }
    }
    let original_dir_count = fs.dirs.len();
    // Explicit directory bases copy the game's load group shape and dependencies.
    let mut explicit_dirs = plan.rules.directories.clone();
    explicit_dirs.extend(plan.rules.directory_bases.keys().cloned());
    for path in explicit_dirs {
        let base = plan.rules.directory_bases.get(&path).map(String::as_str);
        if let Some(b) = base {
            if !dir_map.contains_key(&paths::path_hash(b)) {
                if plan.rules.optional_bases.contains(&path) {
                    continue;
                }
                return Err(format!("base directory {b} for {path} does not exist"));
            }
        }
        ensure_dir(&path, base, &mut fs, &mut search, &mut dir_map)?;
    }
    // Resolve after additions and shared aliases exist. ARCropolis's
    // replacement/addition.rs::add_files_to_directory skips missing references;
    // native packages often retain entries for textures or colors not shipped.
    // Keep the disk config intact and use one resolved list for both passes.
    let mut directory_files = BTreeMap::new();
    for (dh, files) in &plan.rules.directory_files {
        let Some(&di) = dir_map.get(dh) else {
            let sources = files
                .iter()
                .filter_map(|h| plan.rules.directory_file_sources.get(&(*dh, *h)))
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            view.diagnostics.push(Diagnostic {
                level: "warning".into(),
                code: "missing_directory".into(),
                message: format!("skipped new-dir-files directory {dh:#x}: directory does not exist; {sources}"),
            });
            continue;
        };
        let mut resolved = Vec::new();
        for h in files {
            let Some(&pi) = path_map.get(h) else {
                let sources = plan.rules.directory_file_sources
                    .get(&(*dh, *h))
                    .map(|s| s.join("; "))
                    .unwrap_or_default();
                view.diagnostics.push(Diagnostic {
                    level: "warning".into(),
                    code: "missing_directory_file".into(),
                    message: format!("skipped new-dir-files asset {h:#x} in directory {dh:#x}: resource does not exist; {sources}"),
                });
                continue;
            };
            resolved.push(pi);
            if !payloads.contains_key(&pi) {
                payloads.insert(
                    pi,
                    original_payload(&fs, &view, fs.info(pi)?, region, file_off)?,
                );
            }
        }
        directory_files.insert(di, resolved);
    }
    // Only original aliases have meaningful indice values before new rows are assigned.
    let old_path_count = u32at(&fs.header, 4)? as usize;
    let mut shared_payloads = HashMap::new();
    for h in &plan.rules.keep_shared {
        if let Some(&pi) = path_map.get(h) {
            if pi < old_path_count {
                if let Some(payload) = payloads.get(&pi) {
                    shared_payloads.insert(fs.paths.r64(pi, 0)? >> 40, payload.clone());
                }
            }
        }
    }
    if !shared_payloads.is_empty() {
        for pi in 0..old_path_count {
            if let Some(payload) = shared_payloads.get(&(fs.paths.r64(pi, 0)? >> 40)) {
                payloads.insert(pi, payload.clone());
            }
        }
    }
    let original_path_count = u32at(&fs.header, 4)? as usize;
    let mut parent_dirs = HashMap::<u64, usize>::new();
    let mut memberships = HashMap::<usize, Vec<usize>>::new();
    for di in 0..original_dir_count {
        let start = fs.dirs.r32(di, 32)? as usize;
        let count = fs.dirs.r32(di, 36)? as usize;
        for j in start..start.checked_add(count).ok_or("directory overflow")? {
            let pi = fs.infos.r32(j, 0)? as usize;
            if pi >= fs.paths.len() {
                return Err("directory has invalid file path index".into());
            }
            let ph = fs.paths.r64(pi, 16)? & MASK;
            parent_dirs.entry(ph).or_insert(di);
            if payloads.contains_key(&pi) {
                memberships.entry(pi).or_default().push(di);
            }
        }
    }
    // The native effect registry enumerates every EFF in an actor's search
    // folder (0x710355f8f0), including costume libraries. Its consumer assumes
    // each enumerated library is loaded. A library owned only by c00 therefore
    // becomes a null buffer when the next match loads c01. Keep added libraries
    // with the vanilla common library, and make costume requests retain that
    // group through normal directory dependencies. One owner avoids duplicate
    // loaded-data identities and gives unload/reload the native reference counts.
    let mut effect_owners = HashMap::<usize, usize>::new();
    let mut dependencies = BTreeMap::<usize, std::collections::BTreeSet<usize>>::new();
    for (&pi, path) in &new_paths {
        if pi < original_path_count || !path.ends_with(".eff") {
            continue;
        }
        let components: Vec<_> = path.split('/').collect();
        if components.len() != 4 || components[0] != "effect" || components[1] != "fighter" {
            continue;
        }
        let fighter = components[2];
        let common_path = format!("fighter/{fighter}/cmn/effect");
        let Some(&owner) = dir_map.get(&paths::path_hash(&common_path)) else {
            continue;
        };
        let base_path = format!("effect/fighter/{fighter}/ef_{fighter}.eff");
        let Some(&base_pi) = path_map.get(&paths::path_hash(&base_path)) else {
            continue;
        };
        if base_pi >= original_path_count {
            continue;
        }
        let base_xi = (fs.paths.r64(base_pi, 0)? >> 40) as usize;
        if fs.indices.r32(base_xi, 0)? as usize != owner {
            continue;
        }
        effect_owners.insert(pi, owner);
        for slot in 0..256 {
            let costume = format!("fighter/{fighter}/c{slot:02}");
            if let Some(&di) = dir_map.get(&paths::path_hash(&costume)) {
                dependencies.entry(di).or_default().insert(owner);
            }
        }
    }
    let mut extra = BTreeMap::<usize, Vec<usize>>::new();
    for (&di, files) in &directory_files {
        for &pi in files {
            let target = effect_owners.get(&pi).copied().unwrap_or(di);
            if target != di {
                dependencies.entry(di).or_default().insert(target);
            }
            if !memberships.get(&pi).is_some_and(|v| v.contains(&target)) {
                memberships.entry(pi).or_default().push(target);
                extra.entry(target).or_default().push(pi);
            }
        }
    }
    for (&pi, &owner) in &effect_owners {
        if !memberships.get(&pi).is_some_and(|v| v.contains(&owner)) {
            memberships.entry(pi).or_default().push(owner);
            extra.entry(owner).or_default().push(pi);
        }
    }
    for (di, targets) in dependencies {
        let start = fs.dirs.r32(di, 40)? as usize;
        let count = fs.dirs.r32(di, 44)? as usize;
        let mut children = fs
            .children
            .0
            .get(start..start + count)
            .ok_or("invalid directory dependency range")?
            .to_vec();
        for target in targets {
            if target != di
                && !children
                    .iter()
                    .any(|r| u64at(r, 0).unwrap() >> 40 == target as u64)
            {
                let hash = fs.dirs.r64(target, 0)? & MASK;
                children.push((hash | ((target as u64) << 40)).to_le_bytes());
            }
        }
        if children.len() != count {
            fs.dirs.w32(di, 40, idx(fs.children.len())?);
            fs.dirs.w32(di, 44, idx(children.len())?);
            for child in children {
                fs.children.add(child)?;
            }
        }
    }
    for &pi in payloads.keys() {
        if memberships.get(&pi).is_some_and(|v| !v.is_empty()) {
            continue;
        }
        let ph = fs.paths.r64(pi, 16)? & MASK;
        let path = new_paths.get(&pi);
        let di = if let Some(&di) = parent_dirs.get(&ph) {
            di
        } else if let Some(path) = path {
            let parent = path
                .rsplit_once('/')
                .map(|(p, _)| p)
                .ok_or("new hash-only asset needs new-dir-files configuration")?;
            ensure_dir(parent, None, &mut fs, &mut search, &mut dir_map)?
        } else {
            // A shared canonical record has no directory of its own. Give its replacement
            // a native one-file group rather than relying on an ARCropolis custom flag.
            let path = format!("__mods/{:010x}", fs.paths.r64(pi, 0)? & MASK);
            ensure_dir(&path, None, &mut fs, &mut search, &mut dir_map)?
        };
        memberships.entry(pi).or_default().push(di);
        extra.entry(di).or_default().push(pi);
    }
    let mut touched: Vec<usize> = memberships.values().flatten().copied().collect();
    touched.extend(extra.keys().copied());
    touched.sort_unstable();
    touched.dedup();
    let mut new_indices = HashMap::<usize, u32>::new();
    for &pi in payloads.keys() {
        let xi = fs.indices.add([0; 8])?;
        let h = fs.paths.r64(pi, 0)? & MASK;
        fs.paths.w64(pi, 0, h | ((xi as u64) << 40));
        new_indices.insert(pi, xi);
    }
    for di in touched {
        let start = fs.dirs.r32(di, 32)? as usize;
        let count = fs.dirs.r32(di, 36)? as usize;
        let oldfo = (fs.dirs.r64(di, 0)? >> 40) as usize;
        // The native scheduler indexes regional/localized groups relative to
        // DirInfo.path.index(), not through the individual file's InfoToData.
        // Keep the complete contiguous span when moving the base load group;
        // otherwise a character's language-specific NROs wait on unrelated data.
        let variants = match (fs.dirs.r32(di, 48)? >> 24) & 3 {
            1 => fs.header[56] as usize,
            2 => fs.header[57] as usize,
            _ => 0,
        };
        let offsets = fs
            .folders
            .0
            .get(oldfo..oldfo + 1 + variants)
            .ok_or("invalid regional directory offset range")?
            .to_vec();
        let newfo = idx(fs.folders.len())?;
        for offset in offsets {
            fs.folders.add(offset)?;
        }
        let mut rows: Vec<_> = fs
            .infos
            .0
            .get(start..start + count)
            .ok_or("invalid directory info range")?
            .to_vec();
        let mut present: HashSet<usize> =
            rows.iter().map(|r| u32at(r, 0).unwrap() as usize).collect();
        if let Some(list) = extra.get(&di) {
            for &pi in list {
                if present.insert(pi) {
                    let mut r = [0; 16];
                    put32(&mut r, 0, pi as u32);
                    put32(
                        &mut r,
                        12,
                        if [paths::hash("nutexb"), paths::hash("eff")]
                            .contains(&(fs.paths.r64(pi, 8)? & MASK))
                        {
                            0x1000
                        } else {
                            0x10
                        },
                    );
                    rows.push(r);
                }
            }
        }
        // Localized files must stay in the scheduler's localized group. Moving
        // only their bytes into the base group changes the native request class
        // while the directory still schedules its regional companion.
        let mut localized = HashSet::new();
        let mut variant_payloads = BTreeMap::<usize, BTreeMap<usize, Payload>>::new();
        for (local, row) in rows.iter().enumerate() {
            let pi = u32at(row, 0)? as usize;
            let flags = u32at(row, 12)?;
            if flags & 0x18000 == 0 || !payloads.contains_key(&pi) {
                continue;
            }
            let selector = if flags & 0x8000 != 0 {
                region
            } else {
                paths::locale(region)
            };
            if selector == 0 || selector > variants {
                return Err("localized replacement has no matching directory variant".into());
            }
            // A directory entry can borrow data from a different load group.
            // Its replacement belongs to this directory's selected companion,
            // not to the shared source's physical FileData range.
            variant_payloads
                .entry(selector)
                .or_default()
                .insert(local, payloads[&pi].clone());
            localized.insert(local);
        }
        let original_rows = rows.clone();
        let info_start = fs.infos.len();
        let data_start = fs.data.len();
        let group_start = (view.size + 15) & !15;
        let mut decomp = 0u64;
        append_group(&mut view, Bytes::Zero, 0, group_start)?;
        for (local, mut row) in rows.into_iter().enumerate() {
            let pi = u32at(&row, 0)? as usize;
            let ii = fs.infos.len();
            let payload = payloads.get(&pi);
            let raw = if localized.contains(&local) {
                None
            } else if let Some(p) = payload {
                Some((p.bytes.clone(), p.len, p.size, p.flags))
            } else {
                let olditd = u32at(&row, 8)? as usize;
                let flags = u32at(&row, 12)?;
                let itd = olditd
                    + if flags & 0x8000 != 0 {
                        region
                    } else if flags & 0x10000 != 0 {
                        paths::locale(region)
                    } else {
                        0
                    };
                if fs.itds.r32(itd, 0)? as usize != oldfo {
                    None
                } else {
                    let did = fs.itds.r32(itd, 4)? as usize;
                    let d = fs.data.0[did];
                    let off = file_off + fs.folders.r64(oldfo, 0)? + u32at(&d, 0)? as u64 * 4;
                    Some((
                        Bytes::File(view.original.clone(), off),
                        u32at(&d, 4)? as u64,
                        u32at(&d, 8)? as u64,
                        u32at(&d, 12)?,
                    ))
                }
            };
            if raw.is_none() {
                // FolderOffset.file_start_index is a parallel FileData range, even
                // for shared entries that obtain their payload from another group.
                let start = fs.folders.r32(oldfo, 16)? as usize;
                let placeholder = if local < count {
                    fs.data.0.get(start + local).copied().unwrap_or([0; 16])
                } else {
                    [0; 16]
                };
                fs.data.add(placeholder)?;
            }
            if let Some((bytes, comp, dec, flags)) = raw {
                let off = append_group(&mut view, bytes, comp, group_start)?;
                let mut data = [0; 16];
                put32(&mut data, 0, size((off - group_start) / 4)?);
                put32(&mut data, 4, size(comp)?);
                put32(&mut data, 8, size(dec)?);
                put32(&mut data, 12, flags);
                let did = fs.data.add(data)?;
                let mut itd = [0; 12];
                put32(&mut itd, 0, newfo);
                put32(&mut itd, 4, did);
                put32(&mut itd, 8, idx(ii)? | 0x01000000);
                let iti = fs.itds.add(itd)?;
                put32(&mut row, 8, iti);
                let f = u32at(&row, 12)?;
                put32(&mut row, 12, f & !0x18000);
                decomp = decomp.checked_add(dec).ok_or("directory size overflow")?;
            }
            if let Some(&xi) = new_indices.get(&pi) {
                put32(&mut row, 4, xi);
                let f = u32at(&row, 12)?;
                // The low 15 bits also encode allocation alignment (native 0x7103544b30).
                // Combining 0x1000 and 0x10 is not a power of two and yields a null buffer.
                put32(
                    &mut row,
                    12,
                    f & (0x7fff
                        | if localized.contains(&local) {
                            0x18000
                        } else {
                            0
                        }),
                );
                if fs.indices.r32(xi as usize, 4)? == 0 {
                    fs.indices.w32(xi as usize, 0, di as u32);
                    fs.indices.w32(xi as usize, 4, idx(ii)?);
                }
            } else if pi < original_path_count {
                let xi = (fs.paths.r64(pi, 0)? >> 40) as usize;
                let canonical = (fs.indices.r32(xi, 4)? & NONE) as usize;
                if canonical >= start
                    && canonical < start + count
                    && fs.indices.r32(xi, 0)? as usize == di
                {
                    fs.indices.w32(xi, 4, idx(ii)?);
                }
            }
            fs.infos.add(row)?;
        }
        append_group(&mut view, Bytes::Zero, 0, group_start)?;
        fs.folders.w64(
            newfo as usize,
            0,
            group_start
                .checked_sub(virtual_file_off)
                .ok_or("virtual group below file section")?,
        );
        fs.folders.w32(newfo as usize, 8, size(decomp)?);
        fs.folders
            .w32(newfo as usize, 12, size(view.size - group_start)?);
        fs.folders.w32(newfo as usize, 16, idx(data_start)?);
        fs.folders.w32(
            newfo as usize,
            20,
            size((fs.infos.len() - info_start) as u64)?,
        );
        if !variant_payloads.is_empty() {
            let mut moved_data = HashMap::<(usize, usize), u32>::new();
            let mut replaced_variant_data = HashMap::<(usize, usize), u32>::new();
            for local in 0..count {
                let old = fs.folders.r32(oldfo, 16)? as usize + local;
                moved_data.insert((oldfo, old), idx(data_start + local)?);
            }
            for (variant, replacements) in variant_payloads {
                let old_group = oldfo + variant;
                let new_group = newfo as usize + variant;
                let new_start = fs.data.len();
                let group_start = (view.size + 15) & !15;
                append_group(&mut view, Bytes::Zero, 0, group_start)?;
                let mut old_sum = 0u64;
                let mut new_sum = 0u64;
                let mut old_owned = HashSet::new();
                // file_count includes borrowed entries which have no physical
                // FileData in this group. Walk FileInfos, never blindly copy
                // file_count consecutive data rows (that can enter the next
                // language's range). Borrowed entries keep zero-byte slots.
                for (local, original_info) in original_rows.iter().enumerate() {
                    let flags = u32at(original_info, 12)?;
                    let selected = if flags & 0x8000 != 0 {
                        region
                    } else if flags & 0x10000 != 0 {
                        paths::locale(region)
                    } else {
                        continue;
                    };
                    if selected != variant {
                        continue;
                    }
                    let iti = u32at(original_info, 8)? as usize + selected;
                    let owner = fs.itds.r32(iti, 0)? as usize;
                    let did = fs.itds.r32(iti, 4)? as usize;
                    let original = fs.data.0[did];
                    let owned = owner == old_group;
                    if owned && old_owned.insert(did) {
                        old_sum += u32at(&original, 8)? as u64;
                    }
                    let payload = if let Some(payload) = replacements.get(&local) {
                        payload.clone()
                    } else if owned {
                        Payload {
                            bytes: Bytes::File(
                                view.original.clone(),
                                file_off
                                    + fs.folders.r64(old_group, 0)?
                                    + u32at(&original, 0)? as u64 * 4,
                            ),
                            len: u32at(&original, 4)? as u64,
                            size: u32at(&original, 8)? as u64,
                            flags: u32at(&original, 12)?,
                        }
                    } else {
                        Payload {
                            bytes: Bytes::Zero,
                            len: 0,
                            size: 0,
                            flags: 0,
                        }
                    };
                    let off = append_group(&mut view, payload.bytes, payload.len, group_start)?;
                    let mut row = [0; 16];
                    put32(&mut row, 0, size((off - group_start) / 4)?);
                    put32(&mut row, 4, size(payload.len)?);
                    put32(&mut row, 8, size(payload.size)?);
                    put32(&mut row, 12, payload.flags);
                    let new_did = fs.data.add(row)?;
                    if owned {
                        moved_data.insert((old_group, did), new_did);
                    }
                    if replacements.contains_key(&local) {
                        replaced_variant_data.insert((local, variant), new_did);
                    }
                    new_sum += payload.size;
                }
                append_group(&mut view, Bytes::Zero, 0, group_start)?;
                let overhead = (fs.folders.r32(old_group, 8)? as u64)
                    .checked_sub(old_sum)
                    .ok_or("localized group decompressed size is too small")?;
                fs.folders.w64(new_group, 0, group_start - virtual_file_off);
                fs.folders.w32(new_group, 8, size(new_sum + overhead)?);
                fs.folders
                    .w32(new_group, 12, size(view.size - group_start)?);
                fs.folders.w32(new_group, 16, idx(new_start)?);
                fs.folders
                    .w32(new_group, 20, idx(fs.data.len() - new_start)?);
                view.stats.groups += 1;
            }
            // Each FileInfo needs its whole contiguous InfoToData span. Preserve
            // all other languages, flags and external sharing, and retarget the
            // copied local groups together with their physical FileData slots.
            for (local, original) in original_rows.iter().enumerate() {
                let flags = u32at(original, 12)?;
                if flags & 0x18000 == 0 {
                    continue;
                }
                let num = if flags & 0x8000 != 0 {
                    fs.header[56]
                } else {
                    fs.header[57]
                } as usize
                    + 1;
                let old_start = u32at(original, 8)? as usize;
                let records = fs
                    .itds
                    .0
                    .get(old_start..old_start + num)
                    .ok_or("invalid localized file mapping span")?
                    .to_vec();
                let new_start = idx(fs.itds.len())?;
                for (variant, mut row) in records.into_iter().enumerate() {
                    let owner = u32at(&row, 0)? as usize;
                    let did = u32at(&row, 4)? as usize;
                    if let Some(&new_did) = replaced_variant_data.get(&(local, variant)) {
                        put32(&mut row, 0, newfo + idx(variant)?);
                        put32(&mut row, 4, new_did);
                        // This slot is now owned, not a link into the shared
                        // source's FileInfo chain. Other languages keep theirs.
                        put32(&mut row, 8, 0x01000000 | idx(info_start + local)?);
                    } else if (oldfo..=oldfo + variants).contains(&owner) {
                        put32(&mut row, 0, newfo + idx(owner - oldfo)?);
                        if let Some(&new_did) = moved_data.get(&(owner, did)) {
                            put32(&mut row, 4, new_did);
                        }
                        let index = u32at(&row, 8)?;
                        put32(&mut row, 8, (index & !NONE) | idx(info_start + local)?);
                    }
                    fs.itds.add(row)?;
                }
                fs.infos.w32(info_start + local, 8, new_start);
            }
        }
        let dh = fs.dirs.r64(di, 0)? & MASK;
        fs.dirs.w64(di, 0, dh | ((newfo as u64) << 40));
        fs.dirs.w32(di, 32, idx(info_start)?);
        fs.dirs
            .w32(di, 36, size((fs.infos.len() - info_start) as u64)?);
        view.stats.groups += 1;
    }
    for (&pi, &xi) in &new_indices {
        if fs.indices.r32(xi as usize, 4)? == 0 {
            return Err(format!("no load group owns replacement path {pi}"));
        }
    }
    // One pass through active groups preserves sharing without a files-times-shares scan.
    let mut shared_indices = HashMap::new();
    for &dst in shares.keys() {
        let mut src = shares[&dst];
        while let Some(&n) = shares.get(&src) {
            src = n;
        }
        let xi = fs.paths.r64(src, 0)? >> 40;
        let h = fs.paths.r64(dst, 0)? & MASK;
        fs.paths.w64(dst, 0, h | (xi << 40));
        shared_indices.insert(dst, xi as u32);
    }
    if !shared_indices.is_empty() {
        for di in 0..fs.dirs.len() {
            let start = fs.dirs.r32(di, 32)? as usize;
            let end = start + fs.dirs.r32(di, 36)? as usize;
            for row in start..end {
                if let Some(&xi) = shared_indices.get(&(fs.infos.r32(row, 0)? as usize)) {
                    fs.infos.w32(row, 4, xi);
                }
            }
        }
    }
    // A native shared FileGroup enumerates its original canonical FilePaths.
    // Unsharing such a path changes the path's data identity; leaving it in that
    // group makes even untouched costumes wait for another costume's replacement.
    // Give each affected source a private alias to its ORIGINAL FileInfoIndex.
    // No payload is copied: the original compressed bytes and native group links
    // remain intact, so shared colors still load and unload their original data.
    let shared_start = u32at(&fs.header, 16)? as usize;
    let shared_end = shared_start + u32at(&fs.header, 36)? as usize;
    let mut source_aliases = BTreeMap::<u32, u32>::new();
    for group in shared_start..shared_end {
        let start = fs.folders.r32(group, 16)? as usize;
        let count = fs.folders.r32(group, 20)? as usize;
        for ii in start..start.checked_add(count).ok_or("shared group overflow")? {
            let pi = fs.infos.r32(ii, 0)? as usize;
            let xi = fs.infos.r32(ii, 4)?;
            if fs.paths.r64(pi, 0)? >> 40 == xi as u64 {
                continue;
            }
            let alias = if let Some(&alias) = source_aliases.get(&xi) {
                alias
            } else {
                let path = format!("__mods/shared_sources/{xi:06x}.bin");
                let hash = paths::path_hash(&path);
                if path_map.contains_key(&hash) {
                    return Err(format!("reserved shared-source path collision: {path}"));
                }
                let mut row = [0; 32];
                put64(&mut row, 0, hash | ((xi as u64) << 40));
                put64(&mut row, 8, paths::hash("bin"));
                put64(&mut row, 16, paths::hash("__mods/shared_sources/"));
                put64(&mut row, 24, paths::name_hash(path.rsplit('/').next().unwrap()));
                let alias = fs.paths.add(row)?;
                search.add(&path)?;
                path_map.insert(hash, alias as usize);
                source_aliases.insert(xi, alias);
                view.stats.shared_sources += 1;
                alias
            };
            fs.infos.w32(ii, 0, alias);
        }
    }
    // Hash lookup for streams is sorted separately from the stream data arrays.
    fs.stream_hash
        .0
        .sort_by_key(|r| u64at(r, 0).unwrap() & MASK);
    crate::progress(3, plan.entries.len(), plan.entries.len());
    fs.repartition()?;
    let fs_bytes = fs.encode()?;
    let search_bytes = search.encode()?;
    view.stats.metadata_bytes = fs_bytes.len() + search_bytes.len();
    let fs_off = view.memory(compressed_section(&fs_bytes)?)?;
    let search_off = view.memory(compressed_section(&search_bytes)?)?;
    put64(&mut header, 32, fs_off);
    put64(&mut header, 40, search_off);
    view.segments.insert(
        0,
        Segment {
            start: 0,
            len: 56,
            bytes: Bytes::Memory(header.into()),
        },
    );
    view.stats.virtual_size = view.size;
    Ok(view)
}
fn append_group(
    view: &mut VirtualArchive,
    bytes: Bytes,
    len: u64,
    base: u64,
) -> Result<u64, String> {
    let start = if view.size <= base {
        base
    } else {
        base + ((view.size - base + 15) & !15)
    };
    if start > view.size {
        view.segments.push(Segment {
            start: view.size,
            len: start - view.size,
            bytes: Bytes::Zero,
        });
    }
    if len > 0 {
        view.segments.push(Segment { start, len, bytes });
    }
    view.size = start.checked_add(len).ok_or("group size overflow")?;
    Ok(start)
}
fn ensure_dir(
    path: &str,
    base: Option<&str>,
    fs: &mut Fs,
    search: &mut Search,
    map: &mut HashMap<u64, usize>,
) -> Result<usize, String> {
    let h = paths::path_hash(path);
    if let Some(&i) = map.get(&h) {
        return Ok(i);
    }
    let di = fs.dirs.len();
    let mut row = if let Some(base) = base {
        *fs.dirs
            .0
            .get(
                *map.get(&paths::path_hash(base))
                    .ok_or_else(|| format!("base directory {base} does not exist"))?,
            )
            .unwrap()
    } else {
        [0; 52]
    };
    let fo = if base.is_some() {
        (u64at(&row, 0)? >> 40) as u32
    } else {
        let mut f = [0; 28];
        put32(&mut f, 24, NONE);
        fs.folders.add(f)?
    };
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    put64(&mut row, 0, h | ((fo as u64) << 40));
    put64(&mut row, 8, paths::hash(name));
    put64(&mut row, 16, paths::hash(parent));
    fs.dirs.add(row)?;
    map.insert(h, di);
    search.add_folder(path)?;
    if let Some(&pd) = map.get(&paths::hash(parent)) {
        let start = fs.dirs.r32(pd, 40)? as usize;
        let count = fs.dirs.r32(pd, 44)? as usize;
        let mut children = fs
            .children
            .0
            .get(start..start + count)
            .ok_or("invalid child directories")?
            .to_vec();
        children.push((h | ((di as u64) << 40)).to_le_bytes());
        let newstart = fs.children.len();
        for r in children {
            fs.children.add(r)?;
        }
        fs.dirs.w32(pd, 40, idx(newstart)?);
        fs.dirs.w32(pd, 44, (count + 1) as u32);
    }
    Ok(di)
}
