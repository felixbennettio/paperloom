//! An optional, derived index of the full-text documents' meaning.

use std::collections::{BTreeMap, BTreeSet};

use paperloom_ai::{self as ai, Protocol, Provider};
use paperloom_fulltext::Hit;
use paperloom_http::Http;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AiProvider, CoreError, PaperLibrary};

const TEXT_CHARS: usize = 2_000;
const BATCH_SIZE: usize = 32;
const SEARCH_LIMIT: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AiSemanticStatus {
    pub model: String,
    pub indexed: u64,
    pub total: u64,
}

struct Passage {
    key: String,
    text: String,
    digest: Vec<u8>,
}

struct Stamp {
    model: String,
    digest: Vec<u8>,
}

struct Vector {
    key: String,
    dimensions: usize,
    bytes: Vec<u8>,
}

fn storage(error: impl std::fmt::Display) -> CoreError {
    CoreError::Storage { message: format!("Semantic index: {error}") }
}

fn configured(settings: AiProvider) -> Result<(Provider, String), CoreError> {
    let model = settings.embedding_model.trim().to_string();
    if model.is_empty() {
        return Err(ai::AiError::Setup("Choose an embedding model in Settings.".into()).into());
    }
    let provider = crate::ai::provider(settings)?;
    if provider.protocol != Protocol::OpenAi {
        return Err(ai::AiError::Setup("Semantic search needs an OpenAI-compatible provider.".into()).into());
    }
    Ok((provider, model))
}

fn stamps(db: &Connection) -> Result<BTreeMap<String, Stamp>, CoreError> {
    let mut statement = db.prepare("SELECT key, model, digest FROM vectors").map_err(storage)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, Stamp { model: row.get(1)?, digest: row.get(2)? })))
        .map_err(storage)?;
    rows.collect::<rusqlite::Result<_>>().map_err(storage)
}

fn current(stamps: &BTreeMap<String, Stamp>, passage: &Passage, model: &str) -> bool {
    stamps.get(&passage.key).is_some_and(|stamp| stamp.model == model && stamp.digest == passage.digest)
}

fn normalized(vector: &[f32]) -> Result<Vec<f32>, CoreError> {
    let length = vector.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt();
    if vector.is_empty() || !length.is_finite() || length == 0.0 {
        return Err(ai::AiError::Setup("The provider returned invalid embeddings.".into()).into());
    }
    Ok(vector.iter().map(|&v| (f64::from(v) / length) as f32).collect())
}

#[uniffi::export]
impl PaperLibrary {
    /// Counts vectors matching the chosen model and the current document text.
    pub fn ai_semantic_status(&self, provider: AiProvider) -> Result<AiSemanticStatus, CoreError> {
        let (_, model) = configured(provider)?;
        let passages = self.semantic_passages()?;
        self.semantic_db(|db| {
            let stored = stamps(db)?;
            Ok(AiSemanticStatus {
                indexed: passages.iter().filter(|p| current(&stored, p, &model)).count() as u64,
                total: passages.len() as u64,
                model,
            })
        })
    }

    /// Explicitly indexes up to `limit` missing or changed documents.
    pub fn ai_semantic_index(&self, provider: AiProvider, limit: u32) -> Result<AiSemanticStatus, CoreError> {
        self.semantic_index_with(&crate::ai::http(), provider, limit)
    }
}

impl PaperLibrary {
    fn semantic_db<T>(
        &self,
        use_db: impl FnOnce(&mut Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut kept = self.vectors.lock().map_err(|_| CoreError::Storage {
            message: "The semantic index is unavailable after a crash.".into(),
        })?;
        if kept.is_none() {
            let db = match &self.vectors_path {
                Some(path) => Connection::open(path),
                None => Connection::open_in_memory(),
            }
            .map_err(storage)?;
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS vectors (
                    key TEXT PRIMARY KEY,
                    model TEXT NOT NULL,
                    digest BLOB NOT NULL,
                    dimensions INTEGER NOT NULL,
                    vector BLOB NOT NULL
                );",
            )
            .map_err(storage)?;
            *kept = Some(db);
        }
        use_db(kept.as_mut().expect("semantic database opened"))
    }

    fn semantic_passages(&self) -> Result<Vec<Passage>, CoreError> {
        Ok(self
            .fulltext
            .documents()?
            .into_iter()
            .map(|doc| {
                let text: String =
                    doc.title.chars().chain(['\n']).chain(doc.body.chars()).take(TEXT_CHARS).collect();
                let digest = Sha256::digest(text.as_bytes()).to_vec();
                Passage { key: doc.key, text, digest }
            })
            .collect())
    }

    pub(crate) fn semantic_index_with(
        &self,
        http: &dyn Http,
        settings: AiProvider,
        limit: u32,
    ) -> Result<AiSemanticStatus, CoreError> {
        let (provider, model) = configured(settings.clone())?;
        let passages = self.semantic_passages()?;
        self.semantic_db(|db| {
            let stored = stamps(db)?;
            let live: BTreeSet<&str> = passages.iter().map(|p| p.key.as_str()).collect();
            let transaction = db.transaction().map_err(storage)?;
            for key in stored.keys().filter(|key| !live.contains(key.as_str())) {
                transaction.execute("DELETE FROM vectors WHERE key = ?1", [key]).map_err(storage)?;
            }
            transaction.commit().map_err(storage)?;
            let pending: Vec<&Passage> =
                passages.iter().filter(|p| !current(&stored, p, &model)).take(limit as usize).collect();
            for batch in pending.chunks(BATCH_SIZE) {
                let inputs: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
                let vectors = ai::embed(http, &provider, &model, &inputs)?;
                let vectors = vectors.iter().map(|v| normalized(v)).collect::<Result<Vec<_>, _>>()?;
                let transaction = db.transaction().map_err(storage)?;
                for (passage, vector) in batch.iter().zip(vectors) {
                    let bytes: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
                    transaction
                        .execute(
                            "INSERT OR REPLACE INTO vectors (key, model, digest, dimensions, vector)
                             VALUES (?1, ?2, ?3, ?4, ?5)",
                            params![passage.key, model, passage.digest, vector.len(), bytes],
                        )
                        .map_err(storage)?;
                }
                transaction.commit().map_err(storage)?;
            }
            Ok(())
        })?;
        self.ai_semantic_status(settings)
    }

    pub(crate) fn semantic_hits(
        &self,
        http: &dyn Http,
        provider: &Provider,
        model: &str,
        question: &str,
    ) -> Result<Vec<Hit>, CoreError> {
        if provider.protocol != Protocol::OpenAi {
            return Ok(Vec::new());
        }
        let vectors = self.semantic_db(|db| {
            let mut statement = db
                .prepare("SELECT key, dimensions, vector FROM vectors WHERE model = ?1")
                .map_err(storage)?;
            let rows = statement
                .query_map([model], |row| {
                    Ok(Vector { key: row.get(0)?, dimensions: row.get(1)?, bytes: row.get(2)? })
                })
                .map_err(storage)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)
        })?;
        if vectors.is_empty() {
            return Ok(Vec::new());
        }
        let embedded = ai::embed(http, provider, model, &[question.to_string()])?;
        let question = normalized(&embedded[0])?;
        let mut ranked = Vec::new();
        for vector in vectors {
            if vector.dimensions != question.len()
                || vector.bytes.len() / 4 != question.len()
                || !vector.bytes.len().is_multiple_of(4)
            {
                continue;
            }
            let score: f32 = vector
                .bytes
                .as_chunks::<4>()
                .0
                .iter()
                .zip(&question)
                .map(|(bytes, q)| f32::from_le_bytes(*bytes) * q)
                .sum();
            if score.is_finite() {
                ranked.push((vector.key, score));
            }
        }
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut hits = Vec::new();
        for (key, _) in ranked {
            if let Some(hit) = self.fulltext.hit(&key)? {
                hits.push(hit);
                if hits.len() == SEARCH_LIMIT {
                    break;
                }
            }
        }
        Ok(hits)
    }
}

/// Reciprocal rank fusion keeps the two retrieval scores comparable.
pub(crate) fn fuse(words: Vec<Hit>, semantic: Vec<Hit>) -> Vec<Hit> {
    let mut scores: BTreeMap<String, (Hit, f64)> = BTreeMap::new();
    for list in [words, semantic] {
        for (rank, hit) in list.into_iter().enumerate() {
            let entry = scores.entry(hit.key.clone()).or_insert((hit, 0.0));
            entry.1 += 1.0 / (60.0 + rank as f64 + 1.0);
        }
    }
    let mut ranked: Vec<_> = scores.into_values().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.key.cmp(&b.0.key)));
    ranked.into_iter().map(|(hit, _)| hit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_vectors_and_rejects_invalid_lengths() {
        assert_eq!(normalized(&[3.0, 4.0]).unwrap(), [0.6, 0.8]);
        assert!(normalized(&[f32::MAX, f32::MAX]).unwrap().iter().all(|v| v.is_finite()));
        for vector in [&[][..], &[0.0, 0.0], &[f32::NAN, 1.0], &[f32::INFINITY, 0.0]] {
            assert!(normalized(vector).is_err());
        }
    }
}
