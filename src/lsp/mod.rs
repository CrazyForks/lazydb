pub mod catalog;
pub mod completion;
pub mod diagnostics;
pub mod document;
pub mod position;
mod server;

use std::path::PathBuf;

use anyhow::Result;

pub use server::run;

pub async fn run_with_options(
    project: Option<PathBuf>,
    dialect: crate::cli::LspDialect,
    config: Option<PathBuf>,
) -> Result<()> {
    server::run_server(project, dialect, config).await
}
