//! Validação da sequência de blocos de voz de um pedido `synthesize_stream`.
//!
//! Um bloco só é aceite se for o índice esperado, estiver na pasta do próprio
//! pedido e tiver o nome `chunk-NNNN.wav`; duplicados, saltos e caminhos fora da
//! operação são recusados sem avançar a sequência.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkRejection {
    Duplicate,
    OutOfOrder { expected: u32, got: u32 },
    OutsideDirectory,
    BadName,
}

#[derive(Debug, Clone)]
pub struct ChunkSequence {
    directory: PathBuf,
    next: u32,
}

impl ChunkSequence {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            next: 0,
        }
    }

    /// Quantos blocos já foram aceites.
    pub fn accepted(&self) -> u32 {
        self.next
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn accept(&mut self, index: u32, path: &Path) -> Result<PathBuf, ChunkRejection> {
        if index < self.next {
            return Err(ChunkRejection::Duplicate);
        }
        if index > self.next {
            return Err(ChunkRejection::OutOfOrder {
                expected: self.next,
                got: index,
            });
        }
        if path.components().any(|c| matches!(c, Component::ParentDir))
            || path.parent() != Some(self.directory.as_path())
        {
            return Err(ChunkRejection::OutsideDirectory);
        }
        let expected = format!("chunk-{index:04}.wav");
        if path.file_name().and_then(|n| n.to_str()) != Some(expected.as_str()) {
            return Err(ChunkRejection::BadName);
        }
        self.next += 1;
        Ok(path.to_path_buf())
    }
}
