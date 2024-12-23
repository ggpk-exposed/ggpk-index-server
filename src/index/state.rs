use schema::Schema;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, SchemaBuilder};
use tantivy::tokenizer::TokenizerManager;
use tantivy::{schema, IndexReader};
use tantivy::{Index, Term};
use tempfile::TempDir;

pub struct EntryType;
impl EntryType {
    pub const FILE: &'static str = "file";
    pub const DIR: &'static str = "dir";
    pub const SPRITE: &'static str = "sprite";
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
    pub sprite_sheet: Field,
    pub sprite_txt: Field,
    pub sprite_x: Field,
    pub sprite_y: Field,
    pub sprite_w: Field,
    pub sprite_h: Field,
}

impl Fields {
    pub fn new(schema_builder: &mut SchemaBuilder) -> Self {
        let path = schema_builder.add_text_field("path", schema::TEXT);
        let name = schema_builder.add_text_field("name", schema::STRING | schema::STORED);
        let parent =
            schema_builder.add_text_field("parent", schema::STRING | schema::STORED | schema::FAST);
        let typ = schema_builder.add_text_field("type", schema::STRING | schema::STORED);
        let version = schema_builder.add_text_field("version", schema::STRING | schema::STORED);
        let offset = schema_builder.add_u64_field("offset", schema::STORED);
        let size = schema_builder.add_u64_field("size", schema::STORED);
        let bundle = schema_builder.add_text_field("bundle", schema::STORED);
        let bundle_size = schema_builder.add_u64_field("bundle_size", schema::STORED);
        let sprite_sheet = schema_builder.add_text_field("sprite_sheet", schema::STORED);
        let sprite_txt = schema_builder.add_text_field("sprite_txt", schema::STORED);
        let sprite_x = schema_builder.add_u64_field("sprite_x", schema::STORED);
        let sprite_y = schema_builder.add_u64_field("sprite_y", schema::STORED);
        let sprite_w = schema_builder.add_u64_field("sprite_w", schema::STORED);
        let sprite_h = schema_builder.add_u64_field("sprite_h", schema::STORED);

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
            sprite_sheet,
            sprite_txt,
            sprite_x,
            sprite_y,
            sprite_w,
            sprite_h,
        }
    }

    pub fn version_term(&self, value: &str) -> Term {
        Term::from_field_text(self.version, value)
    }
}
