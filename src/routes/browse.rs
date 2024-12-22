use crate::index::collector::CollectAll;
use crate::index::state::{EntryType, IndexState};
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

#[derive(Deserialize, Default, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Command {
    #[default]
    Ready,
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
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct IndexResponse {
    pub storages: Vec<String>,
    pub adapter: String,
    pub files: Vec<Node>,
}

#[derive(Serialize)]
pub struct Node {
    pub path: String,
    pub basename: String,
    pub extension: Option<String>,
    pub storage: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub mime_type: Option<String>,
}

#[derive(Serialize, Eq, PartialEq)]
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
        mut path,
        mut limit,
    }): Query<Params>,
    State(AppState { urls, index }): State<AppState>,
) -> Result<Json<IndexResponse>, Response> {
    let storages = { urls.read().await.clone() };
    if storages.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    let adapter = adapter.unwrap_or_else(|| storages[0].clone());

    if command == Command::Ready {
        return Ok(Json(IndexResponse {
            adapter,
            storages,
            files: Vec::new(),
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

    let mut query: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::with_capacity(3);

    query.push((
        Occur::Must,
        Box::new(TermQuery::new(fields.version_term(adapter.as_str()), Basic)),
    ));
    if command == Command::Search {
        if limit.is_none() {
            limit = Some(10);
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
                Term::from_field_u64(fields.typ, EntryType::Folder as u64),
                Basic,
            )),
        ))
    }
    let query: Box<dyn tantivy::query::Query> = Box::new(BooleanQuery::new(query));

    let files = perform_query(&reader.searcher(), &storages, query, limit, |doc| {
        let doc = doc?;
        let basename = doc
            .get_first(fields.name)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let parent = doc
            .get_first(fields.parent)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let node_type = if doc.get_first(fields.typ).and_then(|v| v.as_u64())
            == Some(EntryType::Folder as u64)
        {
            NodeType::Dir
        } else {
            NodeType::File
        };
        let path: PathBuf = [&parent, &basename].iter().collect();
        let extension = if node_type == NodeType::Dir {
            None
        } else {
            path.extension().map(|v| v.to_string_lossy().to_string())
        };
        let path = path.to_string_lossy().to_string();
        let mime_type = mime_guess::from_path(&path).first().map(|m| m.to_string());
        Ok(Node {
            path,
            basename,
            node_type,
            extension,
            mime_type,
            storage: adapter.clone(),
        })
    })?;
    Ok(Json(IndexResponse {
        adapter,
        storages,
        files,
    }))
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
