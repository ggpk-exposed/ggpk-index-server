use crate::index::state::{EntryType, Fields};
use anyhow::Context;
use axum::body::Bytes;
use csv::ReaderBuilder;
use std::collections::{BTreeMap, HashSet};
use std::io::SeekFrom::Current;
use std::io::{BufRead, Cursor, Read, Seek};
use tantivy::schema::Value;
use tantivy::{IndexWriter, TantivyDocument};
use url::Url;

pub async fn index(version: &str, writer: &IndexWriter, fields: &Fields) -> anyhow::Result<()> {
    let base = Url::parse(version)?;
    let url = base.join("Bundles2/_.index.bin")?;
    let response = reqwest::get(url).await?;
    let index_bundle = decompress(&mut Cursor::new(response.bytes().await?))?;
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
    let mut sprites = Vec::new();
    decode_paths(path_bundle.as_slice(), &mut |filename| {
        let mut doc = to_doc(
            filename.as_str(),
            version,
            fields,
            &bundle_names,
            &bundle_sizes,
            &files,
        )?;

        if let Some((_, ext)) = filename.rsplit_once('.') {
            doc.add_text(fields.extension, ext);
            if ext == "txt" && filename.starts_with("art") {
                sprites.push(doc.clone());
            }
        }
        writer.add_document(doc)?;

        add_dirs(filename.as_str(), &mut dirs);

        Ok(())
    })?;

    for sprite in sprites {
        add_sprite(sprite, writer, fields, &mut dirs).await?;
    }

    for filename in dirs {
        let mut doc = TantivyDocument::new();
        let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename.as_str()));
        doc.add_text(fields.version, version);
        doc.add_text(fields.path, filename.clone());
        doc.add_text(fields.name, name);
        doc.add_text(fields.parent, dir);
        doc.add_text(fields.typ, EntryType::DIR);
        writer.add_document(doc)?;
    }

    Ok(())
}

fn to_doc(
    filename: &str,
    version: &str,
    fields: &Fields,
    bundle_names: &Vec<&str>,
    bundle_sizes: &Vec<u32>,
    files: &BTreeMap<u64, (u32, u32, u32)>,
) -> anyhow::Result<TantivyDocument> {
    let mut doc = TantivyDocument::new();

    let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename));
    doc.add_text(fields.version, version);
    doc.add_text(fields.path, filename);
    doc.add_text(fields.name, name);
    doc.add_text(fields.parent, dir);
    doc.add_text(fields.typ, EntryType::FILE);

    let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f);
    if let Some(&(bundle_index, offset, size)) = files.get(&hash) {
        let bundle = bundle_names[bundle_index as usize];
        let bundle_size = bundle_sizes[bundle_index as usize];
        doc.add_u64(fields.offset, offset as u64);
        doc.add_u64(fields.size, size as u64);
        doc.add_text(fields.bundle, bundle);
        doc.add_u64(fields.bundle_size, bundle_size as u64);
    } else {
        eprintln!("No file found for hash {} of {}", hash, filename);
    }

    Ok(doc)
}

fn add_dirs(mut filename: &str, dirs: &mut HashSet<String>) {
    while let Some((d, _)) = filename.rsplit_once('/') {
        if dirs.insert(d.to_string()) {
            filename = d;
        } else {
            break;
        }
    }
}

async fn add_sprite(
    base: TantivyDocument,
    writer: &IndexWriter,
    fields: &Fields,
    dirs: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let mut reader = get_data(&base, fields).await?;

    let sprite_txt = base.get_first(fields.path).and_then(|f| f.as_str());

    for record in reader.deserialize::<(String, String, u64, u64, u64, u64)>() {
        let (mut filename, mut source, x, y, x2, y2) = match record {
            Err(e) => {
                eprintln!(
                    "Error parsing record from {:?}: {}",
                    base.get_first(fields.path),
                    e
                );
                continue;
            }
            Ok(r) => r,
        };

        filename = filename.to_lowercase();
        source = source.to_lowercase();

        let mut doc = TantivyDocument::new();
        for v in base.get_all(fields.version) {
            doc.add_field_value(fields.version, v.clone());
        }

        doc.add_text(fields.typ, EntryType::SPRITE);
        doc.add_text(fields.path, filename.clone());
        let filename = filename.as_str();
        let (dir, name) = filename
            .rsplit_once('/')
            .unwrap_or(("art/sprites", filename));
        doc.add_text(fields.name, name);
        doc.add_text(fields.parent, dir);

        doc.add_text(fields.sprite_sheet, source);
        sprite_txt.map(|txt| doc.add_text(fields.sprite_txt, txt));
        // min and abs_diff not really necessary as x1 and y1 should always be top left, but what's the harm
        doc.add_u64(fields.sprite_x, x.min(x2));
        doc.add_u64(fields.sprite_y, y.min(y2));
        doc.add_u64(fields.sprite_w, x.abs_diff(x2) + 1);
        doc.add_u64(fields.sprite_h, y.abs_diff(y2) + 1);

        writer.add_document(doc)?;

        add_dirs(filename, dirs);
    }

    Ok(())
}

async fn get_data(
    doc: &TantivyDocument,
    fields: &Fields,
) -> anyhow::Result<csv::Reader<Cursor<Bytes>>> {
    let size = doc
        .get_first(fields.size)
        .and_then(|v| v.as_u64())
        .context("sprite size")?;
    let bundle_name = doc
        .get_first(fields.bundle)
        .and_then(|v| v.as_str())
        .context("sprite bundle")?;
    let bundle_size = doc
        .get_first(fields.bundle_size)
        .and_then(|v| v.as_u64())
        .context("sprite bundle size")?;
    let bundle_offset = doc
        .get_first(fields.offset)
        .and_then(|v| v.as_u64())
        .context("sprite offset")?;
    let storage = doc
        .get_first(fields.version)
        .and_then(|v| v.as_str())
        .context("sprite storage")?;
    let version = storage
        .split('/')
        .filter(|&v| !v.is_empty())
        .last()
        .context("sprite version")?;

    let frontend = std::env::var("FRONTEND_URL")?;
    let mut url = Url::parse(frontend.as_str()).context(frontend)?;
    url.path_segments_mut()
        .map_err(|_| anyhow::Error::msg("path segments failed"))?
        .push(version)
        .push("sprite.txt");

    {
        let mut query = url.query_pairs_mut();

        // some parameters are not used to fetch data, but must be non-empty to avoid validation errors
        // if validation fails the frontend will fall back to this server's index, which will not be ready
        // until the current process is complete
        query.append_pair("path", "sprite");
        query.append_pair("dirname", "/");
        query.append_pair("basename", "sprite");
        query.append_pair("extension", "txt");
        query.append_pair("type", "file");
        query.append_pair("mime_type", "text/plain");

        query.append_pair("storage", storage);
        query.append_pair("file_size", size.to_string().as_str());
        query.append_pair("bundle_offset", bundle_offset.to_string().as_str());
        query.append_pair("bundle[size]", bundle_size.to_string().as_str());
        query.append_pair("bundle[name]", bundle_name);
    }

    let reader = Cursor::new(reqwest::get(url).await?.bytes().await?);
    Ok(ReaderBuilder::new()
        .has_headers(false)
        .delimiter(b' ')
        .from_reader(reader))
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
