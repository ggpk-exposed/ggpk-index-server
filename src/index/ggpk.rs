use crate::index::state::Fields;
use std::collections::{BTreeMap, HashSet};
use std::io::SeekFrom::Current;
use std::io::{BufRead, BufReader, Cursor, Read, Seek};
use tantivy::{IndexWriter, TantivyDocument};
use url::Url;

pub fn index(version: &str, writer: &IndexWriter, fields: &Fields) -> anyhow::Result<()> {
    let base = Url::parse(version)?;
    let url = base.join("Bundles2/_.index.bin")?;
    let response = reqwest::blocking::get(url)?;
    let index_bundle = decompress(&mut BufReader::new(response))?;
    let cur = &mut Cursor::new(&index_bundle);
    let count = read_u32(cur)? as usize;
    let mut bundle_names = Vec::with_capacity(count);
    let mut bundle_sizes = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = read_u32(cur)? as usize;
        let start = cur.position() as usize;
        let end = start + name_len;
        let name = std::str::from_utf8(&index_bundle[start..end])?;
        cur.seek(Current(name_len as i64))?;
        let bundle_size = read_u32(cur)?;
        bundle_names.push(name);
        bundle_sizes.push(bundle_size);
    }

    let mut files = BTreeMap::new();
    for _ in 0..read_u32(cur)? {
        files.insert(
            // hash
            read_u64(cur)? as u64,
            // bundle index, file offset, file size
            (read_u32(cur)?, read_u32(cur)?, read_u32(cur)?),
        );
    }
    let path_rep_count = read_u32(cur)? as i64;
    cur.seek(Current(path_rep_count * 20))?;

    let path_bundle = decompress(cur)?;
    let mut dirs = HashSet::new();
    decode_paths(path_bundle.as_slice(), &mut |filename| {
        add_file(
            filename.as_str(),
            version,
            writer,
            fields,
            &bundle_names,
            &bundle_sizes,
            &files,
            &mut dirs,
        )
    })?;

    for dir in dirs {
        let mut doc = TantivyDocument::new();
        fields.add_folder(version, dir.as_str(), &mut doc);
        writer.add_document(doc)?;
    }

    Ok(())
}

fn add_file(
    mut filename: &str,
    version: &str,
    writer: &IndexWriter,
    fields: &Fields,
    bundle_names: &Vec<&str>,
    bundle_sizes: &Vec<u32>,
    files: &BTreeMap<u64, (u32, u32, u32)>,
    dirs: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f);
    if let Some(&(bundle_index, offset, size)) = files.get(&hash) {
        let bundle = bundle_names[bundle_index as usize];
        let bundle_size = bundle_sizes[bundle_index as usize];
        let mut doc = TantivyDocument::new();
        fields.add_file(
            version,
            filename,
            offset,
            size,
            bundle,
            bundle_size,
            &mut doc,
        );
        writer.add_document(doc)?;

        // add parent dir and its parents
        while let Some((d, _)) = filename.rsplit_once('/') {
            if dirs.insert(d.to_string()) {
                filename = d;
            } else {
                break;
            }
        }
    } else {
        eprintln!("No file found for hash {} of {}", hash, filename)
    }
    Ok(())
}

fn decompress<T: Read>(f: &mut T) -> anyhow::Result<Vec<u8>> {
    let mut buf = vec![0; 20];
    // uncompressed size u32, payload size u32, header size u32, first file u32, unknown u32
    f.read_exact(buf.as_mut_slice())?;
    let uncompressed_size = read_u64(f)?;
    // payload size
    read_u64(f)?;
    let block_count = read_u32(f)? as usize;
    // granularity u32,
    let granularity = read_u32(f)? as usize;
    println!(
        "uncompressed size: {}, block count: {}, granularity: {}",
        uncompressed_size, block_count, granularity
    );
    buf.reserve(uncompressed_size - 20);
    // unknown [u32; 4]
    buf.resize(16, 0);
    f.read_exact(buf.as_mut_slice())?;
    // block sizes [u32; block_count]
    buf.resize(4 * block_count, 0);
    f.read_exact(buf.as_mut_slice())?;
    buf.resize(uncompressed_size, 0);
    let mut ooz = oozextract::Extractor::new();
    for i in 0..block_count {
        ooz.read(
            f,
            &mut buf[i * granularity..uncompressed_size.min((i + 1) * granularity)],
        )?;
    }
    println!("Decompressed {} bytes", buf.len());
    Ok(buf)
}

fn decode_paths<CB: FnMut(String) -> anyhow::Result<()>>(
    data: &[u8],
    callback: &mut CB,
) -> anyhow::Result<()> {
    let mut bases: Vec<String> = Vec::new();
    let mut base_phase = false;
    let r = &mut Cursor::new(data);
    let fragment = &mut Vec::new();
    while r.position() < data.len() as u64 {
        let cmd = read_u32(r)? as usize;
        if cmd == 0 {
            base_phase = !base_phase;
            if base_phase {
                bases.clear();
            }
        } else {
            fragment.clear();
            r.read_until(b'\0', fragment)?;
            let path = std::str::from_utf8(fragment)?.trim_end_matches('\0');
            let mut full;
            if cmd <= bases.len() {
                full = bases[cmd - 1].clone();
                full.push_str(path);
            } else {
                full = path.to_string();
            }
            if base_phase {
                bases.push(full);
            } else {
                callback(full)?;
            }
        }
    }
    Ok(())
}

fn read_u32<T: Read>(cur: &mut T) -> anyhow::Result<u32> {
    let mut bytes = [0; 4];
    cur.read_exact(&mut bytes[..])?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<T: Read>(cur: &mut T) -> anyhow::Result<usize> {
    let mut bytes = [0; 8];
    cur.read_exact(&mut bytes[..])?;
    Ok(u64::from_le_bytes(bytes) as usize)
}
