use schema::Schema;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, SchemaBuilder};
use tantivy::tokenizer::TokenizerManager;
use tantivy::{schema, IndexReader, TantivyDocument};
use tantivy::{Index, Term};
use tempfile::TempDir;

pub enum EntryType {
    File,
    Folder,
}

pub struct IndexState {
    pub index: Index,
    pub fields: Fields,
    pub path: TempDir,
    pub reader: IndexReader,
    pub query_parser: QueryParser,
}

impl Default for IndexState {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexState {
    pub fn new() -> Self {
        let mut schema_builder = Schema::builder();

        let fields = Fields::new(&mut schema_builder);

        let schema = schema_builder.build();

        let path = TempDir::new().expect("Could not create index directory.");
        let index = Index::create_in_dir(&path, schema.clone()).expect("Could not create index.");
        let reader = index.reader().expect("Could not create reader.");

        let mut query_parser =
            QueryParser::new(schema, vec![fields.path], TokenizerManager::default());
        query_parser.set_field_fuzzy(fields.path, true, 2, true);

        Self {
            path,
            index,
            reader,
            fields,
            query_parser,
        }
    }
}

pub struct Fields {
    pub path: Field,
    pub name: Field,
    pub parent: Field,
    pub typ: Field,
    pub version: Field,
    pub bundle: Field,
    pub bundle_size: Field,
    pub size: Field,
    pub offset: Field,
}

impl Fields {
    pub fn new(schema_builder: &mut SchemaBuilder) -> Self {
        let path = schema_builder.add_text_field("path", schema::TEXT);
        let name = schema_builder.add_text_field("name", schema::STRING | schema::STORED);
        let parent =
            schema_builder.add_text_field("parent", schema::STRING | schema::STORED | schema::FAST);
        let typ = schema_builder.add_u64_field("type", schema::INDEXED | schema::STORED);
        let version = schema_builder.add_text_field("version", schema::STRING | schema::STORED);
        let offset = schema_builder.add_u64_field("offset", schema::STORED);
        let size = schema_builder.add_u64_field("size", schema::STORED);
        let bundle = schema_builder.add_text_field("bundle", schema::STORED);
        let bundle_size = schema_builder.add_u64_field("bundle_size", schema::STORED);

        Self {
            path,
            name,
            parent,
            typ,
            version,
            offset,
            size,
            bundle,
            bundle_size,
        }
    }

    pub fn add_file(
        self: &Fields,
        version: &str,
        filename: &str,
        offset: u32,
        size: u32,
        bundle: &str,
        bundle_size: u32,
        doc: &mut TantivyDocument,
    ) {
        let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename));
        doc.add_text(self.version, version);
        doc.add_text(self.path, filename);
        doc.add_text(self.name, name);
        doc.add_text(self.parent, dir);
        doc.add_u64(self.typ, EntryType::File as u64);
        doc.add_u64(self.offset, offset as u64);
        doc.add_u64(self.size, size as u64);
        doc.add_text(self.bundle, bundle);
        doc.add_u64(self.bundle_size, bundle_size as u64);
    }

    pub fn add_folder(self: &Fields, version: &str, filename: &str, doc: &mut TantivyDocument) {
        let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename));
        doc.add_text(self.version, version);
        doc.add_text(self.path, filename);
        doc.add_text(self.name, name);
        doc.add_text(self.parent, dir);
        doc.add_u64(self.typ, EntryType::Folder as u64);
    }

    pub fn version_term(&self, value: &str) -> Term {
        Term::from_field_text(self.version, value)
    }
}
