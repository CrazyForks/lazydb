use std::path::PathBuf;

use anyhow::Result;
use tower_lsp_server::jsonrpc::Result as LspResult;
use tower_lsp_server::ls_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    InitializeParams, InitializeResult, InitializedParams, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use super::catalog::CatalogCache;
use super::catalog::CatalogProvider;
use super::completion::complete_document_with_embedded_sql;
use super::diagnostics::diagnostics_for_document;
use super::document::Documents;
use crate::sql::{CompletionIndex, SqlDialect};

#[derive(Debug)]
struct LanguageServerState {
    client: Client,
    project: Option<PathBuf>,
    dialect: crate::cli::LspDialect,
    documents: tokio::sync::Mutex<Documents>,
    catalog_cache: CatalogCache,
    catalog_provider: Option<CatalogProvider>,
}

impl LanguageServer for LanguageServerState {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        let _ = (&self.project, self.dialect);
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                tower_lsp_server::ls_types::MessageType::INFO,
                "LazyDB language server initialized",
            )
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };
        let key = super::catalog::CatalogKey {
            connection: self.catalog_provider.as_ref().map_or_else(
                || "offline".into(),
                |provider| provider.profile_id().to_string(),
            ),
            database: None,
            schema: None,
        };
        let snapshot = self
            .catalog_cache
            .snapshot(key, || async {
                let index = match &self.catalog_provider {
                    Some(provider) => match provider.load_index().await {
                        Ok(index) => {
                            eprintln!("lazydb lsp: catalog index loaded with {} entries", index.entries().len());
                            index
                        }
                        Err(error) => {
                            eprintln!("lazydb lsp: catalog load failed: {error:#}");
                            self.client
                                .log_message(
                                    tower_lsp_server::ls_types::MessageType::WARNING,
                                    format!("LazyDB catalog load failed: {error}"),
                                )
                                .await;
                            CompletionIndex::new(&[])
                        }
                    },
                    None => CompletionIndex::new(&[]),
                };
                super::catalog::CatalogSnapshot {
                    generation: 0,
                    index: std::sync::Arc::new(index),
                    complete: self.catalog_provider.is_some(),
                }
            })
            .await;
        Ok(Some(complete_document_with_embedded_sql(
            document,
            params.text_document_position.position,
            sql_dialect(self.dialect),
            &snapshot.index,
            !snapshot.complete,
        )))
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = self.documents.lock().await.open(params);
        self.publish_diagnostics(&document).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let changed = self.documents.lock().await.change(params);
        if let Some(document) = changed {
            self.publish_diagnostics(&document).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.documents.lock().await.save(params);
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().await.close(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }
}

impl LanguageServerState {
    async fn publish_diagnostics(&self, document: &super::document::Document) {
        let diagnostics = diagnostics_for_document(document, sql_dialect(self.dialect));
        self.client
            .publish_diagnostics(document.uri.clone(), diagnostics, Some(document.version))
            .await;
    }
}

pub async fn run(
    args: crate::cli::LspArgs,
    config: Option<PathBuf>,
    profile: Option<String>,
) -> Result<()> {
    let provider =
        match CatalogProvider::from_config(args.project.as_deref(), config, profile.as_deref())
            .await
        {
            Ok(provider) => {
                eprintln!(
                    "lazydb lsp: catalog provider {}",
                    if provider.is_some() {
                        "initialized"
                    } else {
                        "not selected"
                    }
                );
                provider
            }
            Err(error) => {
                eprintln!("lazydb lsp: catalog unavailable; using offline completion: {error}");
                None
            }
        };
    run_server_with_provider(args.project, args.dialect.unwrap_or_default(), provider).await
}

pub async fn run_server(
    project: Option<PathBuf>,
    dialect: crate::cli::LspDialect,
    _config: Option<PathBuf>,
) -> Result<()> {
    run_server_with_provider(project, dialect, None).await
}

async fn run_server_with_provider(
    project: Option<PathBuf>,
    dialect: crate::cli::LspDialect,
    catalog_provider: Option<CatalogProvider>,
) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| LanguageServerState {
        client,
        project,
        dialect,
        documents: tokio::sync::Mutex::new(Documents::default()),
        catalog_cache: CatalogCache::default(),
        catalog_provider,
    });
    Server::new(stdin, stdout, socket)
        .concurrency_level(8)
        .serve(service)
        .await;
    Ok(())
}

fn sql_dialect(dialect: crate::cli::LspDialect) -> SqlDialect {
    match dialect {
        crate::cli::LspDialect::Postgres => SqlDialect::Postgres,
        crate::cli::LspDialect::MySql => SqlDialect::MySql,
        crate::cli::LspDialect::SqlServer => SqlDialect::SqlServer,
        crate::cli::LspDialect::Sqlite => SqlDialect::Sqlite,
        crate::cli::LspDialect::Generic => SqlDialect::Generic,
    }
}
