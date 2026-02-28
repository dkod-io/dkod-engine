use dk_core::{SymbolId, TypeInfo};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// Intermediate row type for mapping between database rows and `TypeInfo`.
#[derive(sqlx::FromRow)]
struct TypeInfoRow {
    symbol_id: Uuid,
    params: Option<serde_json::Value>,
    return_type: Option<String>,
    fields: Option<serde_json::Value>,
    implements: Option<Vec<String>>,
}

impl TypeInfoRow {
    fn into_type_info(self) -> TypeInfo {
        TypeInfo {
            symbol_id: self.symbol_id,
            params: parse_string_pair_array(self.params),
            return_type: self.return_type,
            fields: parse_string_pair_array(self.fields),
            implements: self.implements.unwrap_or_default(),
        }
    }
}

/// Parse a JSONB value containing `[["name","type"], ...]` into `Vec<(String, String)>`.
fn parse_string_pair_array(value: Option<serde_json::Value>) -> Vec<(String, String)> {
    match value {
        Some(serde_json::Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|item| {
                if let serde_json::Value::Array(pair) = item {
                    if pair.len() == 2 {
                        let a = pair[0].as_str()?.to_string();
                        let b = pair[1].as_str()?.to_string();
                        return Some((a, b));
                    }
                }
                None
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Serialize `Vec<(String, String)>` into a JSONB-compatible `serde_json::Value`.
fn pairs_to_json(pairs: &[(String, String)]) -> serde_json::Value {
    serde_json::Value::Array(
        pairs
            .iter()
            .map(|(a, b)| {
                serde_json::Value::Array(vec![
                    serde_json::Value::String(a.clone()),
                    serde_json::Value::String(b.clone()),
                ])
            })
            .collect(),
    )
}

/// PostgreSQL-backed store for type information attached to symbols.
#[derive(Clone)]
pub struct TypeInfoStore {
    pool: PgPool,
}

impl TypeInfoStore {
    /// Create a new `TypeInfoStore` backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert or update type information for a symbol.
    ///
    /// Uses `ON CONFLICT (symbol_id) DO UPDATE` so that re-parsing the same
    /// file replaces old type information.
    ///
    /// `params` and `fields` are serialized as JSONB arrays of `["name","type"]` pairs.
    /// `implements` is stored as a PostgreSQL `TEXT[]` array.
    pub async fn upsert_type_info(&self, info: &TypeInfo) -> dk_core::Result<()> {
        let params_json = pairs_to_json(&info.params);
        let fields_json = pairs_to_json(&info.fields);

        sqlx::query(
            r#"
            INSERT INTO type_info (symbol_id, params, return_type, fields, implements)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (symbol_id) DO UPDATE SET
                params = EXCLUDED.params,
                return_type = EXCLUDED.return_type,
                fields = EXCLUDED.fields,
                implements = EXCLUDED.implements
            "#,
        )
        .bind(info.symbol_id)
        .bind(&params_json)
        .bind(&info.return_type)
        .bind(&fields_json)
        .bind(&info.implements)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get type information for a symbol by its ID.
    pub async fn get_by_symbol_id(
        &self,
        symbol_id: SymbolId,
    ) -> dk_core::Result<Option<TypeInfo>> {
        let row = sqlx::query_as::<_, TypeInfoRow>(
            r#"
            SELECT symbol_id, params, return_type, fields, implements
            FROM type_info
            WHERE symbol_id = $1
            "#,
        )
        .bind(symbol_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(TypeInfoRow::into_type_info))
    }

    /// Delete type information for a symbol. Returns the number of rows deleted.
    pub async fn delete_by_symbol_id(&self, symbol_id: SymbolId) -> dk_core::Result<u64> {
        let result = sqlx::query("DELETE FROM type_info WHERE symbol_id = $1")
            .bind(symbol_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}
