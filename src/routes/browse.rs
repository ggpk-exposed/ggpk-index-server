use crate::index::collector::CollectAll;
use crate::index::state::{EntryType, Fields, IndexState};
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, TermQuery};
use tantivy::schema::IndexRecordOption::Basic;
use tantivy::schema::Value;
use tantivy::{Searcher, TantivyDocument, Term};

pub const TEXT_EXT: &'static [&'static str] = &[
    "otc", "itc", "toy", "fxgraph", "pet", "rs", "tmd", "mat", "dlp", "et", "tgr", "ffx", "aoc",
    "fgp", "txt", "clt", "epk", "gt", "tst", "ui", "hlsl", "arm", "hideout", "csd", "atl", "gft",
    "amd", "tsi", "ecf", "xml", "h", "atlas", "it", "trl", "sm", "ao", "env", "mtd", "cht", "ot",
    "act", "tgt", "dgr", "dct", "ddt", "tmo",
];

#[derive(Deserialize, Default, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Command {
    #[default]
    Ready,
    Details,
    Index,
    Subfolders,
    Search,
}

#[derive(Deserialize)]
pub struct Params {
    #[serde(default)]
    #[serde(rename = "q")]
    command: Command,
    adapter: Option<String>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    filter: String,
    #[serde(default)]
    extension: String,
    limit: Option<usize>,
    #[serde(default)]
    debug_query: bool,
}

#[derive(Serialize)]
pub struct IndexResponse {
    pub storages: Vec<String>,
    pub adapter: String,
    pub files: Vec<Node>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_query: Option<String>,
}

#[derive(Serialize)]
pub struct Node {
    pub path: String,
    pub dirname: String,
    pub basename: String,
    pub storage: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<Bundle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<Sprite>,
}

#[derive(Serialize)]
pub struct Bundle {
    pub name: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct Sprite {
    pub sheet: String,
    pub source: String,
    pub x: u64,
    pub y: u64,
    pub w: u64,
    pub h: u64,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Dir,
    File,
}

#[derive(Serialize)]
pub struct ErrorResponse<'a> {
    pub storages: &'a [String],
    pub error: String,
}

pub async fn handler(
    Query(Params {
        adapter,
        command,
        filter,
        extension,
        mut path,
        mut limit,
        debug_query,
    }): Query<Params>,
    State(AppState { urls, index }): State<AppState>,
) -> Result<Json<IndexResponse>, Response> {
    let storages = { urls.read().await.clone() };
    if storages.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }

    let adapter = adapter
        .and_then(|v| storages.iter().find(|&s| s.as_str() == v.as_str()))
        .unwrap_or_else(|| &storages[0])
        .clone();

    if limit == Some(0) {
        limit = None
    }

    if command == Command::Ready {
        return Ok(Json(IndexResponse {
            adapter,
            storages,
            files: Vec::new(),
            debug_query: None,
        }));
    }

    if path.starts_with('/') {
        path = path.trim_start_matches('/').to_string();
    }

    let IndexState {
        reader,
        fields,
        query_parser,
        ..
    } = index;

    let mut query: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::with_capacity(4);

    query.push((
        Occur::Must,
        Box::new(TermQuery::new(fields.version_term(adapter.as_str()), Basic)),
    ));
    if !extension.is_empty() {
        query.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(fields.extension, extension.as_str()),
                Basic,
            )),
        ));
    }
    if command == Command::Details {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        query.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(fields.parent, parent),
                Basic,
            )),
        ));
        query.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(fields.name, name),
                Basic,
            )),
        ))
    } else if command == Command::Search {
        if limit.is_none() {
            limit = Some(50);
        }
        if !path.is_empty() {
            query.push((
                Occur::Must,
                Box::new(FuzzyTermQuery::new_prefix(
                    Term::from_field_text(fields.parent, path.as_str()),
                    0,
                    false,
                )),
            ))
        }
        if !filter.is_empty() {
            query.push((
                Occur::Must,
                query_parser
                    .parse_query(filter.as_str())
                    .map_err(|e| error(format!("error performing query: {}", e), &storages))?,
            ))
        }
    } else {
        query.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(fields.parent, path.as_str()),
                Basic,
            )),
        ))
    }
    if command == Command::Subfolders {
        query.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(fields.typ, EntryType::DIR),
                Basic,
            )),
        ))
    }
    let query: Box<dyn tantivy::query::Query> = Box::new(BooleanQuery::new(query));

    let debug_query = debug_query.then(|| format!("{:?}", query));

    let mut files = perform_query(&reader.searcher(), &storages, query, limit, |doc| {
        process_doc(adapter.clone(), fields, doc)
    })?;

    if limit.is_none() {
        files.sort_by(|l, r| {
            l.node_type
                .cmp(&r.node_type)
                .then(l.path.cmp(&r.path))
                .then(l.basename.cmp(&r.basename))
        });
    }

    Ok(Json(IndexResponse {
        adapter,
        storages,
        files,
        debug_query,
    }))
}

fn process_doc(
    storage: String,
    fields: &Fields,
    doc: Result<TantivyDocument, Response>,
) -> Result<Node, Response> {
    let doc = doc?;
    let basename = doc
        .get_first(fields.name)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let dirname = doc
        .get_first(fields.parent)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let node_type = if doc.get_first(fields.typ).and_then(|v| v.as_str()) == Some(EntryType::DIR) {
        NodeType::Dir
    } else {
        NodeType::File
    };
    let file_size = doc.get_first(fields.size).and_then(|v| v.as_u64());
    let bundle_offset = doc.get_first(fields.offset).and_then(|v| v.as_u64());
    let path: PathBuf = [&dirname, &basename].iter().collect();
    let extension = if node_type == NodeType::Dir {
        None
    } else {
        path.extension().map(|v| v.to_string_lossy().to_string())
    };
    let path = path.to_string_lossy().to_string();
    let mime_type = extension.as_ref().and_then(|ext| {
        mime_guess::from_ext(ext)
            .first()
            .map(|m| m.to_string())
            .or_else(|| {
                TEXT_EXT
                    .contains(&ext.as_str())
                    .then_some("text/plain".to_string())
            })
    });

    let bundle = doc
        .get_first(fields.bundle)
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .and_then(|name| {
            let bundle_size = doc.get_first(fields.bundle_size).and_then(|v| v.as_u64());
            bundle_size.map(|size| Bundle { name, size })
        });

    let sprite = if let (Some(sheet), Some(x), Some(y), Some(w), Some(h)) = (
        doc.get_first(fields.sprite_sheet)
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
        doc.get_first(fields.sprite_x).and_then(|v| v.as_u64()),
        doc.get_first(fields.sprite_y).and_then(|v| v.as_u64()),
        doc.get_first(fields.sprite_w).and_then(|v| v.as_u64()),
        doc.get_first(fields.sprite_h).and_then(|v| v.as_u64()),
    ) {
        let source = doc
            .get_first(fields.sprite_txt)
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .unwrap_or_default();
        Some(Sprite {
            sheet,
            source,
            x,
            y,
            w,
            h,
        })
    } else {
        None
    };

    Ok(Node {
        path,
        dirname,
        basename,
        node_type,
        extension,
        mime_type,
        storage,
        file_size,
        bundle_offset,
        bundle,
        sprite,
    })
}

fn perform_query<T, M: FnMut(Result<TantivyDocument, Response>) -> Result<T, Response>>(
    searcher: &Searcher,
    storages: &[String],
    query: Box<dyn tantivy::query::Query>,
    limit: Option<usize>,
    map: M,
) -> Result<Vec<T>, Response> {
    let found = if let Some(limit) = limit {
        searcher.search(&query, &TopDocs::with_limit(limit))
    } else {
        searcher.search(&query, &CollectAll)
    }
    .map_err(|e| error(format!("error performing query: {}", e), storages))?;

    let results: Result<Vec<T>, Response> = found
        .iter()
        .map(|&(_, addr)| {
            searcher
                .doc(addr)
                .map_err(|e| error(format!("error fetching results: {}", e), storages))
        })
        .map(map)
        .collect();

    Ok(results?)
}

fn error(error: String, storages: &[String]) -> Response {
    let mut resp = Json(ErrorResponse { error, storages }).into_response();
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp
}
