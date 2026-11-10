//! Tantivy schema of the text index and per-field query weights.

use tantivy::schema::{
    Field, IndexRecordOption, NumericOptions, Schema, TextFieldIndexing, TextOptions,
};

use super::analyzer::TOKENIZER_NAME;

/// Version of the schema below. Bump on any field or option change; with
/// [`crate::text::ANALYZER_VERSION`] it decides whether an on-disk index
/// is reused or rebuilt (ADR-017).
pub const SCHEMA_VERSION: u32 = 1;

/// Name of the `file_id` field (u64 fast field; `files.id`).
pub(crate) const FILE_ID: &str = "file_id";

/// One searchable text field and its weight at query time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextField {
    /// Tantivy field.
    pub field: Field,
    /// Multiplier of every match in this field (name > tags/category >
    /// kind > folders).
    pub weight: f32,
}

/// Field handles of the text index.
#[derive(Debug, Clone)]
pub(crate) struct Fields {
    /// `files.id`: indexed (delete by id) and fast (read per hit).
    pub file_id: Field,
    /// File name without media extension.
    pub name: Field,
    /// Library root folder name + folders between root and file.
    pub path: Field,
    /// User tags (one value per tag).
    pub tags: Field,
    /// Effective category (override, else detected).
    pub category: Field,
    /// Effective kind (`loop` / `one_shot`).
    pub kind: Field,
}

/// Query-time weights, by field name. Name hits dominate; folder hits
/// still count (a `Kicks/` folder says a lot) but less, because a folder
/// is shared by many files and often names a pack, not the sound.
const WEIGHTS: [(&str, f32); 5] = [
    ("name", 3.0),
    ("tags", 2.0),
    ("category", 2.0),
    ("kind", 1.5),
    ("path", 1.0),
];

impl Fields {
    /// Builds the schema and its field handles.
    pub(crate) fn schema() -> (Schema, Self) {
        let mut b = Schema::builder();
        let file_id = b.add_u64_field(FILE_ID, NumericOptions::default().set_indexed().set_fast());
        // Positions are needed for multi-word synonyms (`one shot`).
        let text = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER_NAME)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let name = b.add_text_field("name", text.clone());
        let path = b.add_text_field("path", text.clone());
        let tags = b.add_text_field("tags", text.clone());
        let category = b.add_text_field("category", text.clone());
        let kind = b.add_text_field("kind", text);
        let fields = Self {
            file_id,
            name,
            path,
            tags,
            category,
            kind,
        };
        (b.build(), fields)
    }

    /// Looks the handles up in an existing schema; `None` if a field is
    /// missing (schema from another version).
    pub(crate) fn from_schema(schema: &Schema) -> Option<Self> {
        let f = |name| schema.get_field(name).ok();
        Some(Self {
            file_id: f(FILE_ID)?,
            name: f("name")?,
            path: f("path")?,
            tags: f("tags")?,
            category: f("category")?,
            kind: f("kind")?,
        })
    }

    /// Searchable text fields with their weights.
    pub(crate) fn text_fields(&self) -> [TextField; 5] {
        let by_name = |name: &str| match name {
            "name" => self.name,
            "tags" => self.tags,
            "category" => self.category,
            "kind" => self.kind,
            _ => self.path,
        };
        WEIGHTS.map(|(name, weight)| TextField {
            field: by_name(name),
            weight,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_round_trips_and_name_weighs_most() {
        let (schema, fields) = Fields::schema();
        let again = Fields::from_schema(&schema).expect("fields");
        assert_eq!(again.name, fields.name);
        assert_eq!(again.file_id, fields.file_id);
        let tf = fields.text_fields();
        let name = tf.iter().find(|t| t.field == fields.name).expect("name");
        assert!(tf.iter().all(|t| t.weight <= name.weight));
        let path = tf.iter().find(|t| t.field == fields.path).expect("path");
        assert!(tf.iter().all(|t| t.weight >= path.weight));
        assert!(Fields::from_schema(&Schema::builder().build()).is_none());
    }
}
