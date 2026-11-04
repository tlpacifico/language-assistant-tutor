//! Conversa em inglês sobre um artigo: contratos e regras puras.
//!
//! Modo separado da entrevista STAR. Este módulo não depende de SQLite, de
//! áudio nem de rede; `types` espelha `src/lib/article-contracts.ts` e
//! `state` valida as transições e o consentimento.

mod state;
mod types;

pub use state::*;
pub use types::*;
